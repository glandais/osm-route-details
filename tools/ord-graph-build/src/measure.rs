//! `measure` : tailles, comptes, départs tirés, recherches bornées à froid et à chaud (§5 de la note).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context};
use ord_graph::attrs::{Access, HighwayClass};
use ord_graph::format::GraphFile;
use ord_graph::search::{
    disc_content, DenseState, FlatGraph, FlatSearcher, SearchConfig, SearchResult, Searcher,
};
use ord_graph::stats::{largest_component, page_faults, percentile, proc_status, BuildStats, GraphStats};
use serde::Serialize;

use crate::util::{sample_without_replacement, Poly, SplitMix64};

#[derive(Debug, clap::Args)]
pub struct MeasureArgs {
    #[arg(long)]
    pub graph: PathBuf,
    #[arg(long, default_value_t = 50)]
    pub starts: usize,
    #[arg(long, default_value_t = 20_261_115)]
    pub seed: u64,
    /// Rayons des disques, en km.
    #[arg(long, value_delimiter = ',', default_values_t = [15.0, 25.0, 50.0, 75.0])]
    pub radii: Vec<f64>,
    /// `.poly` Geofabrik de la région : sépare les départs dont le disque est intérieur.
    #[arg(long)]
    pub poly: Option<PathBuf>,
    /// État de la recherche : tableau à estampilles (défaut, référence du plan) ou table de hachage.
    #[arg(long, value_parser = ["map", "dense"], default_value = "dense")]
    pub state: String,
    /// Tours chronométrés à chaud (médiane), après un tour jeté.
    #[arg(long, default_value_t = 3)]
    pub warm_runs: usize,
    /// Mesure aussi, à chaud, la référence à plat (tableaux globaux, état dense) : écart avec le lecteur par cellules.
    #[arg(long)]
    pub flat: bool,
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(Debug, Default, Serialize, Clone)]
struct Run {
    ms: f64,
    settled: u64,
    pushed: u64,
    heap_max: usize,
    state_mb: f64,
    cells_touched: u32,
    bsearch_lookups: u64,
    major_faults: u64,
    rss_file_kb: u64,
    rss_anon_kb: u64,
}

#[derive(Debug, Serialize, Clone)]
struct PerStart {
    node: u32,
    osm_id: u64,
    lat: f64,
    lon: f64,
    interior: bool,
    cold: Run,
    warm: Run,
    /// Référence à plat (avec `--flat`), mêmes états fixés que `warm`.
    flat: Option<Run>,
    disc_edges: u64,
    disc_directed: u64,
    disc_km: f64,
}

#[derive(Debug, Serialize, Default)]
struct Dist {
    n: usize,
    median: f64,
    p90: f64,
    max: f64,
}

fn dist(mut v: Vec<f64>) -> Dist {
    v.sort_by(f64::total_cmp);
    Dist { n: v.len(), median: percentile(&v, 0.5), p90: percentile(&v, 0.9), max: percentile(&v, 1.0) }
}

#[derive(Debug, Serialize, Default)]
struct Summary {
    starts: usize,
    unreliable: bool,
    cold_ms: Dist,
    warm_ms: Dist,
    flat_ms: Option<Dist>,
    reader_over_flat: Option<Dist>,
    bsearch_per_settled: Dist,
    settled_cold: Dist,
    settled_warm: Dist,
    cells_touched: Dist,
    state_mb: Dist,
    heap_max: Dist,
    disc_directed: Dist,
    disc_km: Dist,
    disc_km_per_km2: Dist,
    max_rss_file_mb_cold: f64,
    max_rss_anon_mb_cold: f64,
    max_rss_file_mb_warm: f64,
    max_rss_anon_mb_warm: f64,
    cold_not_guaranteed: usize,
}

