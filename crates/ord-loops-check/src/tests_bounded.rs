//! Tables bornées du contrôleur (note lot 6, §14, point 8) : celles de la zone sont celles du graphe entier, y compris
//! avec une arête plus large qu'une cellule.

use ord_graph::build::{build_graph, BuildOptions, MemSource};
use ord_graph::format::{write_graph, GraphFile};
use ord_graph::overlay::{encode, Overlay, OverlayMeta};
use ord_graph::util::SplitMix64;
use ord_loops::RideType;

use crate::bounded::degrees_of;
use crate::rules::{admitted, Admission};
use crate::{Config, GraphIndex, TeamCheck};

const LAT0: f64 = 47.2;
const LON0: f64 = -1.56;
const M_LAT: f64 = 111_194.926_6;
const N: usize = 16;
const CELL: f64 = 140.0;

fn id(i: usize, j: usize) -> i64 {
    (1 + i * N + j) as i64
}

/// Grille de 16 × 16 carrefours de 140 m, construite à z16 (tuiles de ~400 m), avec **une arête expresse** de 2,9 km
/// d'un coin à l'autre : plus large qu'une cellule.
fn graph() -> (GraphFile<Vec<u8>>, tempfile::TempDir) {
    let mut s = MemSource::default();
    let m_lon = M_LAT * LAT0.to_radians().cos();
    for i in 0..N {
        for j in 0..N {
            s.node(id(i, j), LAT0 + i as f64 * CELL / M_LAT, LON0 + j as f64 * CELL / m_lon);
        }
    }
    let mut rng = SplitMix64(3);
    let mut way = 100i64;
    for i in 0..N {
        for j in 0..N {
            for (di, dj) in [(0, 1), (1, 0)] {
                let (ni, nj) = (i + di, j + dj);
                if ni < N && nj < N && rng.below(100) >= 6 {
                    way += 1;
                    s.way(way, &[id(i, j), id(ni, nj)], &[("highway", "residential")]);
                }
            }
        }
    }
    s.way(9_000, &[id(0, 0), id(N - 1, N - 1)], &[("highway", "tertiary")]);
    let dir = tempfile::tempdir().unwrap();
    let mut data =
        build_graph(&mut s, &BuildOptions { zoom: 16 }, "test", &mut |_| {}).expect("construction");
    let path = dir.path().join("base.ordg");
    write_graph(&path, &mut data, "test", 1_700_000_000).expect("écriture");
    let g = GraphFile::open(std::fs::read(path).unwrap()).expect("ouverture");
    g.verify().expect("verify");
    (g, dir)
}

fn overlay(g: &GraphFile<Vec<u8>>, seed: u64) -> Overlay {
    let mut rng = SplitMix64(seed);
    let pos = |osm: u64| ((osm - 1) as usize / N, (osm - 1) as usize % N);
    let (mut edges, mut counts) = (Vec::new(), Vec::new());
    for e in 0..g.header().edges as u32 {
        let (a, b) = g.edge_ends(e);
        let (pa, pb) = (pos(g.osm_node_id(a)), pos(g.osm_node_id(b)));
        let inside = |p: (usize, usize)| p.0 >= 2 && p.0 <= 11 && p.1 >= 3 && p.1 <= 12;
        if inside(pa) && inside(pb) && rng.below(100) < 75 {
            edges.push(e);
            counts.extend_from_slice(&[1u16, 1, 0, 0, 0, 0, 0, 0]);
        }
    }
    let meta = OverlayMeta {
        graph: g.graph_identity(),
        team_id: 1,
        export_xxh3: 0,
        export_rows: 0,
        params_hash: 0,
        tool_version: "test".into(),
        build_epoch: 0,
        types: vec!["ROAD".into(), "GRAVEL".into(), "MTB".into(), "MIXED".into()],
    };
    Overlay::decode(&encode(&meta, &edges, &counts).unwrap()).unwrap()
}

#[test]
fn the_wide_edge_is_listed_and_only_it() {
    let (g, _d) = graph();
    let gi = GraphIndex::build(&g);
    assert_eq!(gi.wide_edges().len(), 1, "{:?}", gi.wide_edges());
    let (a, b) = g.edge_ends(gi.wide_edges()[0]);
    assert_eq!((g.osm_node_id(a), g.osm_node_id(b)), (1, (N * N) as u64));
}

#[test]
fn the_degrees_of_route_nodes_are_the_degrees_of_the_whole_graph() {
    let (g, _d) = graph();
    let gi = GraphIndex::build(&g);
    for adm in [Admission::Road, Admission::Gravel] {
        let mut all = vec![0u8; g.header().nodes as usize];
        for e in 0..g.header().edges as u32 {
            if admitted(adm, g.edge(e).attrs) {
                let (a, b) = g.edge_ends(e);
                all[a as usize] += 1;
                all[b as usize] += 1;
            }
        }
        let nodes: Vec<u32> = (0..g.header().nodes as u32).collect();
        let got = degrees_of(&g, &gi, adm, &nodes);
        for &n in &nodes {
            assert_eq!(got[&n], all[n as usize], "nœud {n}");
        }
        // Un seul nœud, au bout de l'arête expresse : son degré compte l'arête larges possédée loin de lui.
        let corner = (0..g.header().nodes as u32).find(|&n| g.osm_node_id(n) == (N * N) as u64).unwrap();
        assert_eq!(degrees_of(&g, &gi, adm, &[corner])[&corner], all[corner as usize]);
        // Sans l'exception, le coin perdrait l'arête expresse.
        let blind = degrees_of(&g, &GraphIndex::default(), adm, &[corner])[&corner];
        assert_eq!(blind + 1, all[corner as usize], "l'exception est nécessaire");
    }
}

#[test]
fn the_bridged_edges_of_the_zone_are_those_of_the_whole_graph() {
    let (g, _d) = graph();
    let gi = GraphIndex::build(&g);
    let mut bridged_total = 0;
    for seed in [1u64, 2, 3, 4, 5] {
        let ov = overlay(&g, seed);
        let cfg = Config { hole_m: 300.0, connector_m: 150.0, ..Config::default() };
        let (zone, whole) =
            (TeamCheck::build(&g, &gi, &ov, &cfg), TeamCheck::build_on(&g, &gi, &ov, &cfg, true));
        for ride in [RideType::Road, RideType::Gravel, RideType::Mtb] {
            assert_eq!(zone.bridged(ride), whole.bridged(ride), "seed {seed} {ride:?}");
            bridged_total += zone.bridged(ride).len();
        }
    }
    assert!(bridged_total > 20, "le scénario doit combler des trous : {bridged_total}");
}

#[test]
fn a_planned_build_gives_the_same_tables_and_declares_its_transient_and_resident_bytes() {
    let (g, _d) = graph();
    let gi = GraphIndex::build(&g);
    let ov = overlay(&g, 1);
    let cfg = Config { hole_m: 300.0, connector_m: 150.0, ..Config::default() };
    let plan = TeamCheck::plan(&g, &ov);
    assert!(plan.estimated_bytes > 0, "la construction tient de la mémoire transitoire");
    let (planned, direct) =
        (TeamCheck::build_planned(&g, &gi, &ov, &cfg, &plan), TeamCheck::build(&g, &gi, &ov, &cfg));
    for ride in [RideType::Road, RideType::Gravel, RideType::Mtb] {
        assert_eq!(planned.bridged(ride), direct.bridged(ride));
    }
    // Les octets tenus comptent les tables, et la surcouche décodée a les siens.
    assert!(planned.bytes() >= std::mem::size_of::<TeamCheck>());
    assert!(ov.bytes() > ov.len() * 4);
}
