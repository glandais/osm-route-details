//! Le parcours posé (`posed.rs`) : coutures, marges, projection monotone (note `ord-retouche-lot1.md` §10, tests 1 à 5), sur
//! des graphes faits à la main.

use ord_graph::build::MemSource;

use crate::engine::Engine;
use crate::posed::{PoseError, PoseTune, PosedInput, PosedJoin, PosedLeg, PosedRoute};
use crate::tests::{edge_between, engine_of, grid, ll, pt, tags};
use crate::{LatLon, RideType};

/// Identifiant OSM du nœud `(i, j)` d'une grille de 21 × 21.
pub(crate) fn nid(i: u64, j: u64) -> u64 {
    1 + i * 21 + j
}

/// Position `(i, j)` d'une grille de rues de 300 m.
pub(crate) fn at(i: u64, j: u64) -> LatLon {
    ll(i as f64 * 300.0, j as f64 * 300.0)
}

/// Les jambes entières et les jonctions de nœud d'un chemin de nœuds `(i, j)` de la grille.
pub(crate) fn walk_of(e: &Engine<Vec<u8>>, path: &[(u64, u64)]) -> (Vec<PosedLeg>, Vec<PosedJoin>) {
    let g = e.base();
    let mut legs = Vec::new();
    let mut joins = Vec::new();
    for w in path.windows(2) {
        let (p, q) = (nid(w[0].0, w[0].1), nid(w[1].0, w[1].1));
        let edge = edge_between(e, p, q);
        let (a, b) = g.edge_ends(edge);
        let reverse = g.osm_node_id(a) != p;
        legs.push(PosedLeg { edge, reverse, lo: 0.0, hi: 1.0 });
        joins.push(PosedJoin::Node(if reverse { a } else { b }));
    }
    joins.pop();
    (legs, joins)
}

pub(crate) fn line_of(path: &[(u64, u64)]) -> Vec<LatLon> {
    path.iter().map(|&(i, j)| at(i, j)).collect()
}

pub(crate) fn pose_path(e: &Engine<Vec<u8>>, path: &[(u64, u64)]) -> PosedRoute {
    let (legs, joins) = walk_of(e, path);
    let line = line_of(path);
    PosedRoute::pose(
        e,
        &PosedInput { ride_type: RideType::Road, legs: &legs, joins: &joins, line: &line },
        &PoseTune::default(),
    )
    .expect("parcours posé")
}

/// La rangée 0 vers l'est jusqu'à `(0, 10)`, puis la colonne 10 vers le nord jusqu'à `(10, 10)` : 6 000 m.
pub(crate) fn l_path() -> Vec<(u64, u64)> {
    let mut p: Vec<(u64, u64)> = (0..=10).map(|j| (0, j)).collect();
    p.extend((1..=10).map(|i| (i, 10)));
    p
}

#[test]
fn a_continuous_walk_has_its_seams_at_the_view_nodes_with_their_margins_and_abscissas() {
    let e = engine_of(&mut grid(21, 300.0));
    let p = pose_path(&e, &l_path());
    // 20 jambes, 19 jonctions, toutes des nœuds de vue (carrefours de la grille).
    assert_eq!(p.seams().len(), 19);
    assert!((p.length_m() - 6000.0).abs() < 5.0, "{}", p.length_m());
    for (k, s) in p.seams().iter().enumerate() {
        let want = 300.0 * (k + 1) as f64;
        assert!((s.s_m - want).abs() < 2.0, "couture {k} à {} au lieu de {want}", s.s_m);
        assert!((s.before_m - want).abs() < 2.0 && (s.after_m - (6000.0 - want)).abs() < 2.0);
        assert!(p.valid_both(k));
    }
    // La marge de 100 m : une première jambe de 60 m écarte la première couture.
    let (mut legs, joins) = walk_of(&e, &l_path());
    legs[0].lo = 0.8;
    let mut line = line_of(&l_path());
    line[0] = ll(0.0, 240.0);
    let q = PosedRoute::pose(
        &e,
        &PosedInput { ride_type: RideType::Road, legs: &legs, joins: &joins, line: &line },
        &PoseTune::default(),
    )
    .expect("posé");
    assert!(!q.valid_both(0) && q.valid_both(1), "{:?}", q.seams()[0]);
    assert_eq!(q.stats().refused_by_rule[2], 1);
}

