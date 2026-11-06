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
}
