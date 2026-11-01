//! `ord-bench` (T05 §3, §5) : fait passer le service sur le corpus du banc et calcule les
//! métriques. Il ne recalcule aucune référence : il lit celles de `bench/`.
//!
//! `check` (empreintes du manifest, cohérence `criteria.yaml` ↔ README, Q1), `fill-cache` (cache
//! de banc local), `run --offline` (incrément 2 : E1 jugé, E2 indicatif, E2b jugé ; incrément 3 :
//! R1, R2, R3, M1, typologie à chaud ; incrément 4 : E3, E4, E5, S1, S2, S3, E1 hors ouvrage),
//! `latency`
//! (L1 à froid, live), `diagnose` (causes des plages M1 et R dans les tuiles du cache),
//! `diagnose-structures` (groupes d'ouvrages rejoués étape par étape), `dump-profils` (vidage
//! privé des profils de traces de calibration, `bench/private/profils/`) ; scénarios golden sur des ouvrages de calibration.
//! Les rapports n'écrivent **aucune coordonnée** (T05 §3.4).

pub mod campaign;
pub mod check;
pub mod coherence;
pub mod diagnose;
pub mod dump;
pub mod golden;
pub mod gpx;
pub mod hash;
pub mod inserted;
pub mod judge;
pub mod manifest;
pub mod metrics;
pub mod off_network;
pub mod osm_surface;
pub mod ouvrages;
pub mod privacy;
pub mod report_check;
pub mod service;
pub mod structures_diag;
pub mod typology;

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
/// Remise à `None` avec les règles 2026.10.4 (incrément 4 : métriques d'ouvrage).
/// `bench1` (règles 2026.10.6) : périmètre lu sur la voie de l'ouvrage, détail des bandes de
/// portail retenues, erreur des ouvrages hors graphe contre leur référence. `bench2` : clés
/// privées (`_…`) retirées à tous les niveaux du rapport. `bench3` (README §4 bis complété en
/// 657d2c8) : balcon de moins de 120 m entre tunnels OSM, contrôle `hors_graphe_velo` satisfait
/// par une voie parallèle du graphe ; un passage par traversée de l'ouvrage ; `limite_donnees` sur
/// toute galerie `avalanche_protector` du corpus. Remise à `None` avec les règles 2026.10.7. `bench1`
/// (règles 2026.10.7, README §4 bis complété en 022710f et 4304838) : décalage local E3 sur les
/// approches, référence incertaine, bande sur un ouvrage des tuiles, plausibilité E4 (25 %) ;
/// passages des ouvrages de calibration dans les traces tenues à l'écart (extraits). `bench2`
/// (e48170f) : passage en hauteur sur un pont (test de niveau) ; repli 1–3 km des extraits lu sur la
/// trace (baro − MNT), stations par côté rapportées ; `diagnose-structures` sur les extraits.
/// Remise à `None` avec les règles 2026.10.8. `bench1` (règles 2026.10.8) : géométries et baro
/// recalés régénérés par le banc en 74cdd88 (raccords de tronçons Stadia), outil inchangé. `bench2`
/// (règles 2026.10.8, banc 53225a8 et adbf6f1) : baro des points insérés gardé à 30 m d'une voie
/// des tuiles de la campagne ; recalages régénérés (colonne `insere`, pas de baro dans les trous de
/// la trace brute). Remise à `None` avec les règles 2026.10.9 (incrément 5 : points de rupture ;
/// campagne dans les deux modes, `distance` et `input_with_breakpoints`). `bench1` (règles
/// 2026.10.9) : en mode `input_with_breakpoints`, références des ouvrages (MNT des approches E3,
/// test de niveau, E5, bandes de portail E4, repli 1–3 km) et positions des passages lues en
/// mode `distance` ; seule la sortie testée vient du mode `input_with_breakpoints` (E2b garde
/// la sortie non corrigée de ce mode). `bench2` (règles 2026.10.9, banc 405f956, criteria.yaml
/// `preset_baro: dem`, README §4 bis.5) : D+ baro d'E2b au preset `dem` (`E2b.dplus_baro_dem_m`,
/// `E2b.ratio*`), le même que le service et sa référence sans correction ; E2 reste en
/// `barometric`. `bench3` (règles 2026.10.9, incrément 6) : campagne paramétrée par série
/// (`run_series`, pour `judge`), `increment_plan` 6 ; mesures de calibration inchangées ; service
/// avec admission, budget et annulation (sans effet sur les sorties) ; `latency --profile warm`
/// (L2). Remise à `None` avec les règles 2026.10.10 (tunnels OSM en série : balcons de moins de
/// 120 m, droite entre les portails extérieurs de la série) ; règles du banc du 2026-11-01
/// (README §4 bis.4, 6 et 7 : limite des données hors décompte d'E4/E3 et S1, M1 hors réseau,
/// R3 non bloquant) appliquées dans la même révision. `bench1` (règles 2026.10.10, README §4 ter,
/// règles V1 du 2026-11-01 : 304f767, 16f6097, c1d231c) : S1 en recouvrement continu de
/// [s_A, s_B] (tolérance de 2,5 m), attribution d'un passage par la trace brute
/// (`passage_sur_ouvrage_parallele`), M1 hors graphe vélo avec contrôle `matched: false` +
/// `unmatched_range` ; campagne `increment` V1. Remise à `None` avec les règles 2026.11.1 (état
/// « hors graphe » rendu avec la typologie de la route fermée suivie, api 1.1). `bench1` (règles
/// 2026.11.3, README §4 ter après la calibration 2026.11.2 : ec72f13, 09dcda0, 0b590ff) : M1 hors
/// graphe vélo resserrée (routes motorisées, plages ≥ 50 m, rails exclus), S2 sans les plages hors
/// graphe vélo des témoins, attribution « un tablier, un ouvrage » (couche `bridges` des tuiles).
/// Remise à `None` avec les règles 2026.11.4 (galeries couvertes détectées sur le MNT).
/// Reste à `None` avec les règles 2026.11.5 (couvertures en altitude
/// seule) ; README §4 ter.6 précisé en eb4c4cc (`covered=yes` lu sur la voie des stations du témoin)
/// appliqué dans la même révision.
pub const BENCH_REVISION: Option<&str> = None;

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
    /// Point inséré (`insere = 1`) : baro gardé à cette distance au plus d'une ligne de la couche
    /// `streets` (tuile et 8 voisines, cache de la campagne ; bench §2.1, étape 3).
    pub inserted_street_max_m: f64,
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
            inserted_street_max_m: 30.0,
            passage_tol_m: 40.0,
            mask_half_m: 500.0,
            detrend_window_m: 5_000.0,
            detrend_step_m: 50.0,
        }
    }
}
