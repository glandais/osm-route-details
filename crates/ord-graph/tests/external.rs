//! Construction hors mémoire (`docs/design/technique/ord-graph-build-hors-memoire.md` §8) : mêmes octets que la
//! construction en mémoire, quels que soient le budget (donc le découpage en séries) et le nombre de fils.

#![cfg(feature = "external")]

use std::path::{Path, PathBuf};

use ord_graph::build::{
    build_graph, BuildError, BuildOptions, ElementSink, MemSource, OsmSource, SourceMeta, TaggedNodeFn,
};
use ord_graph::external::{build_external, ExternalError, ExternalOptions, ExternalReport};
use ord_graph::format::{write_graph, GraphFile};
use ord_graph::stats::BuildStats;

const TOOL: &str = "test";
const EPOCH: i64 = 1_700_000_000;

/// Construction en mémoire puis écriture : le fichier de référence et ses compteurs.
fn in_memory(src: &mut dyn OsmSource, zoom: u8, dir: &Path) -> (PathBuf, BuildStats) {
    let mut data =
        build_graph(src, &BuildOptions { zoom }, "test", &mut |_| {}).expect("construction en mémoire");
    let path = dir.join("mem.ordg");
    write_graph(&path, &mut data, TOOL, EPOCH).expect("écriture");
    (path, data.stats)
}

fn external(src: &mut dyn OsmSource, zoom: u8, budget: usize, dir: &Path) -> (PathBuf, ExternalReport) {
    let path = dir.join("ext.ordg");
    let opts = ExternalOptions { zoom, mem_budget: budget, tmp_parent: dir.to_path_buf() };
    let report = build_external(src, &opts, "test", &path, TOOL, EPOCH, Vec::new(), &mut |_| {})
        .expect("construction hors mémoire");
    (path, report)
}

/// Compteurs comparables : tout sauf les durées et le nombre de fils.
fn counters(s: &BuildStats) -> serde_json::Value {
    let mut v = serde_json::to_value(s).unwrap();
    let o = v.as_object_mut().unwrap();
    o.remove("phases");
    o.remove("threads");
    v
}

/// Mêmes en-têtes (outil et date identiques) et mêmes sommes de toutes les sections hors `STATS`.
fn assert_same_graph(a: &Path, b: &Path) {
    let ga = GraphFile::open(std::fs::read(a).unwrap()).unwrap();
    let gb = GraphFile::open(std::fs::read(b).unwrap()).unwrap();
    gb.verify().expect("verify() du fichier hors mémoire");
    assert_eq!(ga.header(), gb.header(), "en-têtes");
    assert_eq!(ga.section_digests(), gb.section_digests(), "sections");
}

/// Le dossier de sortie ne contient plus que le fichier final (pas de `.tmp`, pas de dossier temporaire).
fn assert_clean(dir: &Path, keep: &[&str]) {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut want: Vec<String> = keep.iter().map(|s| (*s).to_string()).collect();
    want.sort();
    assert_eq!(names, want, "contenu du dossier");
}

fn compare(src: &MemSource, zoom: u8, budgets: &[usize], threads: &[usize]) {
    let dir = tempfile::tempdir().unwrap();
    let (mem, mem_stats) = in_memory(&mut src.clone(), zoom, dir.path());
    for &t in threads {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(t).build().unwrap();
        for &budget in budgets {
            let (ext, report) = pool.install(|| external(&mut src.clone(), zoom, budget, dir.path()));
            assert_same_graph(&mem, &ext);
            assert_eq!(
                counters(&mem_stats),
                counters(&report.stats),
                "compteurs (budget {budget}, {t} fils)"
            );
            assert_clean(dir.path(), &["mem.ordg", "ext.ordg"]);
            std::fs::remove_file(&ext).unwrap();
        }
    }
}

// --- Graphes aléatoires ----------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len() as u64) as usize]
    }
}

const TAGS: &[&[(&str, &str)]] = &[
    &[("highway", "residential")],
    &[("highway", "residential"), ("oneway", "yes")],
    &[("highway", "primary"), ("oneway", "-1"), ("surface", "asphalt")],
    &[("highway", "cycleway")],
    &[("highway", "track"), ("tracktype", "grade3")],
    &[("highway", "footway"), ("bicycle", "no")],
    &[("highway", "motorway")],
    &[("highway", "pedestrian"), ("area", "yes")],
    &[("building", "yes")],
    &[("highway", "service"), ("junction", "roundabout")],
];

