//! Les virages et les allers-retours de la recherche organique (lot 4, 4c, §16.2 et §16.4 de
//! `docs/design/technique/ord-loops-lot4.md`).
//!
//! Le contrôle des garanties refuse un virage de 150° ou plus à un nœud de degré 3 ou plus, seul ou sur une **grappe** de
//! nœuds distants de 100 m au plus le long du parcours. Les recherches ne voient qu'un nœud à la fois : elles produisaient
//! des triangles refusés après coup. Le [`TurnModel`] refait ce contrôle **à la relaxation**, sur le chemin courant, avec
//! des caps pris sur un chemin approché (les fenêtres de 25 à 50 m que le contrôle prend sur 50 m de chemin, exactes sur
//! les chaînes droites, §16.13 de la note).
//!
//! Tout est en `f64`, hormis la table des vecteurs unitaires des caps stockés (`f32`, construite une fois, sans état
//! propre à une requête) : mêmes sorties à 1 et 16 fils, et la même dépendance à la libm que la géométrie du contrôle.

use std::cell::{Cell, RefCell};
use std::sync::OnceLock;

use ord_graph::view::{HEADING_FAR_M, HEADING_NEAR_M};

/// Les deux bouts de la fenêtre de cap : le contrôle prend `near = min(30, far / 2)`, donc **25 m** quand le chemin connu
/// atteint les 50 m (la note dit « 30 à 50 m » : écart au §16.2, consigné au §16.13).
const NEAR_M: f64 = if HEADING_NEAR_M < HEADING_FAR_M / 2.0 { HEADING_NEAR_M } else { HEADING_FAR_M / 2.0 };

use crate::engine::ViewData;
use crate::geo::Local;
use crate::search::{angle16, in_head, out_head, Ctx, Tree, NONE};
use crate::verify::{CLUSTER_FREE_ENDS_M, CLUSTER_M};

/// Chaîne de ≥ 50 m : droite le long de son cap de bout (dm).
const LONG_DM: u32 = 500;
/// Au-delà de cette longueur (dm) pour la chaîne qui précède, seul le virage simple du nœud compte (cluster de 100 m).
const CLUSTER_DM_FAST: u32 = 1_000;
/// Chemin lu en arrière (dm) : 50 m de nœuds candidats, 100 m de grappe, 50 m de fenêtre de cap.
const NEED_DM: u32 = 2_000;
/// Au plus ce nombre d'états du chemin sont relus.
const MAX_STATES: usize = 48;
/// Jonction d'un chemin d'arbre (dm) : les états qui commencent à moins de 200 m du nœud de jonction sont rejoués. Une paire
/// dont la fenêtre d'avant `a` déborde sur les jambes posées a son `a` à moins de 50 m après la jonction, son `b` à moins de
/// 150 m ; elle n'est lue avec sa fenêtre d'après entière qu'à l'état qui commence moins de 50 m après `b`.
const JUNCTION_DM: u32 = 2_000;

/// Le modèle : seuil du refus (150° moins la marge), grappe et bouts libres du contrôle, en dm.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TurnModel {
    /// Carré du cosinus du seuil : un virage de `max_deg` ou plus est un produit scalaire négatif de carré au moins
    /// `cos2 × |u|² × |v|²` (le seuil est au-dessus de 90°).
    cos2: f64,
    /// Seuil de la voie rapide, sur l'angle `u16` des caps stockés (360/65536 degrés).
    max16: u32,
    cluster_dm: u32,
    free_end_dm: u32,
}

impl TurnModel {
    pub fn new(max_turn_deg: f64, margin_deg: u8) -> Self {
        let max_deg = max_turn_deg - f64::from(margin_deg);
        Self {
            cos2: max_deg.to_radians().cos().powi(2),
            max16: (max_deg / 180.0 * 32_768.0).ceil() as u32,
            cluster_dm: (CLUSTER_M * 10.0) as u32,
            free_end_dm: (CLUSTER_FREE_ENDS_M * 10.0) as u32,
        }
    }
}

