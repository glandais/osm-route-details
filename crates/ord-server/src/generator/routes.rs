//! Routes `/v1/generator/*` (api 1.9) : dépôt, état et effacement de la surcouche d'une équipe, génération.
//!
//! **Aucun identifiant dans le chemin ni dans la requête** : `TraceLayer` journalise le chemin, les métriques le
//! gabarit de route ; les en-têtes `Ord-Namespace-Key` et `Ord-Team` ne sont jamais journalisés.
//! Les `///` des types sérialisés sont recopiés dans le contrat OpenAPI : ils sont en anglais.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use ord_loops::{Bearing8, LatLon, RideType};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::crypto::{decode_key, valid_team_ref, Credentials};
use super::deposit::DepositReportV1;
use super::generate::{unavailable, GenInput};
use super::store::{StaleReason, State as OverlayState};
use super::{cursor, iso8601, Ready, GENERATE_BODY_MAX};
use crate::app::AppState;
use crate::problem::{FieldError, Problem, ProblemCode};
use crate::strict_json::read_json;

pub const KEY_HEADER: &str = "ord-namespace-key";
pub const TEAM_HEADER: &str = "ord-team";
pub const DIGEST_HEADER: &str = "ord-source-digest";
const NDJSON: &str = "application/x-ndjson";

/// Compass bearing asked for (one of the eight principal directions).
#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
pub enum BearingV1 {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl From<BearingV1> for Bearing8 {
    fn from(b: BearingV1) -> Self {
        match b {
            BearingV1::N => Self::N,
            BearingV1::NE => Self::NE,
            BearingV1::E => Self::E,
            BearingV1::SE => Self::SE,
            BearingV1::S => Self::S,
            BearingV1::SW => Self::SW,
            BearingV1::W => Self::W,
            BearingV1::NW => Self::NW,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RideTypeV1 {
    Road,
    Gravel,
    Mtb,
}

#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
pub struct GeoPointV1 {
    pub lat: f64,
    pub lon: f64,
}

/// Route generation request.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct GenerateRequestV1 {
    pub start: GeoPointV1,
    /// Arrival; the start (a loop) when absent or null.
    #[serde(default)]
    pub end: Option<GeoPointV1>,
    #[serde(default)]
    pub bearing: Option<BearingV1>,
    /// Target length, within `capabilities.generator.limits.distance_m_min` and `distance_m_max`.
    pub distance_m: f64,
    pub ride_type: RideTypeV1,
    /// From 0 (the team's roads only: strict) to 1 (discovery), rounded to 0.01. Below 1 the team overlay is
    /// required (headers `Ord-Namespace-Key` and `Ord-Team`); see `capabilities.generator.cursor`.
    pub cursor: f64,
    /// Seed of the choice among equivalent routes (default 0).
    #[serde(default)]
    pub seed: Option<u64>,
}

/// Mode applied by the engine for the cursor value.
#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModeV1 {
    /// Only edges the team has ridden (short holes bridged).
    Strict,
    /// The team's roads strongly preferred, OSM edges to connect them.
    TeamFirst,
    /// OSM without preference for the team.
    Discovery,
}

/// Whether the disc of radius D/2 around the start (and end) lies inside the covered polygon.
#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoverageV1 {
    Inside,
    /// The start (and end) are covered but the area a route may roam leaves the polygon.
    Partial,
}

/// The taste a generation was served with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TasteV1 {
    /// The team's own profile, learned when its overlay was deposited.
    Team,
    /// The common table of the engine (no team, or a team without a usable profile).
    Common,
}

/// The team overlay a generation used.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OverlayInfoV1 {
    pub deposited_at: String,
}

/// One generated route. Distances come from the independent checker, in metres.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct RouteV1 {
    /// Google polyline, precision 6 (the one `/v1/route-details` takes).
    pub polyline: String,
    pub distance_m: f64,
    /// Length on edges the team has ridden, and on short holes bridged between them.
    pub team_m: f64,
    pub bridged_m: f64,
    /// (team_m + bridged_m) / distance_m.
    pub team_share: f64,
    /// Length ridden twice (the way back shares the way out).
    pub tail_m: f64,
    pub crossings: u32,
    pub unpaved_m: f64,
    pub unpaved_share: f64,
    pub presumed_surface_m: f64,
    /// Gap to the requested bearing, in degrees; null without a bearing.
    pub bearing_deviation_deg: Option<f64>,
    /// `turnaround` or `polygon`.
    pub family: &'static str,
    /// Distance from the asked start and end to the graph, in metres.
    pub start_snap_m: f64,
    pub end_snap_m: f64,
    /// Always null: the engine has no elevation (`capabilities.generator.ascent` is false).
    pub ascent_m: Option<f64>,
    pub descent_m: Option<f64>,
}

