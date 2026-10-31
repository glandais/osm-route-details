//! Câblage des implémentations (T01 §2.1) : `ord-server` seul assemble les ports.
//!
//! Incrément 1 : terrain Mapterhorn seul, quelle que soit `elevation.ign.enabled` (l'IGN arrive à
//! l'incrément 2 ; un avertissement est journalisé au démarrage s'il est configuré).

use std::sync::Arc;
use std::time::Duration;

use ord_adapters::TerrariumCodec;
use ord_core::Rules;
use ord_data::catalog::StaticCatalog;
use ord_data::disk::DiskStore;
use ord_data::elevation::{MapterhornTerrainReader, PixelConvention};
use ord_data::http::{build_client, HttpSettings, XyzHttpSource};
use ord_data::port::{namespace, ReaderSettings, SourceDescriptor, StatusPolicy, TileReader};
use ord_domain::tiles::SourceId;
use ord_pipeline::{Orchestrator, RetryAfterPolicy, Sources};

use crate::config::Config;
use crate::API_VERSION;

/// Choix internes, hors configuration et hors contrat (outillage de mesure seulement).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wiring {
    pub pixel_convention: PixelConvention,
}

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error("http client: {0}")]
    HttpClient(String),
}

/// Espace de noms disque de Mapterhorn : version publiée et hachage du gabarit (T02 §5.2).
pub fn mapterhorn_namespace(config: &Config) -> String {
    let m = &config.elevation.mapterhorn;
    namespace(&format!("mapterhorn-{}", m.version.as_deref().unwrap_or("unversioned")), &m.url)
}

pub fn orchestrator(
    config: &Config,
    catalog: Arc<StaticCatalog>,
    wiring: Wiring,
) -> Result<Orchestrator, StartupError> {
    let h = &config.http_client;
    let client = build_client(&HttpSettings {
        user_agent: h.user_agent.clone(),
        connect_timeout: Duration::from_secs(h.connect_timeout_s),
        fetch_timeout: Duration::from_secs(h.fetch_timeout_s),
    })
    .map_err(|e| StartupError::HttpClient(e.to_string()))?;
    let m = &config.elevation.mapterhorn;
    let ns = mapterhorn_namespace(config);
    let descriptor = SourceDescriptor {
        id: SourceId::Mapterhorn,
        zoom: m.zoom,
        template: m.url.clone(),
        namespace: ns.clone(),
        status_policy: StatusPolicy::MAPTERHORN,
        concurrency: m.concurrency,
        // Même règle de relance pour toutes les sources (T02 §4.2 : 6 essais).
        max_attempts: config.elevation.ign.max_attempts.max(1),
        retry_base: Duration::from_secs(1),
    };
    let source = Arc::new(XyzHttpSource::new(descriptor, client));
    let settings = ReaderSettings {
        offline: config.cache.offline,
        memory_bytes: config.cache.memory_elevation_mb.saturating_mul(1024 * 1024),
        negative_ttl: Duration::from_secs(config.cache.negative_ttl_s),
    };
    let disk = DiskStore::new(&config.cache.dir, &ns);
    let reader = TileReader::new(source, TerrariumCodec { size: m.tile_size }, Some(disk), &settings);
    let terrain = MapterhornTerrainReader::new(
        Arc::new(reader),
        m.zoom,
        u32::from(m.tile_size),
        wiring.pixel_convention,
    );
    let b = &config.budget;
    Ok(Orchestrator::new(
        Sources { terrain: Arc::new(terrain), catalog },
        Rules::default(),
        API_VERSION,
        RetryAfterPolicy {
            min: Duration::from_secs(b.retry_after_min_s),
            max: Duration::from_secs(b.retry_after_max_s),
            default: Duration::from_secs(b.retry_after_default_s),
        },
    ))
}
