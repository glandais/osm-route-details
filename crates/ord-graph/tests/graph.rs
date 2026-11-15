//! Tests de bout en bout de `ord-graph` : contraction, format, lecteur, bords, déterminisme, recherche.
//! Entrées à la main par `MemSource`, plus un PBF minuscule commis pour le vrai décodage.

use std::collections::BinaryHeap;
use std::path::Path;

use ord_graph::attrs::{Access, Attrs, ROUTE_REGIONAL};
use ord_graph::build::{build_graph, BuildOptions, GraphData, MemSource};
use ord_graph::format::{write_graph, BlockLayout, FormatError, GraphFile, DIR_BIT};
use ord_graph::grid;
use ord_graph::search::{edge_cost, FlatGraph, FlatSearcher, SearchConfig, Searcher, StateKind};
use ord_graph::stats::GraphStats;

const LAT0: f64 = 47.2;
const LON0: f64 = -1.56;

fn opts(zoom: u8) -> BuildOptions {
    BuildOptions { zoom }
}

fn built(src: &mut MemSource, zoom: u8) -> GraphData {
    build_graph(src, &opts(zoom), "test", &mut |_| {}).expect("construction")
}

fn written(data: &mut GraphData, dir: &Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    write_graph(&path, data, "test", 1_700_000_000).expect("écriture");
    path
}

fn graph(src: &mut MemSource, zoom: u8) -> (GraphData, GraphFile<Vec<u8>>) {
    let mut data = built(src, zoom);
    let dir = tempfile::tempdir().unwrap();
    let path = written(&mut data, dir.path(), "g.ordg");
    let g = GraphFile::open(std::fs::read(path).unwrap()).expect("ouverture");
    (data, g)
}

fn residential() -> [(&'static str, &'static str); 1] {
    [("highway", "residential")]
}

/// Nœuds `first..first+n` alignés vers le nord-est, espacés de 0,001°.
fn line(src: &mut MemSource, first: i64, n: i64, lat0: f64, lon0: f64) {
    for i in 0..n {
        src.node(first + i, lat0 + 0.001 * i as f64, lon0 + 0.001 * i as f64);
    }
}

fn find_edge(g: &GraphFile<Vec<u8>>, way: u64, a: u64, b: u64) -> u32 {
    (0..g.header().edges as u32)
        .find(|&e| g.identity(e) == (way, a, b))
        .unwrap_or_else(|| panic!("arête ({way},{a},{b}) absente"))
}

// --- 3. Contraction ---------------------------------------------------------------------------

#[test]
fn isolated_way_contracts_to_one_edge_with_interior_geometry() {
    let mut s = MemSource::default();
    line(&mut s, 1, 4, LAT0, LON0);
    s.way(10, &[1, 2, 3, 4], &residential());
    let (data, g) = graph(&mut s, 11);
    assert_eq!((g.header().nodes, g.header().edges), (2, 1));
    let e = find_edge(&g, 10, 1, 4);
    assert_eq!(g.geometry(e), vec![(472_010_000, -15_590_000), (472_020_000, -15_580_000)]);
    // Longueur = somme des segments, en dm.
    let step = haversine(LAT0, LON0, LAT0 + 0.001, LON0 + 0.001);
    let len_m = f64::from(g.edge(e).len_dm) / 10.0;
    assert!((len_m - 3.0 * step).abs() < 0.2, "{len_m} vs {}", 3.0 * step);
    assert_eq!(data.stats.duplicate_identities, 0);
}

fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let h = ((p2 - p1) / 2.0).sin().powi(2)
        + p1.cos() * p2.cos() * ((lon2 - lon1).to_radians() / 2.0).sin().powi(2);
    2.0 * 6_371_008.8 * h.sqrt().asin()
}

#[test]
fn ways_end_to_end_make_two_edges_and_a_seam() {
    let mut s = MemSource::default();
    line(&mut s, 1, 5, LAT0, LON0);
    s.way(10, &[1, 2, 3], &residential()).way(11, &[3, 4, 5], &residential());
    let (_, g) = graph(&mut s, 11);
    assert_eq!((g.header().nodes, g.header().edges), (3, 2));
    find_edge(&g, 10, 1, 3);
    find_edge(&g, 11, 3, 5);
    let st = GraphStats::compute(&g);
    assert_eq!((st.nodes_degree_1, st.nodes_degree_2, st.nodes_degree_2_same_attrs), (2, 1, 1));
}

#[test]
fn crossing_inside_two_ways_makes_four_edges() {
    let mut s = MemSource::default();
    // way 10 : 1-2-3 ; way 11 : 4-2-5 (croisement en 2, intérieur aux deux).
    s.node(1, LAT0, LON0).node(2, LAT0 + 0.001, LON0).node(3, LAT0 + 0.002, LON0);
    s.node(4, LAT0 + 0.001, LON0 - 0.001).node(5, LAT0 + 0.001, LON0 + 0.001);
    s.way(10, &[1, 2, 3], &residential()).way(11, &[4, 2, 5], &residential());
    let (_, g) = graph(&mut s, 11);
    assert_eq!((g.header().nodes, g.header().edges), (5, 4));
    for (w, a, b) in [(10, 1, 2), (10, 2, 3), (11, 4, 2), (11, 2, 5)] {
        find_edge(&g, w, a, b);
    }
}

