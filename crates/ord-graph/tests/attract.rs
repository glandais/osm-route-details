//! Fichier d'attracteurs (`.orda`) : format, identité, accroche, zone, extraction, grille (lot 4d, §17.13 1 à 7).

use std::path::Path;

use ord_graph::attract::{
    anchors_for, build_attr, cats_of, encode_attr, extract_objects, zone_for, AnchorGrid, AttrData,
    AttrError, AttrFile, AttrWrite, Object, CATS, CAT_BAKERY, CAT_CAFE, CAT_MOUNTAIN_PASS, CAT_PEAK,
    CAT_TOWN, CAT_VIEWPOINT, CAT_VILLAGE,
};
use ord_graph::build::{build_graph, BuildOptions, GraphData, MemSource};
use ord_graph::format::{write_graph, GraphFile};
use ord_graph::view::{build_view, ViewSpec};
use xxhash_rust::xxh3::xxh3_64;

const LAT0: f64 = 47.2;
const LON0: f64 = -1.56;
/// Quadrillage dense à cheval sur deux tuiles z11 (la frontière est à -1,40625).
const DLON0: f64 = -1.42;
const R: f64 = 6_371_008.8;

fn rng(seed: u64) -> impl FnMut() -> u64 {
    let mut x = seed;
    move || {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

fn base_of(data: GraphData, epoch: i64, xxh3: u64) -> GraphFile<Vec<u8>> {
    let mut data = data;
    data.meta.xxh3 = xxh3;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("b.ordg");
    write_graph(&path, &mut data, "test", epoch).expect("écriture");
    GraphFile::open(std::fs::read(path).unwrap()).expect("ouverture")
}

fn built(src: &mut MemSource) -> GraphData {
    build_graph(src, &BuildOptions { zoom: 11 }, "test", &mut |_| {}).expect("construction")
}

fn view_of(base: &GraphFile<Vec<u8>>, name: &str, epoch: i64) -> GraphFile<Vec<u8>> {
    let mut data = build_view(base, &ViewSpec::parse(name).unwrap()).expect("vue");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v.ordv");
    write_graph(&path, &mut data, "test", epoch).expect("écriture");
    GraphFile::open(std::fs::read(path).unwrap()).expect("ouverture")
}

/// Un quadrillage dense de rues (jonctions partout) et un chemin, dans un carré de ~1 km.
fn dense_source(seed: u64) -> MemSource {
    let mut r = rng(seed);
    let mut s = MemSource::default();
    let n = 2000i64;
    for i in 0..n {
        let lat = LAT0 + (r() % 100_000) as f64 / 100_000.0 * 0.02;
        let lon = DLON0 + (r() % 100_000) as f64 / 100_000.0 * 0.03;
        s.node(1000 + i, lat, lon);
    }
    for w in 0..1500i64 {
        let len = 2 + (r() % 5) as usize;
        let start = (r() % n as u64) as i64;
        let refs: Vec<i64> =
            (0..len as i64).map(|k| 1000 + (start + k * (1 + (r() % 3) as i64)) % n).collect();
        let class = if w % 5 == 0 { "track" } else { "residential" };
        s.way(500 + w, &refs, &[("highway", class)]);
    }
    s
}

struct Fixture {
    base: GraphFile<Vec<u8>>,
    road: GraphFile<Vec<u8>>,
    gravel: GraphFile<Vec<u8>>,
}

fn fixture() -> Fixture {
    let base = base_of(built(&mut dense_source(5)), 1_700_000_000, 0xABCD);
    let road = view_of(&base, "road", 1_700_000_100);
    let gravel = view_of(&base, "gravel", 1_700_000_200);
    Fixture { base, road, gravel }
}

fn write_params(meta: &[u8]) -> AttrWrite<'_> {
    AttrWrite { snap_m: 300, reach_m: 300, meta, built_epoch: 1_700_000_300 }
}

/// Objets pseudo-aléatoires dans la zone du quadrillage.
fn random_objects(seed: u64, n: usize) -> Vec<Object> {
    let mut r = rng(seed);
    let mut v: Vec<Object> = (0..n)
        .map(|i| Object {
            cat: (r() % 7) as u8,
            lat: ((LAT0 + (r() % 100_000) as f64 / 100_000.0 * 0.024) * 1e7) as i32,
            lon: ((DLON0 + (r() % 100_000) as f64 / 100_000.0 * 0.034) * 1e7) as i32,
            kind: (i % 2) as u8,
            id: i as i64,
        })
        .collect();
    v.sort_unstable();
    v
}

fn data_for(f: &Fixture) -> AttrData {
    build_attr(&f.road, &random_objects(1, 400), 300, 300)
}

fn refix(bytes: &mut [u8]) {
    let n = bytes.len() - 8;
    let sum = xxh3_64(&bytes[..n]);
    bytes[n..].copy_from_slice(&sum.to_le_bytes());
}

// --- 1. Format -----------------------------------------------------------------------------------------------------

#[test]
fn a_written_file_reads_back_exactly() {
    let f = fixture();
    let data = data_for(&f);
    assert!(data.anchors.len() > 50 && data.zone.len() > 300, "{} {}", data.anchors.len(), data.zone.len());
    let meta = br#"{"tool_version":"test","per_cat":[1,2,3]}"#;
    let bytes = encode_attr(f.base.header(), &f.road, &data, &write_params(meta)).unwrap();
    assert_eq!(bytes.len() % 8, 0);
    let a = AttrFile::open(bytes.clone()).expect("ouverture");
    let h = a.header();
    assert_eq!((h.major, h.minor, h.view_kind, h.n_cats, h.snap_m, h.reach_m), (1, 0, 0, 7, 300, 300));
    assert_eq!((h.base_nodes, h.base_edges), (f.base.header().nodes, f.base.header().edges));
    assert_eq!((h.base_pbf_size, h.base_pbf_xxh3, h.base_build_epoch), (0, 0xABCD, 1_700_000_000));
    assert_eq!(
        (h.view_nodes, h.view_edges, h.view_build_epoch),
        (f.road.header().nodes, f.road.header().edges, 1_700_000_100)
    );
    assert_eq!((h.view_cells as usize, h.built_epoch), (f.road.n_cells(), 1_700_000_300));
    assert_eq!(a.cats(), CATS.map(String::from).to_vec());
    assert_eq!(a.meta(), meta);
    assert_eq!(a.anchors().collect::<Vec<_>>(), data.anchors);
    let zone: Vec<(u32, u8)> =
        a.zone_nodes().iter().zip(a.zone_masks()).map(|(n, &m)| (n.get(), m)).collect();
    assert_eq!(zone, data.zone);
    // Accès par cellule : chaque nœud de zone est retrouvé dans sa cellule, avec son masque.
    for &(node, mask) in &data.zone {
        assert_eq!(a.zone_mask(f.road.node_cell(node), node), mask);
    }
    // Un nœud hors zone : 0.
    let outside =
        (0..f.road.header().nodes as u32).find(|n| data.zone.binary_search_by_key(n, |z| z.0).is_err());
    if let Some(n) = outside {
        assert_eq!(a.zone_mask(f.road.node_cell(n), n), 0);
    }
    a.check_for(f.base.header(), &f.road).expect("identité");
    // Écriture sur disque, lecture par mmap.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.road.orda");
    let n =
        ord_graph::attract::write_attr(&path, f.base.header(), &f.road, &data, &write_params(meta)).unwrap();
    assert_eq!(n as usize, bytes.len());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let m = AttrFile::open_mmap(&path).unwrap();
    assert_eq!(m.anchors().collect::<Vec<_>>(), data.anchors);
}

#[test]
fn a_damaged_file_is_refused() {
    let f = fixture();
    let data = data_for(&f);
    let good = encode_attr(f.base.header(), &f.road, &data, &write_params(b"{}")).unwrap();
    let open = |b: Vec<u8>| AttrFile::open(b).map(|_| ()).unwrap_err();

    let mut b = good.clone();
    b[0] = b'X';
    assert!(matches!(open(b), AttrError::BadMagic));

    let mut b = good.clone();
    b[8] = 2; // major 2
    assert!(matches!(open(b), AttrError::UnsupportedMajor(2)));

    assert!(matches!(open(good[..good.len() - 16].to_vec()), AttrError::Truncated(_)));
    assert!(matches!(open(good[..60].to_vec()), AttrError::Truncated(_)));

    let mut b = good.clone();
    let mid = b.len() / 2;
    b[mid] ^= 0xFF;
    assert!(matches!(open(b), AttrError::Corrupt(m) if m.contains("empreinte")), "empreinte de fin");

    // Modifications cohérentes avec l'empreinte : seule la structure les refuse.
    let na = data.anchors.len();
    let nz = data.zone.len();
    let cells = f.road.n_cells() + 1;
    let a_off = 128 + 7 * 16usize;
    let a_off = a_off.next_multiple_of(8);
    let a_node = a_off + (4 * cells).next_multiple_of(8);
    let a_cat = a_node + (4 * na).next_multiple_of(8);
    let z_off = a_cat + na.next_multiple_of(8);
    let z_node = z_off + (4 * cells).next_multiple_of(8);
    let z_mask = z_node + (4 * nz).next_multiple_of(8);

    // Décalage décroissant : on échange deux décalages consécutifs distincts.
    let mut b = good.clone();
    let offs: Vec<u32> = (0..cells)
        .map(|i| u32::from_le_bytes(b[a_off + 4 * i..a_off + 4 * i + 4].try_into().unwrap()))
        .collect();
    let i = (1..cells - 1).find(|&i| offs[i] != offs[i + 1]).expect("deux décalages distincts");
    b[a_off + 4 * i..a_off + 4 * i + 4].copy_from_slice(&offs[i + 1].to_le_bytes());
    b[a_off + 4 * (i + 1)..a_off + 4 * (i + 1) + 4].copy_from_slice(&offs[i].to_le_bytes());
    refix(&mut b);
    assert!(matches!(open(b), AttrError::Corrupt(m) if m.contains("ANCHOR_OFF")));

    // Nœuds de zone non triés.
    let mut b = good.clone();
    let (x, y) = (z_node + 4 * 3, z_node + 4 * 4);
    let (vx, vy): (Vec<u8>, Vec<u8>) = (b[x..x + 4].to_vec(), b[y..y + 4].to_vec());
    b[x..x + 4].copy_from_slice(&vy);
    b[y..y + 4].copy_from_slice(&vx);
    refix(&mut b);
    assert!(matches!(open(b), AttrError::Corrupt(m) if m.contains("zone")));

    // Masque nul, puis bit hors table (bit 7 avec 7 catégories).
    let mut b = good.clone();
    b[z_mask + 2] = 0;
    refix(&mut b);
    assert!(matches!(open(b), AttrError::Corrupt(m) if m.contains("ZONE_MASK")));
    let mut b = good.clone();
    b[z_mask + 2] = 0x81;
    refix(&mut b);
    assert!(matches!(open(b), AttrError::Corrupt(m) if m.contains("ZONE_MASK")));

    // Table des catégories renumérotée (boulangerie au bit du café) : refusée.
    let mut b = good.clone();
    b[128..128 + 16].copy_from_slice(b"bakery\0\0\0\0\0\0\0\0\0\0");
    refix(&mut b);
    assert!(matches!(open(b), AttrError::Corrupt(m) if m.contains("CATS")));

    // Catégorie d'ancrage hors table.
    let mut b = good.clone();
    b[a_cat] = 9;
    refix(&mut b);
    assert!(matches!(open(b), AttrError::Corrupt(m) if m.contains("ANCHOR_CAT")));

    // Ancrages en double : le deuxième recopie le premier.
    let mut b = good.clone();
    let first: Vec<u8> = b[a_node..a_node + 4].to_vec();
    b[a_node + 4..a_node + 8].copy_from_slice(&first);
    let c0 = b[a_cat];
    b[a_cat + 1] = c0;
    refix(&mut b);
    assert!(matches!(open(b), AttrError::Corrupt(m) if m.contains("ancrages")));
}

// --- 2. Identité -----------------------------------------------------------------------------------------------------

#[test]
fn check_for_refuses_another_view_another_base_and_misplaced_nodes() {
    let f = fixture();
    let data = data_for(&f);
    let a =
        AttrFile::open(encode_attr(f.base.header(), &f.road, &data, &write_params(b"{}")).unwrap()).unwrap();
    a.check_for(f.base.header(), &f.road).expect("la vue d'origine");

    // L'autre type de vue.
    assert!(
        matches!(a.check_for(f.base.header(), &f.gravel), Err(AttrError::Mismatch(m)) if m.contains("type"))
    );
    // Une vue reconstruite (autre date de construction).
    let rebuilt = view_of(&f.base, "road", 1_700_000_999);
    assert!(
        matches!(a.check_for(f.base.header(), &rebuilt), Err(AttrError::Mismatch(m)) if m.contains("vue"))
    );
    // Un graphe de base d'un autre extrait (même contenu, autre empreinte de PBF).
    let other_base = base_of(built(&mut dense_source(5)), 1_700_000_000, 0xDCBA);
    assert!(
        matches!(a.check_for(other_base.header(), &f.road), Err(AttrError::Mismatch(m)) if m.contains("base"))
    );
    // … ou d'une autre construction.
    let other_epoch = base_of(built(&mut dense_source(5)), 1_700_000_001, 0xABCD);
    assert!(a.check_for(other_epoch.header(), &f.road).is_err());
    // Une vue élaguée n'a pas de fichier.
    let pruned = view_of(&f.base, "road-prune", 1_700_000_100);
    assert!(encode_attr(f.base.header(), &pruned, &data, &write_params(b"{}")).is_err());

    // Un nœud hors de sa cellule : un id au-delà du dernier nœud de la vue.
    let mut bad = data.clone();
    let past = f.road.header().nodes as u32 + 5;
    bad.zone.push((past, 1));
    let b =
        AttrFile::open(encode_attr(f.base.header(), &f.road, &bad, &write_params(b"{}")).unwrap()).unwrap();
    assert!(
        matches!(b.check_for(f.base.header(), &f.road), Err(AttrError::Mismatch(m)) if m.contains("cellule"))
    );
}

// --- 3. Accroche -----------------------------------------------------------------------------------------------------

/// Deux extrémités `A`, `B` à 1 km l'une de l'autre sur la même latitude : deux nœuds de vue, rien entre eux.
fn two_ends() -> (GraphFile<Vec<u8>>, GraphFile<Vec<u8>>) {
    let mut s = MemSource::default();
    s.node(1, LAT0, LON0).node(2, LAT0, LON0 + 0.015).node(3, LAT0 + 0.0001, LON0 + 0.0075);
    s.way(10, &[1, 3, 2], &[("highway", "residential")]);
    let base = base_of(built(&mut s), 1_700_000_000, 7);
    let view = view_of(&base, "road", 1_700_000_100);
    (base, view)
}

fn east_of(lat: f64, lon: f64, metres: f64) -> (i32, i32) {
    let dlon = (metres / (R * lat.to_radians().cos())).to_degrees();
    ((lat * 1e7).round() as i32, ((lon + dlon) * 1e7).round() as i32)
}

#[test]
fn anchors_keep_the_nearest_node_within_the_snap_distance() {
    let (_base, view) = two_ends();
    assert_eq!(view.header().nodes, 2);
    let a_gid = (0..2u32).find(|&g| view.node_pos(g).1 < ((LON0 + 0.001) * 1e7) as i32).unwrap();
    let (la, lo) = east_of(LAT0, LON0, 299.0);
    let near = Object { cat: CAT_CAFE, lat: la, lon: lo, kind: 0, id: 1 };
    assert_eq!(anchors_for(&view, &[near], 300), vec![(a_gid, CAT_CAFE)]);
    let (la, lo) = east_of(LAT0, LON0, 301.0);
    let far = Object { cat: CAT_CAFE, lat: la, lon: lo, kind: 0, id: 2 };
    assert!(anchors_for(&view, &[far], 300).is_empty());
    // Doublons fondus : même nœud et même catégorie ; deux catégories donnent deux ancrages.
    let twin = Object { id: 3, kind: 1, ..near };
    let other = Object { cat: CAT_VIEWPOINT, id: 4, ..near };
    assert_eq!(
        anchors_for(&view, &[near, twin, other], 300),
        vec![(a_gid, CAT_CAFE), (a_gid, CAT_VIEWPOINT)]
    );
}

#[test]
fn a_tie_goes_to_the_smaller_node_id() {
    // Deux nœuds à égale distance (même latitude, longitudes symétriques) de l'objet.
    let mut s = MemSource::default();
    s.node(1, LAT0, LON0 - 0.001).node(2, LAT0, LON0 + 0.001).node(3, LAT0 + 0.002, LON0);
    s.way(10, &[1, 3, 2], &[("highway", "residential")]);
    let base = base_of(built(&mut s), 1_700_000_000, 7);
    let view = view_of(&base, "road", 1_700_000_100);
    assert_eq!(view.header().nodes, 2);
    let o = Object { cat: CAT_VILLAGE, lat: (LAT0 * 1e7) as i32, lon: (LON0 * 1e7) as i32, kind: 0, id: 1 };
    let got = anchors_for(&view, &[o], 300);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, 0, "égalité : le plus petit id");
}

