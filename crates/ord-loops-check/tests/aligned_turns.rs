//! Test 10 du §16.7 de la note du lot 4 (4c, étape « virages ») : le contrôle interne `Classic` prend ses caps dans un
//! repère centré au **départ** du parcours, le contrôleur indépendant et le contrôle aligné dans un repère centré au
//! **nœud**. À 40 km au nord du départ, à 47,2° de latitude, un virage de 150,02° au nœud ne fait plus que 149,8° dans
//! le repère du départ : le contrôle `Classic` l'accepte, le contrôle aligné et le contrôleur le refusent.

use ord_graph::build::{build_graph, BuildOptions, MemSource};
use ord_graph::format::{write_graph, GraphFile};
use ord_graph::view::{build_view, ViewSpec};
use ord_loops::{
    check_route, check_route_aligned, Engine, Family, LatLon, Mode, Params, Position, Request, RideType,
    Route, Step, Violation,
};
use ord_loops_check::{Checker, Config};

const LAT0: f64 = 47.2;
const LON0: f64 = -1.56;
const M_LAT: f64 = 111_194.926_6;
const RES: &[(&str, &str)] = &[("highway", "residential")];

fn open(mut data: ord_graph::build::GraphData, dir: &std::path::Path, name: &str) -> GraphFile<Vec<u8>> {
    let path = dir.join(name);
    write_graph(&path, &mut data, "test", 1_700_000_000).unwrap();
    let g = GraphFile::open(std::fs::read(path).unwrap()).unwrap();
    g.verify().unwrap();
    g
}

/// Ce que vaut le virage en J d'un parcours S → J → K dont les deux arêtes sont droites, dans le repère équirectangulaire
/// centré en `origin` (latitude en degrés) : les deux caps sont ceux des deux segments, pris près de J.
fn turn_in_frame(origin_lat: f64, d_in: (f64, f64), d_out: (f64, f64)) -> f64 {
    let kx = M_LAT * origin_lat.to_radians().cos();
    let h = |d: (f64, f64)| (d.1 * kx).atan2(d.0 * M_LAT).to_degrees();
    let t = (h(d_out) - h(d_in)).rem_euclid(360.0);
    if t > 180.0 {
        360.0 - t
    } else {
        t
    }
}

#[test]
fn an_angle_of_150_02_degrees_at_the_node_is_refused_by_the_aligned_check_and_the_checker_not_by_the_classic_one(
) {
    let north_m = 40_000.0;
    let j_lat = LAT0 + north_m / M_LAT;
    let kx_j = M_LAT * j_lat.to_radians().cos();
    let (h_in, h_out) = (20.0f64.to_radians(), (20.0 + 150.02f64).to_radians());
    // S est au sud-ouest de J : le dernier mètre de l'arête S-J est de cap 20° au nœud.
    let (s_lat, s_lon) = (LAT0, LON0);
    let j_lon = s_lon + north_m * h_in.tan() / kx_j;
    // K, à 2 km de J, de cap 170,02° au nœud ; L, une troisième arête pour que J soit un carrefour.
    let (k_lat, k_lon) = (j_lat + 2000.0 * h_out.cos() / M_LAT, j_lon + 2000.0 * h_out.sin() / kx_j);
    let (l_lat, l_lon) = (j_lat + 1000.0 / M_LAT, j_lon - 1000.0 / kx_j);
    let mut src = MemSource::default();
    src.node(1, s_lat, s_lon).node(2, j_lat, j_lon).node(3, k_lat, k_lon).node(4, l_lat, l_lon);
    src.way(1, &[1, 2], RES).way(2, &[2, 3], RES).way(3, &[2, 4], RES);
    let dir = tempfile::tempdir().unwrap();
    let data = build_graph(&mut src, &BuildOptions { zoom: 11 }, "test", &mut |_| {}).unwrap();
    let base = open(data, dir.path(), "base.ordg");
    let road = open(build_view(&base, &ViewSpec::parse("road").unwrap()).unwrap(), dir.path(), "road.ordv");
    let gravel =
        open(build_view(&base, &ViewSpec::parse("gravel").unwrap()).unwrap(), dir.path(), "gravel.ordv");
    let engine = Engine::new(base, road, gravel, Params::default()).unwrap();
    let g = engine.base();
    let edge = |x: u64, y: u64| {
        (0..g.header().edges as u32)
            .find_map(|e| {
                let (a, b) = g.edge_ends(e);
                let (oa, ob) = (g.osm_node_id(a), g.osm_node_id(b));
                if (oa, ob) == (x, y) {
                    Some((e, false))
                } else if (oa, ob) == (y, x) {
                    Some((e, true))
                } else {
                    None
                }
            })
            .expect("arête")
    };
    let ((e1, r1), (e2, r2)) = (edge(1, 2), edge(2, 3));
    let len = |e: u32| f64::from(g.edge(e).len_dm) / 10.0;
    let route = Route {
        start: Position { edge: e1, offset_m: 0.0 },
        end: Position { edge: e2, offset_m: len(e2) },
        steps: vec![Step { base_edge: e1, reverse: r1 }, Step { base_edge: e2, reverse: r2 }],
        length_m: 0.0,
        team_m: 0.0,
        bridged_m: 0.0,
        shared_m: 0.0,
        crossings: 0,
        unpaved_m: 0.0,
        offroad_m: 0.0,
        presumed_surface_m: 0.0,
        bearing_deviation_deg: None,
        familiarity: 0.5,
        family: Family::Turnaround,
    };
    let req = Request {
        start: LatLon::new(s_lat, s_lon),
        end: None,
        bearing: None,
        distance_m: 100_000.0,
        ride_type: RideType::Road,
        mode: Mode::Discovery,
        familiarity: 0.5,
        off_team_cost_q8: ord_loops::DEFAULT_OFF_TEAM_COST_Q8,
        seed: 1,
        figures: ord_loops::Figures::Classic,
    };
    // Garde : les deux repères doivent diverger, sinon le test ne prouve rien.
    let d_in = (north_m / M_LAT, north_m * h_in.tan() / kx_j);
    let d_out = (2000.0 * h_out.cos() / M_LAT, 2000.0 * h_out.sin() / kx_j);
    let (at_node, at_start) = (turn_in_frame(j_lat, d_in, d_out), turn_in_frame(LAT0, d_in, d_out));
    assert!(at_node >= 150.0 && at_start < 150.0, "repère du nœud {at_node}, repère du départ {at_start}");
    let classic = check_route(&engine, None, &req, &route);
    let aligned = check_route_aligned(&engine, None, &req, &route);
    assert!(!classic.violations.contains(&Violation::SharpTurn), "Classic : {:?}", classic.violations);
    assert!(aligned.violations.contains(&Violation::SharpTurn), "aligné : {:?}", aligned.violations);
    let rep = Checker::new(engine.base(), None, Config::default()).check(&req, &route);
    assert!(
        rep.violations.iter().any(|v| format!("{v:?}") == "SharpTurn"),
        "contrôleur indépendant : {:?}",
        rep.violations
    );
}