/// Le chemin que le modèle remonte au-delà de la racine d'une recherche : les états déjà posés (jambes précédentes), et
/// les mètres du parcours jusqu'au nœud de départ de la recherche.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Lead<'a> {
    /// Recherche en avant : les états qui **précèdent** le nœud de départ, dans l'ordre du parcours, le dernier arrivant à
    /// ce nœud. Recherche à rebours : les états qui **suivent** le nœud d'arrivée, dans l'ordre du parcours.
    pub path: &'a [u32],
    /// Mètres (dm) du parcours : du départ au nœud de départ de la recherche (en avant), du nœud d'arrivée de la recherche
    /// à l'arrivée du parcours (à rebours).
    pub dm: u32,
    /// L'arbre de l'autre bout, qui dit la distance d'un nœud à l'extrémité du parcours que la recherche ne connaît pas :
    /// l'arbre retour pour une recherche en avant, l'arbre aller pour une recherche à rebours. Une approximation.
    pub aux: Option<&'a Tree>,
}

impl Lead<'static> {
    pub const NONE: Lead<'static> = Lead { path: &[], dm: 0, aux: None };
}

/// Ce que l'organique ajoute aux relaxations d'une requête : le modèle des virages, l'interdiction de l'aller-retour, et
/// leurs compteurs (toute la requête).
#[derive(Debug)]
pub(crate) struct OrgSearch {
    pub model: Option<TurnModel>,
    pub no_reuse: bool,
    pub turn_cut: Cell<u64>,
    pub turn_evals: Cell<u64>,
    pub reuse_cut: Cell<u64>,
    scratch: RefCell<Vec<u32>>,
}

impl OrgSearch {
    pub fn new(model: Option<TurnModel>, no_reuse: bool) -> Self {
        Self {
            model,
            no_reuse,
            turn_cut: Cell::new(0),
            turn_evals: Cell::new(0),
            reuse_cut: Cell::new(0),
            scratch: RefCell::new(Vec::new()),
        }
    }

    /// Aller-retour dans une jambe (§16.4) : la relaxation `f → fo` reprend en sens inverse une chaîne que le chemin de `f`
    /// a déjà prise. `d_f` : coût de `f`. Valable dans les deux sens de recherche (en arrière, `f` est l'état déjà fixé
    /// et `fo` le candidat placé avant lui : les prédécesseurs sont les successeurs du parcours).
    #[inline]
    fn reused(&self, t: &Tree, f: u32, fo: u32, d_f: u32) -> bool {
        let r = fo ^ 1;
        if t.get(r) >= d_f {
            return false;
        }
        let mr = t.meters(r);
        let mut cur = f;
        while cur != NONE && t.meters(cur) >= mr {
            if cur == r {
                self.reuse_cut.set(self.reuse_cut.get() + 1);
                return true;
            }
            cur = t.pred(cur);
        }
        false
    }