/// Generated routes, best first (at most 3).
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct GenerateResponseV1 {
    pub graph_id: String,
    pub mode: ModeV1,
    /// Cost of an edge outside the team's roads (1 to 3); null in strict mode.
    pub lambda: Option<f64>,
    pub coverage: CoverageV1,
    /// The overlay the routes were measured against; null when none was used.
    pub overlay: Option<OverlayInfoV1>,
    /// The taste of the weights: `team` when the team has a taste profile, `common` otherwise.
    pub taste: TasteV1,
    /// Routes the engine produced and the independent checker refused (never returned).
    pub dropped: u32,
    pub routes: Vec<RouteV1>,
}

/// State of a team overlay.
#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OverlayStateV1 {
    Absent,
    Ready,
    /// Built for another graph or by another matcher, or without a usable taste profile: deposit it again.
    Stale,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StaleReasonV1 {
    Graph,
    Matcher,
    /// No usable taste profile (absent, or learned with other settings), or only partial ones (1 or 2 steps) for more
    /// than 7 days: a new deposit learns it. Generations are served meanwhile, with the profile if any, else the
    /// common table.
    Profile,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProfileStateV1 {
    Present,
    Absent,
    Incompatible,
}

/// The taste profile of a team, learned when its overlay was deposited.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ProfileStatusV1 {
    pub state: ProfileStateV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learned_at: Option<String>,
    /// Steps of learning finished (0 to 3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<u8>,
    /// Loops it was learned from, all ride types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loops: Option<u32>,
}

/// Status of a team overlay. Deposit again when `state` is not `ready`, when `source_digest` differs from the
/// backend's, or when `expires_at` is near.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OverlayStatusV1 {
    pub state: OverlayStateV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_reason: Option<StaleReasonV1>,
    pub graph_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deposited_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edges: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_km: Option<f64>,
    /// The taste profile; absent when there is no overlay or profiles are disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileStatusV1>,
}

fn header_error(name: &str, message: &str) -> Problem {
    Problem::invalid_input("invalid header", vec![FieldError::new(format!("/headers/{name}"), message)])
}

/// La clé de l'environnement, puis ce qu'on en dérive. Jamais la valeur dans l'erreur.
fn credentials(headers: &HeaderMap, team: bool) -> Result<Credentials, Problem> {
    let key = headers
        .get(KEY_HEADER)
        .ok_or_else(|| header_error("Ord-Namespace-Key", "required"))?
        .to_str()
        .ok()
        .and_then(decode_key)
        .ok_or_else(|| header_error("Ord-Namespace-Key", "must be 43 base64url characters (32 bytes)"))?;
    if !team {
        return Ok(Credentials::derive_namespace(&key));
    }
    let team = headers
        .get(TEAM_HEADER)
        .ok_or_else(|| header_error("Ord-Team", "required"))?
        .to_str()
        .ok()
        .filter(|t| valid_team_ref(t))
        .ok_or_else(|| header_error("Ord-Team", "must match [A-Za-z0-9_-]{1,64}"))?;
    Ok(Credentials::derive(&key, team))
}

fn source_digest(headers: &HeaderMap) -> Result<Option<String>, Problem> {
    let Some(v) = headers.get(DIGEST_HEADER) else { return Ok(None) };
    v.to_str()
        .ok()
        .filter(|s| (1..=128).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_graphic()))
        .map(|s| Some(s.to_owned()))
        .ok_or_else(|| header_error("Ord-Source-Digest", "must be 1 to 128 printable characters"))
}

fn is_ndjson(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case(NDJSON))
}

fn ready(state: &AppState) -> Result<&std::sync::Arc<Ready>, Problem> {
    state.generator().ready().ok_or_else(unavailable)
}

