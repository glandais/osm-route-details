//! Câblage des implémentations (T01 §2.1) : `ord-server` seul assemble les ports.
//!
//! Incrément 2 : MNT IGN LiDAR HD (WMS-R, BIL32, z15) sous un budget d'hôte de 20 req/s, repli
//! Mapterhorn point par point, référentiel de couverture (contour embarqué, index et polygones de
//! Mapterhorn). Les lectures de démarrage (`Startup`) sont faites une fois, avant de servir.

use std::sync::Arc;
use std::time::Duration;

use ord_adapters::{BilCodec, CoverageCodec, TerrariumCodec};
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
use ord_domain::elevation::TerrainMode;
use ord_domain::tiles::{ElevationTile, SourceId, TileStore};
use ord_pipeline::{Orchestrator, RetryAfterPolicy, Sources};

use crate::config::{Config, IgnMode};
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

    fn ign_layer(&self, id: SourceId, product: &str, layer: &str) -> ElevationLayer {
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
        ElevationLayer { store, zoom: ign.zoom, size: u32::from(IGN_TILE_SIZE), rows: RowMode::LatLinear }
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

pub fn orchestrator(
    config: &Config,
    startup: &Startup,
    catalog: Arc<StaticCatalog>,
    wiring: Wiring,
) -> Result<Orchestrator, StartupError> {
    let b = Builder {
        config,
        client: client(config)?,
        budgets: HostBudgets::default(),
        settings: ReaderSettings {
            offline: config.cache.offline,
            memory_bytes: config.cache.memory_elevation_mb.saturating_mul(1024 * 1024),
            negative_ttl: Duration::from_secs(config.cache.negative_ttl_s),
        },
    };
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
    });
    let ign = &config.elevation.ign;
    let (dtm, dsm) = if ign.enabled {
        (
            Some(b.ign_layer(SourceId::IgnDtm, "mnt", &ign.dtm_layer)),
            Some(b.ign_layer(SourceId::IgnDsm, "mns", &ign.dsm_layer)),
        )
    } else {
        (None, None)
    };
    let mode = match ign.mode {
        IgnMode::Direct => TerrainMode::Full,
        IgnMode::Sober => TerrainMode::Sober,
    };
    let coverage: Arc<dyn CoverageRef> = Arc::new(b.coverage(startup));
    let terrain =
        LayeredTerrainReader::new(dtm, dsm, mapterhorn, Arc::clone(&coverage), mode, wiring.pixel_convention);
    let budget = &config.budget;
    Ok(Orchestrator::new(
        Sources { terrain: Arc::new(terrain), coverage, catalog },
        Rules::default(),
        API_VERSION,
        RetryAfterPolicy {
            min: Duration::from_secs(budget.retry_after_min_s),
            max: Duration::from_secs(budget.retry_after_max_s),
            default: Duration::from_secs(budget.retry_after_default_s),
        },
    ))
}
