//! Configuration (T07) : défauts compilés < fichier TOML < variables `ORD_<SECTION>__<CLÉ>`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use figment::providers::{Env, Format, Serialized, Toml};
use figment::Figment;
use serde::{Deserialize, Serialize};

/// Variable qui désigne le fichier de configuration (équivalent de `--config`).
pub const CONFIG_ENV: &str = "ORD_CONFIG";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub server: ServerConfig,
    pub limits: LimitsConfig,
    pub budget: BudgetConfig,
    pub typology: TypologySourceConfig,
    pub elevation: ElevationSourcesConfig,
    pub coverage: CoverageConfig,
    pub cache: CacheConfig,
    pub http_client: HttpClientConfig,
    pub log: LogConfig,
    pub generator: GeneratorConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub listen_addr: SocketAddr,
    /// Port interne : `/metrics`, `/openapi.json`.
    pub metrics_addr: SocketAddr,
    pub shutdown_grace_s: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::from(([0, 0, 0, 0], 8080)),
            metrics_addr: SocketAddr::from(([0, 0, 0, 0], 9090)),
            shutdown_grace_s: 90,
        }
    }
}

/// Bornes d'entrée seulement (doc 02 §7).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LimitsConfig {
    pub max_points: usize,
    pub max_length_m: f64,
    pub max_body_bytes: usize,
    pub debug_allowed: bool,
    pub options: OptionLimits,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_points: 16_000,
            max_length_m: 200_000.0,
            max_body_bytes: 8 * 1024 * 1024,
            debug_allowed: false,
            options: OptionLimits::default(),
        }
    }
}

/// Bornes publiées des options (T07 §2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OptionLimits {
    pub step_m_min: f64,
    pub step_m_max: f64,
    pub step_m_default: f64,
    /// Le lissage vaut 0 (désactivé) ou est compris dans [min ; max].
    pub smoothing_m_min: f64,
    pub smoothing_m_max: f64,
    pub smoothing_m_default: f64,
}