// --- 4. Zone -------------------------------------------------------------------------------------------------------

fn haversine(a: (i32, i32), b: (i32, i32)) -> f64 {
    let (p1, p2) = (f64::from(a.0) * 1e-7, f64::from(b.0) * 1e-7);
    let (dphi, dl) = ((p2 - p1).to_radians(), (f64::from(b.1) - f64::from(a.1)).to_radians() * 1e-7);
    let h =
        (dphi / 2.0).sin().powi(2) + p1.to_radians().cos() * p2.to_radians().cos() * (dl / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().min(1.0).asin()
}

#[test]
fn the_zone_equals_an_exhaustive_search() {
    let f = fixture();
    let n = f.road.header().nodes as u32;
    assert!(n > 800 && f.road.n_cells() >= 2, "{n} nœuds, {} cellules", f.road.n_cells());
    let pos: Vec<(i32, i32)> = (0..n).map(|g| f.road.node_pos(g)).collect();
    // 2 000 couples (nœud, catégorie) tirés au hasard (avec doublons) comme ancrages.
    let mut r = rng(9);
    let mut anchors: Vec<(u32, u8)> =
        (0..2000).map(|_| ((r() % u64::from(n)) as u32, (r() % 7) as u8)).collect();
    anchors.sort_unstable();
    anchors.dedup();
    let zone = zone_for(&f.road, &anchors, 300);
    let mut want = vec![0u8; n as usize];
    for &(a, c) in &anchors {
        for g in 0..n {
            if haversine(pos[a as usize], pos[g as usize]) <= 300.0 {
                want[g as usize] |= 1 << c;
            }
        }
    }
    let got: std::collections::BTreeMap<u32, u8> = zone.iter().copied().collect();
    assert_eq!(got.len(), zone.len(), "nœuds uniques");
    assert!(zone.windows(2).all(|w| w[0].0 < w[1].0), "nœuds strictement croissants");
    for g in 0..n {
        assert_eq!(got.get(&g).copied().unwrap_or(0), want[g as usize], "nœud {g}");
    }
    // L'ancrage est dans sa zone, avec sa catégorie.
    for &(a, c) in &anchors {
        assert!(got[&a] >> c & 1 != 0);
    }
}

// --- 5. Objets -------------------------------------------------------------------------------------------------------

fn objects_source() -> MemSource {
    let mut s = MemSource::default();
    s.node(1, 47.0, -1.0).node(2, 47.0, -0.9).node(3, 47.1, -1.0).node(4, 47.3, -1.3).node(5, 47.31, -1.31);
    s.node(6, 47.4, -1.4).node(7, 47.5, -1.5).node(8, 47.6, -1.6);
    // Un café en bâtiment : triangle fermé (le premier nœud est répété).
    s.way(10, &[1, 2, 3, 1], &[("amenity", "cafe"), ("building", "yes")]);
    // Un café en nœud (le nœud 4) ; un point de vue et un sommet sur le nœud 5.
    s.node_tag(4, &[("amenity", "cafe")]);
    s.node_tag(5, &[("tourism", "viewpoint"), ("natural", "peak")]);
    // Ignorés : hameau, col « no », sommet en way, relation.
    s.node_tag(6, &[("place", "hamlet")]).node_tag(7, &[("mountain_pass", "no")]);
    s.way(11, &[6, 7], &[("natural", "peak")]);
    s.relation(900, &[("amenity", "cafe")], &[10]);
    // Deux catégories sur un objet : deux objets.
    s.node_tag(8, &[("place", "town"), ("mountain_pass", "yes")]);
    s
}

#[test]
fn objects_come_from_nodes_and_ways_and_relations_are_ignored() {
    let mut src = objects_source();
    let (objs, stats) = extract_objects(&mut src).unwrap();
    let simple: std::collections::BTreeSet<(u8, u8, i64)> =
        objs.iter().map(|o| (o.cat, o.kind, o.id)).collect();
    let want: std::collections::BTreeSet<(u8, u8, i64)> = [
        (CAT_CAFE, 0, 4),
        (CAT_CAFE, 1, 10),
        (CAT_VIEWPOINT, 0, 5),
        (CAT_PEAK, 0, 5),
        (CAT_TOWN, 0, 8),
        (CAT_MOUNTAIN_PASS, 0, 8),
    ]
    .into_iter()
    .collect();
    assert_eq!(simple, want);
    assert_eq!(objs.len(), 6);
    assert_eq!((stats.ways_tagged, stats.ways_without_node), (1, 0));
    // Barycentre d'une way fermée : le nœud répété n'est compté qu'une fois (47,0333…, -0,9667…).
    let way = objs.iter().find(|o| o.kind == 1).unwrap();
    assert_eq!(
        (way.lat, way.lon),
        (
            ((470_000_000i64 + 470_000_000 + 471_000_000) / 3) as i32,
            ((-10_000_000i64 - 9_000_000 - 10_000_000) / 3) as i32
        )
    );
    // Tri par (catégorie, lat, lon, type, id).
    assert!(objs.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(cats_of([("natural", "peak")], false), 0);
    assert_eq!(cats_of([("natural", "peak")], true), 1 << CAT_PEAK);
    assert_eq!(cats_of([("place", "hamlet"), ("mountain_pass", "no")], true), 0);
    assert_eq!(cats_of([("shop", "bakery")], false), 1 << CAT_BAKERY);
}

#[test]
fn a_way_without_any_known_node_is_dropped_and_counted() {
    let mut s = MemSource::default();
    s.node(1, 47.0, -1.0);
    s.way(10, &[100, 101], &[("tourism", "viewpoint")]);
    s.way(11, &[1], &[("tourism", "viewpoint")]);
    let (objs, stats) = extract_objects(&mut s).unwrap();
    assert_eq!(objs.len(), 1);
    assert_eq!(
        (stats.ways_tagged, stats.ways_without_node, stats.nodes_wanted, stats.nodes_found),
        (2, 1, 3, 1)
    );
}

// --- 6. Déterminisme -------------------------------------------------------------------------------------------------

#[cfg(feature = "pbf")]
#[test]
fn extraction_from_the_pbf_does_not_depend_on_threads_or_batch_size() {
    use ord_graph::pbf::PbfSource;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/attr.osm.pbf");
    let run = |threads: usize, batch: Option<usize>| {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        pool.install(|| {
            let mut src = PbfSource::open(&path).unwrap();
            if let Some(n) = batch {
                src = src.with_batch_len(n);
            }
            extract_objects(&mut src).unwrap()
        })
    };
    let (reference, stats) = run(1, None);
    // Café, point de vue et sommet (même nœud), bourg, col, boulangerie en bâtiment ; ni hameau, ni way de sommet.
    let cats: std::collections::BTreeSet<u8> = reference.iter().map(|o| o.cat).collect();
    let want: std::collections::BTreeSet<u8> =
        [CAT_CAFE, CAT_BAKERY, CAT_VIEWPOINT, CAT_VILLAGE, CAT_MOUNTAIN_PASS, CAT_PEAK].into_iter().collect();
    assert_eq!(cats, want);
    assert_eq!(reference.len(), 6);
    assert_eq!(stats.ways_tagged, 1, "seule la boulangerie en bâtiment est une way retenue");
    for (threads, batch) in [(4, None), (1, Some(1)), (4, Some(1)), (2, Some(2))] {
        assert_eq!(run(threads, batch).0, reference, "{threads} fils, lots de {batch:?}");
    }
}

// --- 7. Grille des ancrages -----------------------------------------------------------------------------------

#[test]
fn the_anchor_grid_equals_an_exhaustive_search() {
    let f = fixture();
    let data = data_for(&f);
    let a =
        AttrFile::open(encode_attr(f.base.header(), &f.road, &data, &write_params(b"{}")).unwrap()).unwrap();
    let grid = AnchorGrid::new(&a, &f.road);
    let mut r = rng(31);
    let mut nonzero = 0;
    for _ in 0..3000 {
        let lat = ((LAT0 - 0.003 + (r() % 100_000) as f64 / 100_000.0 * 0.026) * 1e7) as i32;
        let lon = ((DLON0 - 0.004 + (r() % 100_000) as f64 / 100_000.0 * 0.038) * 1e7) as i32;
        let mut want = 0u8;
        for (node, cat) in a.anchors() {
            if haversine((lat, lon), f.road.node_pos(node)) <= 300.0 {
                want |= 1 << cat;
            }
        }
        assert_eq!(grid.hits(lat, lon), want);
        nonzero += usize::from(want != 0);
    }
    assert!(nonzero > 300, "{nonzero} points touchés");
}
