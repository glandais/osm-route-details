//! Le contrôle du recollé en trois étages (note : `ord-retouche-lot0.md` §4.4), commun à `move_end` et à `resize`.
//!
//! - **C1** : le tronçon neuf seul, par `ord-loops-check`, sans aucune tolérance.
//! - **C2** : les morceaux continus autour des coutures, **par différence** avec les mêmes morceaux de l'original (les
//!   traces réelles violent déjà des règles). Les trous de l'original restent des trous : ce qu'ils laissent sans
//!   contrôle est compté (`unchecked_m`). Le contrôleur ne lit que `±c2_window_m` de part et d'autre d'une couture :
//!   ses règles de virage, d'impasse et de grappe sont locales, et la réutilisation (la règle globale) est le C3.
//! - **C3** : la réutilisation des arêtes gardées, comptée par le banc.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::ops::Range;

use memmap2::Mmap;
use ord_graph::mmap::MappedGraph;
use ord_graph_match::walk::{haversine_len, Join, Leg, Walk};
use ord_loops::{LatLon, Position, Request, Route, Step};
use ord_loops_check::{Checker, Report};
use rustc_hash::FxHashSet;

use super::posed::{leg_len_m, turn_deg, Posed};

/// Les codes que le découpage fabrique : ils ne sont jamais un refus de C2.
const SPLIT_ARTEFACTS: [&str; 4] = ["WRONG_START", "WRONG_END", "BAD_OFFSET", "DISTANCE_OUT_OF_TOLERANCE"];

pub struct Ctl<'a> {
    pub gf: &'a MappedGraph,
    pub checker: &'a Checker<'a, Mmap>,
    /// Demi-largeur de la fenêtre de C2 autour d'une couture, m de graphe.
    pub c2_window_m: f64,
}

/// Position `(lat, lon)` d'un nœud de base.
pub fn node_latlon(gf: &MappedGraph, node: u32) -> LatLon {
    let p = gf.node_pos(node);
    LatLon::new(f64::from(p.0) * 1e-7, f64::from(p.1) * 1e-7)
}

pub fn codes_of(rep: &Report) -> BTreeSet<&'static str> {
    rep.violations.iter().map(|v| v.code()).collect()
}

/// La requête de contrôle d'un tronçon : la requête du curseur, avec les points et la distance du tronçon.
pub fn seg_request(proto: &Request, start: LatLon, end: Option<LatLon>, distance_m: f64) -> Request {
    Request { start, end, distance_m, ..proto.clone() }
}

fn endpoints(gf: &MappedGraph, l: &Leg) -> (u32, u32) {
    let (a, b) = gf.edge_ends(l.edge);
    if l.reverse {
        (b, a)
    } else {
        (a, b)
    }
}

/// Les mètres de graphe de chaque jambe.
pub fn leg_lens(gf: &MappedGraph, w: &Walk) -> Vec<f64> {
    w.legs.iter().map(|l| leg_len_m(gf, l)).collect()
}

