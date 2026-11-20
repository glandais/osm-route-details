//! `index` : fichier d'index de service (`.ordi`) d'un graphe et de ses vues `road` et `gravel` (note
//! `docs/design/technique/ord-loops-planete.md` §5) : ce que le générateur calculait à chaque démarrage, calculé une
//! fois ici, par les mêmes fonctions.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use ord_graph::format::GraphFile;
use ord_graph::service_index::{node_sample, write_index, IndexData, IndexFile, ViewTables};
use ord_graph::stats::proc_status;
use ord_graph::view::ViewKind;
use serde::Serialize;

use crate::views_cmd::view_path;

#[derive(Debug, clap::Args)]
pub struct IndexArgs {
    /// Graphe de base (`.ordg`).
    #[arg(long)]
    pub graph: PathBuf,
    /// Dossier des vues `.ordv` (défaut : celui du graphe).
    #[arg(long)]
    pub views_dir: Option<PathBuf>,
    /// Fichier écrit (défaut : le graphe avec l'extension `.ordi`).
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Seuil des composantes courtes, en km : le service refuse un fichier dont le seuil est sous le sien
    /// (`min_component_share × max_distance_m`, 360 km par défaut).
    #[arg(long, default_value_t = 400)]
    pub cap_km: u32,
    /// Fils de l'index des chaînes étrangères (défaut : tous les cœurs).
    #[arg(long)]
    pub threads: Option<usize>,
    /// Relit le fichier écrit (`open`, `check_for`) et compare ses tables à celles calculées.
    #[arg(long)]
    pub check: bool,
    /// Écrit aussi le rapport en JSON.
    #[arg(long)]
    pub report: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct ViewIndexOut {
    pub view: String,
    pub file: PathBuf,
    pub nodes: u64,
    pub chains: u64,
    pub foreign: u64,
    pub short: u64,
    pub lmax_dm: u32,
    pub foreign_s: f64,
    pub scc_s: f64,
}

#[derive(Debug, Serialize)]
pub struct IndexReport {
    pub graph: PathBuf,
    pub out: PathBuf,
    pub file_bytes: u64,
    pub cap_dm: u32,
    pub views: Vec<ViewIndexOut>,
    pub wide_seg: u64,
    pub wide_end: u64,
    pub wide_s: f64,
    pub node_sample: u64,
    pub sample_s: f64,
    pub write_s: f64,
    pub checked: bool,
    pub wall_s: f64,
    /// Pic de `RssAnon`, relevé toutes les 100 ms : la mémoire du calcul, sans les pages des fichiers projetés.
    pub peak_rss_anon_kb: u64,
    pub peak_rss_kb: u64,
}

/// `<graphe>.ordi`.
pub fn index_path(graph: &Path) -> PathBuf {
    graph.with_extension("ordi")
}

/// Relève le pic de `RssAnon` en fond, jusqu'à la chute.
struct AnonPeak {
    peak: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl AnonPeak {
    fn start() -> Self {
        let peak = Arc::new(AtomicU64::new(proc_status().rss_anon_kb));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (Arc::clone(&peak), Arc::clone(&stop));
        let handle = std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                p.fetch_max(proc_status().rss_anon_kb, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        Self { peak, stop, handle: Some(handle) }
    }

    fn finish(mut self) -> u64 {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.peak.load(Ordering::Relaxed).max(proc_status().rss_anon_kb)
    }
}

pub fn run(args: &IndexArgs) -> anyhow::Result<IndexReport> {
    let started = Instant::now();
    let peak = AnonPeak::start();
    let base = GraphFile::open_mmap(&args.graph).context("ouverture du graphe de base")?;
    if base.is_view() {
        bail!("{} est une vue, pas un graphe de base", args.graph.display());
    }
    let bh = base.header().clone();
    let cap_dm = args.cap_km.checked_mul(10_000).context("--cap-km trop grand")?;
    let threads = args.threads.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));

    // Les deux vues, de ce graphe.
    let mut views = Vec::new();
    for (name, kind) in [("road", ViewKind::Road), ("gravel", ViewKind::Gravel)] {
        let path = view_path(&args.graph, name, args.views_dir.as_deref());
        let view = GraphFile::open_mmap(&path).with_context(|| format!("ouverture de {}", path.display()))?;
        let meta = view.view_meta().with_context(|| format!("{} n'est pas une vue", path.display()))?;
        if meta.kind != kind || meta.prune_dead_ends {
            bail!("{} : type de vue ou élagage inattendu", path.display());
        }
        if (meta.base_nodes, meta.base_edges, meta.base_pbf_xxh3, meta.base_build_epoch)
            != (bh.nodes, bh.edges, bh.pbf_xxh3, bh.build_epoch)
        {
            bail!(
                "{} n'est pas une vue de ce graphe (comptes, extrait ou date de construction)",
                path.display()
            );
        }
        views.push((name, path, view));
    }

