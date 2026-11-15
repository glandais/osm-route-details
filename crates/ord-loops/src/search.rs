//! Les recherches (note lot 2, §4 et §5.1) : arbres aller et retour sur les arêtes de vue orientées avec la règle
//! de virage, puis A* ciblés d'heuristique « arbre retour ». Les états sont des **arêtes de vue orientées**
//! (`champ = arête << 1 | sens`, sens 0 = de `a` vers `b`), étiquetés d'un coût entier et de mètres exacts.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use ord_graph::format::DIR_BIT;

use crate::cost::{self, RideType};
use crate::engine::{BaseData, Engine, Snap, ViewData};
use crate::geo::{heading_deg, polyline_m, Local};
use crate::team::{ChainTeam, TeamData, TypeTeam, BRIDGED, TEAM};
use crate::verify::slice;
use crate::Mode;
use ord_graph::view::{heading_to_u16, polyline_heading};

/// Transitions `(état d'arrivée, état de départ)` que le contrôle a refusées : interdites aux recherches suivantes.
pub(crate) type Forbidden = rustc_hash::FxHashSet<(u32, u32)>;

pub(crate) const NONE: u32 = u32::MAX;
pub(crate) const INF: u32 = u32::MAX;

/// Coupé par l'appelant.
#[derive(Debug)]
pub(crate) struct Stopped;

#[derive(Clone, Copy, Default, Debug)]
struct Slot {
    stamp: u32,
    d: u32,
    pred: u32,
    m: u32,
}

/// Un arbre de plus courts chemins sur les états orientés, remis à zéro par estampille.
#[derive(Default, Debug)]
pub(crate) struct Tree {
    slots: Vec<Slot>,
    cur: u32,
    nstamp: Vec<u32>,
    nbest: Vec<u32>,
    /// Nœuds atteints, dans l'ordre de première fixation.
    pub reached: Vec<u32>,
    /// États fixés, dans l'ordre.
    pub settled: Vec<u32>,
}

impl Tree {
    pub fn begin(&mut self, n_fields: usize, n_nodes: usize) {
        if self.slots.len() != n_fields {
            self.slots = vec![Slot::default(); n_fields];
            self.nstamp = vec![0; n_nodes];
            self.nbest = vec![NONE; n_nodes];
            self.cur = 0;
        }
        self.cur += 1;
        self.reached.clear();
        self.settled.clear();
    }

    #[inline]
    pub fn get(&self, f: u32) -> u32 {
        let s = &self.slots[f as usize];
        if s.stamp == self.cur {
            s.d
        } else {
            INF
        }
    }

    #[inline]
    pub fn pred(&self, f: u32) -> u32 {
        self.slots[f as usize].pred
    }

    #[inline]
    pub fn meters(&self, f: u32) -> u32 {
        self.slots[f as usize].m
    }

    #[inline]
    pub(crate) fn set(&mut self, f: u32, d: u32, pred: u32, m: u32) {
        self.slots[f as usize] = Slot { stamp: self.cur, d, pred, m };
    }

    #[inline]
    fn note_node(&mut self, v: u32, f: u32) {
        if self.nstamp[v as usize] != self.cur {
            self.nstamp[v as usize] = self.cur;
            self.nbest[v as usize] = f;
            self.reached.push(v);
        }
    }

    /// Meilleur état qui arrive (aller) ou part (retour) du nœud.
    #[inline]
    pub fn node_best(&self, v: u32) -> Option<u32> {
        (self.nstamp[v as usize] == self.cur).then(|| self.nbest[v as usize])
    }

    /// Chemin de départ à `f` : les états dans l'ordre du parcours (suit les prédécesseurs).
    pub fn path_to(&self, f: u32) -> Vec<u32> {
        let mut out = vec![f];
        let mut cur = f;
        while self.pred(cur) != NONE {
            cur = self.pred(cur);
            out.push(cur);
        }
        out.reverse();
        out
    }
}

