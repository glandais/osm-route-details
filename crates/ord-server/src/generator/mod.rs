//! Le générateur de parcours servi par le service (lot 3a) : dépôt de la surcouche d'une équipe, génération, état,
//! effacement. Note de conception : `docs/design/technique/ord-loops-lot3a.md`.
//!
//! Sans graphe configuré, ou si un fichier est absent ou illisible, le service **démarre** et sert l'altitude ;
//! les routes `/v1/generator/*` rendent 503 `data_unavailable` (`details.source = graph`) et `/v1/capabilities` dit
//! `generator.enabled = false` et pourquoi.
//!
//! Rien de privé n'est journalisé : ni coordonnée, ni clé d'environnement, ni référence d'équipe, ni nom de fichier ;
//! seule l'étiquette `ns8` (8 premiers caractères du nom d'espace, non secrets) relie les lignes d'un environnement.

use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::Arc;
use std::time::Duration;

use ord_graph::format::GraphIdentity;
use ord_graph::mmap::MappedGraph;
use ord_graph::util::Poly;
use ord_loops::{Engine, Params};
use serde::Serialize;
use tokio::sync::Semaphore;
use utoipa::ToSchema;
use xxhash_rust::xxh3::xxh3_64;

use crate::config::GeneratorConfig;

pub mod crypto;
pub mod cursor;
mod deposit;
mod generate;
pub mod routes;
pub mod store;
#[cfg(test)]
mod testkit;
pub(crate) mod windows;

pub use store::{GenEngine, TestHooks};

/// Corps d'une demande de génération : petit, borné à part de celui de `/v1/route-details`.
pub const GENERATE_BODY_MAX: usize = 64 * 1024;
/// Cohérence du polygone avec le graphe (écart à la note : la règle « boîte à 5 km de celle des nœuds » refusait la
/// vraie région, dont le polygone Geofabrik couvre la mer jusqu'à 9 km au-delà des nœuds) :
/// le polygone peut dépasser la boîte des nœuds de 15 km (mer) **ou de 20 % de sa largeur** (le polygone Geofabrik de la
/// France déborde de 1,8° à l'ouest, soit 12 %, sur l'Atlantique : un polygone de la France sur un graphe régional
/// déborde de bien plus), les nœuds peuvent dépasser celle du polygone de 5 km (voies qui sortent de l'extrait), et 95 %
/// au moins des nœuds échantillonnés sont dans le polygone.
const POLY_OVERHANG_M: f64 = 15_000.0;
const POLY_OVERHANG_FRACTION: f64 = 0.20;
const NODES_OUTSIDE_M: f64 = 5_000.0;
const POLY_MIN_NODES_INSIDE: f64 = 0.95;
/// Un nœud sur `NODE_SAMPLE_EVERY` est testé contre le polygone.
const NODE_SAMPLE_EVERY: usize = 16;

/// Why the generator is off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DisabledReason {
    /// No graph is configured.
    NotConfigured,
    /// A graph or view file is missing or unreadable.
    GraphUnreadable,
    /// A view does not belong to this graph.
    ViewMismatch,
    /// The polygon is missing, unreadable, or does not match the graph (extents, nodes inside).
    PolygonMismatch,
}