    // Composantes d'abord (le gros transitoire, une vue après l'autre), puis les chaînes étrangères (gardées).
    let mut tables: [ViewTables; 2] = Default::default();
    let mut outs = Vec::new();
    for (vi, (name, path, view)) in views.iter().enumerate() {
        let t = Instant::now();
        let (short, lmax_dm) = ord_loops::short_components(view, u64::from(cap_dm));
        tables[vi].short = short
            .into_iter()
            .map(|(c, l)| (c, u32::try_from(l).expect("longueur sous cap_dm, qui tient sur 32 bits")))
            .collect();
        tables[vi].lmax_dm = lmax_dm;
        let scc_s = t.elapsed().as_secs_f64();
        eprintln!("{name} : {} chaînes en composantes courtes, {scc_s:.1} s", tables[vi].short.len());
        outs.push(ViewIndexOut {
            view: (*name).to_string(),
            file: path.clone(),
            nodes: view.header().nodes,
            chains: view.header().edges,
            foreign: 0,
            short: tables[vi].short.len() as u64,
            lmax_dm,
            foreign_s: 0.0,
            scc_s,
        });
    }
    for (vi, (name, _, view)) in views.iter().enumerate() {
        let t = Instant::now();
        tables[vi].foreign = ord_loops::foreign_index(view, threads);
        outs[vi].foreign = tables[vi].foreign.len() as u64;
        outs[vi].foreign_s = t.elapsed().as_secs_f64();
        eprintln!("{name} : {} chaînes étrangères, {:.1} s", outs[vi].foreign, outs[vi].foreign_s);
    }

    // Base : arêtes larges (par un segment, par un bout), échantillon des nœuds.
    let t = Instant::now();
    let wide_seg = ord_graph_match::CellIndex::exceptions(&base);
    let wide_end = ord_loops_check::GraphIndex::build(&base).wide_edges().to_vec();
    let wide_s = t.elapsed().as_secs_f64();
    eprintln!(
        "arêtes larges : {} par un segment, {} par un bout, {wide_s:.1} s",
        wide_seg.len(),
        wide_end.len()
    );
    let t = Instant::now();
    let (node_bbox, sample) = node_sample(&base);
    let sample_s = t.elapsed().as_secs_f64();

    let data = IndexData { views: tables, cap_dm, wide_seg, wide_end, node_bbox, node_sample: sample };
    let out = args.out.clone().unwrap_or_else(|| index_path(&args.graph));
    let meta = serde_json::to_vec(&serde_json::json!({
        "tool": format!("ord-graph-build {}", env!("CARGO_PKG_VERSION")),
        "graph": args.graph.file_name().and_then(|n| n.to_str()),
        "cap_km": args.cap_km,
        "views": outs,
    }))?;
    let epoch =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let t = Instant::now();
    let file_bytes = write_index(&out, &base, [&views[0].2, &views[1].2], &data, &meta, epoch)
        .with_context(|| out.display().to_string())?;
    let write_s = t.elapsed().as_secs_f64();

    let mut checked = false;
    if args.check {
        let f = IndexFile::open_mmap(&out).with_context(|| format!("relecture de {}", out.display()))?;
        f.check_for(&base, [&views[0].2, &views[1].2])?;
        let pairs = |p: &[ord_graph::service_index::Pair]| -> Vec<(u32, u32)> {
            p.iter().map(|x| (x[0].get(), x[1].get())).collect()
        };
        for (vi, t) in data.views.iter().enumerate() {
            if pairs(f.foreign(vi)) != t.foreign
                || pairs(f.short(vi)) != t.short
                || f.lmax_dm(vi) != t.lmax_dm
            {
                bail!("{} : la vue {vi} relue n'a pas les tables calculées", out.display());
            }
        }
        let u32s = |p: &[zerocopy::little_endian::U32]| -> Vec<u32> { p.iter().map(|x| x.get()).collect() };
        if u32s(f.wide_seg()) != data.wide_seg
            || u32s(f.wide_end()) != data.wide_end
            || f.node_bbox() != data.node_bbox
            || f.node_sample().len() != data.node_sample.len()
        {
            bail!("{} : les tables de la base relues ne sont pas celles calculées", out.display());
        }
        checked = true;
    }

    let report = IndexReport {
        graph: args.graph.clone(),
        out,
        file_bytes,
        cap_dm,
        views: outs,
        wide_seg: data.wide_seg.len() as u64,
        wide_end: data.wide_end.len() as u64,
        wide_s,
        node_sample: data.node_sample.len() as u64,
        sample_s,
        write_s,
        checked,
        wall_s: started.elapsed().as_secs_f64(),
        peak_rss_anon_kb: peak.finish(),
        peak_rss_kb: proc_status().vm_hwm_kb,
    };
    if let Some(p) = &args.report {
        std::fs::write(p, serde_json::to_string_pretty(&report)?)?;
    }
    Ok(report)
}
