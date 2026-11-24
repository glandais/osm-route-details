//! Retouche d'un parcours enregistré, lot 0 : une mesure (note : `docs/design/technique/ord-retouche-lot0.md`).
//!
//! Livraison A : **la continuité de l'appariement et l'emplacement des coutures** sur toutes les traces de l'export, sans
//! rien changer au moteur. Une greffe locale ne se coud qu'à un nœud où la trace passe par un nœud commun : si les
//! marches réelles sont pleines de trous, l'entrée de la retouche est à compléter avant tout.
//!
//! **Privé** : l'export, les départs, les coordonnées et les ids de trace ou d'équipe sont des données d'équipes
//! réelles. Les sorties vont sous `~/.cache/osm-route-details/graph/retouche/` et nulle part ailleurs ; les équipes y
//! sont lettrées, une trace y est désignée par `(lettre, rang)` et seuls des agrégats sont recopiables.

pub mod continuity;
pub mod control;
pub mod draw;
pub mod engine;
pub mod move_end;
pub mod out;
pub mod out_play;
pub mod play;
pub mod posed;
pub mod project;
pub mod resize;

use std::path::PathBuf;

use anyhow::{anyhow, Context as _, Result};
use clap::Args;
use ord_graph::mmap::MappedGraph;
use ord_graph::util::Poly;
use ord_graph_match::SegIndex;
use rayon::prelude::*;
use xxhash_rust::xxh3::xxh3_64;

use self::continuity::{Ctx, Outcome};
use self::posed::{SeamOpts, ViewInfo, ViewLookup};
use crate::likeness::export::{self, RetainParams};
use crate::likeness::{read_letters, Phases};

/// Les chemins communs aux sous-commandes. Les défauts sont ceux de `likeness` (lot 6, export privé).
#[derive(Debug, Args, Clone)]
pub struct PathOpts {
    /// Graphe de base (défaut : `G/lot6/france-20261115-z11.ordg`).
    #[arg(long)]
    pub graph: Option<PathBuf>,
    /// Vue `road` (défaut : `G/lot6/france-20261115-z11.road.ordv`).
    #[arg(long)]
    pub road: Option<PathBuf>,
    /// Vue `gravel` (défaut : `G/lot6/france-20261115-z11.gravel.ordv`).
    #[arg(long)]
    pub gravel: Option<PathBuf>,
    /// `.poly` de la région de `match_job` (défaut : `G/lot6/france.poly`).
    #[arg(long)]
    pub poly: Option<PathBuf>,
    /// Export privé (défaut : `~/code/perso/tribly-generator-bench/data/prod-network.csv.gz`).
    #[arg(long)]
    pub export: Option<PathBuf>,
    /// `{ id d'équipe : lettre }` (défaut : `G/lot2/team-letters.json`).
    #[arg(long)]
    pub letters: Option<PathBuf>,
    /// Sortie (défaut : `G/retouche/`).
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Surcouches d'équipe, `<id>-….ordo` (défaut : `G/lot6/overlays/`) ; lues par `retouche-play` seulement.
    #[arg(long)]
    pub overlays: Option<PathBuf>,
}

/// Arguments de `retouche-continuity`.
#[derive(Debug, Args)]
pub struct RetoucheContinuityArgs {
    #[command(flatten)]
    pub paths: PathOpts,
    /// Préfixe des fichiers de sortie : `<label>-continuity.{jsonl,json,md}`.
    #[arg(long, default_value = "continuity")]
    pub label: String,
    /// Équipes mesurées (lettres, séparées par des virgules) ; toutes par défaut.
    #[arg(long, value_delimiter = ',')]
    pub teams: Vec<String>,
    /// Au plus ce nombre de traces par équipe (les premières dans l'ordre de l'export).
    #[arg(long)]
    pub limit: Option<usize>,
    /// Marge continue exigée de chaque côté d'une couture, m (§2.3).
    #[arg(long, default_value_t = 200.0)]
    pub seam_margin_m: f64,
    /// Distance maximale d'un nœud à la polyligne du document pour la projection d'une couture, m (§2.4).
    #[arg(long, default_value_t = 15.0)]
    pub proj_tol_m: f64,
    /// `Touch` (contact à 5 m) ne coupe pas une plage continue (variante ; la référence est stricte).
    #[arg(long)]
    pub touch_continues: bool,
    /// Fils (défaut : `available_parallelism()`).
    #[arg(long)]
    pub threads: Option<usize>,
}