/// Réseau aléatoire sur une zone qui couvre plusieurs cellules : jonctions, ways qui repassent par un nœud,
/// boucles fermées, doublons consécutifs, nœuds référencés mais absents, relations vers des ways absentes ou
/// rejetées, ids de nœuds clairsemés.
fn random_source(seed: u64) -> MemSource {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut s = MemSource::default();
    let n_nodes = 20 + r.below(400) as i64;
    let mut ids = Vec::new();
    let mut id = 1 + r.below(1000) as i64;
    for _ in 0..n_nodes {
        id += 1 + r.below(50) as i64;
        ids.push(id);
        // Un nœud sur 15 manque à l'extrait.
        if r.below(15) != 0 {
            let lat = 47.0 + r.below(20_000) as f64 * 1e-5;
            let lon = -1.6 + r.below(20_000) as f64 * 1e-5;
            s.node(id, lat, lon);
        }
    }
    let n_ways = r.below(120);
    let mut way_ids = Vec::new();
    let mut wid = 1 + r.below(100) as i64;
    for _ in 0..n_ways {
        wid += 1 + r.below(10) as i64;
        way_ids.push(wid);
        let len = 1 + r.below(12) as usize;
        let mut refs: Vec<i64> = (0..len).map(|_| *r.pick(&ids)).collect();
        match r.below(6) {
            0 => refs.push(refs[0]),                  // boucle fermée
            1 => refs.insert(len / 2, refs[len / 2]), // doublon consécutif
            2 if len > 3 => refs.push(refs[1]),       // repasse par un nœud de la way
            _ => {}
        }
        s.way(wid, &refs, r.pick(TAGS));
    }
    for rel in 0..r.below(5) {
        let net = *r.pick(&["rcn", "lcn", "ncn", "icn"]);
        let mut members: Vec<i64> = (0..r.below(8))
            .map(|_| *r.pick(&way_ids[..].iter().chain(&[999_999]).copied().collect::<Vec<_>>()))
            .collect();
        members.push(1); // way absente
        s.relation(rel as i64 + 1, &[("type", "route"), ("route", "bicycle"), ("network", net)], &members);
    }
    s
}

// --- Oracle : le code d'avant la découpe partagée ------------------------------------------------------

/// Les deux constructions partagent `keep_way` et `cut_way` (§13.3, point 8) : se comparer l'une à l'autre ne
/// dit rien d'une erreur dans ce code commun. Ces sommes viennent du code de `develop` (`b47ae2d`), d'avant la
/// construction hors mémoire, sur les mêmes entrées : elles gardent une référence indépendante de la découpe.
/// Une somme : xxh3 de « nom=somme; » pour chaque section hors `STATS`, outil `test`, date 1 700 000 000.
const ORACLE_RANDOM: [u64; 60] = [
    0x44757c4856d77639,
    0xed0daa89b0286efb,
    0xaac1ca9dd0b169af,
    0x3f340b8340a27112,
    0x5fe8a9d8c1122011,
    0x4710809034d2b23c,
    0x578168ce40a856a3,
    0x1d3a01802078d267,
    0x8c303f398115232e,
    0xf1beea3dd3886b63,
    0xbfaf927814aa6beb,
    0xc264dbc72bc5bb06,
    0x5dfd1bc488d88ab2,
    0x9cb90804218c7a4e,
    0x3599e5ba34f8309a,
    0xcd4785e81f1c443c,
    0x755363b0945c87f7,
    0x6eb9e210f01439de,
    0x30faaface04689fa,
    0x9b0c00183fdf525f,
    0x6bce1a211e8ee33c,
    0x237271a6159f8ecf,
    0x5559d85091dffb3d,
    0x78fdc8d3953d1453,
    0x50c14b710553a6f0,
    0x165ffb91f5018463,
    0xfe007aede173fd10,
    0x011550813f51556e,
    0xca3988c6c79b50f1,
    0x1455e9025cbc672d,
    0x3df10cebca9296f0,
    0x487ded3f2ed8a8cd,
    0xf1e45ebd07a0ddd7,
    0x40f23b2f4fadfe5a,
    0xdd140a21f5a4d569,
    0x369d098ab1b03e80,
    0x7fff1114b3afb6eb,
    0xe319538a690abd7a,
    0xd6e870c00cc17db7,
    0x4a95dca47a5f34ee,
    0x4a3609e0f4b424ec,
    0xc2dccbb9265e5de6,
    0x4155c7c41d691e5a,
    0x4a428b9f6bdfdfc7,
    0xf75f5ad5f17f6cf2,
    0x87e7fc85baab7d32,
    0x8c596796b31cc5c7,
    0x4399289866ed7fe5,
    0x5ac046f9703e2e02,
    0x6a7fa927051954fa,
    0xd5487fd54b5bc6d9,
    0x6fd568f69ceeb483,
    0x55522c247eca4991,
    0x21f95fa1517390c9,
    0x8e3d34bf8316295d,
    0x557d5b6e81611573,
    0xadccca3623539de0,
    0x37814a93a4c9e192,
    0x49087254f040a375,
    0xc08e5dd8a1cc8203,
];
const ORACLE_DENSE: u64 = 0xf59670f898c031e6;
#[cfg(feature = "pbf")]
const ORACLE_MINI: u64 = 0x8aedcc66a86535f6;

