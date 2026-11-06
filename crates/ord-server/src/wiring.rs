//! Câblage des implémentations (T01 §2.1) : `ord-server` seul assemble les ports.
//!
//! Incrément 2 : MNT IGN LiDAR HD (WMS-R, BIL32, z15) sous un budget d'hôte de 20 req/s, repli
//! Mapterhorn point par point, référentiel de couverture (contour embarqué, index et polygones de
//! Mapterhorn). Incrément 3 : tuiles de rues VersaTiles (Shortbread z14) et chaîne typologie. Les
//! lectures de démarrage (`Startup`) sont faites une fois, avant de servir.

use std::sync::Arc;
use std::time::Duration;

use ord_adapters::{BilCodec, CoverageCodec, Shortbread, StreetsCodec, TerrariumCodec};
use ord_core::Rules;
use ord_data::budget::HostBudgets;
use ord_data::catalog::{ConfiguredSources, StaticCatalog};
use ord_data::coverage::{CoverageIndex, MapterhornCoverage};
use ord_data::disk::DiskStore;
use ord_data::elevation::{ElevationLayer, LayeredTerrainReader, MapterhornLayer, PixelConvention, RowMode};
use ord_data::http::{build_client, wms_template, HttpSettings, WmsGetMapSource, XyzHttpSource};
use ord_data::port::{
    namespace, ReaderSettings, SourceDescriptor, StatusMeaning, StatusPolicy, TileReader, TileSource,
};
use ord_domain::coverage::{CoverageRef, CoverageTile};
use ord_domain::elevation::{Plausibility, TerrainMode};
use ord_domain::street::{StreetSchemaAdapter, StreetTile};
use ord_domain::tiles::{ElevationTile, SourceId, TileStore};
use ord_pipeline::typology::TileTypology;
use ord_pipeline::{Admission, AdmissionLimits, Orchestrator, RetryAfterPolicy, Sources};

use crate::config::{Config, IgnMode, TypologySourceKind};
use crate::API_VERSION;

/// Côté des tuiles BIL demandées au WMS (vcyclist : 256 px à z15, ≈ 3,4 m par pixel).
pub const IGN_TILE_SIZE: u16 = 256;

/// Statuts du WMS IGN (T02 §3) : 204 et 404 sont des échecs, jamais « pas de donnée ».
pub const IGN_STATUS: StatusPolicy =
    StatusPolicy { no_content: StatusMeaning::Failed, not_found: StatusMeaning::Failed };

/// Choix internes, hors configuration et hors contrat (outillage de mesure seulement).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wiring {
    pub pixel_convention: PixelConvention,
}

/// Données lues au démarrage (T02 §9) : version de Mapterhorn et index de couverture.
#[derive(Clone, Debug, Default)]
pub struct Startup {
    pub mapterhorn_version: Option<String>,
    pub coverage_index: Option<Arc<CoverageIndex>>,
}

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error("http client: {0}")]
    HttpClient(String),
    #[error("typology source not supported yet: {0}")]
    Unsupported(&'static str),
    /// Fenêtres des détecteurs V1 invalides (T04 §4.1, vérifiées au démarrage).
    #[error("invalid rules: {0}")]
    Rules(String),
}

fn client(config: &Config) -> Result<reqwest::Client, StartupError> {
    let h = &config.http_client;
    build_client(&HttpSettings {
        user_agent: h.user_agent.clone(),
        connect_timeout: Duration::from_secs(h.connect_timeout_s),
        fetch_timeout: Duration::from_secs(h.fetch_timeout_s),
    })
    .map_err(|e| StartupError::HttpClient(e.to_string()))
}

