//! Outils des tests du générateur : un petit moteur sur une grille de rues, sans fichier.

use ord_graph::build::{build_graph, BuildOptions, MemSource};
use ord_graph::format::{write_graph, GraphFile};
use ord_graph::view::{build_view, ViewSpec};
use ord_loops::{Engine, Mode, Params, Request, RideType};

/// Grille de 30 × 30 carrefours de 140 m à z16 (tuiles de ~400 m) : plusieurs fenêtres disjointes.
pub(crate) fn engine() -> Engine<Vec<u8>> {
    let (n, cell) = (30usize, 140.0f64);
    let (lat0, lon0, m_lat) = (47.2f64, -1.56f64, 111_194.926_6f64);
    let m_lon = m_lat * lat0.to_radians().cos();
    let mut s = MemSource::default();
    let id = |i: usize, j: usize| (1 + i * n + j) as i64;
    for i in 0..n {
        for j in 0..n {
            s.node(id(i, j), lat0 + i as f64 * cell / m_lat, lon0 + j as f64 * cell / m_lon);
        }
    }
    let mut way = 0i64;
    for i in 0..n {
        for j in 0..n {
            for (di, dj) in [(0, 1), (1, 0)] {
                if i + di < n && j + dj < n {
                    way += 1;
                    s.way(way, &[id(i, j), id(i + di, j + dj)], &[("highway", "residential")]);
                }
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let file = |mut data: ord_graph::build::GraphData, name: &str| {
        let path = dir.path().join(name);
        write_graph(&path, &mut data, "test", 1_700_000_000).unwrap();
        GraphFile::open(std::fs::read(path).unwrap()).unwrap()
    };
    let base = file(build_graph(&mut s, &BuildOptions { zoom: 16 }, "test", &mut |_| {}).unwrap(), "b.ordg");
    let view = |name: &str| file(build_view(&base, &ViewSpec::parse(name).unwrap()).unwrap(), name);
    let (road, gravel) = (view("road"), view("gravel"));
    Engine::new(base, road, gravel, Params { window_margin_m: 20.0, ..Params::default() }).unwrap()
}

pub(crate) fn request(north: f64, east: f64) -> Request {
    let (lat0, lon0, m_lat) = (47.2f64, -1.56f64, 111_194.926_6f64);
    Request {
        start: ord_loops::LatLon::new(lat0 + north / m_lat, lon0 + east / (m_lat * lat0.to_radians().cos())),
        end: None,
        bearing: None,
        distance_m: 1500.0,
        ride_type: RideType::Road,
        mode: Mode::Discovery,
        familiarity: 0.5,
        off_team_cost_q8: 768,
        seed: 1,
        figures: ord_loops::Figures::Classic,
    }
}