fn summarize(rows: &[&PerStart], radius_km: f64) -> Summary {
    let area = std::f64::consts::PI * radius_km * radius_km;
    let col = |f: &dyn Fn(&PerStart) -> f64| dist(rows.iter().map(|r| f(r)).collect());
    let mx = |f: &dyn Fn(&PerStart) -> u64| rows.iter().map(|r| f(r)).max().unwrap_or(0) as f64 / 1024.0;
    Summary {
        starts: rows.len(),
        unreliable: rows.len() < 10,
        cold_ms: col(&|r| r.cold.ms),
        warm_ms: col(&|r| r.warm.ms),
        flat_ms: rows
            .iter()
            .all(|r| r.flat.is_some())
            .then(|| col(&|r| r.flat.as_ref().map_or(0.0, |f| f.ms))),
        reader_over_flat: rows
            .iter()
            .all(|r| r.flat.is_some())
            .then(|| col(&|r| r.flat.as_ref().map_or(0.0, |f| r.warm.ms / f.ms.max(1e-9)))),
        bsearch_per_settled: col(&|r| r.warm.bsearch_lookups as f64 / (r.warm.settled.max(1)) as f64),
        settled_cold: col(&|r| r.cold.settled as f64),
        settled_warm: col(&|r| r.warm.settled as f64),
        cells_touched: col(&|r| f64::from(r.warm.cells_touched)),
        state_mb: col(&|r| r.warm.state_mb),
        heap_max: col(&|r| r.warm.heap_max as f64),
        disc_directed: col(&|r| r.disc_directed as f64),
        disc_km: col(&|r| r.disc_km),
        disc_km_per_km2: col(&|r| r.disc_km / area),
        max_rss_file_mb_cold: mx(&|r| r.cold.rss_file_kb),
        max_rss_anon_mb_cold: mx(&|r| r.cold.rss_anon_kb),
        max_rss_file_mb_warm: mx(&|r| r.warm.rss_file_kb),
        max_rss_anon_mb_warm: mx(&|r| r.warm.rss_anon_kb),
        cold_not_guaranteed: rows.iter().filter(|r| r.cold.major_faults == 0).count(),
    }
}

#[derive(Debug, Serialize)]
struct RadiusReport {
    radius_km: f64,
    all: Summary,
    interior: Summary,
}

#[derive(Debug, Serialize)]
struct Environment {
    cpu: String,
    mem_total_mb: u64,
    rustc: String,
    commit: String,
    state: String,
    seed: u64,
}

#[derive(Debug, Serialize)]
pub struct MeasureReport {
    graph: PathBuf,
    environment: Environment,
    open_ms: f64,
    verify_ms: f64,
    stats_ms: f64,
    graph_stats: GraphStats,
    build_stats: Option<BuildStats>,
    candidate_starts: usize,
    radii: Vec<RadiusReport>,
    starts: BTreeMap<String, Vec<PerStart>>,
}