fn digest(path: &Path) -> u64 {
    let g = GraphFile::open(std::fs::read(path).unwrap()).unwrap();
    let s: String = g.section_digests().iter().map(|(n, d)| format!("{n}={d:016x};")).collect();
    xxhash_rust::xxh3::xxh3_64(s.as_bytes())
}

/// Les deux constructions retrouvent, octet pour octet, le graphe du code d'avant.
fn assert_oracle(src: &mut dyn FnMut() -> Box<dyn OsmSource>, zoom: u8, want: u64, what: &str) {
    let dir = tempfile::tempdir().unwrap();
    let (mem, _) = in_memory(&mut *src(), zoom, dir.path());
    assert_eq!(digest(&mem), want, "{what} : en mémoire contre develop");
    let (ext, _) = external(&mut *src(), zoom, 1, dir.path());
    assert_eq!(digest(&ext), want, "{what} : hors mémoire contre develop");
}

#[test]
fn same_graphs_as_develop() {
    for (seed, &want) in ORACLE_RANDOM.iter().enumerate() {
        let zoom = [11u8, 13, 15][seed % 3];
        assert_oracle(&mut || Box::new(random_source(seed as u64)), zoom, want, &format!("graine {seed}"));
    }
    assert_oracle(&mut || Box::new(dense_source()), 13, ORACLE_DENSE, "réseau dense");
    #[cfg(feature = "pbf")]
    {
        let pbf = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/mini.osm.pbf");
        assert_oracle(
            &mut || Box::new(ord_graph::pbf::PbfSource::open(&pbf).unwrap()),
            11,
            ORACLE_MINI,
            "mini-PBF",
        );
    }
}

#[test]
fn random_graphs_same_bytes_whatever_budget_and_threads() {
    let mut total = BuildStats::default();
    let mut max_cells = 0;
    let mut rejected = 0;
    for seed in 0..60 {
        let src = random_source(seed);
        let zoom = [11u8, 13, 15][(seed % 3) as usize];
        // 1 octet : séries de 16 enregistrements, des dizaines de séries par tri ; 4 Kio : quelques séries ;
        // 1 Gio : tout tient en mémoire.
        compare(&src, zoom, &[1, 4096, 1 << 30], &[1, 4]);
        let dir = tempfile::tempdir().unwrap();
        let (mem, s) = in_memory(&mut src.clone(), zoom, dir.path());
        max_cells = max_cells.max(GraphFile::open(std::fs::read(mem).unwrap()).unwrap().header().cells);
        total.edges += s.edges;
        total.loops_cut += s.loops_cut;
        total.ways_cut_by_missing_nodes += s.ways_cut_by_missing_nodes;
        total.relation_way_members_matched += s.relation_way_members_matched;
        total.relation_way_members_not_in_graph += s.relation_way_members_not_in_graph;
        total.duplicate_identities += s.duplicate_identities;
        rejected += s.ways_total - s.ways_kept;
    }
    // Les cas que le tirage doit couvrir sont bien couverts.
    assert!(total.edges > 1_000, "{}", total.edges);
    assert!(total.loops_cut > 0, "boucles");
    assert!(total.ways_cut_by_missing_nodes > 0, "nœuds manquants");
    assert!(total.relation_way_members_matched > 0, "itinéraires appliqués");
    assert!(total.relation_way_members_not_in_graph > 0, "membres hors graphe");
    assert!(total.duplicate_identities > 0, "identités en double");
    assert!(rejected > 0, "ways rejetées");
    assert!(max_cells > 4, "cellules : {max_cells}");
}

