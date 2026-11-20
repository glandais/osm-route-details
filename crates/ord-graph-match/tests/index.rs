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
fn an_edge_wider_than_a_cell_goes_to_the_exception_list_and_the_index_stays_exact() {
    let dir = tempfile::tempdir().unwrap();
    // Arêtes de 20 km : plus larges qu'une cellule (≈ 13 km à cette latitude).
    let gf = graph(dir.path(), 5, 20_000.0);
    assert!(!CellIndex::fits(&gf));
    let wide = CellIndex::exceptions(&gf);
    assert!(!wide.is_empty() && wide.windows(2).all(|w| w[0] < w[1]));
    let full = SegIndex::build(&gf);
    let cell = CellIndex::new(&gf, 64 << 20);
    assert_eq!(cell.exception_count(), wide.len());
    let m_lon = M_LAT * LAT0.to_radians().cos();
    let (mut compared, mut non_empty) = (0usize, 0usize);
    for i in -2..=26 {
        for j in -2..=26 {
            let (lat, lon) = (LAT0 + f64::from(i) * 4_000.0 / M_LAT, LON0 + f64::from(j) * 4_000.0 / m_lon);
            for radius in [8.0, 600.0, 5_000.0] {
                let (mut a, mut b) = (Vec::new(), Vec::new());
                full.near(lat, lon, radius, &mut a);
                cell.near(lat, lon, radius, &mut b);
                non_empty += usize::from(!a.is_empty());
                assert_eq!(sorted(a), sorted(b), "point ({i}, {j}), rayon {radius}");
                compared += 1;
            }
        }
    }
    assert!(compared > 2_000 && non_empty > 300, "{compared} comparaisons, {non_empty} non vides");
}

/// Des arêtes de 0,3 à 30 km dans tous les sens, près du pôle (84° N : tuiles de ≈ 2 km de large) et à l'équateur
/// (≈ 19,6 km) : la liste des arêtes plus larges qu'une cellule, au seuil propre à chaque latitude, est celle d'un
/// examen de **toutes** les arêtes (note `ord-loops-planete.md` §4).
#[test]
fn wide_edges_with_a_threshold_per_latitude_are_those_of_a_full_scan() {
    use ord_graph::grid::tile_of_e7;
    let dir = tempfile::tempdir().unwrap();
    let mut s = MemSource::default();
    let mut rng = ord_graph::util::SplitMix64(11);
    let mut next = 1i64;
    for (lat0, lon0) in [(84.0, 20.0), (0.5, 20.0)] {
        for w in 0..300 {
            let len_m = 300.0 + rng.below(30_000) as f64;
            let dir_rad = rng.below(360) as f64 * std::f64::consts::PI / 180.0;
            let (n_m, e_m) = (len_m * dir_rad.cos(), len_m * dir_rad.sin());
            let m_lon = M_LAT * f64::to_radians(lat0).cos();
            let p0 = (lat0 + rng.below(20_000) as f64 / M_LAT, lon0 + rng.below(20_000) as f64 / m_lon);
            let mut refs = Vec::new();
            for k in 0..=2u32 {
                let f = f64::from(k) / 2.0;
                s.node(next, p0.0 + f * n_m / M_LAT, p0.1 + f * e_m / m_lon);
                refs.push(next);
                next += 1;
            }
            // Un sur deux, un nœud intermédiaire de plus (un coude) : la géométrie compte, pas seulement les bouts.
            if w % 2 == 0 {
                refs.remove(1);
            }
            s.way(10_000 + next, &refs, &[("highway", "residential")]);
        }
    }
    let mut data = build_graph(&mut s, &BuildOptions { zoom: 11 }, "test", &mut |_| {}).unwrap();
    let path = dir.path().join("g.ordg");
    write_graph(&path, &mut data, "test", 1_700_000_000).unwrap();
    let g = MappedGraph::open_mmap(&path).unwrap();
    // Examen exhaustif : un segment dont la boîte touche une tuile hors des voisines de la cellule de l'arête.
    let mut want = Vec::new();
    for ci in 0..g.n_cells() {
        let info = *g.cell_info(ci);
        for k in 0..info.n_edges {
            let e = info.first_edge + k;
            let (a, b) = g.edge_ends(e);
            let mut pts = vec![g.node_pos(a)];
            pts.extend(g.geometry(e));
            pts.push(g.node_pos(b));
            let out = pts.windows(2).any(|w| {
                let (x0, y0) = tile_of_e7(w[0].0.max(w[1].0), w[0].1.min(w[1].1), 11);
                let (x1, y1) = tile_of_e7(w[0].0.min(w[1].0), w[0].1.max(w[1].1), 11);
                x0 + 1 < info.x || y0 + 1 < info.y || x1 > info.x + 1 || y1 > info.y + 1
            });
            if out {
                want.push(e);
            }
        }
    }
    let got = CellIndex::exceptions(&g);
    assert_eq!(got, want);
    // Et celle du contrôleur : les arêtes dont un bout n'est ni dans leur cellule ni dans une voisine.
    let far: Vec<u32> = (0..g.n_cells())
        .flat_map(|ci| {
            let info = *g.cell_info(ci);
            (info.first_edge..info.first_edge + info.n_edges).map(move |e| (info, e))
        })
        .filter(|&(info, e)| {
            let (a, b) = g.edge_ends(e);
            [a, b].iter().any(|&n| {
                let (x, y) = g.cell_xy(g.node_cell(n));
                x.abs_diff(info.x) > 1 || y.abs_diff(info.y) > 1
            })
        })
        .map(|(_, e)| e)
        .collect();
    assert!(!far.is_empty());
    assert_eq!(ord_loops_check::GraphIndex::build(&g).wide_edges(), &far[..]);
    // Les deux latitudes en ont : le test départage bien quelque chose.
    let lat_of = |e: u32| f64::from(g.node_pos(g.edge_ends(e).0).0) * 1e-7;
    assert!(got.iter().any(|&e| lat_of(e) > 80.0) && got.iter().any(|&e| lat_of(e) < 10.0), "{got:?}");
}
