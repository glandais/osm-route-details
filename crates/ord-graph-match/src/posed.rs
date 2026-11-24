//! Du document `network_edges` d'une trace à l'entrée de la retouche (lot 7b, note `ord-retouche-lot1.md` §15.3) : la
//! marche que la surcouche a comptée, et les étendues de l'axe du document qui n'ont pas été appariées.
//!
//! La règle d'appariement est **exactement celle du dépôt** ([`crate::deposit::Deposit::push_track`]) : chaque arête passe
//! par [`match_job`] puis [`verdict`] ; une chaîne retenue (`Counted`) ajoute ses morceaux dans l'ordre, tout le reste fait
//! un trou. La marche est donc celle que la surcouche a comptée.
//!
//! Les données lues sont **privées** : rien d'ici n'est journalisé.

use ord_graph::mmap::MappedGraph;
use ord_graph::util::Poly;
use ord_loops::{PosedJoin, PosedLeg};

use crate::agg::{verdict, Verdict};
use crate::doc::ParsedEdge;
use crate::index::Near;
use crate::job::match_job;
use crate::walk::{Join, Walk};

/// Un écart de l'axe du document plus court que cela (arrondis entre deux arêtes voisines) n'est pas un trou d'appariement.
const MIN_GAP_M: f64 = 1.0;

/// Une trace posée depuis son document : la marche, et les étendues non appariées sur l'axe du document.
#[derive(Clone, Debug)]
pub struct DocWalk {
    pub walk: Walk,
    /// Étendues `[fm, tm]` de l'axe du document sans arête retenue (arêtes non comptées, écarts entre arêtes, bouts), de
    /// plus de [`MIN_GAP_M`] ; triées, disjointes. `None` si une arête n'a pas `fm` et `tm` : rien n'est localisable.
    pub unmatched: Option<Vec<(f64, f64)>>,
    /// Somme des `trv` des arêtes non comptées (la borne haute quand `unmatched` est `None`).
    pub unmatched_trv_m: f64,
    /// Longueur de l'axe du document : `len` du document s'il est là et utilisable, sinon le plus grand `tm`.
    pub axis_m: f64,
}

/// Apparie les arêtes d'un document, dans l'ordre, et en tire la marche. `doc_len` : le `len` du document
/// ([`crate::doc::Document::len_m`]). `stop` est lu entre deux arêtes ; `None` : arrêté.
pub fn doc_walk(
    gf: &MappedGraph,
    idx: &dyn Near,
    poly: &Poly,
    edges: &[ParsedEdge],
    doc_len: Option<f64>,
    stop: &dyn Fn() -> bool,
) -> Option<DocWalk> {
    let mut pieces = Vec::new();
    let mut counted: Vec<(f64, f64)> = Vec::new();
    let mut locatable = true;
    let mut unmatched_trv_m = 0.0;
    let mut max_tm = 0.0f64;
    for e in edges {
        if stop() {
            return None;
        }
        let job = match_job(gf, idx, poly, &e.g, e.trv);
        let (fm, tm) = (f64::from(e.fm), f64::from(e.tm));
        let here = fm.is_finite() && tm.is_finite() && tm >= fm;
        if here {
            max_tm = max_tm.max(tm);
        } else {
            locatable = false;
        }
        match verdict(e.wk, &job) {
            Verdict::Counted(chain) => {
                pieces.extend_from_slice(&chain.pieces);
                if here {
                    counted.push((fm, tm));
                }
            }
            Verdict::Outside | Verdict::Fail { .. } => {
                let trv = if e.trv.is_nan() { job.glen } else { e.trv };
                unmatched_trv_m += f64::from(trv);
            }
        }
    }
    let walk = Walk::from_pieces(gf, &pieces);
    let axis_m = doc_len.filter(|l| l.is_finite() && *l > 0.0).unwrap_or(max_tm);
    let unmatched = locatable.then(|| complement(&mut counted, axis_m));
    Some(DocWalk { walk, unmatched, unmatched_trv_m, axis_m })
}

/// Le complément des étendues `covered` dans `[0, axis_m]`, sans les écarts de moins de [`MIN_GAP_M`].
fn complement(covered: &mut [(f64, f64)], axis_m: f64) -> Vec<(f64, f64)> {
    covered.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut at = 0.0f64;
    for &(a, b) in covered.iter() {
        if a - at > MIN_GAP_M {
            out.push((at, a));
        }
        at = at.max(b);
    }
    if axis_m - at > MIN_GAP_M {
        out.push((at, axis_m));
    }
    out
}

