//! La marche : une représentation **commune au réel et au généré** d'une suite d'arêtes de base (§11.4 de la note).
//!
//! Une `Walk` est une suite de jambes (une arête parcourue sur la portion `[lo, hi]` de sa longueur, de `a` vers
//! `b` ou en sens inverse) et de jonctions (nœud commun, contact à 5 m ou moins, trou). Elle se fabrique depuis la
//! suite appariée d'une trace réelle (`from_pieces`) ou depuis une `Route` du moteur (`from_route`), et rend une
//! `Route` à passer au contrôleur (`to_route`, marche continue) et une polyligne plane (`trace`).

use ord_graph::mmap::MappedGraph;
use ord_graph_match::geo::{Frame, P2};
use ord_graph_match::matcher::{Piece, MIN_PIECE_M};
use ord_loops::{Family, Position, Route, Step};
use ord_loops_check::haversine_m;

use super::shape::{self, Pt};

/// Marge de fusion de deux morceaux consécutifs d'une même arête, et d'accrochage d'une extrémité à son nœud, m.
const SNAP_M: f64 = 2.0;
/// Écart de deux points au plus pour un `Touch`, m.
const TOUCH_M: f64 = 5.0;

/// Une arête de base parcourue sur la portion `[lo, hi]` ⊂ `[0, 1]` de sa longueur (paramètre `a → b`), dans le sens
/// `b → a` si `reverse`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leg {
    pub edge: u32,
    pub reverse: bool,
    pub lo: f64,
    pub hi: f64,
}

impl Leg {
    /// Fraction d'entrée et de sortie, dans le sens de la marche.
    pub fn entry(&self) -> f64 {
        if self.reverse {
            self.hi
        } else {
            self.lo
        }
    }

    pub fn exit(&self) -> f64 {
        if self.reverse {
            self.lo
        } else {
            self.hi
        }
    }
}