pub(crate) struct Paths {
    pub graph: PathBuf,
    pub road: PathBuf,
    pub gravel: PathBuf,
    pub poly: PathBuf,
    pub export: PathBuf,
    pub letters: PathBuf,
    pub out: PathBuf,
    pub overlays: PathBuf,
}

impl PathOpts {
    pub(crate) fn resolve(&self) -> Result<Paths> {
        let home =
            std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| anyhow!("HOME n'est pas défini"))?;
        let g = home.join(".cache/osm-route-details/graph");
        let lot6 = g.join("lot6");
        let pick = |a: &Option<PathBuf>, d: PathBuf| a.clone().unwrap_or(d);
        Ok(Paths {
            graph: pick(&self.graph, lot6.join("france-20261115-z11.ordg")),
            road: pick(&self.road, lot6.join("france-20261115-z11.road.ordv")),
            gravel: pick(&self.gravel, lot6.join("france-20261115-z11.gravel.ordv")),
            poly: pick(&self.poly, lot6.join("france.poly")),
            export: pick(
                &self.export,
                home.join("code/perso/tribly-generator-bench/data/prod-network.csv.gz"),
            ),
            letters: pick(&self.letters, g.join("lot2/team-letters.json")),
            out: pick(&self.out, g.join("retouche")),
            overlays: pick(&self.overlays, lot6.join("overlays")),
        })
    }
}

fn open(p: &std::path::Path) -> Result<MappedGraph> {
    MappedGraph::open_mmap(p).with_context(|| p.display().to_string())
}