    /// La relaxation en avant `f → fo` est-elle refusée (aller-retour, ou virage que le contrôle refuserait) ? `m` : mètres
    /// (dm) de la recherche à la fin de `f`, `d_f` son coût ; `in_tree` faux pour le premier pas, où `f` est le dernier
    /// état de `lead.path` et n'est pas dans l'arbre.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn refuses_fwd(
        &self,
        cx: &Ctx<'_>,
        t: &Tree,
        lead: &Lead<'_>,
        f: u32,
        fo: u32,
        m: u32,
        d_f: u32,
        in_tree: bool,
    ) -> bool {
        if self.no_reuse && in_tree && self.reused(t, f, fo, d_f) {
            return true;
        }
        let Some(model) = &self.model else { return false };
        let v = cx.v;
        let lf = v.len_dm[(f >> 1) as usize];
        let junction = v.deg[v.tgt(f) as usize] >= 3;
        if lf >= LONG_DM {
            // Aucun nœud ne précède `b0` de moins de 50 m : `b0` seul, s'il est un carrefour.
            if !junction {
                return false;
            }
            if lf > CLUSTER_DM_FAST && v.len_dm[(fo >> 1) as usize] >= LONG_DM {
                // Voie rapide : ni grappe, ni chemin à remonter, l'angle des deux caps de chaîne.
                let cut = angle16(in_head(v, f), out_head(v, fo)) >= model.max16;
                if cut {
                    self.turn_cut.set(self.turn_cut.get() + 1);
                }
                return cut;
            }
        }
        if lf > CLUSTER_DM_FAST {
            // Chaîne longue puis chaîne courte : le seul virage est le virage simple en `b0`, de cap avant celui de la
            // chaîne et de cap après celui de la corde de `fo` (la fenêtre de 25 à 50 m le dépasse : son dernier segment est
            // prolongé). Même angle que `eval`, sans remonter le chemin.
            let cut = model.simple_turn(v, f, fo);
            if cut {
                self.turn_cut.set(self.turn_cut.get() + 1);
            }
            return cut;
        }
        self.turn_evals.set(self.turn_evals.get() + 1);
        let mut s = self.scratch.borrow_mut();
        s.clear();
        collect_before(&mut s, v, t, lead, f, in_tree, NEED_DM);
        s.push(fo);
        let cut = model.eval(v, &s, lead.dm + m, lead.aux);
        if cut {
            self.turn_cut.set(self.turn_cut.get() + 1);
        }
        cut
    }

    /// Le symétrique à rebours : le candidat `fg` précède `f` (déjà fixé) dans le parcours. Le modèle y est le même, sur le
    /// parcours retourné (même suite d'angles de virage, mêmes grappes) : les états qui suivent `f` sont lus, retournés,
    /// puis `fg` retourné ; `m` : mètres (dm) de `src(f)` à l'extrémité du parcours que la recherche connaît.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn refuses_bwd(
        &self,
        cx: &Ctx<'_>,
        t: &Tree,
        lead: &Lead<'_>,
        fg: u32,
        f: u32,
        m: u32,
        d_f: u32,
        in_tree: bool,
    ) -> bool {
        if self.no_reuse && in_tree && self.reused(t, f, fg, d_f) {
            return true;
        }
        let Some(model) = &self.model else { return false };
        let v = cx.v;
        let lf = v.len_dm[(f >> 1) as usize];
        let junction = v.deg[v.src(f) as usize] >= 3;
        if lf >= LONG_DM {
            if !junction {
                return false;
            }
            if lf > CLUSTER_DM_FAST && v.len_dm[(fg >> 1) as usize] >= LONG_DM {
                let cut = angle16(in_head(v, fg), out_head(v, f)) >= model.max16;
                if cut {
                    self.turn_cut.set(self.turn_cut.get() + 1);
                }
                return cut;
            }
        }
        if lf > CLUSTER_DM_FAST {
            let cut = model.simple_turn_back(v, fg, f);
            if cut {
                self.turn_cut.set(self.turn_cut.get() + 1);
            }
            return cut;
        }
        self.turn_evals.set(self.turn_evals.get() + 1);
        let mut s = self.scratch.borrow_mut();
        s.clear();
        collect_after(&mut s, v, t, lead, f, in_tree, NEED_DM);
        s.push(fg ^ 1);
        let cut = model.eval(v, &s, lead.dm + m, lead.aux);
        if cut {
            self.turn_cut.set(self.turn_cut.get() + 1);
        }
        cut
    }

    /// Jonction d'un chemin d'arbre posé après des jambes déjà fixées (§16.2) : le modèle rejoué sur chaque état de `ret`
    /// dont le début est à moins de 200 m du nœud de jonction (`JUNCTION_DM`), avec `ctx` (les jambes) pour contexte. `base_dm` : mètres du
    /// parcours à la jonction ; `aux` : l'arbre retour. Vrai si aucun virage n'est refusé.
    pub fn junction_ok(
        &self,
        cx: &Ctx<'_>,
        ctx: &[u32],
        base_dm: u32,
        ret: &[u32],
        aux: Option<&Tree>,
    ) -> bool {
        let Some(model) = &self.model else { return true };
        let v = cx.v;
        let ctx = &ctx[ctx.len().saturating_sub(MAX_STATES)..];
        let mut full: Vec<u32> = Vec::with_capacity(ctx.len() + ret.len().min(MAX_STATES));
        full.extend_from_slice(ctx);
        let mut along = 0u32;
        for &x in ret {
            if along >= JUNCTION_DM {
                break;
            }
            full.push(x);
            along += v.len_dm[(x >> 1) as usize];
        }
        let mut dm = base_dm;
        for k in 0..full.len() - ctx.len() {
            let n = ctx.len() + k + 1;
            if n >= 2 {
                self.turn_evals.set(self.turn_evals.get() + 1);
                if model.eval(v, &full[..n], dm, aux) {
                    self.turn_cut.set(self.turn_cut.get() + 1);
                    return false;
                }
            }
            dm += v.len_dm[(full[n - 1] >> 1) as usize];
        }
        true
    }
}