#[test]
fn a_u_turn_is_not_a_seam_and_holes_and_touches_cut_the_runs() {
    let e = engine_of(&mut grid(21, 300.0));
    // Aller-retour sur la même arête : même arête, refusé par la règle 3.
    let back = [(0, 0), (0, 1), (0, 0)];
    let p = pose_path(&e, &back);
    assert!(p.seams().is_empty());
    assert_eq!(p.stats().refused_by_rule[1], 1);
    // Un trou de 300 m entre `(0,1)` et `(0,2)` : deux plages, et la jonction du trou n'est pas une couture.
    let (l1, _) = walk_of(&e, &[(0, 0), (0, 1), (0, 2)]);
    let (l2, _) = walk_of(&e, &[(0, 2), (0, 3), (0, 4)]);
    let legs = [l1[0], l1[1], l2[0], l2[1]];
    let node_of = |l: &PosedLeg| {
        let (a, b) = e.base().edge_ends(l.edge);
        if l.reverse {
            a
        } else {
            b
        }
    };
    let joins = [
        PosedJoin::Node(node_of(&legs[0])),
        PosedJoin::Hole { chord_m: 300.0 },
        PosedJoin::Node(node_of(&legs[2])),
    ];
    let line = line_of(&[(0, 0), (0, 1), (0, 2), (0, 2), (0, 3), (0, 4)]);
    let tune = PoseTune { max_drift: 0.5, ..PoseTune::default() };
    let p = PosedRoute::pose(
        &e,
        &PosedInput { ride_type: RideType::Road, legs: &legs, joins: &joins, line: &line },
        &tune,
    )
    .expect("posé");
    assert_eq!(p.stats().runs, 2);
    assert_eq!(p.stats().holes, 1);
    assert_eq!(p.seams().len(), 2, "une couture par plage : {:?}", p.seams());
    // Un contact (`Touch`) coupe aussi.
    let joins = [joins[0], PosedJoin::Touch, joins[2]];
    let t = PosedRoute::pose(
        &e,
        &PosedInput { ride_type: RideType::Road, legs: &legs, joins: &joins, line: &line },
        &tune,
    )
    .expect("posé");
    assert_eq!((t.stats().touches, t.stats().runs), (1, 2));
}

/// Un virage de 170° à un carrefour : `1 → 2` vers l'est, `2 → 3` qui repart presque en arrière, `2 → 4` au nord.
fn hairpin_graph() -> MemSource {
    let mut s = MemSource::default();
    pt(&mut s, 1, 0.0, 0.0);
    pt(&mut s, 2, 0.0, 400.0);
    pt(&mut s, 3, -70.0, 0.0);
    pt(&mut s, 4, 400.0, 400.0);
    s.way(101, &[1, 2], &tags("residential", &[]));
    s.way(102, &[2, 3], &tags("residential", &[]));
    s.way(103, &[2, 4], &tags("residential", &[]));
    s
}

#[test]
fn a_turn_of_more_than_150_degrees_is_not_a_seam() {
    let mut src = hairpin_graph();
    let e = engine_of(&mut src);
    let g = e.base();
    let leg = |a: u64, b: u64| {
        let edge = edge_between(&e, a, b);
        let (na, _) = g.edge_ends(edge);
        PosedLeg { edge, reverse: g.osm_node_id(na) != a, lo: 0.0, hi: 1.0 }
    };
    let (x, y) = (leg(1, 2), leg(2, 3));
    let node = |l: &PosedLeg| {
        let (a, b) = g.edge_ends(l.edge);
        if l.reverse {
            a
        } else {
            b
        }
    };
    let legs = [x, y];
    let joins = [PosedJoin::Node(node(&x))];
    let line = [ll(0.0, 0.0), ll(0.0, 400.0), ll(-70.0, 0.0)];
    let p = PosedRoute::pose(
        &e,
        &PosedInput { ride_type: RideType::Road, legs: &legs, joins: &joins, line: &line },
        &PoseTune { max_drift: 0.5, ..PoseTune::default() },
    )
    .expect("posé");
    assert!(p.seams().is_empty());
    assert_eq!(p.stats().refused_by_rule[1], 1);
}

