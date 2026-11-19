//! Vues hors mémoire (`docs/design/technique/ord-graph-views-hors-memoire.md` §7) : mêmes octets que
//! `build_view` + `write_graph`, quel que soit le budget (donc le découpage en séries des tris).

#![cfg(feature = "external")]

use std::path::Path;

use ord_graph::build::{build_graph, BuildOptions, MemSource};
use ord_graph::format::{write_graph, GraphFile};
use ord_graph::view::{build_view, check_view_structure, ViewSpec};
use ord_graph::view_external::{build_view_external, ViewExternalOptions};

const TOOL: &str = "test";
const EPOCH: i64 = 1_700_000_000;
const SPECS: [&str; 3] = ["road", "gravel", "all"];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
}

/// Étiquettes des ways : classes routières et chemins, sens uniques dans les deux sens, accès privé, surfaces non
/// revêtues, voie verte : de quoi différencier `road`, `gravel` et `all`, et casser des fusions par les sens.
const TAGS: &[&[(&str, &str)]] = &[
    &[("highway", "residential")],
    &[("highway", "residential")],
    &[("highway", "tertiary"), ("oneway", "yes")],
    &[("highway", "unclassified"), ("oneway", "-1")],
    &[("highway", "track"), ("surface", "gravel")],
    &[("highway", "track"), ("tracktype", "grade1")],
    &[("highway", "path"), ("bicycle", "designated")],
    &[("highway", "footway")],
    &[("highway", "service"), ("access", "private")],
    &[("highway", "cycleway"), ("surface", "asphalt")],
    &[("highway", "residential"), ("maxspeed", "30")],
];

/// Réseau riche en chaînes : une grille d'ancres reliées par des ways à nœuds intermédiaires (parfois deux ways
/// entre les mêmes ancres), des raquettes (way fermée sur une ancre, de longueur paire ou impaire) et des
/// anneaux isolés (tous leurs nœuds fondus).
fn chain_source(seed: u64) -> MemSource {
    grid_source(seed, None)
}

/// [`chain_source`] sur une grille de `k × k` ancres (défaut : 2 à 8, au hasard).
fn grid_source(seed: u64, k: Option<i64>) -> MemSource {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut s = MemSource::default();
    let k = k.unwrap_or_else(|| 2 + r.below(7) as i64);
    let mut next_id = 1_000_000i64;
    let mut fresh = |s: &mut MemSource, lat: f64, lon: f64| {
        next_id += 1;
        s.node(next_id, lat, lon);
        next_id
    };
    let grid = |i: i64, j: i64| 1 + i * 100 + j;
    let pos = |i: i64, j: i64| (47.0 + i as f64 * 0.01, -1.6 + j as f64 * 0.01);
    for i in 0..k {
        for j in 0..k {
            let (la, lo) = pos(i, j);
            s.node(grid(i, j), la, lo);
        }
    }
    let mut wid = 0i64;
    let mut way = |s: &mut MemSource, refs: &[i64], r: &mut Rng| {
        wid += 1;
        s.way(wid, refs, r.pick(TAGS));
    };
    for i in 0..k {
        for j in 0..k {
            for (di, dj) in [(0, 1), (1, 0)] {
                let (i2, j2) = (i + di, j + dj);
                if i2 >= k || j2 >= k || r.below(5) == 0 {
                    continue;
                }
                for _ in 0..1 + u64::from(r.below(4) == 0) {
                    let (la, lo) = pos(i, j);
                    let (la2, lo2) = pos(i2, j2);
                    let n = r.below(7) as i64;
                    let mut refs = vec![grid(i, j)];
                    for t in 1..=n {
                        let f = t as f64 / (n + 1) as f64;
                        let jitter = (r.below(100) as f64 - 50.0) * 1e-5;
                        refs.push(fresh(&mut s, la + (la2 - la) * f + jitter, lo + (lo2 - lo) * f - jitter));
                    }
                    refs.push(grid(i2, j2));
                    // Parfois coupée en deux ways à un nœud intermédiaire (une couture à fondre ou non).
                    if refs.len() > 3 && r.below(3) == 0 {
                        let m = refs.len() / 2;
                        way(&mut s, &refs[..=m], &mut r);
                        way(&mut s, &refs[m..], &mut r);
                    } else {
                        way(&mut s, &refs, &mut r);
                    }
                }
            }
            // Raquette : boucle de 2 à 8 nœuds fermée sur l'ancre.
            if r.below(4) == 0 {
                let (la, lo) = pos(i, j);
                let n = 2 + r.below(7) as i64;
                let mut refs = vec![grid(i, j)];
                for t in 0..n {
                    let a = t as f64 / n as f64 * std::f64::consts::TAU;
                    refs.push(fresh(&mut s, la + 0.002 + 0.001 * a.cos(), lo + 0.001 * a.sin()));
                }
                refs.push(grid(i, j));
                way(&mut s, &refs, &mut r);
            }
        }
    }
    // Anneaux isolés, de 3 à 10 nœuds.
    for _ in 0..r.below(4) {
        let n = 3 + r.below(8) as i64;
        let (la, lo) = (46.9 + r.below(100) as f64 * 1e-4, -1.7 + r.below(100) as f64 * 1e-4);
        let first = fresh(&mut s, la, lo);
        let mut refs = vec![first];
        for t in 1..n {
            let a = t as f64 / n as f64 * std::f64::consts::TAU;
            refs.push(fresh(&mut s, la + 0.001 * a.cos() - 0.001, lo + 0.001 * a.sin()));
        }
        refs.push(first);
        way(&mut s, &refs, &mut r);
    }
    s
}

