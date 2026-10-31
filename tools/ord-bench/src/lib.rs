//! `ord-bench` (T05 §3, §5) : fait passer le service sur le corpus du banc et calcule les
//! métriques. Il ne recalcule aucune référence : il lit celles de `bench/`.
//!
//! `check` (empreintes du manifest, cohérence `criteria.yaml` ↔ README, Q1), `fill-cache` (cache
//! de banc local), `run --offline` (incrément 2 : E1 jugé, E2 indicatif, E2b jugé), `latency`
//! (L1 à froid, live) ; scénarios golden sur des ouvrages de calibration.
//! Les rapports n'écrivent **aucune coordonnée** (T05 §3.4).

pub mod campaign;
pub mod check;
pub mod coherence;
pub mod golden;
pub mod gpx;
pub mod hash;
pub mod manifest;
pub mod metrics;
pub mod privacy;
pub mod service;

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Emplacements du banc.
#[derive(Clone, Debug)]
pub struct BenchPaths {
    /// `bench/` du dépôt.
    pub bench: PathBuf,
    /// Cache de banc local, hors dépôt (T05 §5.3).
    pub cache: PathBuf,
    /// Traces personnelles (`bench/private/`, hors git) ; absentes en CI.
    pub private: PathBuf,
}

impl BenchPaths {
    pub fn new(bench: &Path, cache: Option<&Path>, private: Option<&Path>) -> Self {
        let cache = cache
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("ORD_BENCH_CACHE").map(PathBuf::from))
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
                home.join(".cache/osm-route-details/bench")
            });
        let private = private.map(Path::to_path_buf).unwrap_or_else(|| bench.join("private"));
        Self { bench: bench.to_path_buf(), cache, private }
    }

    /// `bench/` du dépôt, vu depuis ce crate.
    pub fn repo_bench() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench")
    }

    pub fn private_present(&self) -> bool {
        self.private.join("traces").is_dir()
    }

    /// Chemin d'un fichier du manifest : `private/…` est lu dans le dossier privé (déplaçable par
    /// `--private-dir`), le reste dans `bench/`.
    pub fn resolve(&self, fichier: &str) -> PathBuf {
        match fichier.strip_prefix("private/") {
            Some(rest) => self.private.join(rest),
            None => self.bench.join(fichier),
        }
    }
}

/// Révision de l'outil de mesure à `rules_version` égale : suffixe du dossier de rapport
/// (`<date>-<rules_version>-<révision>`), remis à `None` à la prochaine montée des règles.
/// `bench1` : tronçons recollés, segments `.sN`, E2b `non_discriminant`, diagnostics E1.
pub const BENCH_REVISION: Option<&str> = Some("bench1");

/// Règles de calcul non fixées par le banc (T05 §3.1) : constantes hachées dans `results.json`
/// (`bench_rules_sha256`) et gelées avec les seuils.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BenchRules {
    /// Point baro écarté au-delà de cette distance de son point apparié (bench §2.1, étape 3).
    pub match_max_m: f64,
    /// Grille curviligne commune du service et de la référence (bench §2.2, étape 3).
    pub grid_step_m: f64,
    /// Pas des points envoyés quand la géométrie recalée dépasse 16 000 sommets (T05 §3.4).
    pub request_resample_m: f64,
    /// Au-delà, deux points baro consécutifs ne sont pas interpolés (trou de référence).
    pub baro_gap_max_m: f64,
    /// Passage d'un ouvrage : trace à moins de cette distance de ses points de référence.
    pub passage_tol_m: f64,
    /// Demi-largeur de la fenêtre masquée autour d'un ouvrage d'une autre série.
    pub mask_half_m: f64,
    /// Retrait de la dérive lente : médiane glissante sur cette fenêtre (bench §2.2, étape 4).
    pub detrend_window_m: f64,
    /// Pas de calcul de cette médiane glissante, interpolée entre deux calculs.
    pub detrend_step_m: f64,
}

impl Default for BenchRules {
    fn default() -> Self {
        Self {
            match_max_m: 30.0,
            grid_step_m: 5.0,
            request_resample_m: 10.0,
            baro_gap_max_m: 100.0,
            passage_tol_m: 40.0,
            mask_half_m: 500.0,
            detrend_window_m: 5_000.0,
            detrend_step_m: 50.0,
        }
    }
}