/// Lectures de démarrage : en ligne, ou dans le cache disque en mode hors ligne.
pub async fn startup(config: &Config) -> Result<Startup, StartupError> {
    let client = client(config)?;
    let m = &config.elevation.mapterhorn;
    let (dir, offline) = (&config.cache.dir, config.cache.offline);
    let mapterhorn_version = match &m.version {
        Some(v) => Some(v.clone()),
        None => {
            ord_data::startup::mapterhorn_version(&client, &m.tilejson_url, &m.versions_url, dir, offline)
                .await
        }
    };
    let c = &config.coverage;
    if c.coverage.is_some() {
        tracing::warn!("coverage.coverage (local coverage.pmtiles) is not read yet: polygons come from coverage.polygons_url");
    }
    let coverage_index = ord_data::startup::coverage_index(
        &client,
        c.coverage_index_url.as_deref(),
        c.coverage_index.as_deref(),
        dir,
        offline,
    )
    .await
    .map(Arc::new);
    Ok(Startup { mapterhorn_version, coverage_index })
}

/// Espace de noms disque de Mapterhorn : version publiée et hachage du gabarit (T02 §5.2).
pub fn mapterhorn_namespace(config: &Config, startup: &Startup) -> String {
    let m = &config.elevation.mapterhorn;
    let version = m.version.as_deref().or(startup.mapterhorn_version.as_deref()).unwrap_or("unversioned");
    namespace(&format!("mapterhorn-{version}"), &m.url)
}

/// Espace de noms disque d'une couche IGN (`mnt` ou `mns`).
pub fn ign_namespace(config: &Config, product: &str, layer: &str) -> String {
    let ign = &config.elevation.ign;
    namespace(&format!("ign-lidar-hd-{product}-z{}", ign.zoom), &wms_template(&ign.wms_url, layer))
}

/// Sources configurées, telles que le catalogue les publie (`/v1/capabilities`, `data_versions`).
pub fn configured_sources(config: &Config, startup: &Startup) -> ConfiguredSources {
    let m = &config.elevation.mapterhorn;
    ConfiguredSources {
        ign_enabled: config.elevation.ign.enabled && config.elevation.ign.mode == IgnMode::Direct,
        ign_zoom: config.elevation.ign.zoom,
        mapterhorn_zoom: m.zoom,
        mapterhorn_version: m.version.clone().or(startup.mapterhorn_version.clone()),
        coverage_version: startup
            .coverage_index
            .as_ref()
            .map(|i| i.version.clone().unwrap_or_else(|| "local".into())),
        streets_zoom: config.typology.zoom,
    }
}

struct Builder<'a> {
    config: &'a Config,
    client: reqwest::Client,
    budgets: HostBudgets,
    settings: ReaderSettings,
}

impl Builder<'_> {
    fn descriptor(
        &self,
        id: SourceId,
        zoom: u8,
        template: String,
        ns: String,
        policy: StatusPolicy,
        concurrency: usize,
    ) -> SourceDescriptor {
        SourceDescriptor {
            id,
            zoom,
            template,
            namespace: ns,
            status_policy: policy,
            concurrency,
            // Même règle de relance pour toutes les sources (T02 §4.2 : 6 essais).
            max_attempts: self.config.elevation.ign.max_attempts.max(1),
            retry_base: Duration::from_secs(1),
        }
    }

    fn reader<C: ord_domain::tiles::TileCodec>(
        &self,
        source: Arc<dyn TileSource>,
        codec: C,
    ) -> TileReader<C> {
        let ns = source.descriptor().namespace.clone();
        TileReader::new(source, codec, Some(DiskStore::new(&self.config.cache.dir, &ns)), &self.settings)
    }

    fn ign_layer(
        &self,
        id: SourceId,
        product: &str,
        layer: &str,
        plausibility: Plausibility,
    ) -> ElevationLayer {
        let ign = &self.config.elevation.ign;
        let template = wms_template(&ign.wms_url, layer);
        let budget = self.budgets.for_url(&ign.wms_url, Some(ign.rate_per_s), ign.concurrency);
        let descriptor = self.descriptor(
            id,
            ign.zoom,
            template,
            ign_namespace(self.config, product, layer),
            IGN_STATUS,
            ign.concurrency,
        );
        let source = Arc::new(WmsGetMapSource::new(descriptor, self.client.clone(), budget, IGN_TILE_SIZE));
        let codec = BilCodec {
            size: IGN_TILE_SIZE,
            nodata_ttl: Duration::from_secs(self.config.cache.ttl_ign_nodata_days.saturating_mul(86_400)),
        };
        let store: Arc<dyn TileStore<ElevationTile>> = Arc::new(self.reader(source, codec));
        ElevationLayer {
            store,
            zoom: ign.zoom,
            size: u32::from(IGN_TILE_SIZE),
            rows: RowMode::LatLinear,
            plausibility: Some(plausibility),
        }
    }

    fn coverage(&self, startup: &Startup) -> MapterhornCoverage {
        let c = &self.config.coverage;
        let polygons: Option<Arc<dyn TileStore<CoverageTile>>> =
            match (&startup.coverage_index, &c.polygons_url) {
                (Some(_), Some(url)) => {
                    let budget = self.budgets.for_url(url, None, c.polygons_concurrency);
                    let ns = namespace("mapterhorn-coverage", url);
                    let descriptor = self.descriptor(
                        SourceId::MapterhornCoverage,
                        c.polygons_zoom,
                        url.clone(),
                        ns,
                        StatusPolicy::MAPTERHORN,
                        c.polygons_concurrency,
                    );
                    let source =
                        Arc::new(XyzHttpSource::with_budget(descriptor, self.client.clone(), budget));
                    Some(Arc::new(self.reader(source, CoverageCodec::default())))
                }
                _ => None,
            };
        MapterhornCoverage::new(
            startup.coverage_index.clone(),
            polygons,
            c.polygons_zoom,
            Rules::default().coverage.seam_m,
        )
    }
}