/// Ramène un tronçon rendu par le moteur aux nœuds de ses coutures : l'accroche tombe à 1 m ou plus d'un bout d'arête,
/// ce qui fait un morceau de départ ou d'arrivée de quelques mètres. Un morceau de 5 m ou moins qui touche la couture est
/// retiré ; un morceau qui part de la couture est prolongé jusqu'à elle. `None` si le tronçon ne part pas (ou n'arrive pas)
/// à la couture : `seam_snap_off`.
pub fn normalize_between(gf: &MappedGraph, r: &Route, node_i: u32, node_j: u32) -> Option<Route> {
    let mut steps = r.steps.clone();
    let (mut start, mut end) = (r.start, r.end);
    let ends = |s: &Step| {
        let (a, b) = gf.edge_ends(s.base_edge);
        if s.reverse {
            (b, a)
        } else {
            (a, b)
        }
    };
    // Départ.
    let s0 = *steps.first()?;
    let (f0, t0) = ends(&s0);
    if f0 == node_i {
        start = Position {
            edge: s0.base_edge,
            offset_m: if s0.reverse { haversine_len(gf, s0.base_edge) } else { 0.0 },
        };
    } else if t0 == node_i {
        let len = haversine_len(gf, s0.base_edge);
        let piece = if s0.reverse { start.offset_m } else { len - start.offset_m };
        if piece > 5.0 || steps.len() < 2 {
            return None;
        }
        steps.remove(0);
        let s1 = steps[0];
        if ends(&s1).0 != node_i {
            return None;
        }
        start = Position {
            edge: s1.base_edge,
            offset_m: if s1.reverse { haversine_len(gf, s1.base_edge) } else { 0.0 },
        };
    } else {
        return None;
    }
    // Arrivée.
    let sl = *steps.last()?;
    let (fl, tl) = ends(&sl);
    if tl == node_j {
        end = Position {
            edge: sl.base_edge,
            offset_m: if sl.reverse { 0.0 } else { haversine_len(gf, sl.base_edge) },
        };
    } else if fl == node_j {
        let len = haversine_len(gf, sl.base_edge);
        let piece = if sl.reverse { len - end.offset_m } else { end.offset_m };
        if piece > 5.0 || steps.len() < 2 {
            return None;
        }
        steps.pop();
        let s1 = *steps.last()?;
        if ends(&s1).1 != node_j {
            return None;
        }
        end = Position {
            edge: s1.base_edge,
            offset_m: if s1.reverse { 0.0 } else { haversine_len(gf, s1.base_edge) },
        };
    } else {
        return None;
    }
    Some(Route { start, end, steps, ..r.clone() })
}

/// Un recollé : la marche d'origine dont un tronçon est remplacé (`resize`) ou dont la fin est remplacée (`move_end`).
pub struct Spliced {
    pub walk: Walk,
    /// Jambes neuves.
    pub new_legs: Range<usize>,
    /// Jonctions aux coutures (dans `walk.joins`) : l'entrée du tronçon neuf, et sa sortie pour `resize`.
    pub join_in: usize,
    pub join_out: Option<usize>,
}

/// Recolle `new` entre la jambe `join_i` (incluse) et la jambe qui suit `join_j` (`resize`) ou jusqu'à la fin
/// (`move_end`, `join_j = None`). Les jonctions aux coutures sont des `Node` : le tronçon neuf part du nœud de la couture.
pub fn splice(
    p: &Posed,
    join_i: usize,
    join_j: Option<usize>,
    new: &Walk,
    node_i: u32,
    node_j: Option<u32>,
) -> Spliced {
    let mut legs: Vec<Leg> = p.walk.legs[..=join_i].to_vec();
    let mut joins: Vec<Join> = p.walk.joins[..join_i].to_vec();
    joins.push(Join::Node(node_i));
    let start = legs.len();
    legs.extend(new.legs.iter().copied());
    joins.extend(new.joins.iter().cloned());
    let end = legs.len();
    let mut join_out = None;
    if let (Some(jj), Some(nj)) = (join_j, node_j) {
        join_out = Some(joins.len());
        joins.push(Join::Node(nj));
        legs.extend(p.walk.legs[jj + 1..].iter().copied());
        joins.extend(p.walk.joins[jj + 1..].iter().cloned());
    }
    debug_assert_eq!(joins.len() + 1, legs.len());
    Spliced { walk: Walk { legs, joins }, new_legs: start..end, join_in: join_i, join_out }
}

/// La fenêtre de jambes autour de la jonction `join` : on s'étend de part et d'autre jusqu'à `w` mètres de graphe. Avec
/// `stop_at_holes`, l'extension s'arrête à la première jonction qui n'est pas un `Node` (la fenêtre est continue).
fn window_around(walk: &Walk, lens: &[f64], join: usize, w: f64, stop_at_holes: bool) -> Range<usize> {
    let node = |j: usize| matches!(walk.joins[j], Join::Node(_));
    let (mut lo, mut acc) = (join, 0.0);
    while lo > 0 && acc < w && (!stop_at_holes || node(lo - 1)) {
        lo -= 1;
        acc += lens[lo];
    }
    let (mut hi, mut acc) = (join + 1, 0.0);
    while hi < walk.legs.len() && acc < w && (!stop_at_holes || node(hi - 1)) {
        acc += lens[hi];
        hi += 1;
    }
    lo..hi
}

