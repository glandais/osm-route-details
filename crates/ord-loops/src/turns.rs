//! Les virages et les allers-retours de la recherche organique (lot 4, 4c, §16.2 et §16.4 de
//! `docs/design/technique/ord-loops-lot4.md`).
//!
//! Le contrôle des garanties refuse un virage de 150° ou plus à un nœud de degré 3 ou plus, seul ou sur une **grappe** de
//! nœuds distants de 100 m au plus le long du parcours. Les recherches ne voient qu'un nœud à la fois : elles produisaient
//! des triangles refusés après coup. Le [`TurnModel`] refait ce contrôle **à la relaxation**, sur le chemin courant, avec
//! des caps pris sur un chemin approché (les mêmes fenêtres de 30 à 50 m que le contrôle, sur les chaînes droites).
//!
//! Tout est en `f64`, sans table ni état caché : même résultat en natif et en WASM, mêmes sorties à 1 et 16 fils.

use std::cell::{Cell, RefCell};

use ord_graph::view::{HEADING_FAR_M, HEADING_NEAR_M};

/// Les deux bouts de la fenêtre de cap : le contrôle prend `near = min(30, far / 2)`, donc **25 m** quand le chemin connu
/// atteint les 50 m (la note dit « 30 à 50 m » : écart au §16.2, consigné au §16.13).
const NEAR_M: f64 = if HEADING_NEAR_M < HEADING_FAR_M / 2.0 { HEADING_NEAR_M } else { HEADING_FAR_M / 2.0 };

use crate::engine::ViewData;
use crate::geo::{heading_deg, turn_angle_deg, Local};
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

/// Le modèle : seuil du refus (150° moins la marge), grappe et bouts libres du contrôle, en dm.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TurnModel {
    max_deg: f64,
    /// Seuil de la voie rapide, sur l'angle `u16` des caps stockés (360/65536 degrés).
    max16: u32,
    cluster_dm: u32,
    free_end_dm: u32,
}

