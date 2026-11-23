//! Contrôle interne des garanties (note lot 2, §6). Le moteur ne rend que des candidats qui le passent.
//!
//! Le contrôleur **indépendant** (`ord-loops-check`) est écrit par une autre personne depuis le §6 seul ; celui-ci en
//! est la lecture du moteur, sur les tableaux du moteur, et c'est lui qui juge chaque candidat avant de le rendre.
//! Toutes les longueurs sont celles du §6 : `len_dm / 10` pour un morceau entier, abscisse haversine pour un morceau
//! partiel ; les caps sont pris dans un repère équirectangulaire local sur 30 à 50 m **le long du parcours**.

use std::collections::{BTreeMap, BTreeSet};

use crate::cost::presumed_surface;
use crate::engine::Engine;
use crate::geo::{haversine_m, heading_deg, turn_angle_deg, Local};
use crate::team::{TeamData, BRIDGED, TEAM};
use crate::window::Window;
use crate::{Mode, Request, Route};

/// Codes de violation, ceux du §6.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Violation {
    Empty,
    NotOnGraph,
    Discontinuous,
    OneWay,
    NotAdmitted,
    WrongStart,
    WrongEnd,
    OffTeam,
    UTurn,
    SharpTurn,
    DeadEnd,
    EdgeUsedMoreThanTwice,
    ReuseOutsideTail,
    TailTooLong,
    TooManyCrossings,
    CorridorDoubled,
    DistanceOutOfTolerance,
}

impl Violation {
    pub fn code(self) -> &'static str {
        match self {
            Self::Empty => "EMPTY",
            Self::NotOnGraph => "NOT_ON_GRAPH",
            Self::Discontinuous => "DISCONTINUOUS",
            Self::OneWay => "ONE_WAY",
            Self::NotAdmitted => "NOT_ADMITTED",
            Self::WrongStart => "WRONG_START",
            Self::WrongEnd => "WRONG_END",
            Self::OffTeam => "OFF_TEAM",
            Self::UTurn => "U_TURN",
            Self::SharpTurn => "SHARP_TURN",
            Self::DeadEnd => "DEAD_END",
            Self::EdgeUsedMoreThanTwice => "EDGE_USED_MORE_THAN_TWICE",
            Self::ReuseOutsideTail => "REUSE_OUTSIDE_TAIL",
            Self::TailTooLong => "TAIL_TOO_LONG",
            Self::TooManyCrossings => "TOO_MANY_CROSSINGS",
            Self::CorridorDoubled => "CORRIDOR_DOUBLED",
            Self::DistanceOutOfTolerance => "DISTANCE_OUT_OF_TOLERANCE",
        }
    }
}

/// Résultat du contrôle : les violations (vide = parcours valide) et les mesures.
#[derive(Clone, Debug, Default)]
pub struct Check {
    pub violations: Vec<Violation>,
    pub length_m: f64,
    pub shared_m: f64,
    pub crossings: u32,
    pub team_m: f64,
    pub bridged_m: f64,
    pub unpaved_m: f64,
    /// Longueur non revêtue au sens route-details (`Attrs::surface_rd`, lot 5, §2.2), à côté de `unpaved_m` (goût).
    pub offroad_m: f64,
    pub presumed_surface_m: f64,
    /// Indices `i` des nœuds (entre le morceau `i` et le suivant) où un virage trop serré a été relevé.
    pub sharp_at: Vec<usize>,
}

pub(crate) const CLUSTER_M: f64 = 100.0;
/// La règle des grappes est levée quand la grappe tient dans les 200 premiers ou les 200 derniers mètres du
/// parcours : en quittant son départ, faire le tour d'un pâté de maisons n'est pas un retour (Java,
/// `RouteGuarantees.CLUSTER_FREE_ENDS_M`, relevé en production le 15 novembre 2026).
pub(crate) const CLUSTER_FREE_ENDS_M: f64 = 200.0;
const HEAD_NEAR_M: f64 = 30.0;
const HEAD_FAR_M: f64 = 50.0;
const CONNECTOR_MIN_M: f64 = 15.0;
const CROSSING_MIN_M: f64 = 30.0;
const RUN_BEND_DEG: f64 = 20.0;
const CONNECTOR_TOL_DEG: f64 = 25.0;
const RUN_MIN_PIECE_M: f64 = 1.0;
const CORRIDOR_STEP_M: f64 = 10.0;
const CORRIDOR_DIST_M: f64 = 25.0;
const CORRIDOR_GAP_M: f64 = 400.0;
const CORRIDOR_END_M: f64 = 30.0;
const CORRIDOR_MAX: f64 = 0.20;

