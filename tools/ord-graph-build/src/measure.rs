//! `measure` : tailles, comptes, départs tirés, recherches bornées à froid et à chaud (§5 de la note), pour le
//! graphe de base et, avec `--views`, pour chaque vue (lot 0 bis, §11) ; `--check` contrôle les vues contre le
//! graphe de base filtré.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context};
use ord_graph::attrs::{Access, HighwayClass};
use ord_graph::format::GraphFile;
use ord_graph::mmap::MappedGraph;
use ord_graph::search::{
    disc_content, FlatGraph, FlatSearcher, SearchConfig, SearchResult, SearchState, Searcher, StateKind,
};
use ord_graph::stats::{largest_component, page_faults, percentile, proc_status, BuildStats, GraphStats};
use ord_graph::view::{
    alive_edges, check_view_structure, filtered_reference_distances, BaseEdges, Incidence, Snapper, ViewKind,
    ViewMeta, ViewSpec, ViewStructure,
};
use serde::Serialize;

use crate::util::{sample_without_replacement, Poly, SplitMix64};
use crate::views_cmd::view_path;

#[derive(Debug, clap::Args)]
pub struct MeasureArgs {
    /// Graphe de base (`.ordg`).
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
    /// États de la recherche, séparés par des virgules : `dense` (tableau sur tout le graphe, référence du plan),
    /// `map` (table de hachage) ou `local` (tableau dense local au disque). Tout est refait par état.
    #[arg(long, value_delimiter = ',', default_values_t = ["dense".to_string()])]
    pub state: Vec<String>,
    /// Vues à mesurer en plus du graphe de base (`road`, `gravel`, `all`, suffixe `-prune`), construites par
    /// `views` ; leurs fichiers sont cherchés à côté du graphe, ou dans `--view-dir`.
    #[arg(long, value_delimiter = ',')]
    pub views: Vec<String>,
    #[arg(long)]
    pub view_dir: Option<PathBuf>,
    /// Tours chronométrés à chaud (médiane), après un tour jeté.
    #[arg(long, default_value_t = 3)]
    pub warm_runs: usize,
    /// Mesure aussi, à chaud, la référence à plat (tableaux globaux, état dense) : écart avec le lecteur par cellules.
    #[arg(long)]
    pub flat: bool,
    /// Contrôle de correction : chaque vue contre le graphe de base filtré (chaînes, poids, distances de tous ses
    /// nœuds depuis des départs tirés) et les trois états entre eux. Échoue au premier désaccord.
    #[arg(long)]
    pub check: bool,
    /// Départs du contrôle (les premiers départs tirés, rattachés à la vue).
    #[arg(long, default_value_t = 5)]
    pub check_starts: usize,
    /// N'exécute pas les mesures de temps (tailles, comptes et contrôle seulement).
    #[arg(long)]
    pub skip_timing: bool,
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
    /// Nœud de départ dans ce graphe (pour une vue : nœud rattaché).
    node: u32,
    /// Départ tiré dans le graphe de base.
    base_node: u32,
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
    /// Mémoire de l'état, en Mo (dense : tout le graphe ; local : pages du disque ; map : capacité).
    state_mb: Dist,
    heap_max: Dist,
    /// Arêtes dont les deux extrémités sont dans le disque ; orientées ; km.
    disc_edges: Dist,
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
    let with_flat = !rows.is_empty() && rows.iter().all(|r| r.flat.is_some());
    Summary {
        starts: rows.len(),
        unreliable: rows.len() < 10,
        cold_ms: col(&|r| r.cold.ms),
        warm_ms: col(&|r| r.warm.ms),
        flat_ms: with_flat.then(|| col(&|r| r.flat.as_ref().map_or(0.0, |f| f.ms))),
        reader_over_flat: with_flat
            .then(|| col(&|r| r.flat.as_ref().map_or(0.0, |f| r.warm.ms / f.ms.max(1e-9)))),
        bsearch_per_settled: col(&|r| r.warm.bsearch_lookups as f64 / (r.warm.settled.max(1)) as f64),
        settled_cold: col(&|r| r.cold.settled as f64),
        settled_warm: col(&|r| r.warm.settled as f64),
        cells_touched: col(&|r| f64::from(r.warm.cells_touched)),
        state_mb: col(&|r| r.warm.state_mb),
        heap_max: col(&|r| r.warm.heap_max as f64),
        disc_edges: col(&|r| r.disc_edges as f64),
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
struct StateReport {
    state: String,
    radii: Vec<RadiusReport>,
    starts: BTreeMap<String, Vec<PerStart>>,
}

#[derive(Debug, Serialize)]
struct TargetReport {
    /// `base`, ou le nom de la vue.
    name: String,
    file: PathBuf,
    open_ms: f64,
    verify_ms: f64,
    stats_ms: f64,
    graph_stats: GraphStats,
    view_meta: Option<ViewMeta>,
    /// Départs utilisables dans ce graphe : tous pour la base, les rattachés pour une vue.
    available_starts: usize,
    /// Départs rattachés à un autre nœud que le leur (nœud fondu) ; médiane et maximum de l'écart, en m.
    snapped_starts: usize,
    snap_median_m: f64,
    snap_max_m: f64,
    states: Vec<StateReport>,
}

#[derive(Debug, Serialize)]
struct Environment {
    cpu: String,
    mem_total_mb: u64,
    rustc: String,
    commit: String,
    states: Vec<String>,
    seed: u64,
}

#[derive(Debug, Serialize)]
struct ViewCheck {
    view: String,
    structure: ViewStructure,
    starts_compared: usize,
    nodes_compared: u64,
    nodes_reached: u64,
    ms: f64,
}

#[derive(Debug, Serialize)]
struct CheckReport {
    ok: bool,
    views: Vec<ViewCheck>,
    /// Graphes sur lesquels les trois états rendent les mêmes distances et les mêmes comptes (25 km, demi-tour interdit).
    states_agree_on: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MeasureReport {
    graph: PathBuf,
    environment: Environment,
    build_stats: Option<BuildStats>,
    candidate_starts: usize,
    targets: Vec<TargetReport>,
    check: Option<CheckReport>,
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

fn environment(states: &[StateKind], seed: u64) -> Environment {
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
        states: states.iter().map(|s| s.name().to_string()).collect(),
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

fn drop_page_cache(path: &Path) -> anyhow::Result<()> {
    let f = std::fs::File::open(path)?;
    rustix::fs::fadvise(&f, 0, None, rustix::fs::Advice::DontNeed)?;
    Ok(())
}

/// Départ tiré dans le graphe de base : nœud, id OSM, position en degrés.
#[derive(Clone, Copy, Debug)]
struct Start {
    base_node: u32,
    osm_id: u64,
    lat: f64,
    lon: f64,
}

/// Distance équirectangulaire en mètres.
fn dist_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_008.8;
    let dy = (lat2 - lat1).to_radians() * R;
    let dx = (lon2 - lon1).to_radians() * lat1.to_radians().cos() * R;
    dx.hypot(dy)
}

/// Mesure un graphe (base ou vue) pour un état : phase froide (une vue neuve par requête, pages chassées) pour
/// tous les rayons, puis phase chaude.
fn measure_state(
    args: &MeasureArgs,
    path: &Path,
    kind: StateKind,
    starts: &[Start],
    nodes: &[Option<u32>],
    poly: Option<&Poly>,
) -> anyhow::Result<StateReport> {
    let n_edges = GraphFile::open_mmap(path)?.header().edges;
    let avail: Vec<(Start, u32)> = starts.iter().zip(nodes).filter_map(|(s, n)| n.map(|n| (*s, n))).collect();

    // Phase froide, tous rayons : le tableau dense est alloué une fois, hors de la zone chronométrée (un serveur
    // le ferait au démarrage) ; l'état local est dimensionné par un passage à vide, pour la même raison.
    let mut cold_rows: Vec<Vec<PerStart>> = Vec::new();
    for &radius_km in &args.radii {
        let cfg = SearchConfig {
            radius_m: radius_km * 1000.0,
            forbid_uturn: true,
            state: kind,
            collect_nodes: false,
        };
        let mut state = SearchState::new(kind, n_edges);
        if kind == StateKind::Local {
            let g = GraphFile::open_mmap(path)?;
            let mut s = Searcher::with_state(&g, state);
            for &(_, node) in &avail {
                s.run(node, &cfg);
            }
            state = s.into_state();
        }
        let mut rows = Vec::new();
        for &(st, node) in &avail {
            drop_page_cache(path)?;
            let cold = {
                let cold_g = GraphFile::open_mmap(path)?;
                let mut cold_search = Searcher::with_state(&cold_g, state);
                let run = one_run(&mut cold_search, node, &cfg);
                state = cold_search.into_state();
                run
            };
            let interior = poly.is_some_and(|p| p.contains_disc(st.lat, st.lon, cfg.radius_m));
            rows.push(PerStart {
                node,
                base_node: st.base_node,
                osm_id: st.osm_id,
                lat: st.lat,
                lon: st.lon,
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
    let g = GraphFile::open_mmap(path)?;
    let flat = (args.flat && kind == StateKind::Dense).then(|| FlatGraph::from_graph(&g));
    let mut radii_reports = Vec::new();
    let mut per_radius: BTreeMap<String, Vec<PerStart>> = BTreeMap::new();
    for (&radius_km, mut rows) in args.radii.iter().zip(cold_rows) {
        let cfg = SearchConfig {
            radius_m: radius_km * 1000.0,
            forbid_uturn: true,
            state: kind,
            collect_nodes: false,
        };
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
                // Référence à plat : état dense ; mêmes états fixés exigés.
                let flat_cfg = SearchConfig { state: StateKind::Dense, ..cfg };
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
        radii_reports.push(RadiusReport {
            radius_km,
            all: summarize(&all, radius_km),
            interior: summarize(&inner, radius_km),
        });
        per_radius.insert(format!("{radius_km}"), rows);
    }
    Ok(StateReport { state: kind.name().to_string(), radii: radii_reports, starts: per_radius })
}

/// Une vue ouverte, avec ses départs rattachés.
struct LoadedView {
    spec: ViewSpec,
    path: PathBuf,
    graph: MappedGraph,
    /// Pour chaque départ tiré : nœud de la vue (id de vue), s'il y en a un.
    nodes: Vec<Option<u32>>,
    open_ms: f64,
    verify_ms: f64,
}

/// Les trois états rendent les mêmes distances et les mêmes comptes sur `g` depuis `starts`.
fn states_agree(g: &MappedGraph, label: &str, starts: &[u32]) -> anyhow::Result<()> {
    for &s in starts {
        let mut results = Vec::new();
        for kind in StateKind::ALL {
            let cfg =
                SearchConfig { radius_m: 25_000.0, forbid_uturn: true, state: kind, collect_nodes: true };
            results.push((kind, Searcher::new(g).run(s, &cfg)));
        }
        let (k0, r0) = &results[0];
        for (kind, r) in &results[1..] {
            if r.node_dist != r0.node_dist
                || (r.stats.settled, r.stats.pushed) != (r0.stats.settled, r0.stats.pushed)
            {
                bail!(
                    "CONTRÔLE ÉCHOUÉ : {label}, départ {s} : l'état {} diffère de {} (nœuds {} contre {}, fixées {} contre {})",
                    kind.name(),
                    k0.name(),
                    r.node_dist.len(),
                    r0.node_dist.len(),
                    r.stats.settled,
                    r0.stats.settled
                );
            }
        }
    }
    Ok(())
}

/// Contrôle des vues contre le graphe de base filtré, et des trois états entre eux.
fn run_checks(
    args: &MeasureArgs,
    base: &MappedGraph,
    base_starts: &[u32],
    views: &[LoadedView],
) -> anyhow::Result<CheckReport> {
    let be = BaseEdges::load(base);
    let mut checks = Vec::new();
    let take: Vec<u32> = base_starts.iter().copied().take(args.check_starts).collect();
    states_agree(base, "base", &take)?;
    let mut agree = vec!["base".to_string()];
    for v in views {
        let t = Instant::now();
        let name = v.spec.name();
        let structure = check_view_structure(base, &v.graph)
            .map_err(|e| anyhow::anyhow!("CONTRÔLE ÉCHOUÉ : vue {name} : {e}"))?;
        let (alive, _) = alive_edges(&be, &v.spec);
        let inc = Incidence::new(&be, &alive);
        let vnodes = v.graph.view_base_nodes().context("nœuds de base de la vue")?;
        let starts: Vec<u32> = v.nodes.iter().flatten().copied().take(args.check_starts).collect();
        let (mut compared, mut reached) = (0u64, 0u64);
        for &s in &starts {
            let reference = filtered_reference_distances(&be, &inc, vnodes[s as usize].get());
            // Sans disque ni interdiction de demi-tour : la même question que le Dijkstra par nœuds de référence.
            let cfg = SearchConfig {
                radius_m: 1e9,
                forbid_uturn: false,
                state: StateKind::Dense,
                collect_nodes: true,
            };
            let got: HashMap<u32, u32> = Searcher::new(&v.graph).run(s, &cfg).node_dist.into_iter().collect();
            let mut bad = Vec::new();
            for (vn, bn) in vnodes.iter().enumerate() {
                let want = reference[bn.get() as usize];
                let have = got.get(&(vn as u32)).copied().unwrap_or(u32::MAX);
                if want != have {
                    bad.push((vn, want, have));
                }
                compared += 1;
                reached += u64::from(want != u32::MAX);
            }
            if !bad.is_empty() {
                bail!(
                    "CONTRÔLE ÉCHOUÉ : vue {name}, départ {s} : {} nœuds sur {} diffèrent de la base filtrée (nœud, base, vue) : {:?}",
                    bad.len(),
                    vnodes.len(),
                    &bad[..bad.len().min(5)]
                );
            }
        }
        states_agree(&v.graph, &name, &starts)?;
        agree.push(name.clone());
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "contrôle {name:<13} : chaînes, longueurs, poids et sens exacts sur {} arêtes ; {} départs, {compared} distances de nœuds égales à la base filtrée ({reached} atteints) ; trois états d'accord ; {ms:.0} ms",
            structure.view_edges,
            starts.len(),
        );
        checks.push(ViewCheck {
            view: name,
            structure,
            starts_compared: starts.len(),
            nodes_compared: compared,
            nodes_reached: reached,
            ms,
        });
    }
    Ok(CheckReport { ok: true, views: checks, states_agree_on: agree })
}

pub fn run(args: &MeasureArgs) -> anyhow::Result<MeasureReport> {
    let states: Vec<StateKind> = args
        .state
        .iter()
        .map(|s| StateKind::parse(s).with_context(|| format!("état inconnu : {s} (map, dense, local)")))
        .collect::<Result<_, _>>()?;
    if states.is_empty() {
        bail!("aucun état demandé");
    }
    let specs: Vec<ViewSpec> = args
        .views
        .iter()
        .map(|v| ViewSpec::parse(v).with_context(|| format!("vue inconnue : {v}")))
        .collect::<Result<_, _>>()?;

    let t = Instant::now();
    let g = GraphFile::open_mmap(&args.graph).context("ouverture")?;
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    g.verify().context("verify()")?;
    let verify_ms = t.elapsed().as_secs_f64() * 1000.0;
    if g.is_view() {
        bail!("--graph doit être le graphe de base, pas une vue");
    }
    let t = Instant::now();
    let graph_stats = GraphStats::compute(&g);
    let stats_ms = t.elapsed().as_secs_f64() * 1000.0;
    let build_stats: Option<BuildStats> = g.stats_json().ok().and_then(|b| serde_json::from_slice(b).ok());
    eprintln!(
        "base : ouverture {open_ms:.2} ms, verify() {verify_ms:.0} ms, comptes {stats_ms:.0} ms ; {} nœuds, {} arêtes, {} cellules",
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
    let n_candidates = cand.len();
    // Positions des départs, relevées avant de fermer la vue : pendant le froid, aucune autre projection du
    // fichier ne doit rester ouverte, sinon `DONTNEED` ne chasse pas ses pages et le froid n'est pas froid.
    let starts: Vec<Start> = picked
        .iter()
        .map(|&s| {
            let (lat, lon) = g.node_pos(s);
            Start {
                base_node: s,
                osm_id: g.osm_node_id(s),
                lat: f64::from(lat) * 1e-7,
                lon: f64::from(lon) * 1e-7,
            }
        })
        .collect();

    // Vues : ouverture, vérification de la base, rattachement des départs.
    let mut snappers: HashMap<ViewKind, Snapper> = HashMap::new();
    let mut loaded: Vec<LoadedView> = Vec::new();
    for spec in &specs {
        let path = view_path(&args.graph, &spec.name(), args.view_dir.as_deref());
        let t = Instant::now();
        let vg = GraphFile::open_mmap(&path)
            .with_context(|| format!("vue {} ({})", spec.name(), path.display()))?;
        let vopen_ms = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        vg.verify().with_context(|| format!("verify() de la vue {}", spec.name()))?;
        let vverify_ms = t.elapsed().as_secs_f64() * 1000.0;
        let meta = vg.view_meta().with_context(|| format!("{} n'est pas une vue", path.display()))?;
        let h = g.header();
        if (meta.base_nodes, meta.base_edges, meta.base_pbf_xxh3, meta.base_build_epoch)
            != (h.nodes, h.edges, h.pbf_xxh3, h.build_epoch)
        {
            bail!("la vue {} a été construite pour un autre graphe de base : relancer `views`", spec.name());
        }
        let snapper = snappers.entry(spec.kind).or_insert_with(|| Snapper::new(&g, spec.kind));
        let vnodes = vg.view_base_nodes().context("nœuds de base de la vue")?;
        let nodes: Vec<Option<u32>> = starts.iter().map(|s| snapper.snap(vnodes, s.base_node)).collect();
        loaded.push(LoadedView {
            spec: *spec,
            path,
            graph: vg,
            nodes,
            open_ms: vopen_ms,
            verify_ms: vverify_ms,
        });
    }

    let check = if args.check {
        let base_starts: Vec<u32> = starts.iter().map(|s| s.base_node).collect();
        Some(run_checks(args, &g, &base_starts, &loaded)?)
    } else {
        None
    };

    // Rapports de taille de chaque graphe, avant de tout fermer.
    let poly = args.poly.as_ref().map(|p| Poly::read(p)).transpose()?;
    let mut targets = vec![TargetReport {
        name: "base".into(),
        file: args.graph.clone(),
        open_ms,
        verify_ms,
        stats_ms,
        graph_stats,
        view_meta: None,
        available_starts: starts.len(),
        snapped_starts: 0,
        snap_median_m: 0.0,
        snap_max_m: 0.0,
        states: Vec::new(),
    }];
    let mut target_nodes: Vec<Vec<Option<u32>>> = vec![starts.iter().map(|s| Some(s.base_node)).collect()];
    for v in &loaded {
        let t = Instant::now();
        let stats = GraphStats::compute(&v.graph);
        let mut snaps: Vec<f64> = Vec::new();
        for (s, n) in starts.iter().zip(&v.nodes) {
            if let Some(n) = n {
                let (la, lo) = v.graph.node_pos(*n);
                let d = dist_m(s.lat, s.lon, f64::from(la) * 1e-7, f64::from(lo) * 1e-7);
                if d > 0.5 {
                    snaps.push(d);
                }
            }
        }
        let snapped = snaps.len();
        let snap_median = {
            snaps.sort_by(f64::total_cmp);
            percentile(&snaps, 0.5)
        };
        targets.push(TargetReport {
            name: v.spec.name(),
            file: v.path.clone(),
            open_ms: v.open_ms,
            verify_ms: v.verify_ms,
            stats_ms: t.elapsed().as_secs_f64() * 1000.0,
            graph_stats: stats,
            view_meta: v.graph.view_meta(),
            available_starts: v.nodes.iter().flatten().count(),
            snapped_starts: snapped,
            snap_median_m: snap_median,
            snap_max_m: snaps.last().copied().unwrap_or(0.0),
            states: Vec::new(),
        });
        target_nodes.push(v.nodes.clone());
    }
    let paths: Vec<PathBuf> =
        std::iter::once(args.graph.clone()).chain(loaded.iter().map(|v| v.path.clone())).collect();
    // Plus aucune projection ouverte avant la phase froide.
    drop(loaded);
    drop(snappers);
    drop(g);

    if !args.skip_timing {
        for ((target, path), nodes) in targets.iter_mut().zip(&paths).zip(&target_nodes) {
            for &kind in &states {
                eprintln!("mesure : {} / {}", target.name, kind.name());
                let rep = measure_state(args, path, kind, &starts, nodes, poly.as_ref())?;
                print_state(&target.name, &rep);
                target.states.push(rep);
            }
        }
    }

    let report = MeasureReport {
        graph: args.graph.clone(),
        environment: environment(&states, args.seed),
        build_stats,
        candidate_starts: n_candidates,
        targets,
        check,
    };
    print_sizes(&report.targets);
    if let Some(out) = &args.out {
        std::fs::write(out, serde_json::to_vec_pretty(&report)?)?;
    }
    Ok(report)
}

fn print_sizes(targets: &[TargetReport]) {
    println!("\n== Tailles et comptes ==");
    println!(
        "{:<14} {:>10} {:>10} {:>9} {:>9} {:>8} {:>11} {:>8} {:>9} {:>7}",
        "graphe", "nœuds", "arêtes", "orientées", "km", "m/arête", "octets", "o/arête", "départs", "rattach."
    );
    for t in targets {
        let s = &t.graph_stats;
        println!(
            "{:<14} {:>10} {:>10} {:>9} {:>9.0} {:>8.1} {:>11} {:>8.1} {:>9} {:>7}",
            t.name,
            s.nodes,
            s.edges,
            s.directed,
            s.km_total,
            s.m_per_edge,
            s.file_bytes,
            s.bytes_per_edge,
            t.available_starts,
            t.snapped_starts
        );
        if let Some(m) = &t.view_meta {
            println!(
                "    filtrées {} ; après élagage {} ; nœuds fondus {} ; boucles coupées {} ; anneaux {} ; rattachement : médiane {:.0} m, max {:.0} m",
                m.filtered_edges,
                m.filtered_edges - m.pruned_edges,
                m.merged_nodes,
                m.loops_broken,
                m.rings,
                t.snap_median_m,
                t.snap_max_m
            );
        }
    }
}

fn print_state(name: &str, r: &StateReport) {
    for rr in &r.radii {
        for (label, s) in [("tous", &rr.all), ("intérieurs", &rr.interior)] {
            println!(
                "{name:<13} {:<5} {:>3} km, {label:>10} ({:>2}{}) : arêtes du disque {:>8.0} ; fixées {:>8.0} ; chaud {:>7.1}/{:>7.1} ms ; froid {:>7.1}/{:>7.1} ms ; état {:>6.1} Mo",
                r.state,
                rr.radius_km,
                s.starts,
                if s.unreliable { ", peu fiable" } else { "" },
                s.disc_edges.median,
                s.settled_warm.median,
                s.warm_ms.median,
                s.warm_ms.p90,
                s.cold_ms.median,
                s.cold_ms.p90,
                s.state_mb.median
            );
        }
    }
}