/// Les états qui précèdent `fo`, dans l'ordre du parcours, jusqu'à `need` dm derrière la fin de `f` (au plus
/// `MAX_STATES`) : `f` et ses prédécesseurs dans l'arbre, puis la fin de `lead.path`.
fn collect_before(
    out: &mut Vec<u32>,
    v: &ViewData,
    t: &Tree,
    lead: &Lead<'_>,
    f: u32,
    in_tree: bool,
    need: u32,
) {
    let mut acc = 0u32;
    let mut full = false;
    if in_tree {
        let mut cur = f;
        loop {
            out.push(cur);
            acc += v.len_dm[(cur >> 1) as usize];
            if acc >= need || out.len() >= MAX_STATES {
                full = true;
                break;
            }
            let p = t.pred(cur);
            if p == NONE {
                break;
            }
            cur = p;
        }
    }
    if !full {
        for &x in lead.path.iter().rev() {
            out.push(x);
            acc += v.len_dm[(x >> 1) as usize];
            if acc >= need || out.len() >= MAX_STATES {
                break;
            }
        }
    }
    out.reverse();
}

/// Les états qui suivent le nœud `src(f)` d'une recherche à rebours : `f`, ses successeurs dans l'arbre, puis le début de
/// `lead.path`, **retournés** (parcours inversé : le dernier état du parcours devient le premier).
fn collect_after(
    out: &mut Vec<u32>,
    v: &ViewData,
    t: &Tree,
    lead: &Lead<'_>,
    f: u32,
    in_tree: bool,
    need: u32,
) {
    let mut acc = 0u32;
    let mut full = false;
    if in_tree {
        let mut cur = f;
        loop {
            out.push(cur);
            acc += v.len_dm[(cur >> 1) as usize];
            if acc >= need || out.len() >= MAX_STATES {
                full = true;
                break;
            }
            let p = t.pred(cur);
            if p == NONE {
                break;
            }
            cur = p;
        }
    }
    if !full {
        for &x in lead.path {
            out.push(x);
            acc += v.len_dm[(x >> 1) as usize];
            if acc >= need || out.len() >= MAX_STATES {
                break;
            }
        }
    }
    // `out` est dans l'ordre du parcours (de `f` vers l'arrivée) : le parcours retourné le prend à l'envers, chaque état
    // dans l'autre sens.
    out.reverse();
    for x in out.iter_mut() {
        *x ^= 1;
    }
}