/// Espace de travail d'un fil : états des recherches par vue, alloués au premier usage.
#[derive(Default, Debug)]
pub struct Workspace {
    pub(crate) views: [ViewWs; 2],
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Default, Debug)]
pub(crate) struct ViewWs {
    pub t1: Tree,
    pub t2: Tree,
    pub ta: Tree,
    /// Interdits, queues et couloir du retour, par arête de vue : `génération << 3 | bits`.
    pub ban: Vec<u32>,
    pub ban_gen: u32,
}

pub(crate) const BAN: u32 = 1;
pub(crate) const TAIL: u32 = 2;
pub(crate) const COR: u32 = 4;

// --- Poids ---------------------------------------------------------------------------------------------------

/// Poids des arêtes pour une requête : type, mode, familiarité, équipe.
pub(crate) struct Weighter<'a> {
    ride: RideType,
    mode: Mode,
    w_osm: &'a [u32],
    ct: Option<&'a [ChainTeam]>,
    tt: Option<&'a TypeTeam>,
    lambda: u64,
    mt: [u32; 256],
}

impl<'a> Weighter<'a> {
    pub fn new<B: AsRef<[u8]>>(
        e: &'a Engine<B>,
        team: Option<&'a TeamData>,
        ride: RideType,
        mode: Mode,
        familiarity: f32,
    ) -> Self {
        let v = e.view(ride);
        let tt = match (mode, team) {
            (Mode::Discovery, _) | (_, None) => None,
            (_, Some(t)) => Some(t.of(ride)),
        };
        let fam = (familiarity.clamp(0.0, 1.0) * 1000.0).round() as u32;
        Self {
            ride,
            mode,
            w_osm: &v.w_osm[ride.index()],
            ct: tt.map(|t| t.chains.as_slice()),
            tt,
            lambda: u64::from(e.params.lambda),
            mt: cost::multiplier_table(fam),
        }
    }

    #[inline]
    fn mul(&self, a: u32, z: u8) -> u64 {
        (u64::from(a) * u64::from(self.mt[z as usize]) + 128) >> 8
    }

    /// Poids d'une arête de vue, `INF` si elle est interdite (strict, hors réseau d'équipe).
    #[inline]
    pub fn chain(&self, c: u32) -> u32 {
        let w = self.w_osm[c as usize];
        let Some(ct) = self.ct else { return w };
        let t = ct[c as usize];
        let rest = u64::from(w.saturating_sub(t.a).saturating_sub(t.b));
        match self.mode {
            Mode::Strict => {
                if t.strict {
                    (self.mul(t.a, t.z) + u64::from(t.b)).min(u64::from(u32::MAX / 2)) as u32
                } else {
                    INF
                }
            }
            _ => {
                (self.lambda * rest + self.mul(t.a, t.z) + u64::from(t.b)).min(u64::from(u32::MAX / 2)) as u32
            }
        }
    }

    /// Poids d'une arête de base (pour les morceaux partiels du départ et de l'arrivée).
    pub fn edge(&self, bd: &BaseData, e: u32) -> u32 {
        let w = cost::edge_weight(self.ride, bd.attrs[e as usize], bd.len_dm[e as usize]);
        let Some(tt) = self.tt else { return w };
        let (f, z) = tt.flags(e);
        if f & TEAM != 0 {
            self.mul(w, z).min(u64::from(u32::MAX / 2)) as u32
        } else if f & BRIDGED != 0 {
            w
        } else if self.mode == Mode::Strict {
            INF
        } else {
            (self.lambda * u64::from(w)).min(u64::from(u32::MAX / 2)) as u32
        }
    }

    pub fn strict(&self) -> bool {
        self.mode == Mode::Strict
    }

    pub fn tt(&self) -> Option<&TypeTeam> {
        self.tt
    }
}