/// Le graphe de base, écrit puis relu.
fn base(src: &mut MemSource, zoom: u8, dir: &Path) -> GraphFile<Vec<u8>> {
    let mut data = build_graph(src, &BuildOptions { zoom }, "test", &mut |_| {}).expect("construction");
    let path = dir.join("base.ordg");
    write_graph(&path, &mut data, TOOL, EPOCH).expect("écriture");
    GraphFile::open(std::fs::read(path).unwrap()).unwrap()
}

/// Ce que les vues ont traversé : boucles coupées, anneaux, nœuds fondus, et le plus de séries d'un tri.
#[derive(Default)]
struct Seen {
    loops: u64,
    rings: u64,
    merged: u64,
    max_runs: u64,
}

/// Pour chaque vue et chaque budget : la vue hors mémoire a les octets de la vue en mémoire, et passe le contrôle.
fn assert_same_views(g: &GraphFile<Vec<u8>>, budgets: &[usize], dir: &Path) -> Seen {
    let mut seen = Seen::default();
    for name in SPECS {
        let spec = ViewSpec::parse(name).unwrap();
        let mem = dir.join(format!("mem.{name}.ordv"));
        let mut data = build_view(g, &spec).expect("vue en mémoire");
        write_graph(&mem, &mut data, TOOL, EPOCH).expect("écriture de la vue");
        let want = std::fs::read(&mem).unwrap();
        for &budget in budgets {
            let ext = dir.join(format!("ext.{name}.ordv"));
            let opts = ViewExternalOptions { mem_budget: budget, tmp_parent: dir.to_path_buf() };
            let report = build_view_external(g, &spec, &opts, &ext, TOOL, EPOCH).expect("vue hors mémoire");
            let got = std::fs::read(&ext).unwrap();
            assert!(got == want, "vue {name}, budget {budget} : octets différents");
            let v = GraphFile::open(got).unwrap();
            check_view_structure(g, &v).unwrap_or_else(|e| panic!("vue {name} : {e}"));
            assert_eq!(report.meta.view_edges, v.header().edges);
            seen.loops += report.meta.loops_broken;
            seen.rings += report.meta.rings;
            seen.merged += report.meta.merged_nodes;
            seen.max_runs = seen.max_runs.max(report.runs.iter().map(|r| r.1).max().unwrap_or(0));
            std::fs::remove_file(&ext).unwrap();
        }
        std::fs::remove_file(&mem).unwrap();
    }
    seen
}

/// Le dossier ne garde que le graphe de base : ni `.tmp`, ni dossier temporaire, ni verrou.
fn assert_clean(dir: &Path) {
    let names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["base.ordg".to_string()], "contenu du dossier");
}

#[test]
fn chain_rich_views_same_bytes_whatever_budget() {
    let mut all = Seen::default();
    for seed in 0..40 {
        let dir = tempfile::tempdir().unwrap();
        let g = base(&mut chain_source(seed), 14, dir.path());
        // 1 octet : une série par enregistrement, et des fusions à plusieurs niveaux.
        let s = assert_same_views(&g, &[1, 4096, 64 << 20], dir.path());
        assert_clean(dir.path());
        (all.loops, all.rings, all.merged) =
            (all.loops + s.loops, all.rings + s.rings, all.merged + s.merged);
        all.max_runs = all.max_runs.max(s.max_runs);
    }
    // Les cas qui départagent la marche ont bien eu lieu.
    assert!(
        all.loops > 20 && all.rings > 10 && all.merged > 1000,
        "{} {} {}",
        all.loops,
        all.rings,
        all.merged
    );
}

#[test]
fn multi_level_merge_views_same_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let g = base(&mut grid_source(7, Some(30)), 14, dir.path());
    let s = assert_same_views(&g, &[1], dir.path());
    assert!(s.max_runs > 64, "aucune fusion à plusieurs niveaux ({} séries au plus)", s.max_runs);
    assert_clean(dir.path());
}

#[test]
fn views_of_an_empty_graph_same_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let g = base(&mut MemSource::default(), 11, dir.path());
    assert_same_views(&g, &[1, 64 << 20], dir.path());
}

#[test]
fn pruned_views_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let g = base(&mut chain_source(1), 14, dir.path());
    let opts = ViewExternalOptions { mem_budget: 1 << 20, tmp_parent: dir.path().to_path_buf() };
    let err = build_view_external(
        &g,
        &ViewSpec::parse("road-prune").unwrap(),
        &opts,
        &dir.path().join("v"),
        TOOL,
        EPOCH,
    )
    .unwrap_err();
    assert!(err.to_string().contains("-prune"), "{err}");
    assert_clean(dir.path());
}