impl TurnModel {
    /// Le chemin `s` (états dans l'ordre du parcours, le dernier est le candidat `fo`) viole-t-il le contrôle des virages,
    /// aux nœuds que l'ajout de `fo` rend lisibles ? `b0_dm` : mètres du parcours au nœud d'arrivée de l'avant-dernier
    /// état ; `aux` : distance d'un nœud à l'arrivée du parcours (arbre de l'autre bout).
    ///
    /// Les paires `(a, b)` sont celles du contrôle : `b` est `b0` ou un nœud de degré ≥ 3 situé à moins de 50 m avant lui
    /// (sa fenêtre d'après déborde sur `fo`), `a` un nœud de degré ≥ 3 de `a = b` (virage simple, jamais exempté) à 100 m de
    /// chaînes entières avant `b` ; une grappe (`a ≠ b`) est exemptée si `b` est à 200 m ou moins du départ, ou `a` à 200 m
    /// ou moins de l'arrivée.
    pub fn eval(&self, v: &ViewData, s: &[u32], b0_dm: u32, aux: Option<&Tree>) -> bool {
        let n = s.len();
        if n < 2 {
            return false;
        }
        let fi = n - 2;
        let len = |k: usize| v.len_dm[(s[k] >> 1) as usize];
        let junction = |k: usize| v.deg[v.tgt(s[k]) as usize] >= 3;
        // Distance (dm) du nœud `ib` à `b0`.
        let mut to_b0 = 0u32;
        let mut ib = fi;
        loop {
            if junction(ib) {
                let mut hout: Option<V> = None;
                let mut run = 0u32;
                let mut ia = ib;
                loop {
                    if junction(ia) {
                        let exempt = ia < ib
                            && (b0_dm.saturating_sub(to_b0) <= self.free_end_dm
                                || aux.is_some_and(|t| {
                                    t.node_best(v.tgt(s[ia])).is_some_and(|b| t.meters(b) <= self.free_end_dm)
                                }));
                        if !exempt {
                            let vo = *hout.get_or_insert_with(|| vec_after(v, s, ib));
                            if self.turns_back(vec_before(v, s, ia), vo) {
                                return true;
                            }
                        }
                    }
                    if ia == 0 {
                        break;
                    }
                    run += len(ia);
                    if run > self.cluster_dm {
                        break;
                    }
                    ia -= 1;
                }
            }
            if ib == 0 {
                break;
            }
            to_b0 += len(ib);
            if to_b0 >= LONG_DM {
                break;
            }
            ib -= 1;
        }
        false
    }
}

/// Angle (degrés) que le modèle calcule pour la paire `(a, b)` : `a` est le nœud d'arrivée de `s[ia]`, `b` celui de
/// `s[ib]` (`ia ≤ ib`, `ib + 1 < s.len()`). Pour les tests.
#[cfg(test)]
pub(crate) fn pair_angle(v: &ViewData, s: &[u32], ia: usize, ib: usize) -> f64 {
    let (u, w) = (vec_before(v, s, ia), vec_after(v, s, ib));
    let (dot, cross) = (u.0 * w.0 + u.1 * w.1, u.0 * w.1 - u.1 * w.0);
    cross.abs().atan2(dot).to_degrees()
}

type V = (f64, f64);

impl TurnModel {
    /// L'angle entre les vecteurs `u` (cap avant) et `w` (cap après) atteint-il le seuil ? Produit scalaire négatif et
    /// carré au moins `cos² × |u|² × |w|²` : pas de `atan2`, pas de `fmod`.
    #[inline]
    fn turns_back(&self, u: V, w: V) -> bool {
        let dot = u.0 * w.0 + u.1 * w.1;
        dot < 0.0 && dot * dot >= self.cos2 * (u.0 * u.0 + u.1 * u.1) * (w.0 * w.0 + w.1 * w.1)
    }