// --- Règle de virage -------------------------------------------------------------------------------------------

const TURN_MAX16: u32 = 27_307; // ceil(150 / 180 * 32768)
const TURN_FREE16: u32 = 8_192; // 45°
const TURN_COST_MAX: u32 = 2_000; // 200 m équivalents (en décimètres pondérés), à 150°

#[inline]
fn in_head(v: &ViewData, f: u32) -> u16 {
    let h = v.head[(f >> 1) as usize];
    if f & 1 == 0 {
        h[1]
    } else {
        h[0].wrapping_add(32_768)
    }
}

#[inline]
fn out_head(v: &ViewData, f: u32) -> u16 {
    let h = v.head[(f >> 1) as usize];
    if f & 1 == 0 {
        h[0]
    } else {
        h[1].wrapping_add(32_768)
    }
}

#[inline]
fn angle16(a: u16, b: u16) -> u32 {
    (i32::from(b.wrapping_sub(a) as i16)).unsigned_abs()
}

fn unit(h: u16) -> (f32, f32) {
    let r = f32::from(h) * (std::f32::consts::TAU / 65_536.0);
    (r.sin(), r.cos())
}

/// Cap d'arrivée de `f`, prolongé par le prédécesseur si la chaîne fait moins de 50 m (note §4).
#[inline]
pub(crate) fn in_head_ext(v: &ViewData, f: u32, pred: u32) -> u16 {
    let own = in_head(v, f);
    let l = v.len_dm[(f >> 1) as usize];
    if l >= 500 || pred == NONE {
        return own;
    }
    let (so, co) = unit(own);
    let (sp, cp) = unit(in_head(v, pred));
    let w = l as f32 / 500.0;
    let (x, y) = (w * so + (1.0 - w) * sp, w * co + (1.0 - w) * cp);
    let deg = x.atan2(y).to_degrees();
    ((deg.rem_euclid(360.0) / 360.0 * 65_536.0).round() as u32 & 0xffff) as u16
}

/// Contrôle **rétroactif** du virage qui précède une chaîne courte `f` (< 50 m), une fois son successeur `fo`
/// connu : la fenêtre de cap de départ de `f` déborde sur `fo`, que `turn_cost` ne peut pas voir.
#[inline]
pub(crate) fn retro_ok(v: &ViewData, pe: u32, f: u32, fo: u32) -> bool {
    if pe == NONE || v.len_dm[(f >> 1) as usize] >= 500 {
        return true;
    }
    let own = out_head(v, f);
    let (so, co) = unit(own);
    let (sn, cn) = unit(out_head(v, fo));
    let w = v.len_dm[(f >> 1) as usize] as f32 / 500.0;
    let (x, y) = (w * so + (1.0 - w) * sn, w * co + (1.0 - w) * cn);
    let deg = x.atan2(y).to_degrees();
    let ext = ((deg.rem_euclid(360.0) / 360.0 * 65_536.0).round() as u32 & 0xffff) as u16;
    !(angle16(in_head(v, pe), ext) >= TURN_MAX16 && v.deg[v.tgt(pe) as usize] >= 3)
}

/// Coût de virage de `fe` vers `fo`, `None` si interdit (demi-tour, ≥ 150° à un nœud de degré ≥ 3).
#[inline]
pub(crate) fn turn_cost(
    v: &ViewData,
    forb: &Forbidden,
    fe: u32,
    pred: u32,
    fo: u32,
    hin_over: Option<u16>,
    hout_over: Option<u16>,
) -> Option<u32> {
    if fo == fe ^ 1 || (!forb.is_empty() && forb.contains(&(fe, fo))) {
        return None;
    }
    let hin = hin_over.unwrap_or_else(|| in_head_ext(v, fe, pred));
    let hout = hout_over.unwrap_or_else(|| out_head(v, fo));
    let a = angle16(hin, hout);
    if a >= TURN_MAX16 && v.deg[v.tgt(fe) as usize] >= 3 {
        return None;
    }
    Some(if a > TURN_FREE16 {
        (((a - TURN_FREE16) as u64 * u64::from(TURN_COST_MAX) / u64::from(TURN_MAX16 - TURN_FREE16)) as u32)
            .min(TURN_COST_MAX + 500)
    } else {
        0
    })
}

