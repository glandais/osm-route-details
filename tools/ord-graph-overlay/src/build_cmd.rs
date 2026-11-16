//! Les deux commandes : `build` (export + graphe → surcouches + rapport) et `inspect`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use clap::Args;
use ord_graph::mmap::MappedGraph;
use ord_graph::overlay::{write_overlay, Overlay, OverlayMeta};
use ord_graph::util::{sample_without_replacement, Poly, SplitMix64};
use rayon::prelude::*;
use serde_json::{json, Value};

use crate::aggregate::{aggregate, params_hash, TeamAcc};
use crate::checks::{verify_sample, Verification};
use crate::export::TYPES;
use crate::geo::decode_polyline6;
use crate::index::SegIndex;
use crate::matcher;
use crate::pipeline::{ingest, run_jobs, trv_per_geom, Ingest, Job, Outcome};
use crate::report::{overlay_stats, team_json, verification_json};

#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Fichier de graphe `.ordg` (lot 0).
    #[arg(long)]
    pub graph: PathBuf,
    /// Export des équipes `.csv.gz` (privé : lu en local, jamais copié).
    #[arg(long)]
    pub export: PathBuf,
    /// Polygone de la région (`.poly` de Geofabrik).
    #[arg(long)]
    pub poly: PathBuf,
    /// Dossier des surcouches `.ordo`.
    #[arg(long)]
    pub out_dir: PathBuf,
    /// Rapport JSON.
    #[arg(long)]
    pub report: PathBuf,
    /// N'instruire que ces équipes (ids séparés par des virgules) ; toutes par défaut.
    #[arg(long, value_delimiter = ',')]
    pub teams: Vec<u64>,
    /// Nombre de fils (tous les cœurs par défaut).
    #[arg(long)]
    pub threads: Option<usize>,
    /// Arêtes d'équipe appariées re-vérifiées par équipe (Hausdorff et Fréchet en haversine).
    #[arg(long, default_value_t = 2000)]
    pub verify_sample: usize,
    /// Graine des tirages.
    #[arg(long, default_value_t = 20_261_115)]
    pub seed: u64,
}

#[derive(Debug, Args)]
pub struct InspectArgs {
    #[arg(long)]
    pub overlay: PathBuf,
    #[arg(long)]
    pub graph: PathBuf,
}

#[derive(Debug)]
pub struct BuildOutput {
    /// Le rapport complet (aussi écrit dans le fichier) : lu par les tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub report: Value,
    /// Ce qui s'imprime : un résumé court (le rapport complet est dans le fichier).
    pub summary: Value,
}

/// Nom de fichier de surcouche d'une équipe.
pub fn overlay_name(team: u64, graph: &Path) -> String {
    let stem = graph.file_stem().map_or_else(|| "graphe".to_string(), |s| s.to_string_lossy().into_owned());
    format!("{team}-{stem}.ordo")
}

/// Pic de mémoire résidente du processus, en ko (Linux), ou `None`.
fn vm_hwm_kb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines().find_map(|l| l.strip_prefix("VmHWM:")?.split_whitespace().next()?.parse().ok())
}

struct Phases(Vec<(String, f64)>, Instant);

impl Phases {
    fn new() -> Self {
        Self(Vec::new(), Instant::now())
    }
    fn mark(&mut self, name: &str) {
        let secs = self.1.elapsed().as_secs_f64();
        eprintln!("[{secs:8.2} s] {name}");
        self.0.push((name.to_string(), (secs * 100.0).round() / 100.0));
        self.1 = Instant::now();
    }
}

fn hex(v: u64) -> String {
    format!("{v:016x}")
}

fn identity_json(g: &ord_graph::GraphIdentity) -> Value {
    json!({
        "format_major": g.format_major,
        "attr_layout": g.attr_layout,
        "zoom": g.zoom,
        "nodes": g.nodes,
        "edges": g.edges,
        "directed": g.directed,
        "id_edges_xxh3": hex(g.id_edges_xxh3),
        "id_nodes_xxh3": hex(g.id_nodes_xxh3),
        "pbf_xxh3": hex(g.pbf_xxh3),
        "replication_timestamp": g.replication_timestamp,
    })
}