    /// Virage simple en `tgt(f)` quand `f` fait plus de 100 m : cap avant celui de `f` (sa fenêtre de 25 à 50 m est droite le
    /// long de son cap de bout), cap après celui de la corde de `fo` (longueur plus petite que 50 m).
    fn simple_turn(&self, v: &ViewData, f: u32, fo: u32) -> bool {
        let loc = Local::new(v.pos[v.tgt(f) as usize]);
        let (a, b) = (loc.xy(v.pos[v.src(fo) as usize]), loc.xy(v.pos[v.tgt(fo) as usize]));
        self.turns_back(unit(in_head(v, f)), (b.0 - a.0, b.1 - a.1))
    }

    /// Le même, à rebours : `f` (après le nœud `src(f)`) fait plus de 100 m, `fg` le précède.
    fn simple_turn_back(&self, v: &ViewData, fg: u32, f: u32) -> bool {
        let loc = Local::new(v.pos[v.src(f) as usize]);
        let (a, b) = (loc.xy(v.pos[v.src(fg) as usize]), loc.xy(v.pos[v.tgt(fg) as usize]));
        // Cap avant : celui de la corde de `fg` ; cap après : celui de `f` au départ.
        self.turns_back((b.0 - a.0, b.1 - a.1), unit(out_head(v, f)))
    }
}

/// Vecteurs unitaires des 65 536 caps stockés (f32 : 512 Ko, relus plus vite qu'un `sincos`).
fn unit_table() -> &'static [[f32; 2]] {
    static T: OnceLock<Vec<[f32; 2]>> = OnceLock::new();
    T.get_or_init(|| {
        (0..65_536u32)
            .map(|h| {
                let r = f64::from(h) * (std::f64::consts::TAU / 65_536.0);
                [r.sin() as f32, r.cos() as f32]
            })
            .collect()
    })
}

#[inline]
fn unit(h: u16) -> V {
    let [x, y] = unit_table()[usize::from(h)];
    (f64::from(x), f64::from(y))
}

#[inline]
fn norm(d: V) -> f64 {
    (d.0 * d.0 + d.1 * d.1).sqrt()
}

/// Le segment d'un état vu **en remontant** depuis son nœud d'arrivée `to` : `(début, longueur)`. Une chaîne de 50 m ou plus
/// est droite le long de son cap d'arrivée, une plus courte est la corde entre ses nœuds (`from`, déjà dans le repère).
#[inline]
fn seg_back(v: &ViewData, f: u32, to: V, src_xy: impl FnOnce() -> V) -> (V, f64) {
    let l = v.len_dm[(f >> 1) as usize];
    if l >= LONG_DM {
        let len = f64::from(l) / 10.0;
        let u = unit(in_head(v, f));
        ((to.0 - len * u.0, to.1 - len * u.1), len)
    } else {
        let from = src_xy();
        (from, norm((to.0 - from.0, to.1 - from.1)))
    }
}

/// Points situés à `d1` puis `d2 ≥ d1` mètres avant le nœud d'arrivée de `s[ia]` (origine du repère `loc`) ; au-delà du
/// chemin connu, le dernier segment est prolongé.
fn points_back(v: &ViewData, loc: &Local, s: &[u32], ia: usize, d1: f64, d2: f64) -> (V, V) {
    let mut acc = 0.0;
    let mut to: V = (0.0, 0.0);
    let mut last: Option<(V, V)> = None;
    let (mut p1, mut p2): (Option<V>, Option<V>) = (None, None);
    for k in (0..=ia).rev() {
        let f = s[k];
        let (from, len) = seg_back(v, f, to, || loc.xy(v.pos[v.src(f) as usize]));
        if len > 0.0 {
            let at = |d: f64| {
                let t = (d - acc) / len;
                (to.0 + t * (from.0 - to.0), to.1 + t * (from.1 - to.1))
            };
            if p1.is_none() && d1 <= acc + len {
                p1 = Some(at(d1));
            }
            if d2 <= acc + len {
                p2 = Some(at(d2));
                break;
            }
        }
        acc += len;
        last = Some((from, to));
        // Le nœud de départ de cet état est celui d'arrivée du précédent ; après une chaîne longue, `from` est un point
        // extrapolé, pas le nœud : on relit le nœud.
        to = if v.len_dm[(f >> 1) as usize] >= LONG_DM { loc.xy(v.pos[v.src(f) as usize]) } else { from };
    }
    let extend = |d: f64| match last {
        Some((from, end)) => {
            let (dx, dy) = (from.0 - end.0, from.1 - end.1);
            let n = norm((dx, dy));
            if n < 1e-9 {
                from
            } else {
                (from.0 + dx / n * (d - acc), from.1 + dy / n * (d - acc))
            }
        }
        None => (0.0, 0.0),
    };
    (p1.unwrap_or_else(|| extend(d1)), p2.unwrap_or_else(|| extend(d2)))
}