impl TurnModel {
    pub fn new(max_turn_deg: f64, margin_deg: u8) -> Self {
        let max_deg = max_turn_deg - f64::from(margin_deg);
        Self {
            max_deg,
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
        self.turn_evals.set(self.turn_evals.get() + 1);
        let mut s = self.scratch.borrow_mut();
        s.clear();
        collect_before(&mut s, v, t, lead, f, in_tree, if lf > CLUSTER_DM_FAST { LONG_DM } else { NEED_DM });
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
        self.turn_evals.set(self.turn_evals.get() + 1);
        let mut s = self.scratch.borrow_mut();
        s.clear();
        collect_after(&mut s, v, t, lead, f, in_tree, if lf > CLUSTER_DM_FAST { LONG_DM } else { NEED_DM });
        s.push(fg ^ 1);
        let cut = model.eval(v, &s, lead.dm + m, lead.aux);
        if cut {
            self.turn_cut.set(self.turn_cut.get() + 1);
        }
        cut
    }

    /// Jonction d'un chemin d'arbre posé après des jambes déjà fixées (§16.2) : le modèle rejoué sur chaque état de `ret`
    /// dont le début est à moins de 160 m du nœud de jonction, avec `ctx` (les jambes) pour contexte. `base_dm` : mètres du
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
            if along >= 1_600 {
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
                let mut hout: Option<f64> = None;
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
                            let ho = *hout.get_or_insert_with(|| head_after(v, s, ib));
                            if turn_angle_deg(head_before(v, s, ia), ho) >= self.max_deg {
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
    turn_angle_deg(head_before(v, s, ia), head_after(v, s, ib))
}

fn unit(h: u16) -> (f64, f64) {
    let r = f64::from(h) * (std::f64::consts::TAU / 65_536.0);
    (r.sin(), r.cos())
}

/// Le segment d'un état vu **en remontant** depuis son nœud d'arrivée : `(début, arrivée, longueur)`. Une chaîne de 50 m ou
/// plus est droite le long de son cap d'arrivée, une plus courte est la corde entre ses nœuds.
fn seg_back(v: &ViewData, loc: &Local, f: u32) -> ((f64, f64), (f64, f64), f64) {
    let to = loc.xy(v.pos[v.tgt(f) as usize]);
    let l = v.len_dm[(f >> 1) as usize];
    if l >= LONG_DM {
        let len = f64::from(l) / 10.0;
        let u = unit(in_head(v, f));
        ((to.0 - len * u.0, to.1 - len * u.1), to, len)
    } else {
        let from = loc.xy(v.pos[v.src(f) as usize]);
        (from, to, (to.0 - from.0).hypot(to.1 - from.1))
    }
}

/// Le segment d'un état vu en avançant depuis son nœud de départ.
fn seg_fwd(v: &ViewData, loc: &Local, f: u32) -> ((f64, f64), (f64, f64), f64) {
    let from = loc.xy(v.pos[v.src(f) as usize]);
    let l = v.len_dm[(f >> 1) as usize];
    if l >= LONG_DM {
        let len = f64::from(l) / 10.0;
        let u = unit(out_head(v, f));
        (from, (from.0 + len * u.0, from.1 + len * u.1), len)
    } else {
        let to = loc.xy(v.pos[v.tgt(f) as usize]);
        (from, to, (to.0 - from.0).hypot(to.1 - from.1))
    }
}

/// Point situé à `d` mètres avant le nœud d'arrivée de `s[ia]`, dans le repère `loc` ; au-delà du chemin connu, le dernier
/// segment est prolongé.
fn pt_back(v: &ViewData, loc: &Local, s: &[u32], ia: usize, d: f64) -> (f64, f64) {
    let mut acc = 0.0;
    let mut last: Option<((f64, f64), (f64, f64))> = None;
    for k in (0..=ia).rev() {
        let (from, to, len) = seg_back(v, loc, s[k]);
        if len > 0.0 && d <= acc + len {
            let t = (d - acc) / len;
            return (to.0 + t * (from.0 - to.0), to.1 + t * (from.1 - to.1));
        }
        acc += len;
        last = Some((from, to));
    }
    match last {
        Some((from, to)) => {
            let (dx, dy) = (from.0 - to.0, from.1 - to.1);
            let norm = dx.hypot(dy);
            if norm < 1e-9 {
                from
            } else {
                (from.0 + dx / norm * (d - acc), from.1 + dy / norm * (d - acc))
            }
        }
        None => (0.0, 0.0),
    }
}

/// Point situé à `d` mètres après le nœud d'arrivée de `s[ib]`.
fn pt_fwd(v: &ViewData, loc: &Local, s: &[u32], ib: usize, d: f64) -> (f64, f64) {
    let mut acc = 0.0;
    let mut last: Option<((f64, f64), (f64, f64))> = None;
    for &f in &s[ib + 1..] {
        let (from, to, len) = seg_fwd(v, loc, f);
        if len > 0.0 && d <= acc + len {
            let t = (d - acc) / len;
            return (from.0 + t * (to.0 - from.0), from.1 + t * (to.1 - from.1));
        }
        acc += len;
        last = Some((from, to));
    }
    match last {
        Some((from, to)) => {
            let (dx, dy) = (to.0 - from.0, to.1 - from.1);
            let norm = dx.hypot(dy);
            if norm < 1e-9 {
                to
            } else {
                (to.0 + dx / norm * (d - acc), to.1 + dy / norm * (d - acc))
            }
        }
        None => (0.0, 0.0),
    }
}

/// Cap avant le nœud d'arrivée de `s[ia]` : des points à 50 m et 25 m avant, dans un repère centré au nœud.
fn head_before(v: &ViewData, s: &[u32], ia: usize) -> f64 {
    let loc = Local::new(v.pos[v.tgt(s[ia]) as usize]);
    let far = pt_back(v, &loc, s, ia, HEADING_FAR_M);
    let near = pt_back(v, &loc, s, ia, NEAR_M);
    heading_deg(near.0 - far.0, near.1 - far.1)
}

/// Cap après le nœud d'arrivée de `s[ib]` : des points à 25 m et 50 m après, dans un repère centré au nœud.
fn head_after(v: &ViewData, s: &[u32], ib: usize) -> f64 {
    let loc = Local::new(v.pos[v.tgt(s[ib]) as usize]);
    let near = pt_fwd(v, &loc, s, ib, NEAR_M);
    let far = pt_fwd(v, &loc, s, ib, HEADING_FAR_M);
    heading_deg(far.0 - near.0, far.1 - near.1)
}