/// Arêtes triées et comptes à plat (saturés à 65 535) d'une équipe, prêts pour l'écriture.
pub fn overlay_arrays(acc: &TeamAcc) -> (Vec<u32>, Vec<u16>) {
    ord_graph_match::agg::overlay_arrays(&acc.counts)
}

/// Re-vérifie un échantillon d'arêtes appariées de l'équipe `ti` (méthode indépendante, `checks.rs`).
fn verify_team(
    gf: &MappedGraph,
    ing: &Ingest,
    jobs: &[Job],
    acc: &TeamAcc,
    sample: usize,
    seed: u64,
) -> Vec<Verification> {
    let pop = &acc.matched_occurrences;
    let mut rng = SplitMix64(seed ^ acc.id);
    let picked = sample_without_replacement(&mut rng, pop.len(), sample);
    picked
        .into_par_iter()
        .filter_map(|i| {
            let rec = &ing.edges[pop[i] as usize];
            let Outcome::Matched(chain) = &jobs[rec.geom as usize].outcome else { return None };
            let g = decode_polyline6(&ing.geoms[rec.geom as usize])?;
            verify_sample(gf, &g, chain)
        })
        .collect()
}

pub fn run(a: &BuildArgs) -> anyhow::Result<BuildOutput> {
    let pool = rayon::ThreadPoolBuilder::new().num_threads(a.threads.unwrap_or(0)).build()?;
    pool.install(|| run_inner(a))
}

fn run_inner(a: &BuildArgs) -> anyhow::Result<BuildOutput> {
    let mut ph = Phases::new();
    let gf = MappedGraph::open_mmap(&a.graph).with_context(|| a.graph.display().to_string())?;
    gf.verify().context("le graphe ne passe pas verify()")?;
    let identity = gf.graph_identity();
    ph.mark("graphe ouvert et vérifié");
    let poly = Poly::read(&a.poly)?;
    let idx = SegIndex::build(&gf);
    ph.mark("index spatial");
    let ing = ingest(&a.export, &a.teams)?;
    ph.mark("export lu");
    let trv = trv_per_geom(&ing);
    let jobs = run_jobs(&gf, &idx, &poly, &ing.geoms, &trv);
    ph.mark("géométries distinctes appariées");
    let mut teams = aggregate(&gf, &ing, &jobs);
    ph.mark("agrégation");

    std::fs::create_dir_all(&a.out_dir)?;
    teams.sort_by_key(|t| t.id);
    let epoch =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let (mut overall, mut pooled) = (TeamAcc::default(), Vec::<Verification>::new());
    let (mut pooled_pop, mut files) = (0usize, Vec::new());
    let mut team_reports = serde_json::Map::new();
    for acc in &teams {
        let (edges, counts) = overlay_arrays(acc);
        let meta = OverlayMeta {
            graph: identity,
            team_id: acc.id,
            export_xxh3: ing.export_xxh3,
            export_rows: ing.rows,
            params_hash: params_hash(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            build_epoch: epoch,
            types: TYPES.iter().map(|t| (*t).to_string()).collect(),
        };
        let path = a.out_dir.join(overlay_name(acc.id, &a.graph));
        let bytes = write_overlay(&path, &meta, &edges, &counts)?;
        files.push(path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()));
        let ver = verify_team(&gf, &ing, &jobs, acc, a.verify_sample, a.seed);
        let vj = verification_json(&ver, a.verify_sample, acc.matched_occurrences.len());
        team_reports.insert(acc.id.to_string(), team_json(&gf, acc, Some(bytes), &vj));
        overall.absorb(acc);
        pooled_pop += acc.matched_occurrences.len();
        pooled.extend(ver);
    }
    ph.mark("surcouches écrites, échantillons re-vérifiés");
    let total_bytes: u64 = team_reports.values().filter_map(|v| v["overlay"]["file_bytes"].as_u64()).sum();
    let overall_json = team_json(
        &gf,
        &overall,
        Some(total_bytes),
        &verification_json(&pooled, a.verify_sample * teams.len(), pooled_pop),
    );
    let mut overall_json = overall_json;
    overall_json["team_id"] = json!("all");
    // Le recouvrement par type de parcours est celui de l'export, pas celui du graphe.
    let report = json!({
        "tool": {"name": "ord-graph-overlay", "version": env!("CARGO_PKG_VERSION")},
        "graph": {"file": a.graph.file_name().map(|n| n.to_string_lossy()), "identity": identity_json(&identity)},
        "export": {
            "xxh3": hex(ing.export_xxh3),
            "rows": ing.rows,
            "rows_skipped": ing.skipped_rows,
            "distinct_geometries": ing.geoms.len(),
            "team_edges": ing.edges.len(),
        },
        "params": {
            "hash": hex(params_hash()),
            "corridor_m": matcher::CORRIDOR_M,
            "node_snap_m": matcher::NODE_SNAP_M,
            "edge_snap_m": matcher::EDGE_SNAP_M,
            "hausdorff_max_m": matcher::HAUSDORFF_MAX_M,
            "length_tolerance_m": matcher::LEN_TOL_ABS_M,
            "length_tolerance_rel": matcher::LEN_TOL_REL,
            "ambiguity_m": matcher::AMBIGUITY_M,
            "min_piece_m": matcher::MIN_PIECE_M,
            "types": TYPES,
            "seed": a.seed,
            "verify_sample_per_team": a.verify_sample,
        },
        "index": {"segments": idx.segments()},
        "overall": overall_json,
        "teams": team_reports,
        "overlays": files,
    });
    ph.mark("rapport construit");
    let mut report = report;
    report["run"] = json!({
        "phases_s": ph.0.iter().map(|(n, s)| json!({"name": n, "seconds": s})).collect::<Vec<_>>(),
        "vm_hwm_kb": vm_hwm_kb(),
        "threads": rayon::current_num_threads(),
    });
    if let Some(dir) = a.report.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&a.report, serde_json::to_vec_pretty(&report)?)?;
    let summary = json!({
        "report": a.report,
        "overlays_dir": a.out_dir,
        "teams": teams.len(),
        "km_in_region": report["overall"]["km"]["in_region"],
        "km_matched": report["overall"]["km"]["matched"],
        "matched_share_of_in_region": report["overall"]["km"]["matched_share_of_in_region"],
        "run": report["run"],
    });
    Ok(BuildOutput { report, summary })
}