/// Jonction entre deux jambes consécutives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Join {
    /// Sortie de l'une et entrée de l'autre au même nœud.
    Node(u32),
    /// Les deux points sont à 5 m ou moins sans être au même nœud.
    Touch,
    /// Trou : longueur planaire, m.
    Hole(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Walk {
    pub legs: Vec<Leg>,
    /// `legs.len() - 1` jonctions.
    pub joins: Vec<Join>,
}

/// La marche vue comme une polyligne plane en mètres locaux, départ en `(0, 0)`.
#[derive(Clone, Debug)]
pub struct Trace {
    pub pts: Vec<Pt>,
    /// Abscisses cumulées de `pts`.
    pub cum: Vec<f64>,
    /// Abscisse de chaque jonction (fin de la jambe `j`).
    pub join_arc: Vec<f64>,
    /// Longueur **du graphe** de chaque jambe : `(hi − lo) × len_dm / 10`.
    pub leg_m: Vec<f64>,
    /// `Σ leg_m + Σ trous` : la D de la requête.
    pub d_graph: f64,
    pub holes_m: f64,
    pub max_hole_m: f64,
    /// Longueur de la polyligne.
    pub l_poly: f64,
}

/// Géométrie complète d'une arête, de `a` vers `b`, en mètres locaux.
fn edge_pts(gf: &MappedGraph, fr: &Frame, edge: u32) -> Vec<Pt> {
    let (a, b) = gf.edge_ends(edge);
    let mut out = Vec::new();
    let p = gf.node_pos(a);
    out.push(fr.xy_e7(p.0, p.1));
    out.extend(gf.geometry(edge).into_iter().map(|q| fr.xy_e7(q.0, q.1)));
    let p = gf.node_pos(b);
    out.push(fr.xy_e7(p.0, p.1));
    out
}

/// Repère local : celui du premier nœud de la première arête (le départ est ramené à l'origine ensuite).
fn frame_of(gf: &MappedGraph, edge: u32) -> Frame {
    let p = gf.node_pos(gf.edge_ends(edge).0);
    Frame::new(f64::from(p.0) * 1e-7, f64::from(p.1) * 1e-7)
}

/// Sous-polyligne de `pts` entre les abscisses `s0 ≤ s1`.
fn cut(pts: &[Pt], s0: f64, s1: f64) -> Vec<Pt> {
    let cum = shape::cumulative(pts);
    let mut out = vec![shape::point_at(pts, &cum, s0)];
    for (p, &c) in pts.iter().zip(&cum) {
        if c > s0 && c < s1 {
            out.push(*p);
        }
    }
    out.push(shape::point_at(pts, &cum, s1));
    out
}

/// Point d'une arête à la fraction `f` de sa longueur locale.
fn point_on(pts: &[Pt], f: f64) -> Pt {
    let cum = shape::cumulative(pts);
    let total = *cum.last().unwrap_or(&0.0);
    shape::point_at(pts, &cum, f * total)
}

/// Longueur haversine de la géométrie complète d'une arête (celle du contrôleur, pour les décalages d'une `Route`).
pub fn haversine_len(gf: &MappedGraph, edge: u32) -> f64 {
    let (a, b) = gf.edge_ends(edge);
    let e7 = |p: (i32, i32)| (f64::from(p.0) * 1e-7, f64::from(p.1) * 1e-7);
    let mut pts = vec![e7(gf.node_pos(a))];
    pts.extend(gf.geometry(edge).into_iter().map(e7));
    pts.push(e7(gf.node_pos(b)));
    pts.windows(2).map(|w| haversine_m(w[0], w[1])).sum()
}

impl Walk {
    /// Toutes les jonctions sont des nœuds communs.
    pub fn is_continuous(&self) -> bool {
        self.joins.iter().all(|j| matches!(j, Join::Node(_)))
    }

    /// Nœud d'une extrémité si la fraction est à 0 ou 1 (`a` ou `b`).
    fn node_at(gf: &MappedGraph, edge: u32, f: f64) -> Option<u32> {
        let (a, b) = gf.edge_ends(edge);
        if f <= 1e-9 {
            Some(a)
        } else if f >= 1.0 - 1e-9 {
            Some(b)
        } else {
            None
        }
    }

    /// Depuis la suite appariée d'une trace réelle, dans l'ordre de la trace (§11.4) : morceaux de moins de 1 m
    /// jetés, morceaux contigus d'une même arête et d'un même sens fusionnés (2 m), extrémités à 2 m d'un nœud
    /// ramenées au nœud, jonctions classées.
    pub fn from_pieces(gf: &MappedGraph, pieces: &[Piece]) -> Walk {
        struct Raw {
            edge: u32,
            from: f64,
            to: f64,
            elen: f64,
        }
        let mut raws: Vec<Raw> = Vec::new();
        for p in pieces {
            if p.len() < MIN_PIECE_M {
                continue;
            }
            let r =
                Raw { edge: p.edge, from: f64::from(p.from), to: f64::from(p.to), elen: f64::from(p.elen) };
            if let Some(last) = raws.last_mut() {
                if last.edge == r.edge
                    && (last.to >= last.from) == (r.to >= r.from)
                    && (last.to - r.from).abs() <= SNAP_M
                {
                    last.to = r.to;
                    continue;
                }
            }
            raws.push(r);
        }
        let snap = |m: f64, elen: f64| {
            if m <= SNAP_M {
                0.0
            } else if elen - m <= SNAP_M {
                elen
            } else {
                m
            }
        };
        let legs: Vec<Leg> = raws
            .iter()
            .filter(|r| r.elen > 0.0)
            .filter_map(|r| {
                let reverse = r.to < r.from;
                let (entry, exit) = (snap(r.from, r.elen), snap(r.to, r.elen));
                let (lo, hi) = (entry.min(exit) / r.elen, entry.max(exit) / r.elen);
                (hi - lo > 1e-9).then_some(Leg {
                    edge: r.edge,
                    reverse,
                    lo: lo.clamp(0.0, 1.0),
                    hi: hi.clamp(0.0, 1.0),
                })
            })
            .collect();
        let joins = Self::classify_joins(gf, &legs);
        Walk { legs, joins }
    }

    fn classify_joins(gf: &MappedGraph, legs: &[Leg]) -> Vec<Join> {
        let Some(first) = legs.first() else { return Vec::new() };
        let fr = frame_of(gf, first.edge);
        legs.windows(2)
            .map(|w| {
                let (x, y) = (&w[0], &w[1]);
                if let (Some(n), Some(m)) =
                    (Self::node_at(gf, x.edge, x.exit()), Self::node_at(gf, y.edge, y.entry()))
                {
                    if n == m {
                        return Join::Node(n);
                    }
                }
                let p = point_on(&edge_pts(gf, &fr, x.edge), x.exit());
                let q = point_on(&edge_pts(gf, &fr, y.edge), y.entry());
                let d = shape::dist(p, q);
                if d <= TOUCH_M {
                    Join::Touch
                } else {
                    Join::Hole(d)
                }
            })
            .collect()
    }

    /// Depuis une `Route` du moteur : une jambe par étape ; entières, sauf l'entrée de la première et la sortie de la
    /// dernière (décalages de la route sur la longueur haversine de l'arête).
    pub fn from_route(gf: &MappedGraph, route: &Route) -> Walk {
        let n = route.steps.len();
        let mut legs: Vec<Leg> = route
            .steps
            .iter()
            .map(|s| Leg { edge: s.base_edge, reverse: s.reverse, lo: 0.0, hi: 1.0 })
            .collect();
        if n > 0 {
            let frac = |edge: u32, off: f64| {
                let g = haversine_len(gf, edge);
                if g > 0.0 {
                    (off / g).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            };
            let f0 = frac(legs[0].edge, route.start.offset_m);
            let f1 = frac(legs[n - 1].edge, route.end.offset_m);
            if legs[0].reverse {
                legs[0].hi = f0;
            } else {
                legs[0].lo = f0;
            }
            if legs[n - 1].reverse {
                legs[n - 1].lo = f1;
            } else {
                legs[n - 1].hi = f1;
            }
        }
        let joins = route
            .steps
            .windows(2)
            .map(|w| {
                let (a, b) = gf.edge_ends(w[0].base_edge);
                Join::Node(if w[0].reverse { a } else { b })
            })
            .collect();
        Walk { legs, joins }
    }

    /// Une `Route` pour le contrôleur (marche continue) ; les champs de mesure sont à zéro, il ne lit que la suite,
    /// les positions et le graphe.
    pub fn to_route(&self, gf: &MappedGraph) -> Route {
        assert!(
            self.is_continuous() && !self.legs.is_empty(),
            "to_route : marche continue non vide attendue"
        );
        let (first, last) = (&self.legs[0], &self.legs[self.legs.len() - 1]);
        Route {
            start: Position { edge: first.edge, offset_m: first.entry() * haversine_len(gf, first.edge) },
            end: Position { edge: last.edge, offset_m: last.exit() * haversine_len(gf, last.edge) },
            steps: self.legs.iter().map(|l| Step { base_edge: l.edge, reverse: l.reverse }).collect(),
            length_m: 0.0,
            team_m: 0.0,
            bridged_m: 0.0,
            shared_m: 0.0,
            crossings: 0,
            unpaved_m: 0.0,
            presumed_surface_m: 0.0,
            bearing_deviation_deg: None,
            familiarity: 0.0,
            family: Family::Turnaround,
        }
    }

    /// Arêtes de base distinctes de la marche.
    pub fn edges(&self) -> impl Iterator<Item = u32> + '_ {
        self.legs.iter().map(|l| l.edge)
    }

    /// La polyligne plane de la marche (§11.4).
    pub fn trace(&self, gf: &MappedGraph) -> Trace {
        let mut t = Trace {
            pts: Vec::new(),
            cum: Vec::new(),
            join_arc: Vec::new(),
            leg_m: Vec::new(),
            d_graph: 0.0,
            holes_m: 0.0,
            max_hole_m: 0.0,
            l_poly: 0.0,
        };
        let Some(first) = self.legs.first() else { return t };
        let fr = frame_of(gf, first.edge);
        let mut last_idx: Vec<usize> = Vec::with_capacity(self.legs.len());
        for l in &self.legs {
            let full = edge_pts(gf, &fr, l.edge);
            let total = shape::length(&full);
            let mut part = cut(&full, l.lo * total, l.hi * total);
            if l.reverse {
                part.reverse();
            }
            for p in part {
                if t.pts.last().is_none_or(|q| shape::dist(*q, p) > 1e-9) {
                    t.pts.push(p);
                }
            }
            last_idx.push(t.pts.len().saturating_sub(1));
            t.leg_m.push((l.hi - l.lo) * f64::from(gf.edge(l.edge).len_dm) / 10.0);
        }
        // Le départ est l'origine.
        let o = t.pts.first().copied().unwrap_or([0.0, 0.0]);
        for p in &mut t.pts {
            *p = [p[0] - o[0], p[1] - o[1]];
        }
        t.cum = shape::cumulative(&t.pts);
        t.l_poly = t.cum.last().copied().unwrap_or(0.0);
        t.join_arc = last_idx[..last_idx.len().saturating_sub(1)].iter().map(|&i| t.cum[i]).collect();
        for j in &self.joins {
            if let Join::Hole(d) = j {
                t.holes_m += d;
                t.max_hole_m = t.max_hole_m.max(*d);
            }
        }
        t.d_graph = t.leg_m.iter().sum::<f64>() + t.holes_m;
        t
    }
}

/// Point de départ `(lat, lon)` de la marche : l'entrée de la première jambe.
pub fn start_latlon(gf: &MappedGraph, walk: &Walk) -> Option<(f64, f64)> {
    let first = walk.legs.first()?;
    let fr = frame_of(gf, first.edge);
    let full = edge_pts(gf, &fr, first.edge);
    let p: P2 = point_on(&full, first.entry());
    Some(fr.latlon(p))
}

#[cfg(test)]
mod tests {
    use ord_loops::{Mode, Request, RideType};
    use ord_loops_check::{Checker, Config};

    use super::*;
    use crate::likeness::fixture::{edge_between, graph, id, len_m};

    fn piece(gf: &MappedGraph, e: u32, reverse: bool, from_m: f32, to_m: f32) -> Piece {
        let elen = len_m(gf, e) as f32;
        let (from, to) = if reverse { (elen - from_m, elen - to_m) } else { (from_m, to_m) };
        Piece { edge: e, from, to, elen }
    }

    fn full(gf: &MappedGraph, from: i64, to: i64) -> Piece {
        let (e, rev) = edge_between(gf, from, to);
        let l = len_m(gf, e) as f32;
        piece(gf, e, rev, 0.0, l)
    }

    #[test]
    fn two_contiguous_pieces_of_an_edge_merge_and_a_noise_piece_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let (e, _) = edge_between(&gf, id(0, 0), id(0, 1));
        let l = len_m(&gf, e) as f32;
        let pieces = [
            piece(&gf, e, false, 0.0, 100.0),
            piece(&gf, e, false, 101.0, l),
            // 0,5 m : le bruit de recalage d'un carrefour
            Piece { edge: edge_between(&gf, id(0, 1), id(1, 1)).0, from: 0.0, to: 0.5, elen: 300.0 },
        ];
        let w = Walk::from_pieces(&gf, &pieces);
        assert_eq!(w.legs, vec![Leg { edge: e, reverse: false, lo: 0.0, hi: 1.0 }]);
        assert!(w.joins.is_empty() && w.is_continuous());
    }

    #[test]
    fn joins_are_nodes_then_a_hole_when_a_piece_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let ring =
            [full(&gf, id(0, 0), id(0, 1)), full(&gf, id(0, 1), id(1, 1)), full(&gf, id(1, 1), id(1, 0))];
        let w = Walk::from_pieces(&gf, &ring);
        assert_eq!(w.legs.len(), 3);
        assert!(w.is_continuous(), "{:?}", w.joins);
        // Sans le morceau du milieu : un trou d'environ 300 m entre (0,1) et (1,1).
        let w = Walk::from_pieces(&gf, &[ring[0], ring[2]]);
        match w.joins[..] {
            [Join::Hole(d)] => assert!((d - 300.0).abs() < 15.0, "{d}"),
            ref other => panic!("{other:?}"),
        }
        let t = w.trace(&gf);
        assert!((t.holes_m - t.max_hole_m).abs() < 1e-9 && t.holes_m > 250.0);
        assert!((t.d_graph - (t.leg_m.iter().sum::<f64>() + t.holes_m)).abs() < 1e-9);
    }

    #[test]
    fn a_continuous_walk_becomes_a_route_the_checker_reads_and_comes_back_equal() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let ring = [
            full(&gf, id(0, 0), id(0, 1)),
            full(&gf, id(0, 1), id(1, 1)),
            full(&gf, id(1, 1), id(1, 0)),
            full(&gf, id(1, 0), id(0, 0)),
        ];
        let w = Walk::from_pieces(&gf, &ring);
        let r = w.to_route(&gf);
        assert_eq!(Walk::from_route(&gf, &r), w);
        let (lat, lon) = start_latlon(&gf, &w).unwrap();
        let req = Request {
            start: ord_loops::LatLon::new(lat, lon),
            end: None,
            bearing: None,
            distance_m: w.trace(&gf).d_graph,
            ride_type: RideType::Road,
            mode: Mode::Discovery,
            familiarity: 0.5,
            off_team_cost_q8: 256,
            seed: 0,
        };
        let rep = Checker::new(&gf, None, Config::default()).check(&req, &r);
        let codes: Vec<&str> = rep.violations.iter().map(|v| v.code()).collect();
        for bad in ["EMPTY", "DISCONTINUOUS", "BAD_OFFSET", "NOT_ON_GRAPH"] {
            assert!(!codes.contains(&bad), "{bad} dans {codes:?}");
        }
        assert!(
            (rep.length_m - w.trace(&gf).d_graph).abs() < 5.0,
            "{} contre {}",
            rep.length_m,
            w.trace(&gf).d_graph
        );
    }

    #[test]
    fn the_trace_starts_at_the_origin_and_has_the_graph_length_of_its_legs() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let ring = [full(&gf, id(0, 0), id(0, 1)), full(&gf, id(0, 1), id(1, 1))];
        let w = Walk::from_pieces(&gf, &ring);
        let t = w.trace(&gf);
        assert_eq!(t.pts[0], [0.0, 0.0]);
        assert!((t.l_poly - 600.0).abs() < 10.0, "{}", t.l_poly);
        assert_eq!(t.join_arc.len(), 1);
        assert!((t.join_arc[0] - 300.0).abs() < 5.0);
        assert!((t.d_graph - t.leg_m.iter().sum::<f64>()).abs() < 1e-9);
    }
}
