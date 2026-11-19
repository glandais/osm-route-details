//! `attractors` : fichiers d'attracteurs (`.orda`) des vues `road` et `gravel` d'un graphe, depuis l'extrait PBF
//! du graphe (lot 4d, §17.5 de `ord-loops-lot4.md`). Données OSM publiques : le rapport n'a rien de privé.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context};
use ord_graph::attract::{
    build_attr_counted, encode_attr, extract_objects, write_attr, AttrData, AttrFile, AttrWrite,
    ExtractStats, CATS, DEFAULT_REACH_M, DEFAULT_SNAP_M, N_CATS,
};
use ord_graph::format::GraphFile;
use ord_graph::pbf::PbfSource;
use ord_graph::stats::proc_status;
use ord_graph::view::ViewKind;
use ord_graph::OsmSource;
use serde::Serialize;

use crate::views_cmd::view_path;

#[derive(Debug, clap::Args)]
pub struct AttractorsArgs {
    /// Extrait `.osm.pbf` du graphe (le même, octet pour octet : taille et empreinte sont vérifiées).
    #[arg(long)]
    pub pbf: PathBuf,
    /// Graphe de base (`.ordg`).
    #[arg(long)]
    pub graph: PathBuf,
    /// Vues à écrire : `road`, `gravel`.
    #[arg(long, value_delimiter = ',', default_values_t = ["road".to_string(), "gravel".to_string()])]
    pub views: Vec<String>,
    /// Dossier des vues `.ordv` (défaut : celui du graphe).
    #[arg(long)]
    pub views_dir: Option<PathBuf>,
    /// Dossier de sortie (défaut : celui du graphe).
    #[arg(long)]
    pub out_dir: Option<PathBuf>,
    /// Distance d'accroche d'un objet à un nœud de vue, en mètres.
    #[arg(long, default_value_t = DEFAULT_SNAP_M)]
    pub snap_m: u16,
    /// Distance de la zone autour d'un nœud d'ancrage, en mètres.
    #[arg(long, default_value_t = DEFAULT_REACH_M)]
    pub reach_m: u16,
    /// Relit chaque fichier (`open`, `check_for`) et recalcule la zone depuis l'extrait, octet pour octet.
    #[arg(long)]
    pub check: bool,
    /// Écrit aussi le rapport en JSON.
    #[arg(long)]
    pub report: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct CatCounts {
    pub cat: &'static str,
    pub objects: u64,
    pub snapped: u64,
    pub anchors: u64,
    pub zone_nodes: u64,
}

#[derive(Debug, Serialize)]
pub struct ViewOut {
    pub view: String,
    pub file: PathBuf,
    pub file_bytes: u64,
    pub xxh3: String,
    pub view_nodes: u64,
    pub anchors: u64,
    pub zone_nodes: u64,
    pub per_cat: Vec<CatCounts>,
    pub wall_s: f64,
    pub checked: bool,
}

#[derive(Debug, Serialize)]
pub struct AttractorsReport {
    pub pbf: String,
    pub graph: String,
    pub pbf_size: u64,
    pub pbf_xxh3: String,
    pub objects: u64,
    pub ways_tagged: u64,
    pub ways_without_node: u64,
    pub nodes_wanted: u64,
    pub nodes_found: u64,
    pub extract_wall_s: f64,
    pub views: Vec<ViewOut>,
    pub wall_s: f64,
    pub peak_rss_kb: u64,
}

fn base_name(p: &Path) -> String {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string()
}

/// `<radical du graphe>.<vue>.orda`, dans `dir`.
pub fn attr_path(graph: &Path, view: &str, dir: Option<&Path>) -> PathBuf {
    view_path(graph, view, dir).with_extension("orda")
}

pub fn run(args: &AttractorsArgs) -> anyhow::Result<AttractorsReport> {
    let started = Instant::now();
    let base = GraphFile::open_mmap(&args.graph).context("ouverture du graphe de base")?;
    if base.is_view() {
        bail!("{} est une vue, pas un graphe de base", args.graph.display());
    }
    let bh = base.header().clone();
    let views_dir = args.views_dir.as_deref();
    let out_dir = args.out_dir.clone().or_else(|| args.graph.parent().map(Path::to_path_buf));

    // 1. Identité : l'extrait est celui du graphe ; chaque vue appartient à ce graphe.
    let mut views = Vec::new();
    for name in &args.views {
        let kind = match name.as_str() {
            "road" => ViewKind::Road,
            "gravel" => ViewKind::Gravel,
            other => bail!("vue `{other}` : seules road et gravel ont un fichier d'attracteurs"),
        };
        let path = view_path(&args.graph, name, views_dir);
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
        views.push((name.clone(), view));
    }
    let mut src =
        PbfSource::open(&args.pbf).with_context(|| format!("ouverture de {}", args.pbf.display()))?;
    let meta = src.meta();
    if (meta.size, meta.xxh3) != (bh.pbf_size, bh.pbf_xxh3) {
        bail!(
            "l'extrait n'est pas celui du graphe : taille ou empreinte ({} octets, {:016x} ; le graphe attend {} octets, {:016x})",
            meta.size,
            meta.xxh3,
            bh.pbf_size,
            bh.pbf_xxh3
        );
    }

    // 2. Objets (deux passes sur l'extrait).
    let t = Instant::now();
    let (objects, stats) = extract_objects(&mut src).context("extraction des objets")?;
    let extract_wall_s = t.elapsed().as_secs_f64();
    eprintln!("{} objets ({} ways étiquetées), {:.1} s", objects.len(), stats.ways_tagged, extract_wall_s);
    let mut per_cat_objects = [0u64; 8];
    for o in &objects {
        per_cat_objects[usize::from(o.cat & 7)] += 1;
    }

    if let Some(d) = &out_dir {
        std::fs::create_dir_all(d)?;
    }
    // 3. Une vue après l'autre : accroche, zone, écriture, relecture.
    let epoch =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let mut outs = Vec::new();
    for (name, view) in &views {
        let t = Instant::now();
        let (data, snapped) = build_attr_counted(view, &objects, args.snap_m, args.reach_m);
        let (anch_by, zone_by) = data.counts_by_cat(usize::from(N_CATS));
        let per_cat: Vec<CatCounts> = (0..usize::from(N_CATS))
            .map(|c| CatCounts {
                cat: CATS[c],
                objects: per_cat_objects[c],
                snapped: snapped[c],
                anchors: anch_by[c],
                zone_nodes: zone_by[c],
            })
            .collect();
        let json = meta_json(&args.pbf, &args.graph, name, args, &stats, &per_cat);
        let w = AttrWrite { snap_m: args.snap_m, reach_m: args.reach_m, meta: &json, built_epoch: epoch };
        let out = attr_path(&args.graph, name, out_dir.as_deref());
        let bytes = write_attr(&out, &bh, view, &data, &w).with_context(|| out.display().to_string())?;
        let mut checked = false;
        if args.check {
            check_file(&out, &bh, view, &data, &w)?;
            checked = true;
        }
        let file = AttrFile::open_mmap(&out)?;
        let o = ViewOut {
            view: name.clone(),
            file: out.clone(),
            file_bytes: bytes,
            xxh3: format!("{:016x}", file.digest()),
            view_nodes: view.header().nodes,
            anchors: data.anchors.len() as u64,
            zone_nodes: data.zone.len() as u64,
            per_cat,
            wall_s: t.elapsed().as_secs_f64(),
            checked,
        };
        eprintln!(
            "vue {:<7} : {} ancrages, {} nœuds de zone, {} octets, {:.1} s{} -> {}",
            o.view,
            o.anchors,
            o.zone_nodes,
            o.file_bytes,
            o.wall_s,
            if checked { ", contrôlé" } else { "" },
            out.display()
        );
        outs.push(o);
    }
    let report = AttractorsReport {
        pbf: base_name(&args.pbf),
        graph: base_name(&args.graph),
        pbf_size: bh.pbf_size,
        pbf_xxh3: format!("{:016x}", bh.pbf_xxh3),
        objects: objects.len() as u64,
        ways_tagged: stats.ways_tagged,
        ways_without_node: stats.ways_without_node,
        nodes_wanted: stats.nodes_wanted,
        nodes_found: stats.nodes_found,
        extract_wall_s,
        views: outs,
        wall_s: started.elapsed().as_secs_f64(),
        peak_rss_kb: proc_status().vm_hwm_kb,
    };
    if let Some(r) = &args.report {
        std::fs::write(r, serde_json::to_vec_pretty(&report)?)?;
    }
    Ok(report)
}

fn meta_json(
    pbf: &Path,
    graph: &Path,
    view: &str,
    args: &AttractorsArgs,
    stats: &ExtractStats,
    per_cat: &[CatCounts],
) -> Vec<u8> {
    let v = serde_json::json!({
        "tool_version": format!("ord-graph-build {}", env!("CARGO_PKG_VERSION")),
        "pbf": base_name(pbf),
        "graph": base_name(graph),
        "view": view,
        "snap_m": args.snap_m,
        "reach_m": args.reach_m,
        "ways_tagged": stats.ways_tagged,
        "ways_without_node": stats.ways_without_node,
        "per_cat": per_cat,
    });
    serde_json::to_vec(&v).expect("JSON")
}

/// Relit le fichier écrit, le confronte au graphe et à la vue, et recalcule l'encodage : mêmes octets.
fn check_file<B: AsRef<[u8]>>(
    path: &Path,
    base: &ord_graph::format::Header,
    view: &GraphFile<B>,
    data: &AttrData,
    w: &AttrWrite<'_>,
) -> anyhow::Result<()> {
    let file = AttrFile::open_mmap(path).with_context(|| format!("relecture de {}", path.display()))?;
    file.check_for(base, view).with_context(|| format!("{} contre sa vue", path.display()))?;
    // Zone recalculée depuis les ancrages relus (et non ceux du calcul) : la relecture est indépendante.
    let anchors: Vec<(u32, u8)> = file.anchors().collect();
    let zone = ord_graph::attract::zone_for(view, &anchors, w.reach_m);
    let again = encode_attr(base, view, &AttrData { anchors, zone }, w)?;
    let on_disk = std::fs::read(path)?;
    if again != on_disk {
        bail!("{} : la zone recalculée depuis les ancrages relus diffère du fichier écrit", path.display());
    }
    let direct = encode_attr(base, view, data, w)?;
    if direct != on_disk {
        bail!("{} : le fichier écrit diffère de l'encodage du calcul", path.display());
    }
    Ok(())
}