struct Piece {
    edge: u32,
    rev: bool,
    /// Polyligne dans le sens du parcours, rognée pour le premier et le dernier morceau.
    pts: Vec<(i32, i32)>,
    len: f64,
    /// Intervalle couvert sur l'arête, en abscisse naturelle (de `a`).
    lo: f64,
    hi: f64,
    to: u32,
}

fn lerp(a: (i32, i32), b: (i32, i32), t: f64) -> (i32, i32) {
    (a.0 + (f64::from(b.0 - a.0) * t).round() as i32, a.1 + (f64::from(b.1 - a.1) * t).round() as i32)
}

/// Partie de la polyligne entre les abscisses `lo` et `hi` (haversine).
pub(crate) fn slice(pts: &[(i32, i32)], lo: f64, hi: f64) -> Vec<(i32, i32)> {
    let mut out: Vec<(i32, i32)> = Vec::new();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let l = haversine_m(w[0], w[1]);
        let (s, e) = (acc, acc + l);
        acc = e;
        if e < lo || s > hi {
            continue;
        }
        let ta = if l > 0.0 { ((lo - s) / l).clamp(0.0, 1.0) } else { 0.0 };
        let tb = if l > 0.0 { ((hi - s) / l).clamp(0.0, 1.0) } else { 1.0 };
        let (pa, pb) = (lerp(w[0], w[1], ta), lerp(w[0], w[1], tb));
        if out.last() != Some(&pa) {
            out.push(pa);
        }
        out.push(pb);
    }
    if out.is_empty() {
        let p = lerp(pts[0], pts[pts.len() - 1], if acc > 0.0 { (lo / acc).clamp(0.0, 1.0) } else { 0.0 });
        out.push(p);
        out.push(p);
    }
    out
}

/// Morceau `i` du parcours : polyligne naturelle rognée au départ et à l'arrivée, puis orientée dans le sens du
/// parcours. Renvoie aussi l'intervalle couvert, l'arête entière ou non, et la longueur de la géométrie.
pub(crate) fn trim_piece(route: &Route, i: usize, nat: Vec<(i32, i32)>) -> (Vec<(i32, i32)>, f64, f64, bool) {
    let n = route.steps.len();
    let st = route.steps[i];
    let g: f64 = nat.windows(2).map(|w| haversine_m(w[0], w[1])).sum();
    let (mut lo, mut hi) = (0.0, g);
    let (os, oe) = (route.start.offset_m.clamp(0.0, g), route.end.offset_m.clamp(0.0, g));
    if n == 1 {
        (lo, hi) = (os.min(oe), os.max(oe));
    } else if i == 0 {
        if st.reverse {
            hi = os;
        } else {
            lo = os;
        }
    } else if i == n - 1 {
        if st.reverse {
            lo = oe;
        } else {
            hi = oe;
        }
    }
    let whole = lo <= 0.0 && hi >= g;
    let mut pts = if whole { nat } else { slice(&nat, lo, hi) };
    if st.reverse {
        pts.reverse();
    }
    (pts, lo, hi, whole)
}

fn build_pieces<B: AsRef<[u8]>>(e: &Engine<B>, route: &Route) -> Vec<Piece> {
    let mut pieces = Vec::with_capacity(route.steps.len());
    for (i, st) in route.steps.iter().enumerate() {
        let (pts, lo, hi, whole) = trim_piece(route, i, e.edge_polyline(st.base_edge));
        let len = if whole {
            f64::from(e.len_dm(st.base_edge)) / 10.0
        } else {
            pts.windows(2).map(|w| haversine_m(w[0], w[1])).sum()
        };
        let (a, b) = e.ends(st.base_edge);
        let to = if st.reverse { a } else { b };
        pieces.push(Piece { edge: st.base_edge, rev: st.reverse, pts, len, lo, hi, to });
    }
    pieces
}

