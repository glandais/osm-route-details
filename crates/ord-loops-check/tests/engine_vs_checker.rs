//! Propriété du §9 point 4 : sur des grilles à sens uniques tirés au hasard, avec une surcouche d'équipe tirée au hasard
//! (trous courts compris), tout parcours que le moteur rend, **dans les trois modes**, passe le contrôleur
//! indépendant. Le moteur n'est appelé que comme producteur de parcours (et pour construire `TeamData`).

use ord_graph::build::{build_graph, BuildOptions, MemSource};
use ord_graph::format::{write_graph, GraphFile};
use ord_graph::overlay::{encode, Overlay, OverlayMeta};
use ord_graph::util::SplitMix64;
use ord_graph::view::{build_view, ViewSpec};
use ord_loops::{
    generate, Engine, Family, FigureMix, Figures, LatLon, Mode, Organic, OrganicTune, Params, Request,
    RideType, TeamData, Workspace,
};
use ord_loops_check::{Checker, Config};

const LAT0: f64 = 47.2;
const LON0: f64 = -1.56;
const M_LAT: f64 = 111_194.926_6;

fn grid(seed: u64, n: i64, step: f64) -> MemSource {
    let m_lon = M_LAT * LAT0.to_radians().cos();
    let mut s = MemSource::default();
    let id = |r: i64, c: i64| r * 100 + c + 1;
    for r in 0..n {
        for c in 0..n {
            s.node(id(r, c), LAT0 + r as f64 * step / M_LAT, LON0 + c as f64 * step / m_lon);
        }
    }
    let mut rng = SplitMix64(seed);
    let mut way = 1;
    for r in 0..n {
        for c in 0..n {
            for (dr, dc) in [(0, 1), (1, 0)] {
                let (r2, c2) = (r + dr, c + dc);
                if r2 >= n || c2 >= n {
                    continue;
                }
                let roll = rng.below(6);
                let oneway = match roll {
                    0 => "yes",
                    1 => "-1",
                    _ => "no",
                };
                s.way(way, &[id(r, c), id(r2, c2)], &[("highway", "residential"), ("oneway", oneway)]);
                way += 1;
            }
        }
    }
    s
}

fn open(mut data: ord_graph::build::GraphData, dir: &std::path::Path, name: &str) -> GraphFile<Vec<u8>> {
    let path = dir.join(name);
    write_graph(&path, &mut data, "test", 1_700_000_000).unwrap();
    let g = GraphFile::open(std::fs::read(path).unwrap()).unwrap();
    g.verify().unwrap();
    g
}

/// Une surcouche où chaque arête est roulée avec la probabilité `keep` sur 100 (les autres sont des trous).
fn random_overlay(graph: &GraphFile<Vec<u8>>, seed: u64, keep: u64) -> Overlay {
    let mut rng = SplitMix64(seed ^ 0x9e37_79b9);
    let mut edges: Vec<u32> = Vec::new();
    let mut counts: Vec<u16> = Vec::new();
    for e in 0..graph.header().edges as u32 {
        if (rng.below(100) as u64) < keep {
            edges.push(e);
            counts.extend([1u16, 1, 0, 0, 0, 0, 0, 0]);
        }
    }
    let meta = OverlayMeta {
        graph: graph.graph_identity(),
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

/// Les figures organiques du 4c, tous les premiers parcours préférés triangles.
fn organic() -> Figures {
    Figures::Organic(Organic {
        mix: FigureMix { tri_q8: 256, rd_tri_milli: [230, 270, 318, 360, 420] },
        tune: OrganicTune::default(),
    })
}

#[test]
fn every_route_the_engine_returns_passes_the_independent_checker() {
    check_all(Figures::Classic);
}

/// Test 12 du 4c (I3) : tout parcours rendu en organique, triangles compris, passe le contrôleur indépendant.
#[test]
fn every_organic_route_passes_the_independent_checker_and_triangles_are_among_them() {
    let triangles = check_all(organic());
    assert!(triangles > 0, "aucun triangle rendu : le test ne prouve rien pour eux");
}

/// Rend le nombre de triangles contrôlés.
fn check_all(figures: Figures) -> usize {
    let mut triangles = 0usize;
    let mut returned = [0usize; 3];
    let mut refused: Vec<String> = Vec::new();
    for seed in 0..10u64 {
        let dir = tempfile::tempdir().unwrap();
        let mut src = grid(seed, 9, 500.0);
        let data = build_graph(&mut src, &BuildOptions { zoom: 11 }, "test", &mut |_| {}).unwrap();
        let base = open(data, dir.path(), "base.ordg");
        let road =
            open(build_view(&base, &ViewSpec::parse("road").unwrap()).unwrap(), dir.path(), "road.ordv");
        let gravel =
            open(build_view(&base, &ViewSpec::parse("gravel").unwrap()).unwrap(), dir.path(), "gravel.ordv");
        // Surcouche tirée avant de passer le graphe au moteur : 85 % des arêtes roulées, le reste en trous.
        let overlay = random_overlay(&base, seed, 85);
        let engine = Engine::new(base, road, gravel, Params::default()).unwrap();
        let team = TeamData::build(&engine, &overlay).unwrap();
        let checker = Checker::new(engine.base(), Some(&overlay), Config::default());
        let mut ws = Workspace::new();
        for (k, d) in [6_000.0, 9_000.0].into_iter().enumerate() {
            for (mi, mode) in [Mode::Strict, Mode::TeamFirst, Mode::Discovery].into_iter().enumerate() {
                let req = Request {
                    start: LatLon::new(
                        LAT0 + 2_000.0 / M_LAT,
                        LON0 + 2_000.0 / (M_LAT * LAT0.to_radians().cos()),
                    ),
                    end: None,
                    bearing: None,
                    distance_m: d,
                    ride_type: RideType::Road,
                    mode,
                    familiarity: 0.5,
                    off_team_cost_q8: ord_loops::DEFAULT_OFF_TEAM_COST_Q8,
                    seed: seed * 10 + k as u64,
                    figures,
                };
                let Ok(g) = generate(&engine, Some(&team), &req, &mut ws, &mut || false) else { continue };
                for r in &g.routes {
                    returned[mi] += 1;
                    triangles += usize::from(r.family == Family::Triangle);
                    let rep = checker.check(&req, r);
                    if !rep.ok() {
                        refused.push(format!("graine {seed}, D {d}, {mode:?} : {:?}", rep.violations));
                    }
                }
            }
        }
    }
    for (mi, name) in ["strict", "équipe d'abord", "découverte"].into_iter().enumerate() {
        assert!(returned[mi] > 0, "le moteur n'a rendu aucun parcours en {name} : le test ne prouve rien");
    }
    assert!(refused.is_empty(), "{} parcours refusés sur {returned:?} : {refused:?}", refused.len());
    triangles
}