// --- Accroche, disque -------------------------------------------------------------------------------------------

/// Position de départ ou d'arrivée sur une arête de vue, avec les coûts des morceaux partiels.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Anchor {
    pub snap: Snap,
    /// Départ : de la position vers `b` (sens 0) ou vers `a` (sens 1).
    pub out_cost: [u32; 2],
    pub out_len: [u32; 2],
    /// Arrivée : de `a` vers la position (sens 0) ou de `b` vers la position (sens 1).
    pub in_cost: [u32; 2],
    pub in_len: [u32; 2],
    /// Cap à l'arrivée du morceau partiel de départ (au bout `b` pour le sens 0, au bout `a` pour le sens 1).
    pub out_head: [u16; 2],
    /// Cap au départ du morceau partiel d'arrivée (depuis `a` pour le sens 0, depuis `b` pour le sens 1).
    pub in_head: [u16; 2],
}

type Points = Vec<(i32, i32)>;

fn extend_dedupe(acc: &mut Points, pts: Points) {
    let skip = usize::from(acc.last().is_some() && acc.last() == pts.first());
    acc.extend(pts.into_iter().skip(skip));
}

pub(crate) fn make_anchor<B: AsRef<[u8]>>(
    e: &Engine<B>,
    v: &ViewData,
    w: &Weighter<'_>,
    snap: Snap,
) -> Anchor {
    let links = v.links(snap.chain);
    let g = e.geom_len_m(snap.edge).max(1e-6);
    let frac = (snap.offset_m / g).clamp(0.0, 1.0);
    let fb = if snap.rev { 1.0 - frac } else { frac };
    let (mut b_cost, mut b_len, mut a_cost, mut a_len) = (0u64, 0u64, 0u64, 0u64);
    let (mut b_inf, mut a_inf) = (false, false);
    for (li, &l) in links.iter().enumerate() {
        let ed = l & !DIR_BIT;
        let ew = w.edge(&e.bd, ed);
        let len = u64::from(e.bd.len_dm[ed as usize]);
        let (fb_i, fa_i) = if li < snap.link {
            (1.0, 0.0)
        } else if li > snap.link {
            (0.0, 1.0)
        } else {
            (fb, 1.0 - fb)
        };
        let part = |f: f64| (f * f64::from(if ew == INF { 0 } else { ew })).round() as u64;
        if fb_i > 0.0 {
            b_inf |= ew == INF;
            b_cost += part(fb_i);
            b_len += (fb_i * len as f64).round() as u64;
        }
        if fa_i > 0.0 {
            a_inf |= ew == INF;
            a_cost += part(fa_i);
            a_len += (fa_i * len as f64).round() as u64;
        }
    }
    let cost = |x: u64, inf: bool, open: bool| {
        if inf || !open {
            INF
        } else {
            x.min(u64::from(u32::MAX / 2)) as u32
        }
    };
    let (o0, o1) = (v.open[snap.chain as usize] & 1 != 0, v.open[snap.chain as usize] & 2 != 0);
    // Caps des morceaux partiels, sur leur géométrie réelle.
    let (mut before, mut after) = (Points::new(), Points::new());
    for (li, &l) in links.iter().enumerate() {
        let (ed, rev) = (l & !DIR_BIT, l & DIR_BIT != 0);
        let mut nat = e.edge_polyline(ed);
        if li == snap.link {
            let total = polyline_m(&nat);
            let p0 = slice(&nat, 0.0, snap.offset_m);
            let p1 = slice(&nat, snap.offset_m, total);
            let (mut b_part, mut a_part) = (p0, p1);
            if rev {
                (b_part, a_part) = (a_part, b_part);
                b_part.reverse();
                a_part.reverse();
            }
            extend_dedupe(&mut before, b_part);
            extend_dedupe(&mut after, a_part);
        } else {
            if rev {
                nat.reverse();
            }
            if li < snap.link {
                extend_dedupe(&mut before, nat);
            } else {
                extend_dedupe(&mut after, nat);
            }
        }
    }
    let rev_of = |p: &Vec<(i32, i32)>| -> Vec<(i32, i32)> { p.iter().rev().copied().collect() };
    let hd = |p: &[(i32, i32)], start: bool| heading_to_u16(polyline_heading(p, start));
    let out_head = [hd(&after, false), hd(&rev_of(&before), false)];
    let in_head = [hd(&before, true), hd(&rev_of(&after), true)];
    Anchor {
        snap,
        out_cost: [cost(a_cost, a_inf, o0), cost(b_cost, b_inf, o1)],
        out_len: [a_len as u32, b_len as u32],
        in_cost: [cost(b_cost, b_inf, o0), cost(a_cost, a_inf, o1)],
        in_len: [b_len as u32, a_len as u32],
        out_head,
        in_head,
    }
}