fn cmd(prog: &str, args: &[&str]) -> String {
    std::process::Command::new(prog)
        .args(args)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn environment(state: &str, seed: u64) -> Environment {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let cpu = cpuinfo
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mem_kb = meminfo
        .lines()
        .find(|l| l.starts_with("MemTotal"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    Environment {
        cpu: format!("{cpu} ({} fils)", std::thread::available_parallelism().map_or(0, |n| n.get())),
        mem_total_mb: mem_kb / 1024,
        rustc: cmd("rustc", &["--version"]),
        commit: cmd("git", &["rev-parse", "--short", "HEAD"]),
        state: state.to_string(),
        seed,
    }
}

fn one_run<B: AsRef<[u8]>>(searcher: &mut Searcher<'_, B>, start: u32, cfg: &SearchConfig) -> Run {
    timed(|| searcher.run(start, cfg))
}

fn timed(f: impl FnOnce() -> SearchResult) -> Run {
    let (_, maj0) = page_faults();
    let t = Instant::now();
    let r = f();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let (_, maj1) = page_faults();
    let st = proc_status();
    Run {
        ms,
        settled: r.stats.settled,
        pushed: r.stats.pushed,
        heap_max: r.stats.heap_max,
        state_mb: r.stats.state_bytes as f64 / 1e6,
        cells_touched: r.stats.cells_touched,
        bsearch_lookups: r.stats.bsearch_lookups,
        major_faults: maj1 - maj0,
        rss_file_kb: st.rss_file_kb,
        rss_anon_kb: st.rss_anon_kb,
    }
}

fn drop_page_cache(path: &std::path::Path) -> anyhow::Result<()> {
    let f = std::fs::File::open(path)?;
    rustix::fs::fadvise(&f, 0, None, rustix::fs::Advice::DontNeed)?;
    Ok(())
}

pub fn run(args: &MeasureArgs) -> anyhow::Result<MeasureReport> {
    let t = Instant::now();
    let g = GraphFile::open_mmap(&args.graph).context("ouverture")?;
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    g.verify().context("verify()")?;
    let verify_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let graph_stats = GraphStats::compute(&g);
    let stats_ms = t.elapsed().as_secs_f64() * 1000.0;
    let build_stats: Option<BuildStats> = g.stats_json().ok().and_then(|b| serde_json::from_slice(b).ok());
    eprintln!(
        "ouverture {open_ms:.2} ms, verify() {verify_ms:.0} ms, comptes {stats_ms:.0} ms ; {} nœuds, {} arêtes, {} cellules",
        graph_stats.nodes, graph_stats.edges, graph_stats.cells
    );

    // Départs : nœuds de la plus grande composante incidents à une `residential` ou `tertiary` d'accès `Yes`.
    let (in_big, _) = largest_component(&g);
    let mut cand = rustc_hash::FxHashSet::default();
    for e in 0..g.header().edges as u32 {
        let a = g.edge(e).attrs;
        if a.access() == Access::Yes
            && matches!(a.class(), Some(HighwayClass::Residential | HighwayClass::Tertiary))
        {
            let (x, y) = g.edge_ends(e);
            for v in [x, y] {
                if in_big[v as usize] {
                    cand.insert(v);
                }
            }
        }
    }
    let mut cand: Vec<u32> = cand.into_iter().collect();
    cand.sort_unstable_by_key(|&v| g.osm_node_id(v));
    if cand.is_empty() {
        bail!("aucun candidat de départ");
    }
    let picked: Vec<u32> = sample_without_replacement(&mut SplitMix64(args.seed), cand.len(), args.starts)
        .into_iter()
        .map(|i| cand[i])
        .collect();
    eprintln!("{} candidats, {} départs tirés (graine {})", cand.len(), picked.len(), args.seed);

    let poly = args.poly.as_ref().map(|p| Poly::read(p)).transpose()?;
    let dense = args.state == "dense";
    let n_edges = g.header().edges;
    let n_candidates = cand.len();
    // Positions des départs, relevées avant de fermer la vue : pendant le froid, aucune autre projection du
    // fichier ne doit rester ouverte, sinon `DONTNEED` ne chasse pas ses pages et le froid n'est pas froid.
    let infos: Vec<(u32, u64, f64, f64)> = picked
        .iter()
        .map(|&s| {
            let (lat, lon) = g.node_pos(s);
            (s, g.osm_node_id(s), f64::from(lat) * 1e-7, f64::from(lon) * 1e-7)
        })
        .collect();
    drop(g);

    // Phase froide, tous rayons : une vue neuve par requête, pages du fichier chassées juste avant.
    let mut cold_rows: Vec<Vec<PerStart>> = Vec::new();
    for &radius_km in &args.radii {
        let cfg =
            SearchConfig { radius_m: radius_km * 1000.0, forbid_uturn: true, dense, collect_nodes: false };
        let mut rows = Vec::new();
        // Le tableau dense est alloué et rempli une fois, hors de la zone chronométrée : un serveur le ferait au
        // démarrage ; il ne retient aucune page du fichier, le froid reste froid.
        let mut state = if dense { DenseState::new(n_edges) } else { DenseState::default() };
        for &(node, osm_id, lat, lon) in &infos {
            drop_page_cache(&args.graph)?;
            let cold = {
                let cold_g = GraphFile::open_mmap(&args.graph)?;
                let mut cold_search = Searcher::with_state(&cold_g, state);
                let run = one_run(&mut cold_search, node, &cfg);
                state = cold_search.into_state();
                run
            };
            let interior = poly.as_ref().is_some_and(|p| p.contains_disc(lat, lon, cfg.radius_m));
            rows.push(PerStart {
                node,
                osm_id,
                lat,
                lon,
                interior,
                cold,
                warm: Run::default(),
                flat: None,
                disc_edges: 0,
                disc_directed: 0,
                disc_km: 0.0,
            });
        }
        cold_rows.push(rows);
    }

    // Phase chaude : une vue persistante, un tour jeté puis `warm_runs` tours, médiane ; disques statiques.
    let g = GraphFile::open_mmap(&args.graph)?;
    let flat = args.flat.then(|| FlatGraph::from_graph(&g));
    let mut radii_reports = Vec::new();
    let mut per_radius: BTreeMap<String, Vec<PerStart>> = BTreeMap::new();
    for (&radius_km, mut rows) in args.radii.iter().zip(cold_rows) {
        let cfg =
            SearchConfig { radius_m: radius_km * 1000.0, forbid_uturn: true, dense, collect_nodes: false };
        let mut searcher = Searcher::new(&g);
        let mut flat_searcher = flat.as_ref().map(FlatSearcher::new);
        for row in &mut rows {
            let _ = one_run(&mut searcher, row.node, &cfg); // tour jeté
            let mut runs: Vec<Run> =
                (0..args.warm_runs).map(|_| one_run(&mut searcher, row.node, &cfg)).collect();
            runs.sort_by(|a, b| a.ms.total_cmp(&b.ms));
            let mut med = runs[runs.len() / 2].clone();
            med.rss_file_kb = runs.iter().map(|r| r.rss_file_kb).max().unwrap_or(0);
            med.rss_anon_kb = runs.iter().map(|r| r.rss_anon_kb).max().unwrap_or(0);
            row.warm = med;
            if let Some(fs) = flat_searcher.as_mut() {
                // Référence à plat : état dense, comme le lecteur en `--state dense` ; mêmes états fixés exigés.
                let flat_cfg = SearchConfig { dense: true, ..cfg };
                let _ = timed(|| fs.run(row.node, &flat_cfg));
                let mut runs: Vec<Run> =
                    (0..args.warm_runs).map(|_| timed(|| fs.run(row.node, &flat_cfg))).collect();
                runs.sort_by(|a, b| a.ms.total_cmp(&b.ms));
                let f = runs[runs.len() / 2].clone();
                if f.settled != row.warm.settled {
                    bail!("états fixés différents : plat {} contre cellules {}", f.settled, row.warm.settled);
                }
                row.flat = Some(f);
            }
            let dc = disc_content(&g, row.node, cfg.radius_m);
            (row.disc_edges, row.disc_directed, row.disc_km) = (dc.edges, dc.directed, dc.km);
        }
        let all: Vec<&PerStart> = rows.iter().collect();
        let inner: Vec<&PerStart> = rows.iter().filter(|r| r.interior).collect();
        let rep = RadiusReport {
            radius_km,
            all: summarize(&all, radius_km),
            interior: summarize(&inner, radius_km),
        };
        print_radius(&rep);
        radii_reports.push(rep);
        per_radius.insert(format!("{radius_km}"), rows);
    }

    let report = MeasureReport {
        graph: args.graph.clone(),
        environment: environment(&args.state, args.seed),
        open_ms,
        verify_ms,
        stats_ms,
        graph_stats,
        build_stats,
        candidate_starts: n_candidates,
        radii: radii_reports,
        starts: per_radius,
    };
    print_sizes(&report.graph_stats);
    if let Some(out) = &args.out {
        std::fs::write(out, serde_json::to_vec_pretty(&report)?)?;
    }
    Ok(report)
}

fn print_sizes(s: &GraphStats) {
    println!("\n== Tailles et comptes ==");
    println!(
        "fichier {:.1} Mo ; nœuds {} ; arêtes {} ; orientées {} ; cellules {}",
        s.file_bytes as f64 / 1e6,
        s.nodes,
        s.edges,
        s.directed,
        s.cells
    );
    println!(
        "octets par nœud {:.1} ; par arête {:.1} ; par orientée {:.1} ; par km {:.0} ; topologie seule par arête {:.1}",
        s.bytes_per_node, s.bytes_per_edge, s.bytes_per_directed, s.bytes_per_km, s.topology_bytes_per_edge
    );
    println!("sections : {:?}", s.sections);
    println!(
        "km {:.0} ; sens uniques {} ; contresens {} ; fermées {} ; arêtes de bord {} ; attrs distincts {} ; plus grande composante {:.1} %",
        s.km_total, s.oneway_edges, s.contraflow_edges, s.closed_edges, s.border_edges, s.distinct_attrs, s.largest_component_node_share * 100.0
    );
    println!(
        "degrés : 1 {} ; 2 {} (dont attributs identiques {}) ; ≥3 {} ; arêtes/cellule p50 {} p90 {} p99 {} max {}",
        s.nodes_degree_1, s.nodes_degree_2, s.nodes_degree_2_same_attrs, s.nodes_degree_3_plus,
        s.edges_per_cell.p50, s.edges_per_cell.p90, s.edges_per_cell.p99, s.edges_per_cell.max
    );
}

fn print_radius(r: &RadiusReport) {
    for (label, s) in [("tous", &r.all), ("intérieurs", &r.interior)] {
        println!(
            "rayon {:>3} km, {label:>10} ({:>2}{}) : froid {:>7.1}/{:>7.1}/{:>7.1} ms ; chaud {:>7.1}/{:>7.1}/{:>7.1} ms ; fixées {:>8.0}/{:>8.0} ; cellules {:>3.0}/{:>3.0} ; RssFile froid max {:.0} Mo",
            r.radius_km, s.starts, if s.unreliable { ", peu fiable" } else { "" },
            s.cold_ms.median, s.cold_ms.p90, s.cold_ms.max, s.warm_ms.median, s.warm_ms.p90, s.warm_ms.max,
            s.settled_warm.median, s.settled_warm.p90, s.cells_touched.median, s.cells_touched.p90, s.max_rss_file_mb_cold
        );
        if let (Some(f), Some(ratio)) = (&s.flat_ms, &s.reader_over_flat) {
            println!(
                "    référence à plat : chaud {:>7.1}/{:>7.1}/{:>7.1} ms ; lecteur / plat {:.2} (médiane des rapports) ; dichotomies par état fixé {:.3}",
                f.median, f.p90, f.max, ratio.median, s.bsearch_per_settled.median
            );
        }
    }
}
