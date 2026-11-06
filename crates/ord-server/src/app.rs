//! Routeur public et routeur interne (T01 §4.1).

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{Extension, FromRef, Request, State};
use axum::http::{header, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, MethodRouter};
use axum::{Json, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use ord_data::catalog::StaticCatalog;
use ord_domain::budget::{Deadline, RequestId};
use ord_domain::catalog::{DataCatalog, Readiness};
use ord_domain::ctx::RequestCtx;
use ord_pipeline::Orchestrator;
use serde::Serialize;
use tower::ServiceBuilder;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::compression::CompressionLayer;
use tower_http::decompression::RequestDecompressionLayer;
use tower_http::map_request_body::MapRequestBodyLayer;
use tower_http::trace::TraceLayer;
use utoipa::ToSchema;

use crate::capabilities::{AttributionDoc, Capabilities};
use crate::config::{Config, LimitsConfig};
use crate::dto::{to_dto, RouteDetailsRequestV1, RouteDetailsResponseV1};
use crate::problem::{problem_for_status, with_request_id, Problem, ProblemBody};
use crate::strict_json::{MaxBodyBytes, StrictJson};
use crate::validate::validate;
use crate::wiring::{Startup, StartupError, Wiring};

pub const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
const MAX_REQUEST_ID_LEN: usize = 128;

/// État partagé, en lecture seule.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    limits: LimitsConfig,
    budget: Duration,
    /// `Retry-After` du 503 `overloaded` (T01 §3.2).
    overloaded_retry_after: Duration,
    catalog: Arc<dyn DataCatalog>,
    orchestrator: Orchestrator,
    capabilities: Bytes,
    attribution: Bytes,
    attribution_text: String,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").field("limits", &self.inner.limits).finish_non_exhaustive()
    }
}

impl AppState {
    /// Construit l'état, les sources et l'orchestrateur, et précalcule les documents servis tels
    /// quels. Aucune I/O ici, ni lecture de démarrage (version Mapterhorn, index de couverture) :
    /// voir `bootstrap`. Le cache disque est créé à la première écriture.
    pub fn new(config: &Config) -> Result<Self, StartupError> {
        Self::with_startup(config, Wiring::default(), Startup::default())
    }

    pub fn with_wiring(config: &Config, wiring: Wiring) -> Result<Self, StartupError> {
        Self::with_startup(config, wiring, Startup::default())
    }

    /// Lectures de démarrage (en ligne, ou cache disque hors ligne), puis construction.
    pub async fn bootstrap(config: &Config, wiring: Wiring) -> Result<Self, StartupError> {
        let startup = crate::wiring::startup(config).await?;
        Self::with_startup(config, wiring, startup)
    }

    /// Orchestrateur du service (diagnostics du banc).
    pub fn orchestrator(&self) -> &Orchestrator {
        &self.inner.orchestrator
    }

    pub fn with_startup(config: &Config, wiring: Wiring, startup: Startup) -> Result<Self, StartupError> {
        let catalog = Arc::new(StaticCatalog::new(crate::wiring::configured_sources(config, &startup)));
        let capabilities = Capabilities::build(config, &catalog);
        let attribution = AttributionDoc::build(catalog.as_ref());
        let orchestrator = crate::wiring::orchestrator(config, &startup, Arc::clone(&catalog), wiring)?;
        Ok(Self {
            inner: Arc::new(Inner {
                limits: config.limits.clone(),
                budget: Duration::from_secs(config.budget.request_budget_s),
                overloaded_retry_after: Duration::from_secs(config.budget.overloaded_retry_after_s),
                capabilities: to_json_bytes(&capabilities),
                attribution_text: attribution.text.clone(),
                attribution: to_json_bytes(&attribution),
                catalog,
                orchestrator,
            }),
        })
    }
}

fn to_json_bytes<T: Serialize>(value: &T) -> Bytes {
    Bytes::from(serde_json::to_vec(value).unwrap_or_default())
}

impl FromRef<AppState> for MaxBodyBytes {
    fn from_ref(state: &AppState) -> Self {
        MaxBodyBytes(state.inner.limits.max_body_bytes)
    }
}

/// Routes publiques ; le contrat OpenAPI en décrit exactement l'ensemble (test `contract`).
pub fn public_routes() -> Vec<(&'static str, Method, MethodRouter<AppState>)> {
    vec![
        ("/v1/route-details", Method::POST, post(route_details)),
        ("/v1/capabilities", Method::GET, get(capabilities)),
        ("/v1/attribution", Method::GET, get(attribution)),
        ("/health/live", Method::GET, get(health_live)),
        ("/health/ready", Method::GET, get(health_ready)),
    ]
}