/// Espace de noms disque des tuiles de rues : schéma, zoom et hachage du gabarit.
pub fn streets_namespace(config: &Config) -> String {
    let t = &config.typology;
    namespace(&format!("versatiles-shortbread-z{}", t.zoom), &t.url)
}

/// Chaîne typologie seule (aussi utilisée par le banc pour la mesurer à chaud).
pub fn typology_chain(config: &Config) -> Result<TileTypology, StartupError> {
    let b = Builder::new(config)?;
    b.typology()
}

impl<'a> Builder<'a> {
    fn new(config: &'a Config) -> Result<Self, StartupError> {
        Ok(Builder {
            config,
            client: client(config)?,
            budgets: HostBudgets::default(),
            settings: ReaderSettings {
                offline: config.cache.offline,
                memory_bytes: config.cache.memory_elevation_mb.saturating_mul(1024 * 1024),
                negative_ttl: Duration::from_secs(config.cache.negative_ttl_s),
            },
        })
    }

    fn typology(&self) -> Result<TileTypology, StartupError> {
        let t = &self.config.typology;
        if t.source == TypologySourceKind::VersatilesFile {
            return Err(StartupError::Unsupported("versatiles_file"));
        }
        let budget = self.budgets.for_url(&t.url, None, t.concurrency);
        // VersaTiles : 204 = tuile vide, 404 = URL fausse (jamais « pas de rue »).
        let descriptor = self.descriptor(
            SourceId::Streets,
            t.zoom,
            t.url.clone(),
            streets_namespace(self.config),
            StatusPolicy::VERSATILES,
            t.concurrency,
        );
        let source = Arc::new(XyzHttpSource::with_budget(descriptor, self.client.clone(), budget));
        let codec = StreetsCodec::new(
            Shortbread,
            Duration::from_secs(self.config.cache.ttl_streets_days.saturating_mul(86_400)),
        );
        let ns = source.descriptor().namespace.clone();
        let settings = ReaderSettings {
            memory_bytes: self.config.cache.memory_streets_mb.saturating_mul(1024 * 1024),
            offline: self.settings.offline || self.config.cache.offline_streets,
            ..self.settings.clone()
        };
        let store: Arc<dyn TileStore<StreetTile>> = Arc::new(TileReader::new(
            source,
            codec,
            Some(DiskStore::new(&self.config.cache.dir, &ns)),
            &settings,
        ));
        Ok(TileTypology::new(store, Shortbread::GRID, Shortbread.capabilities(), Rules::default().typology))
    }
}