impl Default for OptionLimits {
    fn default() -> Self {
        Self {
            step_m_min: 5.0,
            step_m_max: 1_000.0,
            step_m_default: 25.0,
            smoothing_m_min: 10.0,
            smoothing_m_max: 1_000.0,
            smoothing_m_default: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BudgetConfig {
    pub request_budget_s: u64,
    /// `None` : nombre de CPU.
    pub max_in_flight: Option<usize>,
    /// `None` : 2 × `max_in_flight`.
    pub max_queued: Option<usize>,
    /// `None` : nombre de CPU.
    pub compute_permits: Option<usize>,
    /// File du profil `fast` (api 1.5), distincte de celle du profil `full`. `None` : comme
    /// `max_in_flight`.
    pub fast_max_in_flight: Option<usize>,
    /// `None` : 2 × `fast_max_in_flight`.
    pub fast_max_queued: Option<usize>,
    pub per_request_fetch_concurrency: usize,
    pub overloaded_retry_after_s: u64,
    pub retry_after_min_s: u64,
    pub retry_after_max_s: u64,
    pub retry_after_default_s: u64,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            request_budget_s: 90,
            max_in_flight: None,
            max_queued: None,
            compute_permits: None,
            fast_max_in_flight: None,
            fast_max_queued: None,
            per_request_fetch_concurrency: 10,
            overloaded_retry_after_s: 2,
            retry_after_min_s: 5,
            retry_after_max_s: 120,
            retry_after_default_s: 30,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypologySourceKind {
    VersatilesHttp,
    VersatilesFile,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TypologySourceConfig {
    pub source: TypologySourceKind,
    pub url: String,
    pub path: Option<PathBuf>,
    pub zoom: u8,
    pub concurrency: usize,
}

impl Default for TypologySourceConfig {
    fn default() -> Self {
        Self {
            source: TypologySourceKind::VersatilesHttp,
            url: "https://tiles.versatiles.org/tiles/osm/{z}/{x}/{y}".into(),
            path: None,
            zoom: 14,
            concurrency: 8,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ElevationSourcesConfig {
    pub ign: IgnConfig,
    pub mapterhorn: MapterhornConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IgnMode {
    Direct,
    Sober,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IgnConfig {
    pub enabled: bool,
    pub wms_url: String,
    pub dtm_layer: String,
    pub dsm_layer: String,
    pub zoom: u8,
    /// Cadence du processus entier (une seule instance, T07 §3).
    pub rate_per_s: f64,
    pub max_attempts: u32,
    pub mode: IgnMode,
    /// Requêtes en vol vers l'hôte (≈ débit × latence, T02 §4.2) [à calibrer].
    pub concurrency: usize,
}

impl Default for IgnConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            wms_url: "https://data.geopf.fr/wms-r/wms".into(),
            dtm_layer: "IGNF_LIDAR-HD_MNT_ELEVATION.ELEVATIONGRIDCOVERAGE.WGS84G".into(),
            dsm_layer: "IGNF_LIDAR-HD_MNS_ELEVATION.ELEVATIONGRIDCOVERAGE.WGS84G".into(),
            zoom: 15,
            rate_per_s: 20.0,
            max_attempts: 6,
            mode: IgnMode::Direct,
            concurrency: 12,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MapterhornConfig {
    pub url: String,
    pub zoom: u8,
    /// Côté des tuiles Terrarium, en pixels (512 chez Mapterhorn).
    pub tile_size: u16,
    pub concurrency: usize,
    /// Fixe la version (espace de noms du cache disque) ; `None` : version lue au démarrage.
    pub version: Option<String>,
    /// TileJSON lu au démarrage (champ `version` s'il existe, T02 §9).
    pub tilejson_url: String,
    /// Liste des téléchargements, dont le champ `version` sert quand le TileJSON n'en a pas.
    pub versions_url: String,
}

impl Default for MapterhornConfig {
    fn default() -> Self {
        Self {
            url: "https://tiles.mapterhorn.com/{z}/{x}/{y}.webp".into(),
            zoom: 12,
            tile_size: 512,
            concurrency: 4,
            version: None,
            tilejson_url: "https://tiles.mapterhorn.com/tilejson.json".into(),
            versions_url: "https://download.mapterhorn.com/download_urls.json".into(),
        }
    }
}

/// Fichiers de couverture de Mapterhorn (T02 §7.3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoverageConfig {
    /// `coverage-index.pmtiles` local ; sinon téléchargé depuis `coverage_index_url`.
    pub coverage_index: Option<PathBuf>,
    /// `coverage.pmtiles` local : pas encore lu (polygones lus en MVT par `polygons_url`).
    pub coverage: Option<PathBuf>,
    /// `None` : pas d'index (`DemKind::Unknown` hors des 404 à z13).
    pub coverage_index_url: Option<String>,
    /// Polygones `coverage` en MVT `{z}/{x}/{y}` ; `None` : index seul.
    pub polygons_url: Option<String>,
    pub polygons_zoom: u8,
    pub polygons_concurrency: usize,
}

impl Default for CoverageConfig {
    fn default() -> Self {
        Self {
            coverage_index: None,
            coverage: None,
            coverage_index_url: Some("https://download.mapterhorn.com/coverage-index.pmtiles".into()),
            polygons_url: Some("https://single-archive-tiles.mapterhorn.com/coverage/{z}/{x}/{y}.mvt".into()),
            polygons_zoom: 12,
            polygons_concurrency: 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CacheConfig {
    pub dir: PathBuf,
    pub disk_max_gb_ign: u64,
    pub ttl_streets_days: u64,
    pub ttl_ign_nodata_days: u64,
    pub negative_ttl_s: u64,
    pub memory_elevation_mb: u64,
    pub memory_streets_mb: u64,
    pub offline: bool,
    /// Tuiles de rues lues sur disque seulement, entrées périmées comprises, sans revalidation
    /// (campagne de tuiles figée du banc) ; les sources d'altitude restent en ligne. `offline`
    /// l'implique.
    pub offline_streets: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("/var/cache/osm-route-details"),
            disk_max_gb_ign: 20,
            ttl_streets_days: 7,
            ttl_ign_nodata_days: 30,
            negative_ttl_s: 60,
            memory_elevation_mb: 512,
            memory_streets_mb: 512,
            offline: false,
            offline_streets: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpClientConfig {
    pub user_agent: String,
    pub connect_timeout_s: u64,
    pub fetch_timeout_s: u64,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            user_agent: concat!(
                "osm-route-details/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/glandais)"
            )
            .into(),
            connect_timeout_s: 4,
            fetch_timeout_s: 8,
        }
    }
}

/// Le générateur de parcours (lot 3a) : graphe, vues, polygone, surcouches d'équipe. **Aucun secret** : cette section
/// est journalisée au démarrage. Sans `graph`, les routes `/v1/generator/*` rendent 503 et le reste du service ne change pas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneratorConfig {
    /// Graphe de base `.ordg` (lot 0) ; `None` : générateur désactivé.
    pub graph: Option<PathBuf>,
    /// Vues `road` et `gravel` du même graphe (`.ordv`).
    pub view_road: Option<PathBuf>,
    pub view_gravel: Option<PathBuf>,
    /// Polygone de couverture `.poly` (Geofabrik).
    pub coverage_poly: Option<PathBuf>,
    /// Dossier des surcouches chiffrées : `graph-<id>/<espace>/<équipe>.ordo.enc`.
    pub overlay_dir: PathBuf,
    /// Générations en calcul en même temps (CPU) et en attente au plus ; au-delà, 503 `overloaded`.
    pub max_concurrent: usize,
    pub max_queued: usize,
    /// Budget d'une génération, attente comprise.
    pub budget_ms: u64,
    pub distance_m_min: f64,
    pub distance_m_max: f64,
    /// Équipes gardées en mémoire au plus (plafond en nombre) et leur plafond en octets (Mo) : leurs octets (surcouche,
    /// données, tables du contrôleur) comptent dans `memory_budget_mb`, mais **aucune réservation ne les évince**
    /// (finition 2 du lot 6, §20.6) ; seuls ces deux plafonds retirent la moins récemment utilisée des entrées
    /// inactives, qui se reconstruit à la demande. Une entrée tenue par une génération n'est jamais retirée.
    pub team_cache_entries: usize,
    pub team_cache_mb: usize,
    /// Budget de mémoire du générateur (Mo) : fenêtres, agrégats d'équipe par fenêtre, espaces de travail, mémoire
    /// transitoire des constructions d'équipe **et équipes résidentes**. Le moteur seul tient en quelques Mo ; ce budget
    /// borne ce qui varie avec les requêtes (lot 6, §10.1). 800 : une construction d'équipe creuse réserve moins de
    /// 100 Mo (finition 2, §20.9) ; le plancher est les équipes plus deux générations de 200 km en gravel dense.
    pub memory_budget_mb: usize,
    /// Marge d'arrondi (m) ajoutée aux zones d'une fenêtre.
    pub window_margin_m: f64,
    /// Pas d'arrondi (m) du rayon de classe d'une fenêtre : plus il est grand, plus une fenêtre sert de requêtes.
    pub window_radius_step_m: f64,
    /// Un espace de travail rendu à la réserve qui tient plus que cela (Mo) est libéré.
    pub workspace_keep_mb: usize,
    /// Fils du calcul des index globaux à l'ouverture (une vue par fil au plus).
    pub index_threads: usize,
    /// Une surcouche non redéposée depuis ce nombre de jours est purgée.
    pub overlay_ttl_days: u64,
    /// Un dossier d'un autre graphe est purgé quand son dernier fichier a plus de ces heures.
    pub stale_grace_h: u64,
    /// Période de la purge, en secondes.
    pub purge_interval_s: u64,
    pub storage: GeneratorStorageConfig,
    pub deposit: GeneratorDepositConfig,
    /// Profil de goût d'équipe appris au dépôt (lot 4, 3b).
    pub profile: GeneratorProfileConfig,
    /// Figures de la génération quand la requête n'en nomme pas (lot 4, 4c) : `organic` (défaut depuis 1.12) ou `classic`
    /// (la lentille seule, le moteur de 1.10 au bit près).
    pub figures: GeneratorFigures,
}

/// Les figures que le moteur essaie : la lentille seule (1.10), ou les figures organiques du 4c (lentille au couloir et
/// triangle), le défaut depuis 1.12. Sans effet en strict ni pour un aller simple ; le MTB n'a jamais de triangle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratorFigures {
    Classic,
    #[default]
    Organic,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            graph: None,
            view_road: None,
            view_gravel: None,
            coverage_poly: None,
            overlay_dir: PathBuf::from("/var/cache/osm-route-details/generator"),
            max_concurrent: 2,
            max_queued: 8,
            budget_ms: 8_000,
            distance_m_min: 5_000.0,
            distance_m_max: 200_000.0,
            team_cache_entries: 64,
            team_cache_mb: 96,
            memory_budget_mb: 800,
            window_margin_m: 1_000.0,
            window_radius_step_m: 10_000.0,
            workspace_keep_mb: 160,
            index_threads: 2,
            overlay_ttl_days: 14,
            stale_grace_h: 48,
            purge_interval_s: 3_600,
            storage: GeneratorStorageConfig::default(),
            deposit: GeneratorDepositConfig::default(),
            profile: GeneratorProfileConfig::default(),
            figures: GeneratorFigures::Organic,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneratorStorageConfig {
    pub max_namespaces: usize,
    pub max_overlays_per_namespace: usize,
    /// Octets au plus **par espace** ; `max_namespaces × max_namespace_mb` ne dépasse pas `max_total_mb` (le budget du
    /// volume), de sorte que le quota d'un espace ne dépend jamais de ce que les autres stockent.
    pub max_namespace_mb: u64,
    pub max_total_mb: u64,
    /// Préfixes `ns8` (8 hexadécimaux, affichés dans les journaux au premier dépôt) autorisés à créer un espace ; vide :
    /// n'importe quelle clé, jusqu'à `max_namespaces`. Une liste de quelques valeurs que l'opérateur maîtrise : un
    /// inconnu sur le réseau partagé ne peut alors prendre aucune place.
    pub allowed_namespaces: Vec<String>,
}

impl Default for GeneratorStorageConfig {
    fn default() -> Self {
        Self {
            max_namespaces: 16,
            max_overlays_per_namespace: 2_000,
            max_namespace_mb: 64,
            max_total_mb: 1024,
            allowed_namespaces: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneratorDepositConfig {
    /// Dépôts en cours dans tout le service (l'index et le fil sont partagés).
    pub max_concurrent: usize,
    /// Attente d'une place de dépôt (file équitable, première arrivée première servie) avant 503 `overloaded`.
    pub queue_wait_s: u64,
    pub budget_s: u64,
    /// Sans octet reçu pendant ce délai, le dépôt est abandonné.
    pub idle_timeout_s: u64,
    /// Corps décompressé, document d'une trace, nombre de traces.
    pub max_body_bytes: u64,
    pub max_document_bytes: usize,
    pub max_tracks: u64,
    /// Géométries mémorisées pendant un dépôt, et mémoire de l'index spatial par cellule.
    pub memo_entries: usize,
    pub index_cache_mb: usize,
}

impl Default for GeneratorDepositConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 1,
            queue_wait_s: 120,
            budget_s: 900,
            idle_timeout_s: 30,
            max_body_bytes: 2 * 1024 * 1024 * 1024,
            max_document_bytes: 16 * 1024 * 1024,
            max_tracks: 20_000,
            memo_entries: 400_000,
            index_cache_mb: 128,
        }
    }
}

/// Borne haute de `generator.profile.max_loops` (décision 7 du 17 novembre 2026) : la mémoire et le temps de
/// l'apprentissage croissent avec le plafond, et le défaut est 500.
pub const PROFILE_MAX_LOOPS_BOUND: usize = 5_000;

/// Le profil de goût d'une équipe, appris au dépôt en rejouant le moteur sur ses boucles (3b, §14.7.2). La
/// normalisation n'est pas un réglage : elle va avec la table commune du moteur.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneratorProfileConfig {
    /// `false` : ni collecteur, ni profil écrit, ni lu ; l'état n'est jamais `stale` pour la raison `profile`.
    pub enabled: bool,
    /// Fils du joueur (1 à 16).
    pub threads: usize,
    /// Phases de profil en cours dans tout le service, tous environnements confondus.
    pub max_concurrent: usize,
    /// Budget de la phase, secondes : sous celui du dépôt.
    pub budget_s: u64,
    /// Plafond des boucles d'apprentissage.
    pub max_loops: usize,
    /// Le plafond s'applique par type (ROAD, GRAVEL, MTB) au lieu de par équipe.
    pub per_type: bool,
    /// Forme du mélange avec la table commune. Elle entre dans l'empreinte des réglages d'apprentissage : la changer rend
    /// les profils écrits `incompatibles` (`stale`, raison `profile`) jusqu'au prochain dépôt.
    pub mix: ProfileMix,
    /// Figures jouées pour apprendre un profil (lot 4, 4c) : `organic` (défaut depuis 1.12, celles que sert le service)
    /// entre dans l'empreinte des réglages, si bien qu'un profil appris en `classic` (1.10, 1.11) est `incompatible`
    /// (`stale`, raison `profile`) jusqu'au prochain dépôt ; `classic` garde l'empreinte de 1.10.
    pub learn_figures: GeneratorFigures,
}

/// Forme du mélange d'un profil d'équipe avec la table commune (décision 2 du 17 novembre 2026).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileMix {
    /// L'équipe part de la table commune et corrige par `g^w`, `w` plafonné à `N / (N + n0)` : le 3b tel que codé.
    #[default]
    Capped,
    /// Le mélange de la seconde sonde : départ à la table neutre, mélange à chaque pas avec la cible commune de ce pas,
    /// poids nu `G_T / (G_T + n0 G_P / N_P)`.
    Sonde,
}

impl Default for GeneratorProfileConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threads: 4,
            max_concurrent: 1,
            budget_s: 720,
            max_loops: 500,
            per_type: false,
            mix: ProfileMix::Capped,
            learn_figures: GeneratorFigures::Organic,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    Json,
    Text,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogConfig {
    pub format: LogFormat,
    /// Filtre `tracing` ; `RUST_LOG`, s'il est défini, l'emporte.
    pub filter: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self { format: LogFormat::Json, filter: "info".into() }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error(transparent)]
    Load(#[from] Box<figment::Error>),
    #[error("invalid configuration: {key}: {message}")]
    Invalid { key: &'static str, message: String },
}

impl Config {
    /// Charge la configuration ; `file` (ou `ORD_CONFIG`) est facultatif.
    pub fn load(file: Option<&Path>) -> Result<Self, ConfigError> {
        let file = file.map(Path::to_path_buf).or_else(|| std::env::var_os(CONFIG_ENV).map(PathBuf::from));
        Self::figment(file.as_deref())
            .extract::<Config>()
            .map_err(|e| ConfigError::Load(Box::new(e)))?
            .validated()
    }

    pub fn figment(file: Option<&Path>) -> Figment {
        let mut figment = Figment::from(Serialized::defaults(Config::default()));
        if let Some(path) = file {
            figment = figment.merge(Toml::file_exact(path));
        }
        figment.merge(Env::prefixed("ORD_").ignore(&["config"]).split("__"))
    }

    pub fn validated(self) -> Result<Self, ConfigError> {
        self.validate()?;
        Ok(self)
    }

    /// Cohérence des valeurs ; l'erreur porte le chemin de la clé.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |key, message: &str| Err(ConfigError::Invalid { key, message: message.into() });
        let l = &self.limits;
        let o = &l.options;
        if l.max_points < 2 {
            return invalid("limits.max_points", "must be at least 2");
        }
        if !(l.max_length_m.is_finite() && l.max_length_m > 0.0) {
            return invalid("limits.max_length_m", "must be a positive number");
        }
        if l.max_body_bytes == 0 {
            return invalid("limits.max_body_bytes", "must be positive");
        }
        if !(o.step_m_min > 0.0 && o.step_m_min <= o.step_m_max) {
            return invalid("limits.options.step_m_min", "must be positive and not above step_m_max");
        }
        if !(o.step_m_min..=o.step_m_max).contains(&o.step_m_default) {
            return invalid("limits.options.step_m_default", "must lie within [step_m_min, step_m_max]");
        }
        if !(o.smoothing_m_min > 0.0 && o.smoothing_m_min <= o.smoothing_m_max) {
            return invalid(
                "limits.options.smoothing_m_min",
                "must be positive and not above smoothing_m_max",
            );
        }
        let s = o.smoothing_m_default;
        if s != 0.0 && !(o.smoothing_m_min..=o.smoothing_m_max).contains(&s) {
            return invalid(
                "limits.options.smoothing_m_default",
                "must be 0 or lie within [smoothing_m_min, smoothing_m_max]",
            );
        }
        if self.budget.request_budget_s == 0 {
            return invalid("budget.request_budget_s", "must be positive");
        }
        if self.budget.max_in_flight == Some(0) {
            return invalid("budget.max_in_flight", "must be positive");
        }
        if self.budget.compute_permits == Some(0) {
            return invalid("budget.compute_permits", "must be positive");
        }
        if self.budget.overloaded_retry_after_s == 0 {
            return invalid("budget.overloaded_retry_after_s", "must be positive");
        }
        if self.budget.retry_after_min_s > self.budget.retry_after_max_s {
            return invalid("budget.retry_after_min_s", "must not be above retry_after_max_s");
        }
        if self.elevation.ign.rate_per_s.is_nan() || self.elevation.ign.rate_per_s <= 0.0 {
            return invalid("elevation.ign.rate_per_s", "must be positive");
        }
        if self.elevation.ign.concurrency == 0 {
            return invalid("elevation.ign.concurrency", "must be positive");
        }
        if !(1..=14).contains(&self.coverage.polygons_zoom) || self.coverage.polygons_concurrency == 0 {
            return invalid("coverage.polygons_zoom", "zoom within [1, 14] and positive concurrency");
        }
        if self.typology.concurrency == 0 || self.typology.zoom != 14 {
            return invalid("typology", "zoom must be 14 (Shortbread) and concurrency positive");
        }
        if self.typology.source == TypologySourceKind::VersatilesFile && self.typology.path.is_none() {
            return invalid("typology.path", "required when typology.source = versatiles_file");
        }
        let m = &self.elevation.mapterhorn;
        if m.zoom > 15 {
            return invalid("elevation.mapterhorn.zoom", "must not be above 15");
        }
        if m.tile_size == 0 || m.concurrency == 0 {
            return invalid("elevation.mapterhorn", "tile_size and concurrency must be positive");
        }
        if self.http_client.user_agent.trim().is_empty() {
            return invalid("http_client.user_agent", "must not be empty");
        }
        let g = &self.generator;
        if g.max_concurrent == 0 || g.budget_ms == 0 || g.team_cache_entries == 0 {
            return invalid("generator", "max_concurrent, budget_ms and team_cache_entries must be positive");
        }
        if !(g.distance_m_min > 0.0 && g.distance_m_min <= g.distance_m_max && g.distance_m_max.is_finite()) {
            return invalid("generator.distance_m_min", "must be positive and not above distance_m_max");
        }
        if g.team_cache_mb == 0 || g.team_cache_mb >= g.memory_budget_mb / 2 {
            return invalid(
                "generator.team_cache_mb",
                "team_cache_mb must be positive and below memory_budget_mb / 2",
            );
        }
        if g.memory_budget_mb == 0 || g.index_threads == 0 {
            return invalid(
                "generator.memory_budget_mb",
                "memory_budget_mb and index_threads must be positive",
            );
        }
        if !(g.window_margin_m >= 0.0 && g.window_margin_m.is_finite())
            || !(g.window_radius_step_m > 0.0 && g.window_radius_step_m.is_finite())
        {
            return invalid(
                "generator.window_margin_m",
                "margin must be non-negative and the radius step positive",
            );
        }
        if g.overlay_ttl_days == 0 || g.purge_interval_s == 0 {
            return invalid("generator.overlay_ttl_days", "ttl and purge interval must be positive");
        }
        let d = &g.deposit;
        if d.max_concurrent == 0 || d.budget_s == 0 || d.idle_timeout_s == 0 || d.memo_entries == 0 {
            return invalid("generator.deposit", "concurrency, budgets and memo_entries must be positive");
        }
        if d.max_document_bytes == 0 || d.max_body_bytes == 0 || d.max_tracks == 0 || d.index_cache_mb == 0 {
            return invalid("generator.deposit", "size bounds must be positive");
        }
        let p = &g.profile;
        if !(1..=16).contains(&p.threads) || p.max_concurrent == 0 || p.budget_s == 0 {
            return invalid(
                "generator.profile",
                "threads must be between 1 and 16, and concurrency and budget positive",
            );
        }
        if !(1..=PROFILE_MAX_LOOPS_BOUND).contains(&p.max_loops) {
            return invalid(
                "generator.profile.max_loops",
                "max_loops must be between 1 and 5000 (memory and learning time grow with it)",
            );
        }
        if p.budget_s >= d.budget_s {
            return invalid("generator.profile", "the profile budget must be under the deposit budget");
        }
        let st = &g.storage;
        if st.max_namespaces == 0
            || st.max_overlays_per_namespace == 0
            || st.max_total_mb == 0
            || st.max_namespace_mb == 0
        {
            return invalid("generator.storage", "bounds must be positive");
        }
        if (st.max_namespaces as u64).saturating_mul(st.max_namespace_mb) > st.max_total_mb {
            return invalid(
                "generator.storage",
                "max_namespaces x max_namespace_mb must fit in max_total_mb",
            );
        }
        if st.allowed_namespaces.iter().any(|p| p.len() != 8 || !p.bytes().all(|b| b.is_ascii_hexdigit())) {
            return invalid(
                "generator.storage.allowed_namespaces",
                "each prefix must be 8 hexadecimal characters",
            );
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::result_large_err)] // imposé par la signature de `figment::Jail`
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Config::default().validate().expect("défauts valides");
    }

    #[test]
    fn env_overrides_file_which_overrides_defaults() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("ord.toml", "[limits]\nmax_points = 100\nmax_length_m = 5000.0\n")?;
            jail.set_env("ORD_LIMITS__MAX_POINTS", "200");
            jail.set_env("ORD_LIMITS__OPTIONS__STEP_M_DEFAULT", "50");
            let config = Config::load(Some(Path::new("ord.toml"))).map_err(|e| e.to_string())?;
            assert_eq!(config.limits.max_points, 200);
            assert_eq!(config.limits.max_length_m, 5000.0);
            assert_eq!(config.limits.options.step_m_default, 50.0);
            assert_eq!(config.server, ServerConfig::default());
            Ok(())
        });
    }

    #[test]
    fn unknown_key_is_rejected_with_its_path() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("ORD_LIMITS__MAX_POINT", "200");
            let err = Config::load(None).expect_err("clé inconnue");
            assert!(err.to_string().contains("max_point"), "{err}");
            Ok(())
        });
    }

    #[test]
    fn inconsistent_bounds_are_rejected() {
        let mut config = Config::default();
        config.limits.options.step_m_default = 2.0;
        let err = config.validate().expect_err("défaut hors bornes");
        assert!(err.to_string().contains("limits.options.step_m_default"), "{err}");
    }

    #[test]
    fn generator_section_is_configured_by_environment_and_validated() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("ORD_GENERATOR__GRAPH", "/data/g.ordg");
            jail.set_env("ORD_GENERATOR__DEPOSIT__MAX_TRACKS", "5");
            let c = Config::load(None).map_err(|e| e.to_string())?;
            assert_eq!(c.generator.graph, Some(PathBuf::from("/data/g.ordg")));
            assert_eq!(c.generator.deposit.max_tracks, 5);
            assert_eq!(c.generator.deposit.max_document_bytes, 16 * 1024 * 1024);
            jail.set_env("ORD_GENERATOR__MAX_CONCURRENT", "0");
            let err = Config::load(None).expect_err("sans place de calcul");
            assert!(err.to_string().contains("generator"), "{err}");
            Ok(())
        });
    }

    #[test]
    fn the_window_budget_keys_come_from_the_environment_and_are_validated() {
        figment::Jail::expect_with(|jail| {
            let c = Config::load(None).map_err(|e| e.to_string())?;
            assert_eq!(
                (c.generator.memory_budget_mb, c.generator.workspace_keep_mb, c.generator.team_cache_entries),
                (800, 160, 64)
            );
            assert_eq!(c.generator.team_cache_mb, 96);
            jail.set_env("ORD_GENERATOR__MEMORY_BUDGET_MB", "600");
            jail.set_env("ORD_GENERATOR__WINDOW_RADIUS_STEP_M", "5000");
            let c = Config::load(None).map_err(|e| e.to_string())?;
            assert_eq!((c.generator.memory_budget_mb, c.generator.window_radius_step_m), (600, 5000.0));
            jail.set_env("ORD_GENERATOR__MEMORY_BUDGET_MB", "0");
            assert!(Config::load(None).is_err(), "budget nul");
            jail.set_env("ORD_GENERATOR__MEMORY_BUDGET_MB", "180");
            assert!(Config::load(None).is_err(), "équipes à la moitié du budget ou plus");
            jail.set_env("ORD_GENERATOR__MEMORY_BUDGET_MB", "600");
            jail.set_env("ORD_GENERATOR__TEAM_CACHE_MB", "0");
            assert!(Config::load(None).is_err(), "plafond d'équipes nul");
            jail.set_env("ORD_GENERATOR__TEAM_CACHE_MB", "96");
            jail.set_env("ORD_GENERATOR__MEMORY_BUDGET_MB", "600");
            jail.set_env("ORD_GENERATOR__WINDOW_RADIUS_STEP_M", "0");
            assert!(Config::load(None).is_err(), "pas nul");
            Ok(())
        });
    }

    #[test]
    fn profile_settings_are_validated_and_overridable() {
        let mut config = Config::default();
        assert_eq!(config.generator.profile.threads, 4);
        assert_eq!((config.generator.profile.budget_s, config.generator.profile.max_loops), (720, 500));
        assert!(config.generator.profile.enabled && !config.generator.profile.per_type);
        for bad in [
            |c: &mut Config| c.generator.profile.threads = 0,
            |c: &mut Config| c.generator.profile.threads = 17,
            |c: &mut Config| c.generator.profile.max_concurrent = 0,
            |c: &mut Config| c.generator.profile.max_loops = 0,
            |c: &mut Config| c.generator.profile.max_loops = PROFILE_MAX_LOOPS_BOUND + 1,
            |c: &mut Config| c.generator.profile.budget_s = 0,
            // Le profil se fait sous le budget du dépôt, jamais au-delà.
            |c: &mut Config| c.generator.profile.budget_s = 900,
        ] {
            let mut c = config.clone();
            bad(&mut c);
            assert!(c.validate().is_err());
        }
        assert_eq!(config.generator.profile.mix, ProfileMix::Capped);
        // 1.12 : les figures par défaut sont organiques, pour la génération et pour l'apprentissage (règle L du relevé).
        assert_eq!(
            (config.generator.figures, config.generator.profile.learn_figures),
            (GeneratorFigures::Organic, GeneratorFigures::Organic)
        );
        config.generator.profile.threads = 16;
        assert!(config.validate().is_ok());
        config.generator.profile.max_loops = PROFILE_MAX_LOOPS_BOUND;
        assert!(config.validate().is_ok(), "la borne elle-même est permise");
        config.generator.profile.max_loops = 500;
        figment::Jail::expect_with(|jail| {
            jail.set_env("ORD_GENERATOR__PROFILE__THREADS", "2");
            jail.set_env("ORD_GENERATOR__PROFILE__PER_TYPE", "true");
            jail.set_env("ORD_GENERATOR__PROFILE__MIX", "sonde");
            let c = Config::load(None).map_err(|e| e.to_string())?;
            assert_eq!((c.generator.profile.threads, c.generator.profile.per_type), (2, true));
            assert_eq!(c.generator.profile.mix, ProfileMix::Sonde);
            jail.set_env("ORD_GENERATOR__PROFILE__MIX", "lent");
            assert!(Config::load(None).is_err(), "forme inconnue refusée");
            jail.set_env("ORD_GENERATOR__PROFILE__MIX", "capped");
            jail.set_env("ORD_GENERATOR__PROFILE__THREAD", "2");
            assert!(Config::load(None).is_err(), "clé inconnue refusée");
            jail.set_env("ORD_GENERATOR__PROFILE__THREAD", "");
            Ok(())
        });
        // 4c : `figures` et `profile.learn_figures`, par variable d'environnement, valeurs connues seulement.
        figment::Jail::expect_with(|jail| {
            jail.set_env("ORD_GENERATOR__FIGURES", "classic");
            jail.set_env("ORD_GENERATOR__PROFILE__LEARN_FIGURES", "classic");
            let c = Config::load(None).map_err(|e| e.to_string())?;
            assert_eq!(
                (c.generator.figures, c.generator.profile.learn_figures),
                (GeneratorFigures::Classic, GeneratorFigures::Classic)
            );
            jail.set_env("ORD_GENERATOR__FIGURES", "quadrilateral");
            assert!(Config::load(None).is_err(), "figure inconnue refusée");
            jail.set_env("ORD_GENERATOR__FIGURES", "organic");
            jail.set_env("ORD_GENERATOR__PROFILE__LEARN_FIGURES", "triangle");
            assert!(Config::load(None).is_err(), "figure d'apprentissage inconnue refusée");
            Ok(())
        });
    }
}
