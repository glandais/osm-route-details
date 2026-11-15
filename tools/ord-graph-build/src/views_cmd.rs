//! `views` : construit, hors ligne, les vues par type de parcours d'un graphe de base (lot 0 bis).

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context};
use ord_graph::format::{write_graph, GraphFile};
use ord_graph::stats::proc_status;
use ord_graph::view::{build_view, check_view_structure, ViewMeta, ViewSpec};
use serde::Serialize;

#[derive(Debug, clap::Args)]
pub struct ViewsArgs {
    /// Graphe de base (`.ordg`).
    #[arg(long)]
    pub graph: PathBuf,
    /// Vues à construire : `road`, `gravel`, `all`, avec le suffixe `-prune` pour l'élagage des impasses.
    #[arg(long, value_delimiter = ',', default_values_t = ["road".to_string(), "gravel".to_string(), "all".to_string(), "road-prune".to_string(), "gravel-prune".to_string()])]
    pub views: Vec<String>,
    /// Dossier de sortie (défaut : celui du graphe de base).
    #[arg(long)]
    pub out_dir: Option<PathBuf>,
    /// Écrit aussi le rapport en JSON.
    #[arg(long)]
    pub report: Option<PathBuf>,
}

/// Chemin du fichier d'une vue : `<graphe sans .ordg>.<vue>.ordv`, dans `dir` (défaut : dossier du graphe).
pub fn view_path(graph: &Path, name: &str, dir: Option<&Path>) -> PathBuf {
    let stem = graph.file_stem().and_then(|s| s.to_str()).unwrap_or("graph");
    let dir =
        dir.map(Path::to_path_buf).or_else(|| graph.parent().map(Path::to_path_buf)).unwrap_or_default();
    dir.join(format!("{stem}.{name}.ordv"))
}

#[derive(Debug, Serialize)]
pub struct ViewBuild {
    pub name: String,
    pub file: PathBuf,
    pub file_bytes: u64,
    pub wall_s: f64,
    pub meta: ViewMeta,
    pub base_edges_covered: u64,
    pub max_chain_len: u64,
    pub one_way_edges: u64,
    pub unusable_edges: u64,
    pub parallel_pairs: u64,
}

#[derive(Debug, Serialize)]
pub struct ViewsReport {
    pub base: PathBuf,
    pub views: Vec<ViewBuild>,
    pub peak_rss_kb: u64,
}

pub fn run(args: &ViewsArgs) -> anyhow::Result<ViewsReport> {
    let base = GraphFile::open_mmap(&args.graph).context("ouverture du graphe de base")?;
    base.verify().context("verify() du graphe de base")?;
    if base.is_view() {
        bail!("{} est déjà une vue", args.graph.display());
    }
    if let Some(d) = &args.out_dir {
        std::fs::create_dir_all(d)?;
    }
    let tool_version = format!("ord-graph {}", env!("CARGO_PKG_VERSION"));
    let epoch =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let mut views = Vec::new();
    for name in &args.views {
        let spec = ViewSpec::parse(name).with_context(|| format!("vue inconnue : {name}"))?;
        let started = Instant::now();
        let mut data = build_view(&base, &spec).with_context(|| format!("vue {name}"))?;
        let path = view_path(&args.graph, &spec.name(), args.out_dir.as_deref());
        write_graph(&path, &mut data, &tool_version, epoch).with_context(|| path.display().to_string())?;
        let wall_s = started.elapsed().as_secs_f64();
        // Relecture : la vue contre son graphe de base, exhaustivement (la contraction est correcte ou on s'arrête).
        let view = GraphFile::open_mmap(&path)?;
        let s = check_view_structure(&base, &view).map_err(|e| anyhow::anyhow!("vue {name} : {e}"))?;
        let meta = view.view_meta().context("méta de la vue")?;
        eprintln!(
            "vue {:<13} : {:>9} nœuds, {:>9} arêtes (base {} ; filtrées {}, après élagage {}), {:.1} s -> {}",
            meta.name,
            meta.view_nodes,
            meta.view_edges,
            meta.base_edges,
            meta.filtered_edges,
            meta.filtered_edges - meta.pruned_edges,
            wall_s,
            path.display()
        );
        views.push(ViewBuild {
            name: spec.name(),
            file: path.clone(),
            file_bytes: std::fs::metadata(&path)?.len(),
            wall_s,
            meta,
            base_edges_covered: s.base_edges_covered,
            max_chain_len: s.max_chain_len,
            one_way_edges: s.one_way_edges,
            unusable_edges: s.unusable_edges,
            parallel_pairs: s.parallel_pairs,
        });
    }
    let report = ViewsReport { base: args.graph.clone(), views, peak_rss_kb: proc_status().vm_hwm_kb };
    if let Some(r) = &args.report {
        std::fs::write(r, serde_json::to_vec_pretty(&report)?)?;
    }
    Ok(report)
}