/// Zone de recherche : disque de rayon D/2 autour du départ (boucle), ellipse de foyers départ et arrivée de
/// somme D (aller simple).
pub(crate) struct Disc {
    loc: Local,
    s: (f64, f64),
    e: (f64, f64),
    limit: f64,
    is_loop: bool,
}

impl Disc {
    pub fn new(s: (i32, i32), e: (i32, i32), d_m: f64, is_loop: bool) -> Self {
        let loc = Local::new(s);
        Self { loc, s: loc.xy(s), e: loc.xy(e), limit: if is_loop { d_m / 2.0 } else { d_m }, is_loop }
    }

    #[inline]
    pub fn inside(&self, p: (i32, i32)) -> bool {
        let (x, y) = self.loc.xy(p);
        let ds = (x - self.s.0).hypot(y - self.s.1);
        if self.is_loop {
            ds <= self.limit
        } else {
            ds + (x - self.e.0).hypot(y - self.e.1) <= self.limit
        }
    }

    /// Cap depuis le point de référence (départ, ou milieu départ-arrivée pour un aller simple), en degrés.
    pub fn bearing_of(&self, p: (i32, i32)) -> f64 {
        let (x, y) = self.loc.xy(p);
        let r =
            if self.is_loop { self.s } else { ((self.s.0 + self.e.0) / 2.0, (self.s.1 + self.e.1) / 2.0) };
        heading_deg(x - r.0, y - r.1)
    }
}

pub(crate) struct Ctx<'a> {
    pub v: &'a ViewData,
    pub w: &'a Weighter<'a>,
    pub disc: &'a Disc,
    pub s: &'a Anchor,
    pub e: &'a Anchor,
    pub forb: &'a Forbidden,
}

pub(crate) const POLL: u32 = 2048;