fn poly_len(xy: &[(f64, f64)]) -> f64 {
    xy.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum()
}

fn along(xy: &[(f64, f64)], offset: f64) -> (f64, f64) {
    let mut acc = 0.0;
    for w in xy.windows(2) {
        let seg = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
        if acc + seg >= offset && seg > 0.0 {
            let t = (offset - acc) / seg;
            return (w[0].0 + t * (w[1].0 - w[0].0), w[0].1 + t * (w[1].1 - w[0].1));
        }
        acc += seg;
    }
    xy[xy.len() - 1]
}

/// Cap d'un chemin à son début ou à sa fin (`RouteGuarantees.pathHeading`).
fn path_heading(xy: &[(f64, f64)], at_start: bool) -> f64 {
    let total = poly_len(xy);
    let far = HEAD_FAR_M.min(total);
    let near = HEAD_NEAR_M.min(far / 2.0);
    let (from, to) = if at_start { (near, far) } else { (total - far, total - near) };
    let (mut a, mut b) = (along(xy, from), along(xy, to));
    if (b.0 - a.0).hypot(b.1 - a.1) < 0.5 {
        a = xy[0];
        b = xy[xy.len() - 1];
    }
    heading_deg(b.0 - a.0, b.1 - a.1)
}

/// Cap du parcours sur 30 à 50 m avant (`at_start` faux : fin du morceau `index`) ou après (début du morceau
/// `index`) un nœud, sur autant de morceaux qu'il faut.
fn route_heading(pieces: &[Piece], index: usize, at_start: bool, loc: &Local) -> f64 {
    let mut parts: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut total = 0.0;
    let mut i = index as isize;
    while i >= 0 && (i as usize) < pieces.len() && total < HEAD_FAR_M {
        let xy: Vec<(f64, f64)> = pieces[i as usize].pts.iter().map(|&p| loc.xy(p)).collect();
        total += poly_len(&xy);
        parts.push(xy);
        i += if at_start { 1 } else { -1 };
    }
    if !at_start {
        parts.reverse();
    }
    let all: Vec<(f64, f64)> = parts.into_iter().flatten().collect();
    path_heading(&all, at_start)
}

fn across_m(dx: f64, dy: f64, heading: f64) -> f64 {
    let h = heading.to_radians();
    (dx * h.cos() - dy * h.sin()).abs()
}

fn across_deg(seg: f64, heading: f64) -> bool {
    let d = (seg - heading).rem_euclid(180.0);
    (d - 90.0).abs() <= CONNECTOR_TOL_DEG
}

fn has_connector_run(raw: &[(f64, f64)], hin: f64, hout: f64) -> bool {
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for &q in raw {
        if pts.last().is_none_or(|l| (q.0 - l.0).hypot(q.1 - l.1) >= RUN_MIN_PIECE_M) {
            pts.push(q);
        }
    }
    let mut a = 0;
    while a + 1 < pts.len() {
        let (from, first) = (pts[a], pts[a + 1]);
        let run = (first.0 - from.0).atan2(first.1 - from.1).to_degrees();
        let mut b = a + 1;
        while b + 1 < pts.len() {
            let (p0, p1) = (pts[b], pts[b + 1]);
            let next = (p1.0 - p0.0).atan2(p1.1 - p0.1).to_degrees();
            let bend = ((next - run).rem_euclid(360.0) + 540.0).rem_euclid(360.0) - 180.0;
            if bend.abs() > RUN_BEND_DEG {
                break;
            }
            b += 1;
        }
        let to = pts[b];
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        if dx.hypot(dy) >= CONNECTOR_MIN_M - 0.5 {
            let seg = dx.atan2(dy).to_degrees();
            if across_deg(seg, hin) || across_deg(seg, hout) {
                return true;
            }
        }
        a = b;
    }
    false
}