/// Points situés à `d1` puis `d2 ≥ d1` mètres après le nœud d'arrivée de `s[ib]`.
fn points_fwd(v: &ViewData, loc: &Local, s: &[u32], ib: usize, d1: f64, d2: f64) -> (V, V) {
    let mut acc = 0.0;
    let mut from: V = (0.0, 0.0);
    let mut last: Option<(V, V)> = None;
    let (mut p1, mut p2): (Option<V>, Option<V>) = (None, None);
    for &f in &s[ib + 1..] {
        let l = v.len_dm[(f >> 1) as usize];
        let (to, len) = if l >= LONG_DM {
            let len = f64::from(l) / 10.0;
            let u = unit(out_head(v, f));
            ((from.0 + len * u.0, from.1 + len * u.1), len)
        } else {
            let to = loc.xy(v.pos[v.tgt(f) as usize]);
            (to, norm((to.0 - from.0, to.1 - from.1)))
        };
        if len > 0.0 {
            let at = |d: f64| {
                let t = (d - acc) / len;
                (from.0 + t * (to.0 - from.0), from.1 + t * (to.1 - from.1))
            };
            if p1.is_none() && d1 <= acc + len {
                p1 = Some(at(d1));
            }
            if d2 <= acc + len {
                p2 = Some(at(d2));
                break;
            }
        }
        acc += len;
        last = Some((from, to));
        from = if l >= LONG_DM { loc.xy(v.pos[v.tgt(f) as usize]) } else { to };
    }
    let extend = |d: f64| match last {
        Some((start, end)) => {
            let (dx, dy) = (end.0 - start.0, end.1 - start.1);
            let n = norm((dx, dy));
            if n < 1e-9 {
                end
            } else {
                (end.0 + dx / n * (d - acc), end.1 + dy / n * (d - acc))
            }
        }
        None => (0.0, 0.0),
    };
    (p1.unwrap_or_else(|| extend(d1)), p2.unwrap_or_else(|| extend(d2)))
}

/// Vecteur du cap avant le nœud d'arrivée de `s[ia]` : du point à 50 m avant à celui à 25 m avant, dans un repère centré au
/// nœud.
fn vec_before(v: &ViewData, s: &[u32], ia: usize) -> V {
    let loc = Local::new(v.pos[v.tgt(s[ia]) as usize]);
    let (near, far) = points_back(v, &loc, s, ia, NEAR_M, HEADING_FAR_M);
    (near.0 - far.0, near.1 - far.1)
}

/// Vecteur du cap après le nœud d'arrivée de `s[ib]` : du point à 25 m après à celui à 50 m après.
fn vec_after(v: &ViewData, s: &[u32], ib: usize) -> V {
    let loc = Local::new(v.pos[v.tgt(s[ib]) as usize]);
    let (near, far) = points_fwd(v, &loc, s, ib, NEAR_M, HEADING_FAR_M);
    (far.0 - near.0, far.1 - near.1)
}