/// Arbre aller depuis le départ : états atteints, coût et mètres (dm) du meilleur chemin à chacun.
pub(crate) fn tree_forward(
    cx: &Ctx<'_>,
    t: &mut Tree,
    stop: &mut dyn FnMut() -> bool,
) -> Result<(), Stopped> {
    let v = cx.v;
    t.begin(2 * v.n_chains, v.n_nodes);
    let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
    for dir in 0..2u32 {
        let c = cx.s.out_cost[dir as usize];
        if c != INF {
            let f = (cx.s.snap.chain << 1) | dir;
            t.set(f, c, NONE, cx.s.out_len[dir as usize]);
            heap.push(Reverse((c, f)));
        }
    }
    let mut polls = 0u32;
    while let Some(Reverse((d, f))) = heap.pop() {
        if d > t.get(f) {
            continue;
        }
        t.settled.push(f);
        polls += 1;
        if polls.is_multiple_of(POLL) && stop() {
            return Err(Stopped);
        }
        let node = v.tgt(f);
        t.note_node(node, f);
        let (pred, m) = (t.pred(f), t.meters(f));
        for &fo in v.out(node) {
            let c = fo >> 1;
            let w = cx.w.chain(c);
            if w == INF || !cx.disc.inside(v.pos[v.tgt(fo) as usize]) {
                continue;
            }
            let hin_over = (pred == NONE).then(|| cx.s.out_head[(f & 1) as usize]);
            let Some(tc) = turn_cost(v, cx.forb, f, pred, fo, hin_over, None) else { continue };
            if !retro_ok(v, pred, f, fo) {
                continue;
            }
            let nd = d.saturating_add(w).saturating_add(tc);
            if nd < t.get(fo) {
                t.set(fo, nd, f, m + v.len_dm[c as usize]);
                heap.push(Reverse((nd, fo)));
            }
        }
    }
    Ok(())
}

/// Arbre retour vers l'arrivée : le coût du meilleur chemin **qui commence** par chaque état et finit à l'arrivée.
pub(crate) fn tree_backward(
    cx: &Ctx<'_>,
    t: &mut Tree,
    stop: &mut dyn FnMut() -> bool,
) -> Result<(), Stopped> {
    let v = cx.v;
    t.begin(2 * v.n_chains, v.n_nodes);
    let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
    for dir in 0..2u32 {
        let c = cx.e.in_cost[dir as usize];
        if c != INF {
            let f = (cx.e.snap.chain << 1) | dir;
            t.set(f, c, NONE, cx.e.in_len[dir as usize]);
            heap.push(Reverse((c, f)));
        }
    }
    let mut polls = 0u32;
    while let Some(Reverse((d, f))) = heap.pop() {
        if d > t.get(f) {
            continue;
        }
        t.settled.push(f);
        polls += 1;
        if polls.is_multiple_of(POLL) && stop() {
            return Err(Stopped);
        }
        let node = v.src(f);
        t.note_node(node, f);
        let m = t.meters(f);
        for &fg in v.inn(node) {
            let c = fg >> 1;
            let w = cx.w.chain(c);
            if w == INF || !cx.disc.inside(v.pos[v.src(fg) as usize]) {
                continue;
            }
            let hout_over = (t.pred(f) == NONE).then(|| cx.e.in_head[(f & 1) as usize]);
            let Some(tc) = turn_cost(v, cx.forb, fg, NONE, f, None, hout_over) else { continue };
            let nd = d.saturating_add(w).saturating_add(tc);
            if nd < t.get(fg) {
                t.set(fg, nd, f, m + v.len_dm[c as usize]);
                heap.push(Reverse((nd, fg)));
            }
        }
    }
    Ok(())
}

/// Ce que l'A* a trouvé : les états depuis le point de demi-tour jusqu'à l'arrivée (le dernier est partiel).
#[derive(Debug)]
pub(crate) struct Found {
    pub fields: Vec<u32>,
    pub meters: u32,
}

/// Interdits et pénalités du retour.
pub(crate) struct Ban<'a> {
    pub stamp: &'a [u32],
    pub gen: u32,
}

impl Ban<'_> {
    #[inline]
    pub(crate) fn bits(&self, c: u32) -> u32 {
        let s = self.stamp[c as usize];
        if s >> 3 == self.gen {
            s & 7
        } else {
            0
        }
    }
}