// Dépôt de la surcouche d'une équipe.
#[utoipa::path(
    put,
    path = "/v1/generator/overlay",
    summary = "Deposit a team overlay",
    description = "Streams the team's `network_edges` documents (one NDJSON line per track: \
`{\"type\":\"ROAD\",\"excluded\":false,\"network_edges\":{...}}`, then the last line `{\"end\":{\"tracks\":N}}`). \
The service matches them to OSM edges, aggregates them and stores the overlay, encrypted with a key derived from \
`Ord-Namespace-Key`. The previous overlay keeps serving until the new one is complete; a failed deposit leaves it \
untouched. One deposit at a time per team and one in the whole service. `Content-Encoding: gzip` is accepted; the \
limits apply after decompression.",
    operation_id = "putGeneratorOverlay",
    tag = "generator",
    params(
        ("Ord-Namespace-Key" = String, Header, description = "Environment secret: 32 random bytes, base64url without padding (43 characters). Held by the backend of one environment only; never logged."),
        ("Ord-Team" = String, Header, description = "Opaque team reference, [A-Za-z0-9_-]{1,64}; never logged."),
        ("Ord-Source-Digest" = Option<String>, Header, description = "Opaque digest of the source data, returned by the status route.")
    ),
    request_body(content = String, content_type = "application/x-ndjson"),
    responses(
        (status = 200, description = "Overlay stored", body = DepositReportV1),
        (status = 400, description = "Invalid header, unreadable line, missing end line or wrong track count (`details.errors[].pointer` = `/lines/<n>`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 409, description = "A deposit of this team is already running (`deposit_in_progress`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 413, description = "Body, document or track count above the limits (`details.limit`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 415, description = "Content-Type is not application/x-ndjson", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 504, description = "Deposit budget or idle timeout exceeded (`timeout`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 507, description = "Storage quota reached (`insufficient_storage`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 503, description = "Generator unavailable (`data_unavailable`, `details.source = graph`) or deposit capacity in use after the queue wait (`overloaded`); with `Retry-After`", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
    )
)]
pub(crate) async fn put_overlay(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<DepositReportV1>, Problem> {
    let ready = ready(&state)?;
    let creds = credentials(&headers, true)?;
    let digest = source_digest(&headers)?;
    if !is_ndjson(&headers) {
        return Err(Problem::unsupported_media_type("Content-Type must be application/x-ndjson"));
    }
    ready.deposit(creds, digest, body).await.map(Json)
}

// État de la surcouche d'une équipe.
#[utoipa::path(
    get,
    path = "/v1/generator/overlay",
    summary = "Status of a team overlay",
    description = "Always 200: `absent` covers a team never deposited, deposited under another key, deleted or \
purged, without telling them apart.",
    operation_id = "getGeneratorOverlay",
    tag = "generator",
    params(
        ("Ord-Namespace-Key" = String, Header, description = "Environment secret (see the deposit route)."),
        ("Ord-Team" = String, Header, description = "Opaque team reference.")
    ),
    responses(
        (status = 200, description = "Overlay status", body = OverlayStatusV1),
        (status = 400, description = "Invalid header", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 503, description = "Generator unavailable (`data_unavailable`, `details.source = graph`)", body = crate::problem::ProblemBody, content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "Seconds to wait before retrying"))),
    )
)]
pub(crate) async fn get_overlay(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<OverlayStatusV1>, Problem> {
    let ready = ready(&state)?;
    let creds = credentials(&headers, true)?;
    let ready = std::sync::Arc::clone(ready);
    let status = tokio::task::spawn_blocking(move || ready.store.status(&creds).into_v1(&ready))
        .await
        .map_err(|_| Problem::internal())?;
    Ok(Json(status))
}

impl super::store::Status {
    fn into_v1(self, ready: &Ready) -> OverlayStatusV1 {
        let ttl = i64::try_from(ready.store.ttl().as_secs()).unwrap_or(i64::MAX);
        let (state, stale_reason) = match self.state {
            OverlayState::Absent => (OverlayStateV1::Absent, None),
            OverlayState::Ready => (OverlayStateV1::Ready, None),
            OverlayState::Stale(StaleReason::Graph) => (OverlayStateV1::Stale, Some(StaleReasonV1::Graph)),
            OverlayState::Stale(StaleReason::Matcher) => {
                (OverlayStateV1::Stale, Some(StaleReasonV1::Matcher))
            }
            OverlayState::Stale(StaleReason::Profile) => {
                (OverlayStateV1::Stale, Some(StaleReasonV1::Profile))
            }
        };
        let at = self.meta.as_ref().map(|m| m.deposited_at).or(self.file_time);
        OverlayStatusV1 {
            state,
            stale_reason,
            graph_id: ready.graph_id.clone(),
            deposited_at: at.map(iso8601),
            expires_at: at.map(|t| iso8601(t.saturating_add(ttl))),
            source_digest: self.meta.as_ref().and_then(|m| m.source_digest.clone()),
            edges: self.meta.as_ref().map(|m| m.edges),
            team_km: self.meta.as_ref().map(|m| m.team_km),
            profile: self.profile.as_ref().map(|p| ProfileStatusV1 {
                state: match p.state {
                    super::store::ProfileState::Present => ProfileStateV1::Present,
                    super::store::ProfileState::Absent => ProfileStateV1::Absent,
                    super::store::ProfileState::Incompatible => ProfileStateV1::Incompatible,
                },
                learned_at: p.learned_at.map(iso8601),
                steps: p.steps,
                loops: p.loops,
            }),
        }
    }
}

// Effacement de la surcouche d'une équipe.
#[utoipa::path(
    delete,
    path = "/v1/generator/overlay",
    summary = "Delete a team overlay",
    description = "Idempotent: 204 whether or not an overlay existed (and whichever environment holds a file of this \
name: a key can only name its own files).",
    operation_id = "deleteGeneratorOverlay",
    tag = "generator",
    params(
        ("Ord-Namespace-Key" = String, Header, description = "Environment secret (see the deposit route)."),
        ("Ord-Team" = String, Header, description = "Opaque team reference.")
    ),
    responses(
        (status = 204, description = "Deleted, or nothing to delete"),
        (status = 400, description = "Invalid header", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 409, description = "A deposit of this team is running (`deposit_in_progress`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 503, description = "Generator unavailable (`data_unavailable`, `details.source = graph`)", body = crate::problem::ProblemBody, content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "Seconds to wait before retrying"))),
    )
)]
pub(crate) async fn delete_overlay(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, Problem> {
    let ready = std::sync::Arc::clone(ready(&state)?);
    let creds = credentials(&headers, true)?;
    let busy = || Problem::new(ProblemCode::DepositInProgress, "a deposit of this team is running");
    tokio::task::spawn_blocking(move || {
        let _guard = ready.store.try_lock(&creds).ok_or_else(busy)?;
        ready.store.delete(&creds);
        Ok(StatusCode::NO_CONTENT)
    })
    .await
    .map_err(|_| Problem::internal())?
}

// Effacement de tout l'espace d'un environnement.
#[utoipa::path(
    delete,
    path = "/v1/generator/overlays",
    summary = "Delete every overlay of the environment",
    description = "Removes everything stored under the caller's key (key rotation, abandoned environment). \
Idempotent.",
    operation_id = "deleteGeneratorOverlays",
    tag = "generator",
    params(("Ord-Namespace-Key" = String, Header, description = "Environment secret (see the deposit route).")),
    responses(
        (status = 204, description = "Deleted, or nothing to delete"),
        (status = 400, description = "Invalid header", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 409, description = "A deposit of this environment is running (`deposit_in_progress`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 503, description = "Generator unavailable (`data_unavailable`, `details.source = graph`)", body = crate::problem::ProblemBody, content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "Seconds to wait before retrying"))),
    )
)]
pub(crate) async fn delete_overlays(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, Problem> {
    let ready = std::sync::Arc::clone(ready(&state)?);
    let creds = credentials(&headers, false)?;
    tokio::task::spawn_blocking(move || {
        if ready.store.namespace_busy(&creds) {
            return Err(Problem::new(
                ProblemCode::DepositInProgress,
                "a deposit of this environment is running",
            ));
        }
        ready.store.delete_namespace(&creds);
        Ok(StatusCode::NO_CONTENT)
    })
    .await
    .map_err(|_| Problem::internal())?
}

fn point(p: GeoPointV1, pointer: &str, errors: &mut Vec<FieldError>) -> LatLon {
    if !(p.lat.is_finite() && (-90.0..=90.0).contains(&p.lat)) {
        errors.push(FieldError::new(format!("{pointer}/lat"), "must be within [-90, 90]"));
    }
    if !(p.lon.is_finite() && (-180.0..=180.0).contains(&p.lon)) {
        errors.push(FieldError::new(format!("{pointer}/lon"), "must be within [-180, 180]"));
    }
    LatLon::new(p.lat, p.lon)
}

fn validate(req: &GenerateRequestV1, ready: &Ready) -> Result<(GenInput, u32), Problem> {
    let mut errors = Vec::new();
    let start = point(req.start, "/start", &mut errors);
    let end = req.end.map(|e| point(e, "/end", &mut errors));
    let (min, max) = (ready.cfg.distance_m_min, ready.cfg.distance_m_max);
    if !(req.distance_m.is_finite() && (min..=max).contains(&req.distance_m)) {
        errors.push(FieldError::new("/distance_m", "outside the distance bounds (see capabilities)"));
    }
    let h = cursor::hundredths(req.cursor);
    if h.is_none() {
        errors.push(FieldError::new("/cursor", "must be within [0, 1]"));
    }
    if !errors.is_empty() {
        return Err(Problem::invalid_input("invalid request", errors));
    }
    let h = h.unwrap_or(0);
    let input = GenInput {
        start,
        end,
        bearing: req.bearing.map(Into::into),
        distance_m: req.distance_m,
        ride: match req.ride_type {
            RideTypeV1::Road => RideType::Road,
            RideTypeV1::Gravel => RideType::Gravel,
            RideTypeV1::Mtb => RideType::Mtb,
        },
        applied: cursor::apply(h),
        seed: req.seed.unwrap_or(0),
    };
    Ok((input, h))
}

// Génération.
#[utoipa::path(
    post,
    path = "/v1/generator/routes",
    summary = "Generate routes",
    description = "Up to 3 routes from a start (and optionally an arrival), a bearing, a distance and a ride type. \
`cursor` runs from the team's roads (0, strict) to discovery (1); below 1 the team overlay must have been deposited \
and be `ready`. Every route is verified by an independent checker before the answer; a route that fails is dropped \
and counted in `dropped`. A request outside the covered area gets 422 `out_of_coverage`.",
    operation_id = "postGeneratorRoutes",
    tag = "generator",
    params(
        ("Ord-Namespace-Key" = Option<String>, Header, description = "Environment secret; required when `cursor` < 1."),
        ("Ord-Team" = Option<String>, Header, description = "Opaque team reference; required when `cursor` < 1.")
    ),
    request_body(content = GenerateRequestV1, content_type = "application/json"),
    responses(
        (status = 200, description = "Routes", body = GenerateResponseV1),
        (status = 400, description = "Invalid input (fields listed by JSON pointer, headers by `/headers/<name>`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 409, description = "The overlay is required (`overlay_required`, `details.state` = `absent` or `stale`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 413, description = "Request body above 64 KiB", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 415, description = "Content-Type is not application/json", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 422, description = "Out of coverage (`out_of_coverage`, `details.coverage = graph`) or no route (`generation_failed`, `details.reason`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 503, description = "Generator unavailable (`data_unavailable`, `details.source = graph`) or queue full (`overloaded`); with `Retry-After`", body = crate::problem::ProblemBody, content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "Seconds to wait before retrying"))),
        (status = 504, description = "Generation budget exceeded (`timeout`); never a partial route", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
    )
)]
pub(crate) async fn post_routes(State(state): State<AppState>, req: Request) -> Result<Response, Problem> {
    let ready = std::sync::Arc::clone(ready(&state)?);
    let headers = req.headers().clone();
    let body: GenerateRequestV1 = read_json(req, GENERATE_BODY_MAX).await?;
    let (input, h) = validate(&body, &ready)?;
    // Sous 1, l'équipe est indispensable ; à 1, elle sert seulement si elle est là (parts d'équipe de la réponse).
    let creds = if h < 100 {
        Some(credentials(&headers, true)?)
    } else if headers.contains_key(KEY_HEADER) && headers.contains_key(TEAM_HEADER) {
        credentials(&headers, true).ok()
    } else {
        None
    };
    ready.generate(input, creds).await.map(|r| Json(r).into_response())
}

/// Chemins des handlers pour la dérive OpenAPI.
pub(crate) mod paths {
    pub(crate) use super::{
        __path_delete_overlay, __path_delete_overlays, __path_get_overlay, __path_post_routes,
        __path_put_overlay,
    };
}
