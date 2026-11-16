//! L'index par cellule rend exactement les mêmes segments que l'index entier, sur un graphe synthétique qui s'étend sur
//! plusieurs cellules (zoom 11 : ≈ 13 km de large), et respecte sa borne de mémoire.

use ord_graph::build::{build_graph, BuildOptions, MemSource};
use ord_graph::format::write_graph;
use ord_graph::mmap::MappedGraph;
use ord_graph_match::index::{Hit, Near};
use ord_graph_match::{CellIndex, SegIndex};

const LAT0: f64 = 47.0;
const LON0: f64 = -1.80;
/// Pas du réseau : 4 km, sous la largeur d'une cellule.
const STEP_M: f64 = 4_000.0;
const M_LAT: f64 = 111_194.926_6;

fn graph(dir: &std::path::Path, n: usize, step_m: f64) -> MappedGraph {
    let m_lon = M_LAT * LAT0.to_radians().cos();
    let mut s = MemSource::default();
    let id = |r: usize, c: usize| (r * 100 + c + 1) as i64;
    for r in 0..n {
        for c in 0..n {
            // Un léger décalage pour des géométries qui ne sont pas toutes alignées sur les limites.
            let jitter = ((r * 7 + c * 3) % 5) as f64 * 40.0;
            s.node(id(r, c), LAT0 + (r as f64 * step_m + jitter) / M_LAT, LON0 + (c as f64 * step_m) / m_lon);
        }
    }
    let mut way = 1;
    for r in 0..n {
        for c in 0..n {
            for (dr, dc) in [(0, 1), (1, 0), (1, 1)] {
                if r + dr < n && c + dc < n && (dr + dc < 2 || (r + c) % 2 == 0) {
                    s.way(way, &[id(r, c), id(r + dr, c + dc)], &[("highway", "residential")]);
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

fn sorted(mut hits: Vec<Hit>) -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = hits.drain(..).map(|h| (h.edge, h.dist.to_bits())).collect();
    v.sort_unstable();
    v
}

#[test]
fn cell_index_equals_the_full_index_on_a_grid_of_points() {
    let dir = tempfile::tempdir().unwrap();
    let gf = graph(dir.path(), 9, STEP_M);
    assert!(gf.n_cells() > 1, "le graphe doit couvrir plusieurs cellules : {}", gf.n_cells());
    assert!(CellIndex::fits(&gf));
    let full = SegIndex::build(&gf);
    let cell = CellIndex::new(&gf, 64 << 20);
    let m_lon = M_LAT * LAT0.to_radians().cos();
    let mut compared = 0usize;
    let mut non_empty = 0usize;
    // Un quadrillage plus fin que la maille, débordant du graphe : sur les arêtes, entre elles, hors cellules.
    for i in -3..=38 {
        for j in -3..=38 {
            let (lat, lon) = (LAT0 + f64::from(i) * 1_000.0 / M_LAT, LON0 + f64::from(j) * 1_000.0 / m_lon);
            for radius in [8.0, 200.0, 1_500.0] {
                let (mut a, mut b) = (Vec::new(), Vec::new());
                full.near(lat, lon, radius, &mut a);
                cell.near(lat, lon, radius, &mut b);
                non_empty += usize::from(!a.is_empty());
                assert_eq!(sorted(a), sorted(b), "point ({i}, {j}), rayon {radius}");
                compared += 1;
            }
        }
    }
    assert!(compared > 4_000 && non_empty > 500, "{compared} comparaisons, {non_empty} non vides");
}

#[test]
fn cell_index_is_bounded_by_its_cap_and_rebuilds_what_it_evicted() {
    let dir = tempfile::tempdir().unwrap();
    let gf = graph(dir.path(), 9, STEP_M);
    let m_lon = M_LAT * LAT0.to_radians().cos();
    // Deux points à plus d'une cellule d'écart (≈ 13 km) : deux cellules différentes.
    let a = (LAT0 + 100.0 / M_LAT, LON0 + 100.0 / m_lon);
    let b = (LAT0 + 30_000.0 / M_LAT, LON0 + 30_000.0 / m_lon);
    let mut out = Vec::new();

    let roomy = CellIndex::new(&gf, 64 << 20);
    for p in [a, b, a, b] {
        roomy.near(p.0, p.1, 8.0, &mut out);
    }
    assert_eq!(roomy.built(), 2, "chaque cellule n'est construite qu'une fois");

    let tight = CellIndex::new(&gf, 1);
    let mut max_bytes = 0;
    for p in [a, b, a, b] {
        tight.near(p.0, p.1, 8.0, &mut out);
        max_bytes = max_bytes.max(tight.bytes());
    }
    assert_eq!(tight.built(), 4, "cellule évincée puis redemandée : reconstruite");
    assert!(max_bytes <= roomy.bytes(), "une seule cellule gardée sous une borne minuscule : {max_bytes}");
    assert!(roomy.bytes() >= tight.bytes());
}

#[test]
fn an_edge_wider_than_a_cell_is_refused_by_the_cell_index() {
    let dir = tempfile::tempdir().unwrap();
    // Arêtes de 20 km : plus larges qu'une cellule (≈ 13 km à cette latitude).
    let gf = graph(dir.path(), 3, 20_000.0);
    assert!(!CellIndex::fits(&gf));
}