/// Un réseau assez gros pour plus de K_MAX séries de références au budget minimal.
fn dense_source() -> MemSource {
    let mut s = MemSource::default();
    let mut r = Rng(42);
    for i in 0..3_000i64 {
        s.node(i + 1, 47.0 + r.below(50_000) as f64 * 1e-5, -1.6 + r.below(50_000) as f64 * 1e-5);
    }
    for w in 0..600i64 {
        let refs: Vec<i64> = (0..8).map(|_| 1 + r.below(3_000) as i64).collect();
        s.way(w + 1, &refs, &[("highway", "residential")]);
    }
    s
}

#[test]
fn multi_level_merge_same_bytes() {
    let s = dense_source();
    let dir = tempfile::tempdir().unwrap();
    let (mem, _) = in_memory(&mut s.clone(), 13, dir.path());
    let (ext, report) = external(&mut s.clone(), 13, 1, dir.path());
    assert_same_graph(&mem, &ext);
    let waynode = report.runs.iter().find(|(n, _)| n == "waynode").unwrap().1;
    assert!(waynode > ord_graph::extsort::K_MAX as u64, "séries de références : {waynode}");
}

#[test]
fn empty_graph_same_bytes() {
    let mut s = MemSource::default();
    s.node(1, 47.0, -1.0).node(2, 47.001, -1.0);
    s.way(10, &[1, 2], &[("building", "yes")]);
    compare(&s, 11, &[1, 1 << 20], &[1]);
}

#[cfg(feature = "pbf")]
#[test]
fn mini_pbf_same_bytes() {
    use ord_graph::pbf::PbfSource;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/mini.osm.pbf");
    let dir = tempfile::tempdir().unwrap();
    let (mem, mem_stats) = in_memory(&mut PbfSource::open(&path).unwrap(), 11, dir.path());
    // Lots par défaut, d'un blob, ou plafonnés en octets (1 : un blob par lot ; 100 : coupure en cours de lot).
    for (len, bytes) in [(None, None), (Some(1), None), (None, Some(1)), (None, Some(100))] {
        let mut src = PbfSource::open(&path).unwrap();
        if let Some(n) = len {
            src = src.with_batch_len(n);
        }
        if let Some(n) = bytes {
            src = src.with_batch_bytes(n);
        }
        let (ext, report) = external(&mut src, 11, 1, dir.path());
        assert_same_graph(&mem, &ext);
        assert_eq!(counters(&mem_stats), counters(&report.stats));
        std::fs::remove_file(&ext).unwrap();
    }
}

// --- Échecs ------------------------------------------------------------------------------------------

/// Source qui échoue pendant la passe des nœuds.
struct Failing(MemSource);

impl OsmSource for Failing {
    fn meta(&self) -> SourceMeta {
        SourceMeta::default()
    }
    fn scan(&mut self, sink: &mut dyn ElementSink) -> Result<(), BuildError> {
        self.0.scan(sink)
    }
    fn nodes(&mut self, _f: &(dyn Fn(i64, i32, i32) + Sync)) -> Result<(), BuildError> {
        Err(BuildError::Source("PBF tronqué".into()))
    }
    fn tagged_nodes(&mut self, _f: &TaggedNodeFn<'_>) -> Result<(), BuildError> {
        Err(BuildError::Source("PBF tronqué".into()))
    }
}

#[test]
fn failure_leaves_neither_output_nor_tmp_dir() {
    let src = random_source(7);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ext.ordg");
    let opts = ExternalOptions { zoom: 11, mem_budget: 1, tmp_parent: dir.path().to_path_buf() };
    let e = build_external(&mut Failing(src), &opts, "test", &path, TOOL, EPOCH, Vec::new(), &mut |_| {})
        .unwrap_err();
    assert!(matches!(e, ExternalError::Build(BuildError::Source(_))), "{e}");
    assert_clean(dir.path(), &[]);
}