/// Degré admis d'un nœud de base : arêtes vivantes dans la vue du type. Toute arête vivante est le lien d'une chaîne
/// (vue non élaguée) et la contraction ne fond un nœud que s'il a **exactement** deux arêtes : le degré est celui du
/// nœud de vue, ou 2 pour un nœud intérieur à une chaîne (lot 6, §4.5). Les nœuds d'un parcours rendu par la recherche
/// sont complets dans la fenêtre ; un nœud de vue absent de la fenêtre compte 0 (le parcours n'y passe pas).
fn admitted_degree<B: AsRef<[u8]>>(e: &Engine<B>, win: &Window, node: u32) -> usize {
    match e.views_files[win.vi].view_node_of_base(node) {
        None => 2,
        Some(g) => win.v.ngid.binary_search(&g).map_or(0, |l| usize::from(win.v.deg[l])),
    }
}

/// Contrôle un parcours contre les garanties du §6, dans la fenêtre des tuiles nécessaires à la requête (§5.1).
pub fn check_route<B: AsRef<[u8]>>(
    e: &Engine<B>,
    team: Option<&TeamData>,
    req: &Request,
    route: &Route,
) -> Check {
    let plan = e.plan(req, 0.0);
    let win = e.build_window(plan.view, &plan.need, &mut || false).expect("jamais coupée");
    check_route_in(e, &win, team, req, route)
}

/// Comme [`check_route`], avec les caps pris au nœud comme le fait [`check_route_aligned_in`].
pub fn check_route_aligned<B: AsRef<[u8]>>(
    e: &Engine<B>,
    team: Option<&TeamData>,
    req: &Request,
    route: &Route,
) -> Check {
    let plan = e.plan(req, 0.0);
    let win = e.build_window(plan.view, &plan.need, &mut || false).expect("jamais coupée");
    check_route_aligned_in(e, &win, team, req, route)
}

/// Comme [`check_route`], dans une fenêtre déjà construite.
pub fn check_route_in<B: AsRef<[u8]>>(
    e: &Engine<B>,
    win: &Window,
    team: Option<&TeamData>,
    req: &Request,
    route: &Route,
) -> Check {
    check_route_with(e, win, team, req, route, false)
}

/// Comme [`check_route_in`], mais les caps du virage simple et des grappes sont pris dans un repère centré **au nœud**
/// (le nœud entre les morceaux, comme le contrôleur indépendant) et la corde de l'épingle dans un repère centré au
/// nœud `a` ; [`check_route_in`] les prend dans un repère centré au départ du parcours. Les deux repères ne divergent
/// que de 0,04° à 1,34° sur 10 à 80 km, assez pour qu'un virage de 150,02° passe l'un et pas l'autre (lot 4, 4c, §16.0
/// point 1). Réservé à la génération organique : en `Classic`, aligner changerait les parcours de 1.10.
pub fn check_route_aligned_in<B: AsRef<[u8]>>(
    e: &Engine<B>,
    win: &Window,
    team: Option<&TeamData>,
    req: &Request,
    route: &Route,
) -> Check {
    check_route_with(e, win, team, req, route, true)
}