pub fn orchestrator(
    config: &Config,
    startup: &Startup,
    catalog: Arc<StaticCatalog>,
    wiring: Wiring,
) -> Result<Orchestrator, StartupError> {
    let b = Builder::new(config)?;
    let (admission, compute) = admission(config);
    let fast_admission = fast_admission(config);
    let typology = Arc::new(b.typology()?.with_compute_permits(Arc::clone(&compute)));
    let m = &config.elevation.mapterhorn;
    let budget = b.budgets.for_url(&m.url, None, m.concurrency);
    let descriptor = b.descriptor(
        SourceId::Mapterhorn,
        m.zoom,
        m.url.clone(),
        mapterhorn_namespace(config, startup),
        StatusPolicy::MAPTERHORN,
        m.concurrency,
    );
    let source = Arc::new(XyzHttpSource::with_budget(descriptor, b.client.clone(), budget));
    let mapterhorn = MapterhornLayer::new(ElevationLayer {
        store: Arc::new(b.reader(source, TerrariumCodec { size: m.tile_size })),
        zoom: m.zoom,
        size: u32::from(m.tile_size),
        rows: RowMode::Mercator,
        plausibility: None,
    });
    let rules = Rules::default();
    rules.elevation.validate().map_err(StartupError::Rules)?;
    let ign = &config.elevation.ign;
    let guard = &rules.elevation.ign_plausibility;
    let (dtm, dsm) = if ign.enabled {
        (
            Some(b.ign_layer(SourceId::IgnDtm, "mnt", &ign.dtm_layer, guard.dtm)),
            Some(b.ign_layer(SourceId::IgnDsm, "mns", &ign.dsm_layer, guard.dsm)),
        )
    } else {
        (None, None)
    };
    let mode = match ign.mode {
        IgnMode::Direct => TerrainMode::Full,
        IgnMode::Sober => TerrainMode::Sober,
    };
    let coverage: Arc<dyn CoverageRef> = Arc::new(b.coverage(startup));
    // Profil `fast` (api 1.5) : même couche Mapterhorn (caches mémoire et disque partagés), sans
    // couche IGN, donc sans aucun créneau du budget d'hôte IGN.
    let fast_terrain = LayeredTerrainReader::new(
        None,
        None,
        mapterhorn.clone(),
        Arc::clone(&coverage),
        mode,
        wiring.pixel_convention,
    );
    let terrain =
        LayeredTerrainReader::new(dtm, dsm, mapterhorn, Arc::clone(&coverage), mode, wiring.pixel_convention);
    let budget = &config.budget;
    Ok(Orchestrator::new(
        Sources {
            typology,
            terrain: Arc::new(terrain),
            fast_terrain: Arc::new(fast_terrain),
            coverage,
            catalog,
        },
        rules,
        API_VERSION,
        RetryAfterPolicy {
            min: Duration::from_secs(budget.retry_after_min_s),
            max: Duration::from_secs(budget.retry_after_max_s),
            default: Duration::from_secs(budget.retry_after_default_s),
        },
    )
    .with_admission(admission)
    .with_fast_admission(fast_admission)
    .with_compute_permits(compute))
}

/// File d'admission et jetons de calcul (T01 §3.1–3.2) ; défauts : un par CPU, file de deux fois
/// `max_in_flight`.
pub fn admission(config: &Config) -> (Admission, Arc<tokio::sync::Semaphore>) {
    let cpus = std::thread::available_parallelism().map_or(1, usize::from);
    let b = &config.budget;
    let max_in_flight = b.max_in_flight.unwrap_or(cpus).max(1);
    let limits = AdmissionLimits { max_in_flight, max_queued: b.max_queued.unwrap_or(2 * max_in_flight) };
    let compute = b.compute_permits.unwrap_or(cpus).max(1);
    (Admission::new(limits), Arc::new(tokio::sync::Semaphore::new(compute)))
}

/// File du profil `fast` (api 1.5) ; défauts : ceux de la file `full`.
pub fn fast_admission(config: &Config) -> Admission {
    let cpus = std::thread::available_parallelism().map_or(1, usize::from);
    let b = &config.budget;
    let max_in_flight = b.fast_max_in_flight.or(b.max_in_flight).unwrap_or(cpus).max(1);
    let max_queued = b.fast_max_queued.unwrap_or(2 * max_in_flight);
    Admission::new(AdmissionLimits { max_in_flight, max_queued })
}
