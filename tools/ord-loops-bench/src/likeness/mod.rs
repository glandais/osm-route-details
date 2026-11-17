//! Banc de vraisemblance du lot 4 (4a) : les formes des parcours générés contre celles des traces réelles de
//! l'export, et la reconstruction d'une trace mise à l'écart (note : `docs/design/technique/ord-loops-lot4.md` §2 et
//! §11).
//!
//! **Privé** : l'export, les départs, les coordonnées et les ids de trace ou d'équipe sont des données d'équipes
//! réelles. Les sorties (rapport JSON, résumé) sont écrites sous `~/.cache/osm-route-details/graph/lot4/` et nulle
//! part ailleurs ; les équipes y sont lettrées, une boucle y est désignée par `(lettre, i)`, et seuls des agrégats et
//! des empreintes en sortent.

// Retiré quand le dernier module est branché.
#![allow(dead_code)]

pub mod export;
pub mod shape;

use std::path::PathBuf;

use anyhow::{anyhow, Context as _, Result};
use clap::Args;

/// Arguments de `likeness`. Les défauts sont sous `~/.cache/osm-route-details/graph/` (noté `G`).
#[derive(Debug, Args)]
pub struct LikenessArgs {
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
    /// Dossier des surcouches du lot 6, pour le seul contrôle d'égalité des surcouches reconstruites (défaut :
    /// `G/lot6/overlays` ; absent : contrôle sauté).
    #[arg(long)]
    pub overlays: Option<PathBuf>,
    /// Sortie : `<label>-report.json` et `<label>-summary.md` (défaut : `G/lot4/`).
    #[arg(long)]
    pub out: Option<PathBuf>,
    #[arg(long, default_value = "likeness")]
    pub label: String,
    /// Valeurs du curseur jouées, par `ord_server::generator::cursor::{hundredths, apply}`.
    #[arg(long, value_delimiter = ',', default_values_t = [0.25, 1.0])]
    pub cursors: Vec<f64>,
    /// Équipes jouées (lettres, séparées par des virgules) ; toutes par défaut.
    #[arg(long, value_delimiter = ',')]
    pub teams: Vec<String>,
    /// Au plus ce nombre de boucles retenues par équipe (les premières dans l'ordre de l'export).
    #[arg(long)]
    pub limit: Option<usize>,
    /// Seulement les boucles de D au plus ce nombre de km.
    #[arg(long)]
    pub max_km: Option<f64>,
    /// D minimale d'une boucle retenue, km.
    #[arg(long, default_value_t = 5.0)]
    pub min_km: f64,
    /// L'arrivée est à moins de cette part de D du départ.
    #[arg(long, default_value_t = 0.05)]
    pub loop_gap: f64,
    /// Part appariée minimale d'une trace.
    #[arg(long, default_value_t = 0.95)]
    pub min_matched: f64,
    /// Plus long trou admis dans la suite appariée, m.
    #[arg(long, default_value_t = 1000.0)]
    pub max_hole_m: f64,
    /// Part des boucles mises à l'écart, par équipe.
    #[arg(long, default_value_t = 0.10)]
    pub holdout_share: f64,
    /// Nombre minimal de boucles retenues d'une équipe pour en mettre à l'écart.
    #[arg(long, default_value_t = 10)]
    pub holdout_min_loops: usize,
    /// Saute la reconstruction.
    #[arg(long)]
    pub no_holdout: bool,
    /// Graine de base : la graine d'une requête est `empreinte de la trace ^ graine`.
    #[arg(long, default_value_t = 20_261_117)]
    pub seed: u64,
    /// Fils de génération (défaut : `available_parallelism()`).
    #[arg(long)]
    pub threads: Option<usize>,
    /// Budget du cache de fenêtres partagé, en Mo.
    #[arg(long, default_value_t = 6000)]
    pub budget_mb: usize,
    /// Pas d'arrondi du rayon de classe des fenêtres, m.
    #[arg(long, default_value_t = 10_000.0)]
    pub radius_step_m: f64,
    /// Ne fait que l'étalonnage `shapes.py` (sans graphe) et s'arrête.
    #[arg(long)]
    pub calibrate_only: bool,
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| anyhow!("HOME n'est pas défini"))
}

/// Les chemins résolus.
#[derive(Debug)]
pub struct Paths {
    pub graph: PathBuf,
    pub road: PathBuf,
    pub gravel: PathBuf,
    pub poly: PathBuf,
    pub export: PathBuf,
    pub letters: PathBuf,
    pub overlays: Option<PathBuf>,
    pub out: PathBuf,
}

impl LikenessArgs {
    pub fn paths(&self) -> Result<Paths> {
        let home = home()?;
        let g = home.join(".cache/osm-route-details/graph");
        let lot6 = g.join("lot6");
        let pick = |a: &Option<PathBuf>, d: PathBuf| a.clone().unwrap_or(d);
        let overlays = match &self.overlays {
            Some(p) => Some(p.clone()),
            None => Some(lot6.join("overlays")).filter(|p| p.is_dir()),
        };
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
            overlays,
            out: pick(&self.out, g.join("lot4")),
        })
    }
}

/// `{ id d'équipe : lettre }`.
pub fn read_letters(path: &std::path::Path) -> Result<std::collections::BTreeMap<u64, String>> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    let raw: std::collections::BTreeMap<String, String> = serde_json::from_str(&text)?;
    raw.into_iter()
        .map(|(k, v)| {
            Ok((
                k.parse::<u64>().map_err(|_| anyhow!("id d'équipe illisible dans le fichier des lettres"))?,
                v,
            ))
        })
        .collect()
}

pub fn run(a: &LikenessArgs) -> Result<()> {
    let paths = a.paths()?;
    if a.calibrate_only {
        return calibrate_only(a, &paths);
    }
    Err(anyhow!("likeness : la campagne complète n'est pas encore branchée (étapes suivantes du lot 4a)"))
}

/// `--calibrate-only` : `shapes.py` en entier sur l'export brut, sans graphe, au format de `shapes.py`.
fn calibrate_only(a: &LikenessArgs, p: &Paths) -> Result<()> {
    let t = std::time::Instant::now();
    let ing = export::ingest(&p.export, &[])?;
    let rows = export::raw_legacy(&ing);
    let lines = shape::legacy_lines(&rows);
    let text = lines.join("\n") + "\n";
    std::fs::create_dir_all(&p.out)?;
    let path = p.out.join(format!("{}-calibration.txt", a.label));
    std::fs::write(&path, &text)?;
    print!("{text}");
    eprintln!("étalonnage : {} ({:.1} s)", path.display(), t.elapsed().as_secs_f64());
    Ok(())
}