/// A* depuis le nœud `from_node` (atteint par l'état `last`, de prédécesseur `last_pred`) jusqu'à l'arrivée,
/// d'heuristique « arbre retour » `t2`. `budget` : états fixés restants, décomptés ; `None` si rien n'est trouvé ou
/// si le budget est épuisé (`exhausted` l'indique).
#[allow(clippy::too_many_arguments)]
pub(crate) fn astar(
    cx: &Ctx<'_>,
    ta: &mut Tree,
    t2: &Tree,
    last: u32,
    last_pred: u32,
    ban: &Ban<'_>,
    budget: &mut u64,
    exhausted: &mut bool,
    stop: &mut dyn FnMut() -> bool,
) -> Result<Option<Found>, Stopped> {
    let v = cx.v;
    ta.begin(2 * v.n_chains, v.n_nodes);
    let from_node = v.tgt(last);
    let Some(h0) = t2.node_best(from_node).map(|b| t2.get(b)) else { return Ok(None) };
    let cap = h0.saturating_mul(2).saturating_add(10_000);
    let end_chain = cx.e.snap.chain;
    let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    let expand = |ta: &mut Tree,
                  heap: &mut BinaryHeap<Reverse<(u32, u32, u32)>>,
                  node: u32,
                  f_prev: u32,
                  pred_prev: u32,
                  hin_over: Option<u16>,
                  store: u32,
                  g: u32,
                  m: u32| {
        for &fo in v.out(node) {
            let c = fo >> 1;
            let bits = ban.bits(c);
            let terminal = c == end_chain;
            if !terminal && bits & BAN != 0 {
                continue;
            }
            let hout_over = terminal.then(|| cx.e.in_head[(fo & 1) as usize]);
            let Some(tc) = turn_cost(v, cx.forb, f_prev, pred_prev, fo, hin_over, hout_over) else {
                continue;
            };
            if !retro_ok(v, pred_prev, f_prev, fo) {
                continue;
            }
            let (w, len, h) = if terminal {
                let dir = (fo & 1) as usize;
                let w = cx.e.in_cost[dir];
                if w == INF {
                    continue;
                }
                (w, cx.e.in_len[dir], 0)
            } else {
                let mut w = cx.w.chain(c);
                if w == INF {
                    continue;
                }
                let tn = v.tgt(fo);
                if !cx.disc.inside(v.pos[tn as usize]) {
                    continue;
                }
                let Some(hb) = t2.node_best(tn) else { continue };
                if bits & TAIL != 0 {
                    w = w.saturating_mul(2);
                }
                if bits & COR != 0 {
                    w = w.saturating_mul(3);
                }
                (w, v.len_dm[c as usize], t2.get(hb))
            };
            let ng = g.saturating_add(w).saturating_add(tc);
            if ng < ta.get(fo) {
                ta.set(fo, ng, store, m + len);
                heap.push(Reverse((ng.saturating_add(h), ng, fo)));
            }
        }
    };
    // Le premier pas part du nœud du demi-tour, avec le virage depuis le dernier état de l'aller.
    let first_over = (last_pred == NONE).then(|| cx.s.out_head[(last & 1) as usize]);
    expand(ta, &mut heap, from_node, last, last_pred, first_over, NONE, 0, 0);
    let mut polls = 0u32;
    while let Some(Reverse((key, g, f))) = heap.pop() {
        if key > cap {
            break;
        }
        if g > ta.get(f) {
            continue;
        }
        if *budget == 0 {
            *exhausted = true;
            return Ok(None);
        }
        *budget -= 1;
        ta.settled.push(f);
        polls += 1;
        if polls.is_multiple_of(POLL) && stop() {
            return Err(Stopped);
        }
        if (f >> 1) == end_chain {
            let mut fields = vec![f];
            let mut cur = f;
            while ta.pred(cur) != NONE {
                cur = ta.pred(cur);
                fields.push(cur);
            }
            fields.reverse();
            return Ok(Some(Found { fields, meters: ta.meters(f) }));
        }
        let node = v.tgt(f);
        let (pred, m) = (ta.pred(f), ta.meters(f));
        expand(ta, &mut heap, node, f, pred, None, f, g, m);
    }
    Ok(None)
}
