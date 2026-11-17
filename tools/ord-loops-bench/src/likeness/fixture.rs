//! Un petit graphe synthétique pour les tests : une grille 3 × 3 de `residential` (pas de 300 m), dont l'arête
//! `(1,0)-(1,1)` est une `cycleway`. Aucune donnée de production.

use ord_graph::build::{build_graph, BuildOptions, MemSource};
use ord_graph::format::write_graph;
use ord_graph::mmap::MappedGraph;

const LAT0: f64 = 47.0;
const LON0: f64 = -1.80;
const STEP_M: f64 = 300.0;
const M_LAT: f64 = 111_194.926_6;

/// Id OSM du nœud `(r, c)`.
pub fn id(r: i64, c: i64) -> i64 {
    r * 10 + c + 1
}

pub fn graph(dir: &std::path::Path) -> MappedGraph {
    let m_lon = M_LAT * LAT0.to_radians().cos();
    let mut s = MemSource::default();
    for r in 0..3 {
        for c in 0..3 {
            s.node(id(r, c), LAT0 + r as f64 * STEP_M / M_LAT, LON0 + c as f64 * STEP_M / m_lon);
        }
    }
    let mut way = 1;
    for r in 0..3 {
        for c in 0..3 {
            for (dr, dc) in [(0, 1), (1, 0)] {
                if r + dr < 3 && c + dc < 3 {
                    let kind = if (r, c, dr, dc) == (1, 0, 0, 1) { "cycleway" } else { "residential" };
                    s.way(way, &[id(r, c), id(r + dr, c + dc)], &[("highway", kind)]);
                    way += 1;
                }
            }
        }
    }
    let mut data = build_graph(&mut s, &BuildOptions { zoom: 11 }, "test", &mut |_| {}).unwrap();
    let path = dir.join("g.ordg");
    write_graph(&path, &mut data, "test", 1_700_000_000).unwrap();
    MappedGraph::open_mmap(&path).unwrap()
}

/// L'arête entre les nœuds OSM `from` et `to`, et si on la parcourt de `b` vers `a` (`reverse`).
pub fn edge_between(gf: &MappedGraph, from: i64, to: i64) -> (u32, bool) {
    for e in 0..gf.header().edges as u32 {
        let (_, a, b) = gf.identity(e);
        if (a as i64, b as i64) == (from, to) {
            return (e, false);
        }
        if (a as i64, b as i64) == (to, from) {
            return (e, true);
        }
    }
    panic!("pas d'arête entre {from} et {to}");
}

/// Longueur de l'arête, m.
pub fn len_m(gf: &MappedGraph, e: u32) -> f64 {
    f64::from(gf.edge(e).len_dm) / 10.0
}