fn check_route_with<B: AsRef<[u8]>>(
    e: &Engine<B>,
    win: &Window,
    team: Option<&TeamData>,
    req: &Request,
    route: &Route,
    node_frame: bool,
) -> Check {
    let mut viol: BTreeSet<Violation> = BTreeSet::new();
    let mut out = Check::default();
    let n = route.steps.len();
    if n == 0 {
        out.violations.push(Violation::Empty);
        return out;
    }
    if route.steps.iter().any(|s| s.base_edge >= e.n_edges()) {
        out.violations.push(Violation::NotOnGraph);
        return out;
    }
    let kind = e.kind_of(req.ride_type.view_index());
    let d_target = req.distance_m;
    // 1. Structure.
    for (i, st) in route.steps.iter().enumerate() {
        let a = e.attrs(st.base_edge);
        let (ea, eb) = e.ends(st.base_edge);
        let to = if st.reverse { ea } else { eb };
        if !(if st.reverse { a.bwd_ok() } else { a.fwd_ok() }) {
            viol.insert(Violation::OneWay);
        }
        if !kind.keep(a) {
            viol.insert(Violation::NotAdmitted);
        }
        if i + 1 < n {
            let nx = route.steps[i + 1];
            let (na, nb) = e.ends(nx.base_edge);
            let from = if nx.reverse { nb } else { na };
            if to != from {
                viol.insert(Violation::Discontinuous);
            }
        }
    }
    if viol.contains(&Violation::Discontinuous) {
        out.violations = viol.into_iter().collect();
        return out;
    }
    let pieces = build_pieces(e, route);
    let s_e7 = req.start.to_e7();
    let end_pt = req.end.unwrap_or(req.start).to_e7();
    let loc = Local::new(s_e7);
    // Repère des caps : celui du départ, ou celui du nœud (`node_frame`).
    let frame = |p: (i32, i32)| if node_frame { Local::new(p) } else { loc };
    let last_of = |p: &Piece| *p.pts.last().expect("morceau non vide");
    if haversine_m(s_e7, pieces[0].pts[0]) > e.params.snap_m + 1.0 {
        viol.insert(Violation::WrongStart);
    }
    if haversine_m(end_pt, *pieces[n - 1].pts.last().expect("morceau non vide")) > e.params.snap_m + 1.0 {
        viol.insert(Violation::WrongEnd);
    }
    if req.end.is_none()
        && (route.steps[0].base_edge != route.steps[n - 1].base_edge
            || (route.start.offset_m - route.end.offset_m).abs() > 1.0)
    {
        viol.insert(Violation::WrongEnd);
    }
    let len: Vec<f64> = pieces.iter().map(|p| p.len).collect();
    let total: f64 = len.iter().sum();
    out.length_m = total;

    // Mesures et équipe.
    let tt = team.map(|t| t.of(req.ride_type));
    for p in &pieces {
        let a = e.attrs(p.edge);
        if a.is_unpaved() {
            out.unpaved_m += p.len;
        }
        if a.offroad() {
            out.offroad_m += p.len;
        }
        if presumed_surface(a) {
            out.presumed_surface_m += p.len;
        }
        if let Some(tt) = tt {
            let (f, _) = tt.flags(p.edge);
            if f & TEAM != 0 {
                out.team_m += p.len;
            } else if f & BRIDGED != 0 {
                out.bridged_m += p.len;
            }
        }
    }
    // 2. Équipe (strict) : chaque morceau est une arête d'équipe ou une arête comblée (trou de 100 m touchant un
    // cul-de-sac fictif, jonction courte) ; le contrôleur indépendant recalcule ces deux ensembles lui-même.
    if req.mode == Mode::Strict {
        if let Some(tt) = tt {
            if pieces.iter().any(|p| tt.flags(p.edge).0 & (TEAM | BRIDGED) == 0) {
                viol.insert(Violation::OffTeam);
            }
        }
    }
    // 3. Demi-tour, virage ; 4. impasse.
    for i in 0..n.saturating_sub(1) {
        let (a, b) = (&pieces[i], &pieces[i + 1]);
        let node = a.to;
        if a.edge == b.edge && a.rev != b.rev {
            viol.insert(Violation::UTurn);
        } else if admitted_degree(e, win, node) >= 3 {
            let hin = route_heading(&pieces, i, false, &frame(last_of(a)));
            let hout = route_heading(&pieces, i + 1, true, &frame(b.pts[0]));
            if turn_angle_deg(hin, hout) >= e.params.max_turn_deg {
                viol.insert(Violation::SharpTurn);
                out.sharp_at.push(i);
            }
        }
        if admitted_degree(e, win, node) < 2 {
            viol.insert(Violation::DeadEnd);
        }
    }
    // Grappes de 100 m.
    let mut cum = vec![0.0; n + 1];
    for i in 0..n {
        cum[i + 1] = cum[i] + len[i];
    }
    for i in 0..n.saturating_sub(2) {
        if admitted_degree(e, win, pieces[i].to) < 3
            || (pieces[i].edge == pieces[i + 1].edge && pieces[i].rev != pieces[i + 1].rev)
        {
            continue;
        }
        let mut j = i + 1;
        while j + 1 < n && cum[j + 1] - cum[i + 1] <= CLUSTER_M {
            let at_an_end = cum[j + 1] <= CLUSTER_FREE_ENDS_M || cum[n] - cum[i + 1] <= CLUSTER_FREE_ENDS_M;
            if !at_an_end && admitted_degree(e, win, pieces[j].to) >= 3 {
                let hin = route_heading(&pieces, i, false, &frame(last_of(&pieces[i])));
                let hout = route_heading(&pieces, j + 1, true, &frame(pieces[j + 1].pts[0]));
                if turn_angle_deg(hin, hout) >= e.params.max_turn_deg
                    && !hairpin(e, &pieces, i, j, hin, hout, &frame(last_of(&pieces[i])))
                {
                    viol.insert(Violation::SharpTurn);
                    out.sharp_at.push(i);
                }
            }
            j += 1;
        }
    }
    // 5. Réemprunt.
    let mut by_edge: BTreeMap<u32, Vec<(usize, f64, f64)>> = BTreeMap::new();
    for (i, p) in pieces.iter().enumerate() {
        by_edge.entry(p.edge).or_default().push((i, p.lo, p.hi));
    }
    let overlap = |a: (usize, f64, f64), b: (usize, f64, f64)| (a.2.min(b.2) - a.1.max(b.1)).max(0.0);
    let mut marked = vec![false; n];
    let mut shared = 0.0;
    for list in by_edge.values() {
        if list.len() < 2 {
            continue;
        }
        let mut best = 0.0f64;
        for x in 0..list.len() {
            for y in x + 1..list.len() {
                let o = overlap(list[x], list[y]);
                if o > 1.0 {
                    marked[list[x].0] = true;
                    marked[list[y].0] = true;
                    best = best.max(o);
                    for z in y + 1..list.len() {
                        if overlap(list[x], list[z]) > 1.0 && overlap(list[y], list[z]) > 1.0 {
                            viol.insert(Violation::EdgeUsedMoreThanTwice);
                        }
                    }
                }
            }
        }
        shared += best;
    }
    out.shared_m = shared;
    let limit = e.params.tail_share * d_target;
    let (mut core_from, mut core_to, mut core_len) = (usize::MAX, 0usize, -1.0f64);
    let mut run_start = usize::MAX;
    let mut run_len = 0.0;
    for i in 0..=n {
        if i < n && !marked[i] {
            if run_start == usize::MAX {
                run_start = i;
                run_len = 0.0;
            }
            run_len += len[i];
        } else if run_start != usize::MAX {
            if run_len > core_len {
                core_len = run_len;
                core_from = run_start;
                core_to = i - 1;
            }
            run_start = usize::MAX;
        }
    }
    let has_core = core_from != usize::MAX;
    if !has_core {
        viol.insert(Violation::ReuseOutsideTail);
        core_from = n;
        core_to = n.wrapping_sub(1);
    }
    let start_zone: f64 = len[..core_from.min(n)].iter().sum();
    let end_zone: f64 = if has_core { len[core_to + 1..].iter().sum() } else { 0.0 };
    if start_zone > limit || end_zone > limit {
        viol.insert(Violation::ReuseOutsideTail);
    }
    if shared > limit {
        viol.insert(Violation::TailTooLong);
    }
    // 6. Croisements.
    let is_loop = req.end.is_none();
    let (v_start, v_end) = (u32::MAX, if is_loop { u32::MAX } else { u32::MAX - 1 });
    let node_at = |i: usize| -> u32 {
        if i == 0 {
            v_start
        } else if i == n {
            v_end
        } else {
            pieces[i - 1].to
        }
    };
    let mut crossings = 0u32;
    if has_core {
        let (k, m) = (core_from, core_to + 1);
        let mut tail_inner: BTreeSet<u32> = BTreeSet::new();
        for i in 0..k {
            tail_inner.insert(node_at(i));
        }
        for i in m + 1..=n {
            tail_inner.insert(node_at(i));
        }
        let mut seen: BTreeSet<u32> = BTreeSet::new();
        let first = node_at(k);
        seen.insert(first);
        for i in k + 1..=m {
            let node = node_at(i);
            let closure = i == m && node == first;
            if !closure && (seen.contains(&node) || tail_inner.contains(&node)) {
                crossings += 1;
            }
            seen.insert(node);
        }
    }
    out.crossings = crossings;
    if crossings > e.params.max_crossings {
        viol.insert(Violation::TooManyCrossings);
    }
    // 7. Couloir.
    if has_core && corridor_share(&pieces[core_from..=core_to], &loc) > CORRIDOR_MAX {
        viol.insert(Violation::CorridorDoubled);
    }
    // 8. Distance.
    if (total - d_target).abs() > e.params.tolerance * d_target {
        viol.insert(Violation::DistanceOutOfTolerance);
    }
    out.violations = viol.into_iter().collect();
    out
}