/// What the service publishes about the generator (`/v1/capabilities`, api 1.9).
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct GeneratorCapabilities {
    /// The `/v1/generator/*` routes answer (otherwise 503 `data_unavailable`).
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<DisabledReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<GraphInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<CoverageInfo>,
    /// Accepted ride types (`ride_type`).
    pub ride_types: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<CursorInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<GeneratorLimits>,
    /// Days after which an overlay that was not deposited again is purged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay_ttl_days: Option<u64>,
    /// Fingerprint of the matcher: a change makes the stored overlays `stale` (reason `matcher`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher_hash: Option<String>,
    /// Do the routes carry ascent and descent (`ascent_m`, `descent_m`)? No: the engine has no elevation.
    pub ascent: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct GraphInfo {
    /// Identity of the graph: another identifier makes every stored overlay `stale` (reason `graph`).
    pub id: String,
    /// Date of the OSM data (replication timestamp): ISO 8601, UTC.
    pub data_date: String,
    /// The same, in Unix seconds.
    pub data_timestamp: i64,
    pub tool_version: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CoverageInfo {
    /// `[lon_min, lat_min, lon_max, lat_max]`.
    pub bbox: [f64; 4],
    /// Covered polygon, as GeoJSON (`MultiPolygon`).
    #[schema(value_type = Object)]
    pub polygon: serde_json::Value,
}

/// How the `cursor` maps to the engine.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CursorInfo {
    /// Value that gives the strict mode (the team's roads only).
    pub strict_at: f64,
    /// Value that gives discovery (OSM with no preference for the team).
    pub discovery_at: f64,
    /// In between, team first with an off-team edge cost lambda = lambda_min + (lambda_max - lambda_min) * (1 - cursor).
    pub lambda_max: f64,
    pub lambda_min: f64,
    /// Fixed familiarity (team first and discovery).
    pub familiarity: f64,
    /// The cursor is rounded to this step.
    pub step: f64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct GeneratorLimits {
    pub distance_m_min: f64,
    pub distance_m_max: f64,
    /// Budget of a generation, queue wait included.
    pub budget_ms: u64,
    pub max_concurrent: usize,
    pub max_queued: usize,
    /// Body of a generation request.
    pub request_body_bytes: usize,
    pub deposit: DepositLimits,
    pub storage: StorageLimits,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DepositLimits {
    /// Decompressed body.
    pub max_body_bytes: u64,
    /// One line (one `network_edges` document).
    pub max_document_bytes: usize,
    pub max_tracks: u64,
    pub budget_s: u64,
    /// A deposit that receives no byte for this long is abandoned.
    pub idle_timeout_s: u64,
    /// Longest wait for the deposit capacity (fair queue), then 503 `overloaded`; counted in `budget_s`.
    pub queue_wait_s: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct StorageLimits {
    pub max_namespaces: usize,
    pub max_overlays_per_namespace: usize,
    /// Bytes of stored overlays per environment (the quota a deposit is checked against: 507 beyond).
    pub max_namespace_mb: u64,
    /// Budget of the whole volume (`max_namespaces` times `max_namespace_mb` fits in it).
    pub max_total_mb: u64,
}

/// État du générateur : éteint (avec sa raison) ou prêt.
#[derive(Debug)]
pub struct Generator {
    state: GeneratorState,
}

#[derive(Debug)]
enum GeneratorState {
    Disabled(DisabledReason),
    Ready(Arc<Ready>),
}

/// Tout ce qu'il faut pour servir : le moteur, le polygone, le stockage et les limites.
pub struct Ready {
    cfg: GeneratorConfig,
    pub(crate) engine: GenEngine,
    poly: Poly,
    store: Arc<store::Store>,
    /// L'index par cellule couvre ce graphe : peu d'arêtes plus larges qu'une cellule (liste d'exceptions) ; sinon
    /// index entier.
    cell_index_ok: bool,
    /// Arêtes plus larges qu'une cellule, pour le contrôleur indépendant (une fois, au démarrage).
    pub(crate) graph_index: ord_loops_check::GraphIndex,
    /// Fenêtres du moteur sous un budget de mémoire (lot 6).
    pub(crate) windows: Arc<windows::WindowManager>,
    gen_slots: Arc<Semaphore>,
    gen_waiting: AtomicUsize,
    deposit_slots: Arc<Semaphore>,
    hooks: Arc<TestHooks>,
    /// Le service s'arrête : les attentes de fond (construction des données d'une équipe après un dépôt) s'interrompent.
    stopping: AtomicBool,
    graph_id: String,
    capabilities: GeneratorCapabilities,
}

impl std::fmt::Debug for Ready {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ready").field("graph_id", &self.graph_id).finish_non_exhaustive()
    }
}

/// Au-delà de ce nombre d'arêtes plus larges qu'une cellule, l'index des dépôts retombe sur l'index entier.
const MAX_WIDE_EDGES: usize = 1_000;

/// Identité d'un graphe pour le stockage : xxh3 des champs de `GraphIdentity::same_ids` (un graphe reconstruit à
/// l'identique garde ses surcouches ; un autre les rend `stale`).
pub fn graph_id(i: &GraphIdentity) -> String {
    let s = format!(
        "{}/{}/{}/{}/{}/{}/{:016x}/{:016x}",
        i.format_major, i.attr_layout, i.zoom, i.nodes, i.edges, i.directed, i.id_edges_xxh3, i.id_nodes_xxh3
    );
    format!("{:016x}", xxh3_64(s.as_bytes()))
}

/// Secondes Unix en ISO 8601 UTC (`2026-11-16T03:04:05Z`).
pub fn iso8601(epoch: i64) -> String {
    let days = epoch.div_euclid(86_400);
    let rem = epoch.rem_euclid(86_400);
    // Jours depuis 1970-01-01 → date civile (algorithme de Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn disabled(reason: DisabledReason) -> Generator {
    Generator { state: GeneratorState::Disabled(reason) }
}

/// Boîte des nœuds du graphe `[lon_min, lat_min, lon_max, lat_max]`, et part des nœuds (échantillonnés) que le
/// polygone contient.
fn node_extent(g: &MappedGraph, poly: &Poly) -> Option<([f64; 4], f64)> {
    let mut b: Option<[f64; 4]> = None;
    let (mut seen, mut sampled, mut inside) = (0usize, 0usize, 0usize);
    for ci in 0..g.n_cells() {
        let cv = g.cell(ci);
        for local in 0..cv.info().n_nodes as usize {
            let (lat, lon) = cv.node(local);
            let (lat, lon) = (f64::from(lat) * 1e-7, f64::from(lon) * 1e-7);
            b = Some(match b {
                None => [lon, lat, lon, lat],
                Some(b) => [b[0].min(lon), b[1].min(lat), b[2].max(lon), b[3].max(lat)],
            });
            if seen % NODE_SAMPLE_EVERY == 0 {
                sampled += 1;
                inside += usize::from(poly.contains(lon, lat));
            }
            seen += 1;
        }
    }
    Some((b?, inside as f64 / sampled.max(1) as f64))
}

/// Le polygone est-il celui de ce graphe ?
fn polygon_fits(pb: [f64; 4], nb: [f64; 4], inside: f64) -> bool {
    let dlat = |m: f64| m / (ord_graph_match::geo::EARTH_R * std::f64::consts::PI / 180.0);
    let dlon = |m: f64| dlat(m) / f64::midpoint(nb[1], nb[3]).to_radians().cos().max(0.01);
    let over_lat = dlat(POLY_OVERHANG_M).max(POLY_OVERHANG_FRACTION * (nb[3] - nb[1]));
    let over_lon = dlon(POLY_OVERHANG_M).max(POLY_OVERHANG_FRACTION * (nb[2] - nb[0]));
    let (out_lat, out_lon) = (dlat(NODES_OUTSIDE_M), dlon(NODES_OUTSIDE_M));
    pb[0] >= nb[0] - over_lon
        && pb[2] <= nb[2] + over_lon
        && pb[1] >= nb[1] - over_lat
        && pb[3] <= nb[3] + over_lat
        && nb[0] >= pb[0] - out_lon
        && nb[2] <= pb[2] + out_lon
        && nb[1] >= pb[1] - out_lat
        && nb[3] <= pb[3] + out_lat
        && inside >= POLY_MIN_NODES_INSIDE
}

/// Point dans un anneau (parité des croisements).
fn in_ring(ring: &[(f64, f64)], x: f64, y: f64) -> bool {
    let mut inside = false;
    let mut j = ring.len().wrapping_sub(1);
    for i in 0..ring.len() {
        let ((xi, yi), (xj, yj)) = (ring[i], ring[j]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Le polygone en GeoJSON `MultiPolygon` : chaque anneau extérieur et les trous qu'il contient.
fn poly_geojson(poly: &Poly) -> serde_json::Value {
    let r5 = |v: f64| (v * 1e5).round() / 1e5;
    let ring = |r: &Vec<(f64, f64)>| -> Vec<[f64; 2]> { r.iter().map(|&(x, y)| [r5(x), r5(y)]).collect() };
    let polygons: Vec<Vec<Vec<[f64; 2]>>> = poly
        .outer_rings()
        .iter()
        .map(|outer| {
            let mut rings = vec![ring(outer)];
            for h in poly.hole_rings() {
                if h.first().is_some_and(|&(x, y)| in_ring(outer, x, y)) {
                    rings.push(ring(h));
                }
            }
            rings
        })
        .collect();
    serde_json::json!({"type": "MultiPolygon", "coordinates": polygons})
}

impl Generator {
    /// Générateur éteint sans raison d'erreur (aucun graphe configuré).
    pub fn disabled() -> Self {
        disabled(DisabledReason::NotConfigured)
    }

    /// Charge le graphe, les vues et le polygone ; toute impossibilité éteint le générateur (un `warn`) sans empêcher
    /// le service de démarrer. Bloquant : quelques secondes sur la région.
    pub fn load(cfg: &GeneratorConfig, hooks: Arc<TestHooks>) -> Self {
        let Some(graph_path) = &cfg.graph else { return Self::disabled() };
        if cfg.view_road.is_none() || cfg.view_gravel.is_none() || cfg.coverage_poly.is_none() {
            // Un graphe sans ses vues ni son polygone : éteint, sans empêcher le service de démarrer.
            tracing::warn!("generator graph configured without view_road, view_gravel or coverage_poly");
            return disabled(DisabledReason::NotConfigured);
        }
        let started = std::time::Instant::now();
        let open = |p: &Option<std::path::PathBuf>| -> Option<MappedGraph> {
            let p = p.as_ref()?;
            match MappedGraph::open_mmap(p) {
                Ok(g) => Some(g),
                Err(e) => {
                    tracing::warn!(error = %e, file = %p.display(), "generator graph file unreadable");
                    None
                }
            }
        };
        let (Some(base), Some(road), Some(gravel)) =
            (open(&Some(graph_path.clone())), open(&cfg.view_road), open(&cfg.view_gravel))
        else {
            return disabled(DisabledReason::GraphUnreadable);
        };
        let identity = base.graph_identity();
        let tool_version = base.header().tool_version.clone();
        let params = Params {
            window_margin_m: cfg.window_margin_m,
            index_threads: cfg.index_threads,
            ..Params::default()
        };
        if cfg.distance_m_max > params.max_distance_m {
            // Au-delà, l'index creux des composantes ne répond plus exactement (lot 6, §6) : mieux vaut éteindre.
            tracing::warn!(
                distance_m_max = cfg.distance_m_max,
                engine_max_m = params.max_distance_m,
                "generator distance_m_max exceeds the engine bound"
            );
            return disabled(DisabledReason::NotConfigured);
        }
        let engine = match Engine::new(base, road, gravel, params) {
            Ok(e) => e,
            Err(ord_loops::EngineError::ViewMismatch | ord_loops::EngineError::OverlayMismatch) => {
                tracing::warn!("generator view does not belong to the configured graph");
                return disabled(DisabledReason::ViewMismatch);
            }
            Err(e) => {
                tracing::warn!(error = %e, "generator view unusable");
                return disabled(DisabledReason::GraphUnreadable);
            }
        };
        if std::env::var_os("MALLOC_MMAP_THRESHOLD_").is_none() {
            // Le budget de mémoire du générateur ne tient que si glibc rend les grands blocs au système (lot 6).
            tracing::warn!(
                "MALLOC_MMAP_THRESHOLD_ is not set: evicted windows may stay resident and exceed the memory budget \
                 (set MALLOC_ARENA_MAX=2 MALLOC_MMAP_THRESHOLD_=1048576, as the Docker image does)"
            );
        }
        let poly = match cfg.coverage_poly.as_deref().map(Poly::read) {
            Some(Ok(p)) => p,
            Some(Err(e)) => {
                tracing::warn!(error = %e, "generator coverage polygon unreadable");
                return disabled(DisabledReason::PolygonMismatch);
            }
            None => return disabled(DisabledReason::PolygonMismatch),
        };
        let (Some(pb), Some((nb, inside))) = (poly.bbox(), node_extent(engine.base(), &poly)) else {
            return disabled(DisabledReason::PolygonMismatch);
        };
        if !polygon_fits(pb, nb, inside) {
            tracing::warn!(
                poly_bbox = ?pb,
                nodes_bbox = ?nb,
                nodes_inside = inside,
                "generator coverage polygon does not match the graph extent"
            );
            return disabled(DisabledReason::PolygonMismatch);
        }
        tracing::info!(nodes_inside = inside, "generator coverage polygon checked");
        // Arêtes plus larges qu'une cellule : listées (une sur la France), l'index par cellule les ajoute. Au-delà d'un
        // plafond, repli sur l'index entier.
        let wide_edges = ord_graph_match::CellIndex::exceptions(engine.base()).len();
        let cell_index_ok = wide_edges <= MAX_WIDE_EDGES;
        if !cell_index_ok {
            tracing::warn!(
                wide_edges,
                "too many edges wider than a cell: deposits build the whole spatial index"
            );
        }
        let graph_index = ord_loops_check::GraphIndex::build(engine.base());
        let gid = graph_id(&identity);
        let matcher_hash = ord_graph_match::agg::params_hash();
        // Le budget de mémoire compte les équipes résidentes que le stockage lui déclare ; elles ont leurs propres plafonds
        // (nombre, octets) et aucune réservation ne les évince.
        let windows = Arc::new(windows::WindowManager::new(cfg.memory_budget_mb * 1_000_000));
        let store = store::Store::new(
            store::StoreConfig {
                dir: cfg.overlay_dir.clone(),
                ttl: Duration::from_secs(cfg.overlay_ttl_days * 86_400),
                stale_grace: Duration::from_secs(cfg.stale_grace_h * 3_600),
                max_namespaces: cfg.storage.max_namespaces,
                max_overlays_per_namespace: cfg.storage.max_overlays_per_namespace,
                max_namespace_bytes: cfg.storage.max_namespace_mb * 1024 * 1024,
                allowed_namespaces: cfg.storage.allowed_namespaces.clone(),
                team_cache_entries: cfg.team_cache_entries,
                team_cache_bytes: cfg.team_cache_mb * 1_000_000,
            },
            gid.clone(),
            identity,
            matcher_hash,
            Arc::clone(&hooks),
            Arc::clone(&windows) as Arc<dyn store::TeamMemory>,
        );
        let _ = std::fs::create_dir_all(&cfg.overlay_dir);
        // Au démarrage, tous les temporaires d'écritures interrompues partent.
        store.purge(std::time::SystemTime::now(), true);
        let capabilities = GeneratorCapabilities {
            enabled: true,
            reason: None,
            graph: Some(GraphInfo {
                id: gid.clone(),
                data_date: iso8601(identity.replication_timestamp),
                data_timestamp: identity.replication_timestamp,
                tool_version,
            }),
            coverage: Some(CoverageInfo { bbox: pb, polygon: poly_geojson(&poly) }),
            ride_types: vec!["road", "gravel", "mtb"],
            cursor: Some(CursorInfo {
                strict_at: 0.0,
                discovery_at: 1.0,
                lambda_max: cursor::LAMBDA_MAX,
                lambda_min: cursor::LAMBDA_MIN,
                familiarity: f64::from(cursor::FAMILIARITY),
                step: cursor::STEP,
            }),
            limits: Some(GeneratorLimits {
                distance_m_min: cfg.distance_m_min,
                distance_m_max: cfg.distance_m_max,
                budget_ms: cfg.budget_ms,
                max_concurrent: cfg.max_concurrent,
                max_queued: cfg.max_queued,
                request_body_bytes: GENERATE_BODY_MAX,
                deposit: DepositLimits {
                    max_body_bytes: cfg.deposit.max_body_bytes,
                    max_document_bytes: cfg.deposit.max_document_bytes,
                    max_tracks: cfg.deposit.max_tracks,
                    budget_s: cfg.deposit.budget_s,
                    idle_timeout_s: cfg.deposit.idle_timeout_s,
                    queue_wait_s: cfg.deposit.queue_wait_s,
                },
                storage: StorageLimits {
                    max_namespaces: cfg.storage.max_namespaces,
                    max_overlays_per_namespace: cfg.storage.max_overlays_per_namespace,
                    max_namespace_mb: cfg.storage.max_namespace_mb,
                    max_total_mb: cfg.storage.max_total_mb,
                },
            }),
            overlay_ttl_days: Some(cfg.overlay_ttl_days),
            matcher_hash: Some(format!("{matcher_hash:016x}")),
            ascent: false,
        };
        tracing::info!(
            graph = %gid,
            nodes = identity.nodes,
            edges = identity.edges,
            cell_index = cell_index_ok,
            wide_edges,
            index_ms = engine.index_timings_ms().iter().map(|t| t.0).max().unwrap_or(0),
            scc_ms = engine.index_timings_ms().iter().map(|t| t.1).max().unwrap_or(0),
            index_mb = engine.index_bytes() / 1_000_000,
            memory_budget_mb = cfg.memory_budget_mb,
            load_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "generator ready"
        );
        Self {
            state: GeneratorState::Ready(Arc::new(Ready {
                cfg: cfg.clone(),
                engine,
                poly,
                store,
                cell_index_ok,
                graph_index,
                windows,
                gen_slots: Arc::new(Semaphore::new(cfg.max_concurrent)),
                gen_waiting: AtomicUsize::new(0),
                deposit_slots: Arc::new(Semaphore::new(cfg.deposit.max_concurrent)),
                hooks,
                stopping: AtomicBool::new(false),
                graph_id: gid,
                capabilities,
            })),
        }
    }

    pub(crate) fn ready(&self) -> Option<&Arc<Ready>> {
        match &self.state {
            GeneratorState::Ready(r) => Some(r),
            GeneratorState::Disabled(_) => None,
        }
    }

    pub fn capabilities(&self) -> GeneratorCapabilities {
        match &self.state {
            GeneratorState::Ready(r) => r.capabilities.clone(),
            GeneratorState::Disabled(reason) => GeneratorCapabilities {
                enabled: false,
                reason: Some(*reason),
                graph: None,
                coverage: None,
                ride_types: vec!["road", "gravel", "mtb"],
                cursor: None,
                limits: None,
                overlay_ttl_days: None,
                matcher_hash: None,
                ascent: false,
            },
        }
    }

    /// Purge périodique (si un runtime tokio tourne) : `purge_interval_s`.
    pub fn spawn_purge(&self) {
        let Some(r) = self.ready().cloned() else { return };
        let Ok(handle) = tokio::runtime::Handle::try_current() else { return };
        let every = Duration::from_secs(r.cfg.purge_interval_s);
        handle.spawn(async move {
            let mut tick = tokio::time::interval(every);
            tick.tick().await; // la purge de démarrage a déjà eu lieu
            loop {
                tick.tick().await;
                let store = Arc::clone(&r.store);
                let _ = tokio::task::spawn_blocking(move || store.purge(std::time::SystemTime::now(), false))
                    .await;
            }
        });
    }

    /// Le service s'arrête : les constructions de fond qui attendent de la place dans le budget s'interrompent (une
    /// construction déjà commencée finit, elle ne dure que quelques secondes). Les requêtes en vol ont leur échéance.
    pub fn begin_shutdown(&self) {
        if let Some(r) = self.ready() {
            r.stopping.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Hôte des crochets de test.
    #[doc(hidden)]
    pub fn hooks(&self) -> Option<Arc<TestHooks>> {
        self.ready().map(|r| Arc::clone(&r.hooks))
    }

    /// Une place de calcul, comme la prend une génération (tests de la file d'attente).
    #[doc(hidden)]
    pub async fn test_compute_slot(
        &self,
        wait: Duration,
    ) -> Option<Result<tokio::sync::OwnedSemaphorePermit, crate::problem::Problem>> {
        let r = self.ready()?;
        Some(r.slot(std::time::Instant::now() + wait).await)
    }

    /// Purge à l'instant donné (tests).
    #[doc(hidden)]
    pub fn purge_at(&self, now: std::time::SystemTime) -> Option<store::PurgeReport> {
        self.ready().map(|r| r.store.purge(now, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_known_dates() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(iso8601(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601(-1), "1969-12-31T23:59:59Z");
    }

    /// Boîtes `[lon0, lat0, lon1, lat1]` : celles du polygone Geofabrik de la France et de son graphe (écart de 1,8° à
    /// l'ouest sur l'Atlantique), puis du polygone régional.
    #[test]
    fn polygon_fits_tolerates_sea_but_not_a_larger_region() {
        let france_nodes = [-5.2, 41.3, 9.6, 51.1];
        let france_poly = [-6.94, 41.24, 9.9, 51.43];
        assert!(polygon_fits(france_poly, france_nodes, 1.0));
        // Le polygone de la France sur un graphe régional : refusé.
        let region_nodes = [-2.6, 46.3, 0.9, 48.5];
        assert!(!polygon_fits(france_poly, region_nodes, 1.0));
        // Le polygone régional sur le graphe de la France : des nœuds hors polygone, refusé.
        assert!(!polygon_fits(region_nodes, france_nodes, 0.1));
        // La région, sa mer à 9 km : toujours acceptée.
        assert!(polygon_fits([-2.67, 46.22, 0.92, 48.57], [-2.6, 46.3, 0.85, 48.5], 1.0));
    }
}