pub fn inspect(a: &InspectArgs) -> anyhow::Result<Value> {
    let gf = MappedGraph::open_mmap(&a.graph).with_context(|| a.graph.display().to_string())?;
    gf.verify().context("le graphe ne passe pas verify()")?;
    let bytes = std::fs::read(&a.overlay).with_context(|| a.overlay.display().to_string())?;
    let o = Overlay::decode_for(&bytes, &gf.graph_identity()).context("surcouche refusée")?;
    let mut counts_by_type = vec![[0u64; 2]; o.meta.types.len()];
    let mut max_count = 0u16;
    // Relu du fichier contre les attributs du graphe : une entrée non nulle dans un sens fermé au vélo est un défaut.
    let mut closed_direction_entries = 0u64;
    for (e, c) in o.iter() {
        let attrs = gf.edge(e).attrs;
        for (k, v) in c.iter().enumerate() {
            counts_by_type[k / 2][k % 2] += u64::from(*v);
            max_count = max_count.max(*v);
            let open = if k % 2 == 0 { attrs.fwd_ok() } else { attrs.bwd_ok() };
            if *v > 0 && !open {
                closed_direction_entries += 1;
            }
        }
    }
    Ok(json!({
        "team_id": o.meta.team_id.to_string(),
        "entries": o.len(),
        "file_bytes": bytes.len(),
        "types": o.meta.types,
        "routes_forward_backward_by_type": counts_by_type,
        "max_count": max_count,
        "closed_direction_entries": closed_direction_entries,
        "tool_version": o.meta.tool_version,
        "build_epoch": o.meta.build_epoch,
        "export_xxh3": hex(o.meta.export_xxh3),
        "export_rows": o.meta.export_rows,
        "params_hash": hex(o.meta.params_hash),
        "graph": identity_json(&o.meta.graph),
        "overlay_stats": overlay_stats(&gf, &{
            let mut m = rustc_hash::FxHashMap::default();
            for (e, c) in o.iter() {
                let mut acc = crate::aggregate::EdgeAcc::default();
                for (i, v) in c.iter().enumerate() {
                    acc.counts[i] = u32::from(*v);
                }
                acc.any = acc.counts.iter().copied().max().unwrap_or(0);
                m.insert(e, acc);
            }
            m
        }, Some(bytes.len() as u64)),
    }))
}