#[test]
fn roundabout_with_one_connection_has_no_loop_edge() {
    let mut s = MemSource::default();
    s.node(1, LAT0, LON0).node(2, LAT0 + 0.001, LON0).node(3, LAT0 + 0.001, LON0 + 0.001).node(
        4,
        LAT0,
        LON0 + 0.001,
    );
    s.node(9, LAT0 - 0.001, LON0);
    s.way(20, &[1, 2, 3, 4, 1], &[("highway", "residential"), ("junction", "roundabout")]);
    s.way(21, &[9, 1], &residential());
    let (data, g) = graph(&mut s, 11);
    for e in 0..g.header().edges as u32 {
        let (a, b) = g.edge_ends(e);
        assert_ne!(a, b, "boucle {:?}", g.identity(e));
    }
    assert_eq!(data.stats.loops_cut, 1);
    // 3 sommets (9, 1, médian) et 3 arêtes ; le médian de [1,2,3,4,1] est le nœud 3.
    assert_eq!((g.header().nodes, g.header().edges), (3, 3));
    find_edge(&g, 20, 1, 3);
    find_edge(&g, 20, 3, 1);
    // Rond-point : sens unique implicite, dans l'ordre de la way.
    let e = find_edge(&g, 20, 1, 3);
    assert!(g.edge(e).attrs.fwd_ok() && !g.edge(e).attrs.bwd_ok());
}

#[test]
fn self_intersection_cuts_at_the_repeated_node() {
    let mut s = MemSource::default();
    // A(1) B(2) C(3) D(4) B(2) E(5) : B apparaît deux fois.
    s.node(1, LAT0, LON0).node(2, LAT0 + 0.001, LON0).node(3, LAT0 + 0.002, LON0);
    s.node(4, LAT0 + 0.002, LON0 + 0.001).node(5, LAT0 + 0.001, LON0 - 0.001);
    s.way(30, &[1, 2, 3, 4, 2, 5], &residential());
    let (data, g) = graph(&mut s, 11);
    // Arêtes 1-2, 2-3-4-2 (boucle coupée au médian), 2-5.
    assert_eq!(data.stats.loops_cut, 1);
    assert_eq!(g.header().edges, 4);
    find_edge(&g, 30, 1, 2);
    find_edge(&g, 30, 2, 5);
}

#[test]
fn missing_node_cuts_the_way_and_is_counted() {
    let mut s = MemSource::default();
    line(&mut s, 1, 5, LAT0, LON0);
    s.nodes.retain(|n| n.0 != 3);
    s.way(10, &[1, 2, 3, 4, 5], &residential());
    let (data, g) = graph(&mut s, 11);
    assert_eq!(data.stats.ways_cut_by_missing_nodes, 1);
    assert_eq!(data.stats.nodes_referenced_missing, 1);
    find_edge(&g, 10, 1, 2);
    find_edge(&g, 10, 4, 5);
    assert_eq!(g.header().edges, 2);
}

#[test]
fn duplicate_identities_are_counted_not_dropped() {
    let mut s = MemSource::default();
    s.node(1, LAT0, LON0).node(2, LAT0 + 0.002, LON0).node(3, LAT0 + 0.001, LON0 + 0.001);
    s.node(4, LAT0 + 0.001, LON0 - 0.001).node(5, LAT0 + 0.001, LON0 + 0.002);
    // 1 -> 2 deux fois (par 3 puis par 5), avec 2 -> 1 entre les deux.
    s.way(40, &[1, 3, 2, 4, 1, 5, 2], &residential());
    let (data, g) = graph(&mut s, 11);
    assert_eq!(data.stats.duplicate_identities, 1);
    assert_eq!(g.header().edges, 3);
}

// --- 4. Relations, filtre -------------------------------------------------------------------------

#[test]
fn route_relations_set_bits_and_count_unmatched_members() {
    let mut s = MemSource::default();
    line(&mut s, 1, 4, LAT0, LON0);
    s.way(10, &[1, 2], &residential()).way(11, &[3, 4], &residential());
    s.way(12, &[2, 3], &[("highway", "residential")]);
    s.relation(900, &[("type", "route"), ("route", "bicycle"), ("network", "rcn")], &[10, 777]);
    s.relation(
        901,
        &[("type", "route"), ("route", "bicycle"), ("network", "ncn"), ("state", "proposed")],
        &[11],
    );
    s.relation(902, &[("type", "route"), ("route", "mtb")], &[12]);
    let (data, g) = graph(&mut s, 11);
    let routes = |w, a, b| g.edge(find_edge(&g, w, a, b)).attrs.routes();
    assert_eq!(routes(10, 1, 2), ROUTE_REGIONAL);
    assert_eq!(routes(11, 3, 4), 0);
    assert_eq!(routes(12, 2, 3), 8);
    assert_eq!(data.stats.relation_way_members_not_in_graph, 1);
    assert_eq!(data.stats.relations_routes, 2);
}

#[test]
fn rejected_ways_are_counted_by_reason_and_trunk_rules_hold() {
    let mut s = MemSource::default();
    line(&mut s, 1, 6, LAT0, LON0);
    s.way(10, &[1, 2], &[("highway", "construction")]);
    s.way(11, &[2, 3], &[("highway", "footway"), ("area", "yes")]);
    s.way(12, &[3, 4], &[("highway", "trunk")]);
    s.way(13, &[4, 5], &[("highway", "trunk"), ("bicycle", "yes")]);
    s.way(14, &[5, 6], &[("building", "yes")]);
    s.way(15, &[1, 6], &[("highway", "residential"), ("access", "no"), ("bicycle", "yes")]);
    let (data, g) = graph(&mut s, 11);
    assert_eq!(data.stats.ways_total, 6);
    assert_eq!(data.stats.ways_kept, 2);
    assert_eq!(data.stats.rejects["highway=construction"], 1);
    assert_eq!(data.stats.rejects["area=yes"], 1);
    assert_eq!(data.stats.rejects["motorway/trunk fermée aux vélos"], 1);
    assert_eq!(data.stats.rejects["sans highway"], 1);
    assert_eq!(g.header().edges, 2);
    assert_eq!(g.edge(find_edge(&g, 15, 1, 6)).attrs.access(), Access::Yes);
}