fn merge(mut v: Vec<Range<usize>>) -> Vec<Range<usize>> {
    v.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::new();
    for r in v {
        match out.last_mut() {
            Some(l) if r.start <= l.end => l.end = l.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

/// Les codes d'un morceau continu de marche : une requête dont le départ et l'arrivée sont ses bouts.
fn check_run(ctl: &Ctl<'_>, proto: &Request, legs: &[Leg], lens: &[f64]) -> BTreeSet<&'static str> {
    let walk = Walk { legs: legs.to_vec(), joins: vec![Join::Node(0); legs.len().saturating_sub(1)] };
    let route = walk.to_route(ctl.gf);
    let (first, last) = (&legs[0], &legs[legs.len() - 1]);
    let pos = |l: &Leg, at_start: bool| {
        let (a, b) = endpoints(ctl.gf, l);
        node_latlon(ctl.gf, if at_start { a } else { b })
    };
    let req = seg_request(proto, pos(first, true), Some(pos(last, false)), lens.iter().sum());
    codes_of(&ctl.checker.check(&req, &route))
}

/// Cache des codes des fenêtres de l'original, par jonction : une trace est jouée par un seul fil.
#[derive(Default)]
pub struct OrigCache(RefCell<HashMap<(usize, u8), BTreeSet<&'static str>>>);

impl OrigCache {
    fn codes(
        &self,
        ctl: &Ctl<'_>,
        p: &Posed,
        lens: &[f64],
        proto: &Request,
        join: usize,
    ) -> BTreeSet<&'static str> {
        let key = (join, u8::from(proto.mode == ord_loops::Mode::Discovery));
        if let Some(c) = self.0.borrow().get(&key) {
            return c.clone();
        }
        let r = window_around(&p.walk, lens, join, ctl.c2_window_m, true);
        let c = check_run(ctl, proto, &p.walk.legs[r.clone()], &lens[r]);
        self.0.borrow_mut().insert(key, c.clone());
        c
    }
}

/// Sortie de C2.
#[derive(Clone, Debug, Default)]
pub struct C2 {
    /// Codes du recollé absents de l'original (hors artefacts du découpage) : les refus.
    pub new_codes: Vec<&'static str>,
    /// Mètres de graphe lus par le contrôleur, et mètres voisins des coutures qu'il n'a pas lus (trous).
    pub checked_m: f64,
    pub unchecked_m: f64,
}

/// C2 : les fenêtres continues du recollé autour de ses coutures, contre celles de l'original autour des mêmes coutures.
pub fn c2(
    ctl: &Ctl<'_>,
    cache: &OrigCache,
    p: &Posed,
    sp: &Spliced,
    join_i: usize,
    join_j: Option<usize>,
    proto: &Request,
) -> C2 {
    let lens = leg_lens(ctl.gf, &sp.walk);
    let mut centres = vec![sp.join_in];
    centres.extend(sp.join_out);
    let wins =
        merge(centres.iter().map(|&j| window_around(&sp.walk, &lens, j, ctl.c2_window_m, true)).collect());
    let reach =
        merge(centres.iter().map(|&j| window_around(&sp.walk, &lens, j, ctl.c2_window_m, false)).collect());
    let mut now: BTreeSet<&'static str> = BTreeSet::new();
    let mut checked = 0.0;
    for r in &wins {
        now.extend(check_run(ctl, proto, &sp.walk.legs[r.clone()], &lens[r.clone()]));
        checked += lens[r.clone()].iter().sum::<f64>();
    }
    let reached: f64 = reach.iter().map(|r| lens[r.clone()].iter().sum::<f64>()).sum();
    let orig_lens = leg_lens(ctl.gf, &p.walk);
    let mut before: BTreeSet<&'static str> = BTreeSet::new();
    for j in std::iter::once(join_i).chain(join_j) {
        before.extend(cache.codes(ctl, p, &orig_lens, proto, j));
    }
    let new_codes =
        now.difference(&before).copied().filter(|c| !SPLIT_ARTEFACTS.contains(c)).collect::<Vec<_>>();
    C2 { new_codes, checked_m: checked, unchecked_m: (reached - checked).max(0.0) }
}

/// C3 : mètres du tronçon neuf sur des arêtes gardées (hors zone des coutures), et sur celles de la zone.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct C3 {
    pub outside_m: f64,
    pub zone_m: f64,
}

pub fn c3(gf: &MappedGraph, steps: &[Step], kept: &FxHashSet<u32>, zone: &FxHashSet<u32>) -> C3 {
    let mut out = C3::default();
    for s in steps {
        let len = f64::from(gf.edge(s.base_edge).len_dm) / 10.0;
        if zone.contains(&s.base_edge) {
            out.zone_m += len;
        } else if kept.contains(&s.base_edge) {
            out.outside_m += len;
        }
    }
    out
}

/// Les arêtes de la zone d'une couture : les jambes de `range` (indices) qui sont à moins de `margin_m` de la jonction
/// `join`, par la longueur de graphe cumulée.
pub fn zone_edges(p: &Posed, join: usize, margin_m: f64, side_before: bool) -> Vec<u32> {
    let at = p.cum_end[join];
    if side_before {
        (0..=join)
            .rev()
            .take_while(|&j| at - p.leg_start(j) <= margin_m + 1e-9 || j == join)
            .map(|j| p.walk.legs[j].edge)
            .collect()
    } else {
        (join + 1..p.walk.legs.len())
            .take_while(|&j| p.leg_start(j) - at <= margin_m + 1e-9 || j == join + 1)
            .map(|j| p.walk.legs[j].edge)
            .collect()
    }
}

/// Angle aux coutures : sortie de la jambe gardée → première jambe neuve, dernière jambe neuve → entrée de la jambe
/// gardée qui suit. `None` quand le tronçon ou la jambe manque.
pub fn seam_angles(gf: &MappedGraph, p: &Posed, sp: &Spliced) -> (Option<f64>, Option<f64>) {
    let legs = &sp.walk.legs;
    let a = (sp.new_legs.start > 0 && sp.new_legs.start < legs.len())
        .then(|| turn_deg(gf, &legs[sp.new_legs.start - 1], &legs[sp.new_legs.start]))
        .flatten();
    let b = (sp.join_out.is_some() && sp.new_legs.end < legs.len() && sp.new_legs.end > 0)
        .then(|| turn_deg(gf, &legs[sp.new_legs.end - 1], &legs[sp.new_legs.end]))
        .flatten();
    let _ = p;
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::likeness::fixture::{edge_between, graph, id};

    fn leg(gf: &MappedGraph, from: i64, to: i64) -> Leg {
        let (e, reverse) = edge_between(gf, from, to);
        Leg { edge: e, reverse, lo: 0.0, hi: 1.0 }
    }

    fn node_of(gf: &MappedGraph, l: &Leg, end: bool) -> u32 {
        let (a, b) = endpoints(gf, l);
        if end {
            b
        } else {
            a
        }
    }

    fn walk_of(gf: &MappedGraph, path: &[i64]) -> Walk {
        let legs: Vec<Leg> = path.windows(2).map(|w| leg(gf, w[0], w[1])).collect();
        let joins = legs.windows(2).map(|w| Join::Node(node_of(gf, &w[0], true))).collect();
        Walk { legs, joins }
    }

    #[test]
    fn splice_is_continuous_and_keeps_the_original_joins_around_it() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        // Original : (0,0) (0,1) (0,2) (1,2) (2,2) ; on remplace le tronçon central (0,1) → (1,2) par un détour.
        let orig = walk_of(&gf, &[id(0, 0), id(0, 1), id(0, 2), id(1, 2), id(2, 2)]);
        let new = walk_of(&gf, &[id(0, 1), id(1, 1), id(1, 2)]);
        let ni = node_of(&gf, &orig.legs[0], true);
        let nj = node_of(&gf, &new.legs[1], true);
        // join_i = 0 (fin de la jambe 0, nœud (0,1)) ; join_j = 2 (fin de la jambe 2, nœud (1,2)).
        let mut o = orig.clone();
        o.joins[1] = Join::Hole(500.0);
        let p = fake_posed(o.clone());
        let sp = splice(&p, 0, Some(2), &new, ni, Some(nj));
        assert_eq!(sp.walk.legs.len(), 1 + 2 + 1);
        assert_eq!(sp.walk.joins.len(), 3);
        assert!(sp.walk.is_continuous(), "{:?}", sp.walk.joins);
        assert_eq!(sp.new_legs, 1..3);
        assert_eq!(sp.join_out, Some(2));
        // Un trou de l'original hors du tronçon remplacé reste un trou : ici il était dans le tronçon (jonction 1) donc
        // il disparaît ; un trou après la couture j reste.
        let mut o2 = walk_of(&gf, &[id(0, 0), id(0, 1), id(0, 2), id(1, 2), id(2, 2), id(2, 1)]);
        o2.joins[3] = Join::Hole(500.0);
        let sp2 = splice(&fake_posed(o2), 0, Some(2), &new, ni, Some(nj));
        assert!(matches!(sp2.walk.joins.last(), Some(Join::Hole(_))), "{:?}", sp2.walk.joins);
        // move_end : la fin est remplacée.
        let tail = walk_of(&gf, &[id(0, 1), id(1, 1)]);
        let me = splice(&fake_posed(orig.clone()), 0, None, &tail, ni, None);
        assert_eq!(me.walk.legs.len(), 2);
        assert!(me.walk.is_continuous() && me.join_out.is_none());
    }

    fn fake_posed(walk: Walk) -> Posed {
        use crate::retouche::continuity::Shape;
        use crate::retouche::project::DocLine;
        let n = walk.legs.len();
        Posed {
            team: 0,
            letter: "B".into(),
            rank: 0,
            id_hash: 1,
            ty: 0,
            ride: ord_loops::RideType::Road,
            shape: Shape::Loop,
            walk,
            line: DocLine::from_points(vec![(47.0, -1.5), (47.01, -1.5)]),
            l_m: 1000.0,
            seams: vec![],
            cum_end: (1..=n).map(|k| k as f64 * 300.0).collect(),
            edge_set: Default::default(),
            order_anomalies: 0,
        }
    }

    #[test]
    fn kept_edges_are_counted_outside_the_zone_only() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let w = walk_of(&gf, &[id(0, 0), id(0, 1), id(0, 2), id(1, 2)]);
        let steps: Vec<Step> =
            w.legs.iter().map(|l| Step { base_edge: l.edge, reverse: l.reverse }).collect();
        let kept: FxHashSet<u32> = w.legs[..2].iter().map(|l| l.edge).collect();
        let zone: FxHashSet<u32> = [w.legs[1].edge].into_iter().collect();
        let r = c3(&gf, &steps, &kept, &zone);
        assert!((r.outside_m - 300.0).abs() < 2.0 && (r.zone_m - 300.0).abs() < 2.0, "{r:?}");
        // Rien de gardé : aucune réutilisation.
        assert_eq!(c3(&gf, &steps, &FxHashSet::default(), &FxHashSet::default()), C3::default());
    }

    #[test]
    fn a_route_is_brought_back_to_its_seam_nodes() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let w = walk_of(&gf, &[id(0, 1), id(1, 1), id(1, 2)]);
        let mut r = w.to_route(&gf);
        let (n_i, n_j) = (node_of(&gf, &w.legs[0], false), node_of(&gf, &w.legs[1], true));
        // Le moteur part 1 m après le nœud et arrive 1 m avant : normalisé, le tronçon va du nœud au nœud.
        r.start.offset_m = if w.legs[0].reverse { haversine_len(&gf, w.legs[0].edge) - 1.0 } else { 1.0 };
        let n = normalize_between(&gf, &r, n_i, n_j).expect("normalisé");
        assert_eq!(n.steps, r.steps);
        let walk = Walk::from_route(&gf, &n);
        assert!((walk.legs[0].lo - 0.0).abs() < 1e-9 || (walk.legs[0].hi - 1.0).abs() < 1e-9);
        // Un autre nœud n'est pas la couture : refusé.
        assert!(normalize_between(&gf, &r, n_j, n_i).is_none());
        // Un premier pas de 1 m qui arrive à la couture est retiré.
        let w2 = walk_of(&gf, &[id(0, 0), id(0, 1), id(1, 1), id(1, 2)]);
        let mut r2 = w2.to_route(&gf);
        let n_seam = node_of(&gf, &w2.legs[0], true);
        r2.start.offset_m = if w2.legs[0].reverse { 1.0 } else { haversine_len(&gf, w2.legs[0].edge) - 1.0 };
        let n2 = normalize_between(&gf, &r2, n_seam, n_j).expect("normalisé");
        assert_eq!(n2.steps.len(), 2, "le pas de 1 m est retiré");
    }
}