/// Routeur du port public, couches comprises.
pub fn router(state: AppState) -> Router {
    let mut router = Router::new();
    for (path, _, method_router) in public_routes() {
        router = router.route(path, method_router);
    }
    router
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        // Le corps est borné par `StrictJson`, après décompression.
        .layer(axum::extract::DefaultBodyLimit::disable())
        .layer(
            ServiceBuilder::new()
                .layer(RequestDecompressionLayer::new())
                .layer(MapRequestBodyLayer::new(Body::new)),
        )
        .layer(middleware::from_fn(problem_fallback))
        .layer(CompressionLayer::new())
        .layer(CatchPanicLayer::custom(|_panic: Box<dyn std::any::Any + Send>| {
            tracing::error!("handler panicked");
            Problem::internal().into_response()
        }))
        .layer(TraceLayer::new_for_http().make_span_with(|req: &Request| {
            let id = req.headers().get(X_REQUEST_ID).and_then(|v| v.to_str().ok()).unwrap_or("");
            tracing::info_span!("request", request_id = id, method = %req.method(), path = req.uri().path())
        }))
        .layer(middleware::from_fn(crate::observability::record_http_metrics))
        .layer(middleware::from_fn(request_id))
        .with_state(state)
}

/// Routeur du port interne, hors contrat public : `/metrics` et `/openapi.json`.
pub fn internal_router(metrics: Option<PrometheusHandle>) -> Router {
    let contract = Bytes::from(crate::openapi::render_contract());
    let mut router = Router::new().route(
        "/openapi.json",
        get(move || async move { ([(header::CONTENT_TYPE, "application/json")], contract) }),
    );
    if let Some(handle) = metrics {
        router = router.route("/metrics", get(move || async move { handle.render() }));
    }
    router.fallback(not_found)
}

/// `X-Request-Id` repris s'il est présent et court, sinon UUID v7 ; renvoyé dans la réponse.
async fn request_id(mut req: Request, next: Next) -> Response {
    let id = req
        .headers()
        .get(X_REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty() && s.len() <= MAX_REQUEST_ID_LEN && s.bytes().all(|b| b.is_ascii_graphic()))
        .map_or_else(|| uuid::Uuid::now_v7().to_string(), str::to_owned);
    let header_value = HeaderValue::from_str(&id).ok();
    if let Some(v) = &header_value {
        req.headers_mut().insert(X_REQUEST_ID, v.clone());
    }
    req.extensions_mut().insert(RequestId(id.clone()));
    let mut response = with_request_id(id, next.run(req)).await;
    if let Some(v) = header_value {
        response.headers_mut().insert(X_REQUEST_ID, v);
    }
    response
}

/// Met au format RFC 9457 les erreurs produites par les couches (décompression…) ou le routage.
async fn problem_fallback(req: Request, next: Next) -> Response {
    let response = next.run(req).await;
    let status = response.status();
    let plain = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_none_or(|ct| ct.starts_with("text/plain"));
    if !(status.is_client_error() || status.is_server_error()) || !plain {
        return response;
    }
    let mut problem = problem_for_status(status).into_response();
    if let Some(allow) = response.headers().get(header::ALLOW) {
        problem.headers_mut().insert(header::ALLOW, allow.clone());
    }
    problem
}

async fn not_found() -> Problem {
    Problem::not_found()
}

async fn method_not_allowed() -> Problem {
    Problem::method_not_allowed()
}