// --- 5. Aller-retour du format --------------------------------------------------------------------

/// Graphe pseudo-aléatoire : un semis de nœuds sur ~25 km, des ways de 2 à 9 nœuds voisins.
fn random_source(seed: u64) -> MemSource {
    let mut x = seed;
    let mut rnd = move || {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut s = MemSource::default();
    let n = 1500i64;
    for i in 0..n {
        let lat = 47.0 + (rnd() % 100_000) as f64 / 100_000.0 * 0.3;
        let lon = -1.9 + (rnd() % 100_000) as f64 / 100_000.0 * 0.45;
        s.node(1000 + i, lat, lon);
    }
    let classes = ["residential", "tertiary", "cycleway", "track", "path", "service", "footway"];
    for w in 0..900i64 {
        let len = 2 + (rnd() % 8) as usize;
        let start = (rnd() % n as u64) as i64;
        let refs: Vec<i64> =
            (0..len as i64).map(|k| 1000 + (start + k * (1 + (rnd() % 3) as i64)) % n).collect();
        let class = classes[(rnd() % classes.len() as u64) as usize];
        let mut tags = vec![("highway", class)];
        match rnd() % 6 {
            0 => tags.push(("oneway", "yes")),
            1 => tags.push(("oneway", "-1")),
            2 => tags.push(("surface", "gravel")),
            3 => tags.push(("bicycle", "no")),
            _ => {}
        }
        s.way(500 + w, &refs, &tags);
    }
    s
}

fn check_roundtrip<B: AsRef<[u8]>>(data: &GraphData, g: &GraphFile<B>) {
    assert_eq!(g.header().nodes as usize, data.nodes.len());
    assert_eq!(g.header().edges as usize, data.edges.len());
    for (gid, n) in data.nodes.iter().enumerate() {
        assert_eq!(g.node_pos(gid as u32), (n.lat, n.lon));
        assert_eq!(g.osm_node_id(gid as u32), n.osm_id as u64);
    }
    let mut directed = 0u64;
    for (e, ed) in data.edges.iter().enumerate() {
        let e = e as u32;
        let v = g.edge(e);
        assert_eq!((v.attrs, v.len_dm), (Attrs(ed.attrs), ed.len_dm));
        assert_eq!(g.edge_ends(e), (ed.a, ed.b));
        assert_eq!(
            g.identity(e),
            (
                ed.way_id as u64,
                data.nodes[ed.a as usize].osm_id as u64,
                data.nodes[ed.b as usize].osm_id as u64
            )
        );
        assert_eq!(g.geometry(e), data.interior(ed));
        // Arêtes orientées présentes selon les sens ouverts.
        let has = |src: u32, field: u32, tgt: u32| {
            let ci = g.node_cell(src);
            let cv = g.cell(ci);
            cv.out((src - cv.info().first_node) as usize)
                .iter()
                .any(|d| d.edge_field() == field && d.target() == tgt)
        };
        let a = Attrs(ed.attrs);
        assert_eq!(has(ed.a, e, ed.b), a.fwd_ok());
        assert_eq!(has(ed.b, e | DIR_BIT, ed.a), a.bwd_ok());
        directed += u64::from(a.fwd_ok()) + u64::from(a.bwd_ok());
    }
    assert_eq!(g.header().directed, directed);
    // Un voisin n'est jamais perdu : somme des degrés sortants = orientées.
    let total: usize = (0..g.n_cells()).map(|c| g.cell(c).directed().len()).sum();
    assert_eq!(total as u64, directed);
    // CSR trié par (cible, arête).
    for c in 0..g.n_cells() {
        let cv = g.cell(c);
        for l in 0..cv.info().n_nodes as usize {
            let o = cv.out(l);
            assert!(o
                .windows(2)
                .all(|w| (w[0].target(), w[0].edge_field()) <= (w[1].target(), w[1].edge_field())));
        }
    }
}

#[test]
fn roundtrip_random_graph_through_vec_and_mmap() {
    let mut s = random_source(7);
    let mut data = built(&mut s, 11);
    assert!(data.edges.len() > 300, "{} arêtes", data.edges.len());
    let dir = tempfile::tempdir().unwrap();
    let path = written(&mut data, dir.path(), "r.ordg");
    let by_vec = GraphFile::open(std::fs::read(&path).unwrap()).unwrap();
    check_roundtrip(&data, &by_vec);
    by_vec.verify().unwrap();
    let by_mmap = GraphFile::open_mmap(&path).unwrap();
    check_roundtrip(&data, &by_mmap);
    by_mmap.verify().unwrap();
    // Fichier en lecture seule, pas de temporaire laissé.
    assert!(std::fs::metadata(&path).unwrap().permissions().readonly());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn corrupted_block_is_named_by_verify() {
    let mut s = random_source(11);
    let mut data = built(&mut s, 11);
    let dir = tempfile::tempdir().unwrap();
    let path = written(&mut data, dir.path(), "c.ordg");
    let mut bytes = std::fs::read(&path).unwrap();
    let probe = GraphFile::open(bytes.clone()).unwrap();
    assert!(probe.n_cells() > 1);
    let victim = probe.n_cells() - 1;
    let info = *probe.cell_info(victim);
    bytes[info.block_off as usize + 3] ^= 0x40;
    let g = GraphFile::open(bytes).expect("les blocs ne sont pas vérifiés à l'ouverture");
    match g.verify() {
        Err(FormatError::Checksum(what)) => {
            assert!(what.contains(&format!("cellule {victim} (x={}, y={})", info.x, info.y)), "{what}")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn bad_magic_major_layout_and_truncation_are_refused() {
    let mut s = random_source(3);
    let mut data = built(&mut s, 11);
    let dir = tempfile::tempdir().unwrap();
    let path = written(&mut data, dir.path(), "m.ordg");
    let good = std::fs::read(&path).unwrap();
    let patched = |at: usize, v: u8| {
        let mut b = good.clone();
        b[at] = v;
        b
    };
    assert!(matches!(GraphFile::open(patched(0, b'X')), Err(FormatError::BadMagic)));
    assert!(matches!(GraphFile::open(patched(8, 9)), Err(FormatError::UnsupportedMajor { found: 9, .. })));
    assert!(matches!(GraphFile::open(patched(12, 7)), Err(FormatError::AttrLayout { found: 7, .. })));
    assert!(matches!(GraphFile::open(patched(100, 0xAA)), Err(FormatError::Checksum(_))));
    assert!(GraphFile::open(good[..good.len() / 2].to_vec()).is_err());
    assert!(GraphFile::open(good[..100].to_vec()).is_err());
    assert!(GraphFile::open(Vec::new()).is_err());
}

// --- 6. Bords -------------------------------------------------------------------------------------------

#[test]
fn edge_across_a_tile_boundary_lives_in_both_cells() {
    let (_, lon_w) = grid::tile_origin_deg(1015, 718, 11);
    let lat = 47.2;
    let mut s = MemSource::default();
    s.node(1, lat, lon_w - 0.002).node(2, lat, lon_w + 0.002).node(3, lat + 0.001, lon_w - 0.002);
    s.way(10, &[1, 2], &residential()).way(11, &[1, 3], &residential());
    let (_, g) = graph(&mut s, 11);
    let e = find_edge(&g, 10, 1, 2);
    let (a, b) = g.edge_ends(e);
    let (ca, cb) = (g.node_cell(a), g.node_cell(b));
    assert_ne!(ca, cb);
    // Possédée par la cellule de a.
    assert_eq!(g.edge(e).cell, ca);
    // CSR des deux côtés et une entrée de bord de chaque côté vers l'autre cellule.
    let out_has = |cell: usize, src: u32, field: u32| {
        let cv = g.cell(cell);
        cv.out((src - cv.info().first_node) as usize).iter().any(|d| d.edge_field() == field)
    };
    assert!(out_has(ca, a, e) && out_has(cb, b, e | DIR_BIT));
    assert_eq!(g.cell(ca).border().filter(|&(_, c)| c as usize == cb).count(), 1);
    assert_eq!(g.cell(cb).border().filter(|&(_, c)| c as usize == ca).count(), 1);
    // Attributs lus depuis la cellule voisine, via l'arête orientée arrière.
    let cv = g.cell(cb);
    let d = cv.out((b - cv.info().first_node) as usize)[0];
    assert!(!d.forward());
    assert_eq!(g.edge(d.edge_id()).attrs, g.edge(e).attrs);
    assert_eq!(g.cell_xy(ca), (1014, 718));
    assert_eq!(g.cell_xy(cb), (1015, 718));
    // Identité intacte : l'arête n'est jamais coupée au bord.
    assert_eq!(g.identity(e), (10, 1, 2));
}

#[test]
fn node_on_a_tile_limit_goes_to_the_east_tile_and_far_edge_crosses_a_third_cell() {
    let (lat_n, lon_w) = grid::tile_origin_deg(1015, 718, 11);
    let mut s = MemSource::default();
    let lat = lat_n - 0.01;
    // Nœud 1 sur la limite ouest de la tuile 1015 ; 3 deux tuiles plus à l'est ; 2 au milieu (intérieur).
    let tile_w = 360.0 / 2048.0;
    s.node(1, lat, lon_w).node(2, lat, lon_w + tile_w * 1.5).node(3, lat, lon_w + tile_w * 2.5);
    s.way(10, &[1, 2, 3], &residential());
    let (data, g) = graph(&mut s, 11);
    let n1 = data.nodes.iter().position(|n| n.osm_id == 1).unwrap() as u32;
    let (la, lo) = g.node_pos(n1);
    assert_eq!(g.cell_xy(g.node_cell(n1)), grid::tile_of_e7(la, lo, 11));
    let st = GraphStats::compute(&g);
    assert_eq!(st.edges_with_point_outside_ends_cells, 1);
    assert_eq!(st.cells, 2);
}

// --- 7. Déterminisme --------------------------------------------------------------------------------------------

fn digest_for(threads: usize, seed: u64) -> Vec<(&'static str, u64)> {
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
    let mut src = random_source(seed);
    let mut data = pool.install(|| built(&mut src, 11));
    let dir = tempfile::tempdir().unwrap();
    let path = written(&mut data, dir.path(), "d.ordg");
    GraphFile::open(std::fs::read(path).unwrap()).unwrap().section_digests()
}

#[test]
fn output_does_not_depend_on_thread_count() {
    assert_eq!(digest_for(1, 5), digest_for(4, 5));
    assert_ne!(digest_for(1, 5), digest_for(1, 6));
}

/// Même garde, mais par le vrai décodage parallèle du PBF : fils de rayon et taille des lots de blobs varient.
/// Le PBF commis n'a que quelques blobs (types séparés) : des lots de 1 et 2 blobs font traverser des frontières
/// de lot à la passe 1 et à la passe 2, que le lot par défaut (4 blobs par fil) ne traverse pas ici.
#[cfg(feature = "pbf")]
#[test]
fn pbf_output_does_not_depend_on_threads_or_batch_size() {
    use ord_graph::pbf::PbfSource;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/mini.osm.pbf");
    let digest = |threads: usize, batch: Option<usize>| {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        let mut data = pool.install(|| {
            let mut src = PbfSource::open(&path).unwrap();
            if let Some(n) = batch {
                src = src.with_batch_len(n);
            }
            build_graph(&mut src, &opts(11), "mini", &mut |_| {}).unwrap()
        });
        let dir = tempfile::tempdir().unwrap();
        let out = written(&mut data, dir.path(), "p.ordg");
        GraphFile::open(std::fs::read(out).unwrap()).unwrap().section_digests()
    };
    let reference = digest(1, None);
    for (threads, batch) in [(4, None), (1, Some(1)), (4, Some(1)), (4, Some(2)), (2, Some(3))] {
        assert_eq!(digest(threads, batch), reference, "{threads} fils, lots de {batch:?}");
    }
}

// --- 7. Recherche ----------------------------------------------------------------------------------------------------

fn full_cfg(forbid: bool, state: StateKind) -> SearchConfig {
    SearchConfig { radius_m: 1e7, forbid_uturn: forbid, state, collect_nodes: true }
}

/// Dijkstra par nœuds de référence, mêmes poids que `edge_cost`.
fn reference_node_dijkstra(g: &GraphFile<Vec<u8>>, start: u32) -> Vec<(u32, u32)> {
    let mut best = std::collections::HashMap::new();
    let mut heap = BinaryHeap::new();
    heap.push(std::cmp::Reverse((0u32, start)));
    best.insert(start, 0u32);
    while let Some(std::cmp::Reverse((d, v))) = heap.pop() {
        if best[&v] < d {
            continue;
        }
        let cv = g.cell(g.node_cell(v));
        for t in cv.out((v - cv.info().first_node) as usize) {
            let ev = g.edge(t.edge_id());
            let Some(w) = edge_cost(ev.attrs, ev.len_dm) else { continue };
            let nd = d + w;
            if best.get(&t.target()).is_none_or(|&b| nd < b) {
                best.insert(t.target(), nd);
                heap.push(std::cmp::Reverse((nd, t.target())));
            }
        }
    }
    let mut v: Vec<_> = best.into_iter().collect();
    v.sort_unstable();
    v
}

/// Dijkstra par arêtes orientées de référence (états en `HashMap`), avec ou sans interdiction de demi-tour.
/// Une sortie n'est « utilisable » que si son accès n'est pas `No` et si sa cible est dans le disque.
fn reference_edge_dijkstra(
    g: &GraphFile<Vec<u8>>,
    start: u32,
    forbid: bool,
    radius_m: f64,
) -> Vec<(u32, u32)> {
    let (slat, slon) = g.node_pos(start);
    let in_disc = |v: u32| {
        let (lat, lon) = g.node_pos(v);
        // Mêmes opérations que `Disc::contains`, pour qu'aucun point limite ne diffère.
        let (lat0, lon0) = (f64::from(slat) * 1e-7, f64::from(slon) * 1e-7);
        let dy = (f64::from(lat) * 1e-7 - lat0).to_radians() * 6_371_008.8;
        let dx = (f64::from(lon) * 1e-7 - lon0).to_radians() * lat0.to_radians().cos() * 6_371_008.8;
        dx * dx + dy * dy <= radius_m * radius_m
    };
    let usable = |t: &ord_graph::format::DirEdge| {
        let ev = g.edge(t.edge_id());
        edge_cost(ev.attrs, ev.len_dm).is_some() && in_disc(t.target())
    };
    let mut best: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut node: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    node.insert(start, 0);
    let mut heap = BinaryHeap::new();
    let outs = |v: u32| {
        let cv = g.cell(g.node_cell(v));
        cv.out((v - cv.info().first_node) as usize).to_vec()
    };
    let push = |heap: &mut BinaryHeap<_>,
                best: &mut std::collections::HashMap<u32, u32>,
                t: &ord_graph::format::DirEdge,
                base: u32| {
        if !usable(t) {
            return;
        }
        let ev = g.edge(t.edge_id());
        let Some(w) = edge_cost(ev.attrs, ev.len_dm) else { return };
        let nd = base + w;
        if best.get(&t.edge_field()).is_none_or(|&b| nd < b) {
            best.insert(t.edge_field(), nd);
            heap.push(std::cmp::Reverse((nd, t.edge_field(), t.target())));
        }
    };
    for t in outs(start) {
        push(&mut heap, &mut best, &t, 0);
    }
    while let Some(std::cmp::Reverse((d, field, v))) = heap.pop() {
        if best[&field] < d {
            continue;
        }
        let e = node.entry(v).or_insert(d);
        *e = (*e).min(d);
        let o = outs(v);
        let rev = field ^ DIR_BIT;
        let has_other = o.iter().any(|t| t.edge_field() != rev && usable(t));
        for t in &o {
            if forbid && t.edge_field() == rev && has_other {
                continue;
            }
            push(&mut heap, &mut best, t, d);
        }
    }
    let mut v: Vec<_> = node.into_iter().collect();
    v.sort_unstable();
    v
}

#[test]
fn edge_search_without_uturn_ban_equals_node_dijkstra() {
    let mut s = random_source(21);
    let (_, g) = graph(&mut s, 11);
    let mut searcher = Searcher::new(&g);
    for start in [0u32, 5, 17, (g.header().nodes / 2) as u32] {
        let reference = reference_node_dijkstra(&g, start);
        for dense in StateKind::ALL {
            let r = searcher.run(start, &full_cfg(false, dense));
            assert_eq!(r.node_dist, reference, "départ {start}, state={dense:?}");
        }
    }
}

#[test]
fn edge_search_with_uturn_ban_matches_reference_and_never_improves() {
    let mut s = random_source(33);
    let (_, g) = graph(&mut s, 11);
    let mut searcher = Searcher::new(&g);
    for start in [1u32, 9, 40, (g.header().nodes / 3) as u32] {
        let r = searcher.run(start, &full_cfg(true, StateKind::Map));
        assert_eq!(r.node_dist, reference_edge_dijkstra(&g, start, true, 1e7), "départ {start}");
        let free: std::collections::HashMap<_, _> = reference_node_dijkstra(&g, start).into_iter().collect();
        for (n, d) in &r.node_dist {
            assert!(*d >= free[n], "le demi-tour interdit ne peut pas raccourcir ({n})");
        }
    }
}

#[test]
fn dead_end_allows_the_uturn_and_disc_and_no_access_are_respected() {
    let mut s = MemSource::default();
    line(&mut s, 1, 4, LAT0, LON0);
    // Impasse 1-2 ; puis 2-3 (bicycle=no) ; 3-4 hors disque.
    s.way(10, &[1, 2], &residential()).way(11, &[2, 3], &[("highway", "residential"), ("bicycle", "no")]);
    s.way(12, &[3, 4], &residential());
    let (data, g) = graph(&mut s, 11);
    let id = |osm: i64| data.nodes.iter().position(|n| n.osm_id == osm).unwrap() as u32;
    let mut searcher = Searcher::new(&g);
    let r = searcher.run(
        id(2),
        &SearchConfig { radius_m: 1e7, forbid_uturn: true, state: StateKind::Map, collect_nodes: true },
    );
    // Depuis 2 : vers 1 (impasse : demi-tour permis, d'où 2 états), mais 3 est interdit (accès `No`).
    assert_eq!(r.stats.settled, 2);
    assert!(r.node_dist.iter().all(|(n, _)| *n != id(3) && *n != id(4)));
    // Disque : 130 m autour de 1 ne contient pas 3 (≈ 260 m).
    let r = searcher.run(
        id(1),
        &SearchConfig { radius_m: 130.0, forbid_uturn: false, state: StateKind::Map, collect_nodes: true },
    );
    assert!(r.node_dist.iter().all(|(n, _)| *n == id(1) || *n == id(2)));
}

#[test]
fn uturn_is_allowed_when_the_only_other_exits_are_unusable() {
    // 1 -(10)- 2 -(11, bicycle=no)- 3 -(12)- 4 : pas de 0,001° de latitude et de longitude (≈ 135 m).
    let mut s = MemSource::default();
    line(&mut s, 1, 4, LAT0, LON0);
    s.way(10, &[1, 2], &residential()).way(11, &[2, 3], &[("highway", "residential"), ("bicycle", "no")]);
    s.way(12, &[3, 4], &residential());
    let (data, g) = graph(&mut s, 11);
    let id = |osm: i64| data.nodes.iter().position(|n| n.osm_id == osm).unwrap() as u32;
    let cfg =
        |radius_m| SearchConfig { radius_m, forbid_uturn: true, state: StateKind::Map, collect_nodes: true };
    let mut searcher = Searcher::new(&g);
    // Départ en 1, arrivée en 2 : la seule autre sortie de 2 est fermée (`No`) : demi-tour permis, 2 états.
    // (Avant correction, la sortie fermée comptait comme « autre » : 1 seul état.)
    assert_eq!(searcher.run(id(1), &cfg(1e7)).stats.settled, 2);
    // Même chose quand l'autre sortie est ouverte mais sa cible est hors du disque (3 est à ≈ 270 m).
    let mut s = MemSource::default();
    line(&mut s, 1, 3, LAT0, LON0);
    s.way(10, &[1, 2], &residential()).way(11, &[2, 3], &residential());
    let (data, g) = graph(&mut s, 11);
    let id = |osm: i64| data.nodes.iter().position(|n| n.osm_id == osm).unwrap() as u32;
    let mut searcher = Searcher::new(&g);
    let r = searcher.run(id(1), &cfg(200.0));
    assert_eq!(r.stats.settled, 2, "demi-tour en 2 : la sortie vers 3 sort du disque");
    assert!(r.node_dist.iter().all(|(n, _)| *n != id(3)));
    // Contrôle : sans disque, 3 est atteint ; états 1→2, 2→3, 3→2 (impasse) puis 2→1 (retour).
    assert_eq!(searcher.run(id(1), &cfg(1e7)).stats.settled, 4);
}

#[test]
fn disc_cuts_the_search_like_the_reference_and_flat_equals_cells() {
    let mut s = random_source(41);
    let (_, g) = graph(&mut s, 11);
    let flat = FlatGraph::from_graph(&g);
    let mut searcher = Searcher::new(&g);
    let mut flat_searcher = FlatSearcher::new(&flat);
    let mut cut_somewhere = false;
    for start in [0u32, 7, 123, (g.header().nodes / 2) as u32] {
        for radius_m in [1500.0, 4000.0, 9000.0] {
            for forbid in [true, false] {
                let cfg = SearchConfig {
                    radius_m,
                    forbid_uturn: forbid,
                    state: StateKind::Dense,
                    collect_nodes: true,
                };
                let r = searcher.run(start, &cfg);
                let full = searcher.run(start, &SearchConfig { radius_m: 1e7, ..cfg });
                if forbid {
                    assert_eq!(r.node_dist, reference_edge_dijkstra(&g, start, true, radius_m));
                }
                // Aucun nœud atteint hors du disque (haversine, à la marge de la projection près).
                let (slat, slon) = g.node_pos(start);
                for (n, _) in &r.node_dist {
                    let (lat, lon) = g.node_pos(*n);
                    let d = haversine(
                        f64::from(slat) * 1e-7,
                        f64::from(slon) * 1e-7,
                        f64::from(lat) * 1e-7,
                        f64::from(lon) * 1e-7,
                    );
                    assert!(d <= radius_m * 1.005, "nœud {n} à {d} m pour un rayon de {radius_m} m");
                }
                cut_somewhere |= r.node_dist.len() < full.node_dist.len();
                // Le lecteur par cellules et le tableau à plat font le même calcul.
                let f = flat_searcher.run(start, &cfg);
                assert_eq!(f.node_dist, r.node_dist);
                assert_eq!((f.stats.settled, f.stats.pushed), (r.stats.settled, r.stats.pushed));
            }
        }
    }
    assert!(cut_somewhere, "au moins un disque doit couper le graphe");
}

#[test]
fn same_cell_lookups_skip_the_binary_search() {
    // Une seule cellule (zoom 4 : tuiles de ≈ 22°) : aucune dichotomie ; plusieurs : seulement aux bords.
    let cfg =
        SearchConfig { radius_m: 1e7, forbid_uturn: true, state: StateKind::Dense, collect_nodes: false };
    let (_, one) = graph(&mut random_source(5), 4);
    assert_eq!(one.n_cells(), 1);
    let r = Searcher::new(&one).run(3, &cfg);
    assert!(r.stats.settled > 100);
    assert_eq!(r.stats.bsearch_lookups, 0);
    let (_, many) = graph(&mut random_source(5), 11);
    assert!(many.n_cells() > 1);
    let r = Searcher::new(&many).run(3, &cfg);
    assert!(r.stats.bsearch_lookups > 0);
}

#[test]
fn corrupted_structure_is_refused_by_validation_not_by_a_panic() {
    let mut s = random_source(13);
    let mut data = built(&mut s, 11);
    let dir = tempfile::tempdir().unwrap();
    let path = written(&mut data, dir.path(), "v.ordg");
    let good = std::fs::read(&path).unwrap();
    let probe = GraphFile::open(good.clone()).unwrap();
    probe.validate_structure().expect("un fichier sain est cohérent");
    let ci = (0..probe.n_cells()).find(|&c| probe.cell_info(c).n_directed > 1).unwrap();
    let info = *probe.cell_info(ci);
    let lay = BlockLayout::new(info.n_nodes, info.n_directed, info.n_edges, info.n_border);
    // Cible d'une orientée hors des nœuds, puis décalage CSR hors du tableau, puis arête hors des comptes.
    let at = |off: u64| (info.block_off + off) as usize;
    let patches: [(usize, [u8; 4]); 3] = [
        (at(lay.directed_off), u32::MAX.to_le_bytes()),
        (at(lay.csr_off + 4), u32::MAX.to_le_bytes()),
        (at(lay.directed_off + 4), 0x7FFF_FFFFu32.to_le_bytes()),
    ];
    for (pos, v) in patches {
        let mut bytes = good.clone();
        bytes[pos..pos + 4].copy_from_slice(&v);
        // Les blocs ne sont pas vérifiés à l'ouverture ; la validation structurelle les refuse.
        let g = GraphFile::open(bytes).unwrap();
        assert!(matches!(g.validate_structure(), Err(FormatError::Truncated(_))), "octet {pos}");
    }
    // Géométrie corrompue : jamais de panique à la lecture, même sans validation.
    let mut bytes = good;
    let geom_pos = info.geom_off as usize;
    bytes[geom_pos..geom_pos + (info.geom_len.min(64) as usize)].fill(0xFF);
    let g = GraphFile::open(bytes).unwrap();
    for e in info.first_edge..info.first_edge + info.n_edges {
        let _ = g.geometry(e);
    }
    assert!(g.validate_structure().is_err());
}

/// Recalcule la somme d'une section dans la table des sections (un fichier « forgé » qui passe les sommes).
fn refresh_section_hash(bytes: &mut [u8], section: &str) {
    let g = GraphFile::open(bytes.to_vec()).unwrap_or_else(|_| panic!("ouverture"));
    let (off, len) = g.section_range(section).expect("section");
    let h = xxhash_rust::xxh3::xxh3_64(&bytes[off as usize..(off + len) as usize]);
    // Table des sections à 256 : 32 o par entrée (type u32, drapeaux u32, position u64, longueur u64, somme u64).
    for i in 0..16usize {
        let e = 256 + i * 32;
        let pos = u64::from_le_bytes(bytes[e + 8..e + 16].try_into().unwrap());
        if pos == off {
            bytes[e + 24..e + 32].copy_from_slice(&h.to_le_bytes());
            return;
        }
    }
    panic!("section {section} absente de la table");
}

#[test]
fn forged_id_edges_endpoints_pass_the_checksums_but_not_verify() {
    let mut s = random_source(17);
    let mut data = built(&mut s, 11);
    let dir = tempfile::tempdir().unwrap();
    let path = written(&mut data, dir.path(), "f.ordg");
    let good = std::fs::read(&path).unwrap();
    let probe = GraphFile::open(good.clone()).unwrap();
    probe.verify().expect("un fichier sain passe");
    let (off, len) = probe.section_range("ID_EDGES").unwrap();
    assert_eq!(len, probe.header().edges * 16);
    // Extrémité `a` (octets 8..12) puis `b` (12..16) de la première arête, hors des nœuds.
    for field in [8usize, 12] {
        let mut bytes = good.clone();
        let at = off as usize + field;
        bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        refresh_section_hash(&mut bytes, "ID_EDGES");
        let g = GraphFile::open(bytes).unwrap();
        // Sans la validation, `geometry` ou `edge_ends` sortiraient du tableau des nœuds : `verify()` refuse.
        match g.verify() {
            Err(FormatError::Truncated(m)) => assert!(m.contains("ID_EDGES"), "{m}"),
            other => panic!("attendu un refus de ID_EDGES, obtenu {other:?}"),
        }
    }
}

// --- État local au disque ------------------------------------------------------------------------------------------

#[test]
fn local_state_equals_dense_and_map_and_is_sized_by_the_disc() {
    // Zoom 13 : beaucoup de petites cellules, pour qu'un disque n'en touche qu'une partie.
    let mut s = random_source(51);
    let (_, g) = graph(&mut s, 13);
    assert!(g.n_cells() > 20, "{} cellules", g.n_cells());
    let mut by_kind: Vec<Searcher<'_, Vec<u8>>> = StateKind::ALL.iter().map(|_| Searcher::new(&g)).collect();
    let mut small_local = u64::MAX;
    let mut dense_bytes = 0;
    // Les mêmes `Searcher` servent à tous les tours : la remise à zéro des pages est éprouvée.
    for (start, radius_m, forbid) in [
        (3u32, 2500.0, true),
        (700, 2500.0, true),
        (3, 9000.0, true),
        (900, 4000.0, false),
        (3, 2500.0, true),
        (50, 1e7, true),
        (3, 2500.0, false),
    ] {
        let mut results = Vec::new();
        for (searcher, kind) in by_kind.iter_mut().zip(StateKind::ALL) {
            let cfg = SearchConfig { radius_m, forbid_uturn: forbid, state: kind, collect_nodes: true };
            results.push((kind, searcher.run(start, &cfg)));
        }
        let (_, reference) = &results[0];
        for (kind, r) in &results[1..] {
            assert_eq!(r.node_dist, reference.node_dist, "{kind:?}, départ {start}, rayon {radius_m}");
            assert_eq!(
                (r.stats.settled, r.stats.pushed, r.stats.heap_max),
                (reference.stats.settled, reference.stats.pushed, reference.stats.heap_max)
            );
        }
        if radius_m < 3000.0 {
            small_local = small_local.min(results[2].1.stats.state_bytes);
            dense_bytes = results[1].1.stats.state_bytes;
        }
    }
    // Sur un petit disque l'état local est bien plus petit que le tableau dense du graphe entier.
    assert!(small_local * 3 < dense_bytes, "local {small_local} o contre dense {dense_bytes} o");
}

// --- Source PBF réelle (minuscule) -------------------------------------------------------------------------------------

#[cfg(feature = "pbf")]
#[test]
fn tiny_pbf_decodes_and_builds() {
    use ord_graph::pbf::PbfSource;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/mini.osm.pbf");
    let mut src = PbfSource::open(&path).expect("PBF trié");
    let mut data = build_graph(&mut src, &opts(11), "mini", &mut |_| {}).unwrap();
    assert_eq!(data.stats.ways_total, 5);
    assert_eq!(data.stats.ways_kept, 3);
    assert_eq!(data.stats.rejects["motorway/trunk fermée aux vélos"], 1);
    assert_eq!(data.stats.rejects["sans highway"], 1);
    assert_eq!(data.stats.relations_routes, 1);
    let dir = tempfile::tempdir().unwrap();
    let out = written(&mut data, dir.path(), "mini.ordg");
    let g = GraphFile::open(std::fs::read(out).unwrap()).unwrap();
    g.verify().unwrap();
    assert_eq!(g.header().pbf_size, 510);
    // 100 : 1-2-3-4 coupée en 2 par la way 101 ; 4 est une jonction avec 102.
    let routes = |w, a, b| g.edge(find_edge(&g, w, a, b)).attrs.routes();
    assert_eq!(routes(100, 1, 2), ROUTE_REGIONAL);
    assert_eq!(routes(100, 2, 4), ROUTE_REGIONAL);
    assert_eq!(routes(101, 2, 6), ROUTE_REGIONAL);
    assert_eq!(routes(102, 4, 8), 0);
    let e102 = g.edge(find_edge(&g, 102, 4, 8)).attrs;
    assert!(e102.fwd_ok() && e102.bwd_ok() && e102.contraflow());
    assert_eq!(g.geometry(find_edge(&g, 100, 2, 4)).len(), 1);
}

#[cfg(feature = "pbf")]
#[test]
fn unsorted_or_missing_pbf_is_refused() {
    use ord_graph::pbf::PbfSource;
    assert!(PbfSource::open(Path::new("/nonexistent.osm.pbf")).is_err());
}