fn hairpin<B: AsRef<[u8]>>(
    e: &Engine<B>,
    pieces: &[Piece],
    i: usize,
    j: usize,
    hin: f64,
    hout: f64,
    loc: &Local,
) -> bool {
    let way = e.way(pieces[i].edge);
    if (i..=j + 1).any(|k| e.way(pieces[k].edge) != way) {
        return false;
    }
    let first = loc.xy(e.pos(pieces[i].to));
    let last = loc.xy(e.pos(pieces[j].to));
    let (dx, dy) = (last.0 - first.0, last.1 - first.1);
    if across_m(dx, dy, hin) >= CROSSING_MIN_M || across_m(dx, dy, hout) >= CROSSING_MIN_M {
        return false;
    }
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for p in &pieces[i + 1..=j] {
        for (q, &pt) in p.pts.iter().enumerate() {
            if !pts.is_empty() && q == 0 {
                continue;
            }
            pts.push(loc.xy(pt));
        }
    }
    !has_connector_run(&pts, hin, hout)
}

fn corridor_share(core: &[Piece], loc: &Local) -> f64 {
    let mut samples: Vec<(f64, f64, f64)> = Vec::new();
    let mut offset = 0.0;
    let mut next = 0.0;
    for p in core {
        let xy: Vec<(f64, f64)> = p.pts.iter().map(|&q| loc.xy(q)).collect();
        let l = poly_len(&xy);
        while next <= offset + l {
            let pt = along(&xy, next - offset);
            samples.push((pt.0, pt.1, next));
            next += CORRIDOR_STEP_M;
        }
        offset += l;
    }
    let core_len = offset;
    if samples.is_empty() || core_len <= 2.0 * CORRIDOR_END_M {
        return 0.0;
    }
    let cell = |x: f64| (x / CORRIDOR_DIST_M).floor() as i64;
    let mut grid: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (i, s) in samples.iter().enumerate() {
        if s.2 < CORRIDOR_END_M || s.2 > core_len - CORRIDOR_END_M {
            continue;
        }
        grid.entry((cell(s.0), cell(s.1))).or_default().push(i);
    }
    let mut doubled = 0usize;
    for s in &samples {
        if s.2 < CORRIDOR_END_M || s.2 > core_len - CORRIDOR_END_M {
            continue;
        }
        let (cx, cy) = (cell(s.0), cell(s.1));
        let mut hit = false;
        'cells: for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(list) = grid.get(&(cx + dx, cy + dy)) {
                    for &j in list {
                        let q = samples[j];
                        if (q.2 - s.2).abs() >= CORRIDOR_GAP_M
                            && (q.0 - s.0).hypot(q.1 - s.1) <= CORRIDOR_DIST_M
                        {
                            hit = true;
                            break 'cells;
                        }
                    }
                }
            }
        }
        if hit {
            doubled += 1;
        }
    }
    CORRIDOR_STEP_M * doubled as f64 / (2.0 * core_len)
}

/// Angle (degrés) du virage entre le nœud d'arrivée du morceau `ia` (cap avant) et celui du morceau `ib` (cap après),
/// calculé comme le contrôle aligné : le même que celui d'une grappe `(a, b)` ou, pour `ia == ib`, d'un virage simple.
/// Pour les tests du modèle des virages de la recherche (`turns.rs`).
#[cfg(test)]
pub(crate) fn aligned_pair_angle<B: AsRef<[u8]>>(e: &Engine<B>, route: &Route, ia: usize, ib: usize) -> f64 {
    let pieces = build_pieces(e, route);
    let last_of = |p: &Piece| *p.pts.last().expect("morceau non vide");
    let hin = route_heading(&pieces, ia, false, &Local::new(last_of(&pieces[ia])));
    let hout = route_heading(&pieces, ib + 1, true, &Local::new(pieces[ib + 1].pts[0]));
    turn_angle_deg(hin, hout)
}