#[test]
fn pass_1_error_stops_the_scan() {
    // La troisième way sur mille arrive avec un id déjà vu : la passe 1 échoue, et la source n'en livre pas
    // une de plus.
    let mut s = MemSource::default();
    s.node(1, 47.0, -1.0).node(2, 47.001, -1.0);
    for w in 1..=1000 {
        s.way(w, &[1, 2], &[("highway", "residential")]);
    }
    /// Livre les ways dans l'ordre, la troisième sous l'id 1, et compte celles qu'elle livre.
    struct Unsorted(MemSource, usize);
    impl OsmSource for Unsorted {
        fn meta(&self) -> SourceMeta {
            SourceMeta::default()
        }
        fn scan(&mut self, sink: &mut dyn ElementSink) -> Result<(), BuildError> {
            for (i, (id, refs, _)) in self.0.ways.iter().enumerate() {
                self.1 += 1;
                let id = if i == 2 { 1 } else { *id };
                sink.way(&ord_graph::build::OsmWay { id, refs, tags: &[("highway", "residential")] })?;
            }
            Ok(())
        }
        fn nodes(&mut self, f: &(dyn Fn(i64, i32, i32) + Sync)) -> Result<(), BuildError> {
            self.0.nodes(f)
        }
        fn tagged_nodes(&mut self, f: &TaggedNodeFn<'_>) -> Result<(), BuildError> {
            self.0.tagged_nodes(f)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let opts = ExternalOptions { zoom: 11, mem_budget: 1, tmp_parent: dir.path().to_path_buf() };
    let mut src = Unsorted(s.clone(), 0);
    let e = build_external(
        &mut src,
        &opts,
        "test",
        &dir.path().join("x.ordg"),
        TOOL,
        EPOCH,
        Vec::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(e, ExternalError::Build(BuildError::NotSorted(_))), "{e}");
    assert_eq!(src.1, 3, "ways livrées après l'erreur");
    assert_clean(dir.path(), &[]);
    // Même arrêt pour la construction en mémoire.
    let mut src = Unsorted(s, 0);
    let e = build_graph(&mut src, &BuildOptions { zoom: 11 }, "test", &mut |_| {}).unwrap_err();
    assert!(matches!(e, BuildError::NotSorted(_)), "{e}");
    assert_eq!(src.1, 3);
}

#[test]
fn dead_build_dir_is_removed_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    // Reste d'une construction tuée : dossier et verrou que personne ne tient.
    let stale = dir.path().join(".ordg-build-4242-0-0");
    std::fs::create_dir(&stale).unwrap();
    std::fs::write(stale.join("000000-waynode.run"), vec![0u8; 4096]).unwrap();
    std::fs::write(dir.path().join(".ordg-build-4242-0-0.lock"), b"").unwrap();
    let opts = ExternalOptions { zoom: 11, mem_budget: 1, tmp_parent: dir.path().to_path_buf() };
    let report = build_external(
        &mut random_source(3),
        &opts,
        "test",
        &dir.path().join("x.ordg"),
        TOOL,
        EPOCH,
        Vec::new(),
        &mut |_| {},
    )
    .expect("un reste ne bloque pas la construction suivante");
    assert_eq!(report.stale_tmp_removed, vec![stale]);
    assert_clean(dir.path(), &["x.ordg"]);
}

#[test]
fn concurrent_builds_share_a_tmp_parent() {
    let dir = tempfile::tempdir().unwrap();
    let reference = tempfile::tempdir().unwrap();
    let src = random_source(11);
    let (mem, _) = in_memory(&mut src.clone(), 11, reference.path());
    std::thread::scope(|scope| {
        for i in 0..4 {
            let (src, dir, mem) = (src.clone(), dir.path(), &mem);
            scope.spawn(move || {
                let path = dir.join(format!("ext-{i}.ordg"));
                let opts = ExternalOptions { zoom: 11, mem_budget: 1, tmp_parent: dir.to_path_buf() };
                let report = build_external(
                    &mut src.clone(),
                    &opts,
                    "test",
                    &path,
                    TOOL,
                    EPOCH,
                    Vec::new(),
                    &mut |_| {},
                )
                .expect("construction concurrente");
                assert!(
                    report.stale_tmp_removed.is_empty(),
                    "le dossier d'une construction en cours est gardé"
                );
                assert_same_graph(mem, &path);
            });
        }
    });
    assert_clean(dir.path(), &["ext-0.ordg", "ext-1.ordg", "ext-2.ordg", "ext-3.ordg"]);
}
