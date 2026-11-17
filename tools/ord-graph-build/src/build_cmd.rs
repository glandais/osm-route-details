//! `build` : extrait PBF → fichier de graphe, avec durée par phase et pic de mémoire.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use ord_graph::build::{build_graph, BuildOptions, OsmSource};
use ord_graph::external::{build_external, ExternalOptions};
use ord_graph::format::write_graph;
use ord_graph::pbf::PbfSource;
use ord_graph::stats::{proc_status, Phase, PhaseClock};
use serde::Serialize;

use crate::util::yyyymmdd;

#[derive(Debug, clap::Args)]
pub struct BuildArgs {
    /// Extrait `.osm.pbf` trié Type_then_ID.
    #[arg(long)]
    pub pbf: PathBuf,
    /// Dossier de sortie (défaut : ~/.cache/osm-route-details/graph).
    #[arg(long)]
    pub out_dir: Option<PathBuf>,
    /// Nom de région dans le nom du fichier (défaut : début du nom du PBF, avant `-latest`).
    #[arg(long)]
    pub region: Option<String>,
    /// Zoom de la grille de cellules.
    #[arg(long, default_value_t = 11)]
    pub zoom: u8,
    /// Nombre de fils (défaut : tous les cœurs).
    #[arg(long)]
    pub threads: Option<usize>,
    /// Écrit aussi le rapport de construction en JSON.
    #[arg(long)]
    pub report: Option<PathBuf>,
    /// Construction hors mémoire : tris externes et écriture en flux, pic de mémoire borné par
    /// `--mem-budget-mb` (note `ord-graph-build-hors-memoire.md`). Mêmes octets que la construction en mémoire.
    #[arg(long)]
    pub external: bool,
    /// Budget des tampons de tri de la construction hors mémoire, en Mo.
    #[arg(long, default_value_t = 1024, requires = "external")]
    pub mem_budget_mb: usize,
    /// Dossier des fichiers temporaires de la construction hors mémoire (défaut : le dossier de sortie).
    #[arg(long, requires = "external")]
    pub tmp_dir: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct BuildReport {
    pub output: PathBuf,
    pub file_bytes: u64,
    pub threads: usize,
    pub zoom: u8,
    pub wall_s: f64,
    pub cpu_s: f64,
    /// `VmHWM` en fin de commande. La relecture finale compte les pages du fichier relu : en construction hors
    /// mémoire, le pic de la construction elle-même est `peak_rss_before_verify_kb`.
    pub peak_rss_kb: u64,
    /// Construction hors mémoire seulement : `VmHWM` juste avant la relecture par `mmap` et `verify()`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_rss_before_verify_kb: Option<u64>,
    pub phases: Vec<Phase>,
    pub vertices: u64,
    pub edges: u64,
    pub ways_kept: u64,
    pub ways_total: u64,
    /// Construction hors mémoire seulement : pic d'octets du dossier temporaire.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmp_peak_bytes: Option<u64>,
    /// Construction hors mémoire seulement : séries écrites par tri (0 : tout a tenu en mémoire).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<(String, u64)>,
}

pub fn default_cache_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".cache/osm-route-details/graph")
}

fn region_of(pbf: &Path) -> String {
    let stem = pbf.file_name().and_then(|n| n.to_str()).unwrap_or("region");
    let stem = stem.trim_end_matches(".pbf").trim_end_matches(".osm");
    stem.strip_suffix("-latest").unwrap_or(stem).to_string()
}

pub fn run(args: &BuildArgs) -> anyhow::Result<BuildReport> {
    if let Some(n) = args.threads {
        // Peut échouer si le pool global existe déjà (tests) : sans importance.
        let _ = rayon::ThreadPoolBuilder::new().num_threads(n).build_global();
    }
    let started = Instant::now();
    let mut clock = PhaseClock::new();
    let region = args.region.clone().unwrap_or_else(|| region_of(&args.pbf));
    let mut src = PbfSource::open(&args.pbf).with_context(|| args.pbf.display().to_string())?;
    let open_phase = clock.lap("ouverture du PBF (en-tête, xxh3)");
    eprintln!("{} : {:.1} s", open_phase.name, open_phase.wall_s);

    let meta = src.meta();
    let date =
        if meta.replication_timestamp > 0 { yyyymmdd(meta.replication_timestamp) } else { "inconnue".into() };
    let out_dir = args.out_dir.clone().unwrap_or_else(default_cache_dir);
    std::fs::create_dir_all(&out_dir)?;
    let path = out_dir.join(format!("{region}-{date}-z{}.ordg", args.zoom));
    let tool_version = format!("ord-graph {}", env!("CARGO_PKG_VERSION"));
    let epoch =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);

    let (stats, tmp_peak_bytes, runs, peak_rss_before_verify_kb) = if args.external {
        let opts = ExternalOptions {
            zoom: args.zoom,
            mem_budget: args.mem_budget_mb.saturating_mul(1 << 20),
            tmp_parent: args.tmp_dir.clone().unwrap_or_else(|| out_dir.clone()),
        };
        let r = build_external(
            &mut src,
            &opts,
            &region,
            &path,
            &tool_version,
            epoch,
            vec![open_phase],
            &mut |p| eprintln!("  fait : {p}"),
        )
        .context("construction hors mémoire")?;
        for stale in &r.stale_tmp_removed {
            eprintln!("dossier temporaire d'une construction interrompue supprimé : {}", stale.display());
        }
        (r.stats, Some(r.tmp_peak_bytes), r.runs, Some(r.peak_rss_before_verify_kb))
    } else {
        let mut data = build_graph(&mut src, &BuildOptions { zoom: args.zoom }, &region, &mut |p| {
            eprintln!("  fait : {p}")
        })
        .context("construction")?;
        data.stats.phases.insert(0, open_phase.clone());
        let mut write_clock = PhaseClock::new();
        write_graph(&path, &mut data, &tool_version, epoch).context("écriture")?;
        let write_phase = write_clock.lap("écriture, relecture et verify()");
        eprintln!("{} : {:.1} s", write_phase.name, write_phase.wall_s);
        let mut stats = data.stats;
        stats.phases.push(write_phase);
        (stats, None, Vec::new(), None)
    };

    let report = BuildReport {
        output: path.clone(),
        file_bytes: std::fs::metadata(&path)?.len(),
        threads: stats.threads,
        zoom: args.zoom,
        wall_s: started.elapsed().as_secs_f64(),
        cpu_s: ord_graph::stats::cpu_seconds(),
        peak_rss_kb: proc_status().vm_hwm_kb,
        peak_rss_before_verify_kb,
        vertices: stats.vertices,
        edges: stats.edges,
        ways_kept: stats.ways_kept,
        ways_total: stats.ways_total,
        phases: stats.phases,
        tmp_peak_bytes,
        runs,
    };
    if let Some(r) = &args.report {
        std::fs::write(r, serde_json::to_vec_pretty(&report)?)?;
    }
    Ok(report)
}