/// Les jambes et les jonctions d'une marche, au format de l'entrée du moteur.
pub fn posed_parts(walk: &Walk) -> (Vec<PosedLeg>, Vec<PosedJoin>) {
    let legs =
        walk.legs.iter().map(|l| PosedLeg { edge: l.edge, reverse: l.reverse, lo: l.lo, hi: l.hi }).collect();
    let joins = walk
        .joins
        .iter()
        .map(|j| match j {
            Join::Node(n) => PosedJoin::Node(*n),
            Join::Touch => PosedJoin::Touch,
            Join::Hole(d) => PosedJoin::Hole { chord_m: *d },
        })
        .collect();
    (legs, joins)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{graph, id};
    use crate::geo::encode_polyline6;
    use crate::index::SegIndex;

    fn poly() -> Poly {
        Poly::parse("t\n1\n -4.0 45.0\n 2.0 45.0\n 2.0 49.0\n -4.0 49.0\nEND\nEND\n").unwrap()
    }

    /// L'arête de la grille de 3 × 3 entre deux de ses carrefours, en polyligne, avec son étendue sur l'axe.
    fn edge(gf: &MappedGraph, a: (i64, i64), b: (i64, i64), fm: f32, tm: f32) -> ParsedEdge {
        let pos = |(r, c): (i64, i64)| {
            let n = (0..gf.header().nodes as u32)
                .find(|&n| gf.osm_node_id(n) as i64 == id(r, c))
                .expect("carrefour");
            let p = gf.node_pos(n);
            [f64::from(p.0) * 1e-7, f64::from(p.1) * 1e-7]
        };
        let (pa, pb) = (pos(a), pos(b));
        ParsedEdge {
            g: encode_polyline6(&[pa, pb]),
            trv: (tm - fm).max(0.0),
            len: tm - fm,
            fm,
            tm,
            from_off: f32::NAN,
            to_off: f32::NAN,
            from_node: -1,
            to_node: -1,
            wk: 2,
            ow: 0,
            sc_unpaved: false,
        }
    }

    #[test]
    fn a_readable_edge_counts_an_unreadable_one_makes_a_hole_and_an_extent() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let idx = SegIndex::build(&gf);
        let mut bad = edge(&gf, (0, 1), (0, 2), 300.0, 600.0);
        bad.g = String::from("pas une polyligne");
        let edges = [edge(&gf, (0, 0), (0, 1), 0.0, 300.0), bad, edge(&gf, (0, 2), (1, 2), 600.0, 900.0)];
        let w = doc_walk(&gf, &idx, &poly(), &edges, Some(900.0), &|| false).expect("pas d'arrêt");
        assert_eq!(w.walk.legs.len(), 2, "deux arêtes comptées");
        assert!(matches!(w.walk.joins[0], Join::Hole(_)));
        assert_eq!(w.unmatched, Some(vec![(300.0, 600.0)]));
        assert!((w.unmatched_trv_m - 300.0).abs() < 1e-3);
        assert_eq!(w.axis_m, 900.0);
        let (legs, joins) = posed_parts(&w.walk);
        assert_eq!((legs.len(), joins.len()), (2, 1));
        assert!(matches!(joins[0], PosedJoin::Hole { .. }));
    }

    #[test]
    fn the_ends_of_the_axis_count_and_a_missing_extent_hides_every_position() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let idx = SegIndex::build(&gf);
        let edges = [edge(&gf, (0, 0), (0, 1), 100.0, 400.0)];
        // `len` plus long que la dernière arête : le bout, et le début avant `fm`.
        let w = doc_walk(&gf, &idx, &poly(), &edges, Some(700.0), &|| false).unwrap();
        assert_eq!(w.unmatched, Some(vec![(0.0, 100.0), (400.0, 700.0)]));
        // Sans `len` : le plus grand `tm`.
        let w = doc_walk(&gf, &idx, &poly(), &edges, None, &|| false).unwrap();
        assert_eq!((w.axis_m, w.unmatched), (400.0, Some(vec![(0.0, 100.0)])));
        // Un `fm` absent : rien n'est localisable, la somme des `trv` reste.
        let mut no_fm = edge(&gf, (0, 0), (0, 1), 0.0, 300.0);
        no_fm.fm = f32::NAN;
        no_fm.g = String::new();
        let w = doc_walk(&gf, &idx, &poly(), &[no_fm], None, &|| false).unwrap();
        assert!(w.unmatched.is_none());
        assert!((w.unmatched_trv_m - 300.0).abs() < 1e-3);
        assert!(w.walk.legs.is_empty());
    }

    #[test]
    fn a_stop_between_two_edges_gives_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let idx = SegIndex::build(&gf);
        let edges = [edge(&gf, (0, 0), (0, 1), 0.0, 300.0)];
        assert!(doc_walk(&gf, &idx, &poly(), &edges, None, &|| true).is_none());
    }

    #[test]
    fn a_gap_below_a_metre_is_not_a_hole() {
        let mut covered = [(0.0, 100.0), (100.4, 200.0)];
        assert!(complement(&mut covered, 200.0).is_empty());
        let mut covered = [(50.0, 80.0), (0.0, 60.0)];
        assert_eq!(complement(&mut covered, 100.0), vec![(80.0, 100.0)]);
    }
}