#[test]
fn misordered_or_duplicated_pieces_are_refused_at_the_door() {
    let e = engine_of(&mut grid(21, 300.0));
    let path = l_path();
    let (legs, joins) = walk_of(&e, &path);
    // La polyligne d'origine dont deux tronçons sont permutés (les points 6 à 15 passent après le dernier) : les coutures
    // se projettent hors de l'ordre.
    let mut swapped = path[..6].to_vec();
    swapped.extend_from_slice(&path[15..]);
    swapped.extend_from_slice(&path[6..15]);
    let swapped = line_of(&swapped);
    let lax = PoseTune { max_drift: 10.0, ..PoseTune::default() };
    let err = PosedRoute::pose(
        &e,
        &PosedInput { ride_type: RideType::Road, legs: &legs, joins: &joins, line: &swapped },
        &lax,
    )
    .unwrap_err();
    assert!(matches!(err, PoseError::OrderAnomaly { seams } if seams > 0), "{err:?}");
    // Des jambes en trop pour la polyligne : la dérive.
    let short = line_of(&path[..=8]);
    let err = PosedRoute::pose(
        &e,
        &PosedInput { ride_type: RideType::Road, legs: &legs, joins: &joins, line: &short },
        &PoseTune::default(),
    )
    .unwrap_err();
    assert_eq!(err, PoseError::Drift);
    // Une marche vide et une arête inconnue.
    let empty = PosedRoute::pose(
        &e,
        &PosedInput { ride_type: RideType::Road, legs: &[], joins: &[], line: &short },
        &PoseTune::default(),
    );
    assert_eq!(empty.unwrap_err(), PoseError::Empty);
    let bad = [PosedLeg { edge: u32::MAX - 1, reverse: false, lo: 0.0, hi: 1.0 }];
    let err = PosedRoute::pose(
        &e,
        &PosedInput { ride_type: RideType::Road, legs: &bad, joins: &[], line: &short },
        &PoseTune::default(),
    );
    assert_eq!(err.unwrap_err(), PoseError::NotOnGraph);
}

#[test]
fn a_node_passed_twice_has_two_increasing_abscissas_and_laps_do_not_merge() {
    let e = engine_of(&mut grid(21, 300.0));
    // Deux tours d'un carré de 4 × 600 m : le nœud `(0, 2)` est passé à 600 m et à 3 000 m.
    let lap: Vec<(u64, u64)> = vec![(0, 0), (0, 1), (0, 2), (1, 2), (2, 2), (2, 1), (2, 0), (1, 0), (0, 0)];
    let mut path = lap.clone();
    path.extend_from_slice(&lap[1..]);
    let p = pose_path(&e, &path);
    let ends = e.base().edge_ends(edge_between(&e, nid(0, 1), nid(0, 2)));
    let n02 = if e.base().osm_node_id(ends.0) == nid(0, 2) { ends.0 } else { ends.1 };
    let at_node: Vec<f64> = p.seams().iter().filter(|s| s.node == n02).map(|s| s.s_m).collect();
    assert_eq!(at_node.len(), 2, "{:?}", p.seams());
    assert!((at_node[0] - 600.0).abs() < 2.0 && (at_node[1] - 600.0 - 2400.0).abs() < 2.0, "{at_node:?}");
    assert!(p.seams().windows(2).all(|w| w[1].s_m > w[0].s_m));
}

#[test]
fn the_original_geometry_is_cut_at_the_asked_abscissas() {
    let e = engine_of(&mut grid(21, 300.0));
    let p = pose_path(&e, &l_path());
    let g = p.line_between(450.0, 1950.0);
    let d = |a: LatLon, b: LatLon| {
        ((a.lat - b.lat) * crate::tests::M_LAT).hypot((a.lon - b.lon) * crate::tests::m_lon())
    };
    // Le début est au milieu de la deuxième arête (150 m après `(0, 1)`), la fin 150 m après `(0, 6)`.
    assert!(d(g[0], ll(0.0, 450.0)) < 0.5, "{}", d(g[0], ll(0.0, 450.0)));
    assert!(d(*g.last().unwrap(), ll(0.0, 1950.0)) < 0.5);
    // Les sommets entre les deux sont ceux de la polyligne d'origine (600 à 1800).
    assert_eq!(g.len(), 2 + 5);
    let len: f64 = g.windows(2).map(|w| d(w[0], w[1])).sum();
    assert!((len - 1500.0).abs() < 1.0, "{len}");
    assert!(p.line_between(-10.0, 1e9).len() >= 2);
}