// Calcul des détails d'un parcours. Incrément 1 : profil d'élévation Mapterhorn seul, modes
// `input` et `distance` (`input_with_breakpoints` rend les seuls points d'entrée).
#[utoipa::path(
    post,
    path = "/v1/route-details",
    summary = "Compute elevation and typology along a route",
    description = "Validates the route and its options before any computation. Unknown keys, \
out-of-bounds options and coordinates outside the Web Mercator range give a 400 that lists every \
faulty field by JSON pointer; input bounds give a 413 with `details.limit`. A 200 carries the \
elevation profile (`points`), the route aggregates (`summary`), the terrain provenance and warnings \
(`quality`) and the attribution.",
    operation_id = "postRouteDetails",
    tag = "route",
    request_body(content = RouteDetailsRequestV1, content_type = "application/json"),
    responses(
        (status = 200, description = "Route details", body = RouteDetailsResponseV1),
        (status = 400, description = "Invalid input (format, unknown keys, bounds)", body = ProblemBody, content_type = "application/problem+json"),
        (status = 413, description = "Route too large (`details.limit`)", body = ProblemBody, content_type = "application/problem+json"),
        (status = 415, description = "Content-Type is not application/json", body = ProblemBody, content_type = "application/problem+json"),
        (status = 422, description = "No data from any source on the route", body = ProblemBody, content_type = "application/problem+json"),
        (status = 500, description = "Unexpected error", body = ProblemBody, content_type = "application/problem+json"),
        (status = 503, description = "Data unavailable (`data_unavailable`, `details.source`) or too many requests queued (`overloaded`); always with `Retry-After`", body = ProblemBody, content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "Seconds to wait before retrying"))),
        (status = 504, description = "Request budget exceeded (`timeout`), in queue or during computation; never a truncated response", body = ProblemBody, content_type = "application/problem+json"),
    )
)]
async fn route_details(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    StrictJson(dto): StrictJson<RouteDetailsRequestV1>,
) -> Result<Response, Problem> {
    let started = Instant::now();
    let request = validate(dto, &state.inner.limits)?;
    let sections = request.options.sections;
    let profile = request.options.profile;
    let input_points = request.input_len;
    let ctx = RequestCtx::new(request_id.clone(), Deadline::after(state.inner.budget));
    // Client parti : ce futur est lâché, l'orchestrateur annule le calcul (T01 §3.3).
    let result = state.inner.orchestrator.run(request, &ctx).await;
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let result = result.map_err(|e| {
        let mut p = Problem::from(e.clone());
        if e == ord_pipeline::PipelineError::Overloaded {
            p.retry_after = Some(state.inner.overloaded_retry_after);
        }
        p
    });
    // Ligne de synthèse, sans coordonnée ni clé de tuile (T01 §8) ; 5xx et replis subis en `warn`.
    {
        let log = ctx.log();
        let c = log.counters;
        let (status, code) = match &result {
            Ok(r) if r.status == ord_core::assemble::Status::Complete => ("complete", ""),
            Ok(_) => ("partial", ""),
            Err(p) => ("error", p.code.as_str()),
        };
        // Avertissements par code (jamais leur plage : des distances seulement, mais inutiles ici).
        let mut warnings = std::collections::BTreeMap::<ord_domain::report::WarningCode, usize>::new();
        if let Ok(r) = &result {
            for w in &r.warnings {
                *warnings.entry(w.code).or_default() += 1;
            }
        }
        let degraded = result.as_ref().map_or_else(
            |p| p.code.status().is_server_error(),
            |r| r.status != ord_core::assemble::Status::Complete || !log.failures.is_empty(),
        );
        macro_rules! summary {
            ($level:ident) => {
                tracing::$level!(
                    input_points,
                    tiles_memory = c.memory_hits,
                    tiles_disk = c.disk_hits,
                    tiles_fetched = c.fetched,
                    tiles_not_modified = c.not_modified,
                    tiles_failed = log.failures.len(),
                    tiles_stale = log.stale_served.len(),
                    status,
                    code,
                    warnings = ?warnings,
                    duration_ms,
                    "route details"
                )
            };
        }
        if degraded {
            summary!(warn);
        } else {
            summary!(info);
        }
    }
    let report = result?;
    let body = to_dto(&report, sections, profile, &request_id, duration_ms, &state.inner.attribution_text);
    Ok(Json::<RouteDetailsResponseV1>(body).into_response())
}

// Bornes, défauts, valeurs d'enum, capacités des données et sources d'élévation.
#[utoipa::path(
    get,
    path = "/v1/capabilities",
    summary = "Service capabilities",
    description = "Input bounds, applied defaults, option bounds, enum values, data capabilities \
and elevation sources.",
    operation_id = "getCapabilities",
    tag = "service",
    responses((status = 200, description = "Service capabilities", body = Capabilities))
)]
async fn capabilities(State(state): State<AppState>) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/json")], state.inner.capabilities.clone())
}

// Attributions complètes des sources configurées.
#[utoipa::path(
    get,
    path = "/v1/attribution",
    summary = "Data attributions",
    description = "Full attributions of the configured data sources, to be relayed to end users.",
    operation_id = "getAttribution",
    tag = "service",
    responses((status = 200, description = "Attributions of the configured sources", body = AttributionDoc))
)]
async fn attribution(State(state): State<AppState>) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/json")], state.inner.attribution.clone())
}

/// Liveness status.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct HealthStatus {
    pub status: &'static str,
}

// Le processus répond.
#[utoipa::path(
    get,
    path = "/health/live",
    summary = "Liveness probe",
    operation_id = "getHealthLive",
    tag = "health",
    responses((status = 200, description = "Process is up", body = HealthStatus))
)]
async fn health_live() -> Json<HealthStatus> {
    Json(HealthStatus { status: "ok" })
}

// Les données configurées sont lisibles.
#[utoipa::path(
    get,
    path = "/health/ready",
    summary = "Readiness probe",
    description = "Ready when the configured local data is readable; remote sources are not checked.",
    operation_id = "getHealthReady",
    tag = "health",
    responses(
        (status = 200, description = "Ready", body = Readiness),
        (status = 503, description = "Configured data not readable", body = Readiness),
    )
)]
async fn health_ready(State(state): State<AppState>) -> (StatusCode, Json<Readiness>) {
    let readiness = state.inner.catalog.readiness();
    let status = if readiness.ready { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, Json(readiness))
}

/// Chemins des handlers pour la dérive OpenAPI.
pub(crate) mod paths {
    pub(crate) use super::{
        __path_attribution, __path_capabilities, __path_health_live, __path_health_ready,
        __path_route_details,
    };
}