pub fn run_continuity(a: &RetoucheContinuityArgs) -> Result<()> {
    let paths = a.paths.resolve()?;
    let letters = read_letters(&paths.letters)?;
    let wanted: Vec<u64> = letters
        .iter()
        .filter(|(_, l)| a.teams.is_empty() || a.teams.contains(l))
        .map(|(id, _)| *id)
        .collect();
    anyhow::ensure!(!wanted.is_empty(), "aucune équipe ne correspond à --teams");
    let threads =
        a.threads.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get())).max(1);
    let mut ph = Phases::new();

    // 1. Le graphe et ses vues.
    let gf = open(&paths.graph)?;
    let (road, gravel) = (open(&paths.road)?, open(&paths.gravel)?);
    let n_edges = gf.header().edges as usize;
    let views = [ViewInfo::build(&road, n_edges), ViewInfo::build(&gravel, n_edges)];
    ph.end("graphe et masques des vues");

    // 2. L'export.
    let ing = export::ingest(&paths.export, &wanted)?;
    let letter_of: Vec<String> = ing
        .teams
        .iter()
        .map(|id| {
            letters.get(id).cloned().ok_or_else(|| anyhow!("équipe sans lettre dans le fichier des lettres"))
        })
        .collect::<Result<_>>()?;
    ph.end("lecture de l'export");

    // 3. L'appariement des clés distinctes.
    let poly = Poly::read(&paths.poly).with_context(|| paths.poly.display().to_string())?;
    let seg = SegIndex::build(&gf);
    let jobs = export::match_keys(&gf, &seg, &poly, &ing);
    drop(seg);
    ph.end("appariement");

    // 4. Les mesures, trace par trace.
    let mut next = vec![0u32; ing.teams.len()];
    let mut todo: Vec<(usize, u32)> = Vec::new();
    for (idx, t) in ing.tracks.iter().enumerate() {
        let rank = next[t.team];
        next[t.team] += 1;
        if a.limit.is_none_or(|n| (rank as usize) < n) {
            todo.push((idx, rank));
        }
    }
    let ctx = Ctx {
        gf: &gf,
        ing: &ing,
        jobs: &jobs,
        views: [&views[0] as &dyn ViewLookup, &views[1] as &dyn ViewLookup],
        opts: SeamOpts { margin_m: a.seam_margin_m, touch_continues: a.touch_continues },
        proj_tol_m: a.proj_tol_m,
        retain: RetainParams::default(),
    };
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
    let outcomes: Vec<(String, Outcome)> = pool.install(|| {
        todo.par_iter()
            .map(|&(idx, rank)| {
                let letter = &letter_of[ing.tracks[idx].team];
                (letter.clone(), continuity::measure(&ctx, idx, rank, letter))
            })
            .collect()
    });
    ph.end("mesures");

    // 5. Les sorties.
    std::fs::create_dir_all(&paths.out)?;
    let jsonl = out::jsonl(&outcomes)?;
    let jsonl_xxh3 = xxh3_64(jsonl.as_bytes());
    let meta = vec![
        ("empreinte xxh3 de l'export".to_string(), format!("{:016x}", ing.export_xxh3)),
        ("empreinte xxh3 du jsonl".to_string(), format!("{jsonl_xxh3:016x}")),
        ("marge de couture".to_string(), format!("{} m", a.seam_margin_m)),
        ("tolérance de projection".to_string(), format!("{} m", a.proj_tol_m)),
        ("Touch continue une plage".to_string(), a.touch_continues.to_string()),
        (
            "portée".to_string(),
            format!(
                "{} ; {}",
                if a.teams.is_empty() {
                    "toutes les équipes".to_string()
                } else {
                    format!("équipes {}", a.teams.join(","))
                },
                a.limit.map_or("toutes les traces".to_string(), |n| format!("{n} traces par équipe au plus"))
            ),
        ),
        ("lignes de l'export lues".to_string(), todo.len().to_string()),
        ("fils".to_string(), threads.to_string()),
    ];
    let summary = out::summarise(meta, &outcomes, false);
    let base = |ext: &str| paths.out.join(format!("{}-continuity.{ext}", a.label));
    std::fs::write(base("jsonl"), &jsonl)?;
    std::fs::write(base("json"), serde_json::to_string_pretty(&summary)?)?;
    std::fs::write(base("md"), out::markdown(&summary))?;
    ph.end("sorties");
    eprintln!("sorties : {}", paths.out.display());
    Ok(())
}

pub use self::play::{run_play, RetouchePlayArgs};

/// Une valeur du curseur de familiarité, par la correspondance du service (`cursor::apply`).
#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    pub value: f64,
    applied: ord_server::generator::cursor::Applied,
}

impl Cursor {
    pub fn new(value: f64) -> Result<Self> {
        use ord_server::generator::cursor;
        let h = cursor::hundredths(value).ok_or_else(|| anyhow!("curseur hors de [0, 1] : {value}"))?;
        Ok(Self { value, applied: cursor::apply(h) })
    }

    /// La requête de la trace pour ce curseur ; le départ, l'arrivée et la distance sont posés par l'appelant.
    pub fn request(&self, p: &posed::Posed, seed: u64) -> ord_loops::Request {
        ord_loops::Request {
            start: ord_loops::LatLon::new(0.0, 0.0),
            end: None,
            bearing: None,
            distance_m: 0.0,
            ride_type: p.ride,
            mode: self.applied.mode,
            familiarity: self.applied.familiarity,
            off_team_cost_q8: self.applied.off_team_cost_q8,
            seed: p.id_hash ^ seed,
            figures: ord_loops::Figures::Classic,
        }
    }
}

/// Ce que les essais lisent pour une équipe.
pub struct Env<'a> {
    pub engine: &'a crate::likeness::play::Eng,
    pub wins: &'a crate::likeness::play::SharedWindows<'a>,
    pub team_key: u64,
    pub team: &'a ord_loops::TeamData,
    pub ctl: control::Ctl<'a>,
    pub seed: u64,
    pub margin_m: f64,
    pub max_windows: usize,
    pub max_controls: usize,
    pub no_keep_out: bool,
    /// Réglages de la retouche du moteur (`--impl engine`).
    pub tune: ord_loops::RetouchTune,
}
