//! Route `POST /v1/router/route` (api 1.15, préférence de voie en 1.16) : un itinéraire vélo de A à B sur le graphe du générateur (lot 2 du plan
//! Pédalons `2026-11-23-itineraire-velo-osm.md`, moteur : `docs/design/technique/ord-route-lot1.md`).
//!
//! - **En-têtes d'équipe facultatifs** : sans eux, découverte au goût commun. Avec eux, le curseur donne mode,
//!   familiarité et λ comme pour une génération, sans le mode strict ([`cursor::apply_route`]) ; une surcouche absente
//!   ou périmée ne bloque pas : l'itinéraire est calculé en découverte et la réponse le dit (`mode`, `overlay: null`).
//! - **Deux zones** ([`RouteRequest::reach_m`]) : la seconde seulement si la première n'a pas de chemin. Chaque essai
//!   prend sa fenêtre dans le cache des générations et rejoue une fois, élargi, si la recherche en touche le bord.
//! - **Sa propre file** (`router_max_concurrent`, `router_max_queued`) et son budget (`router_budget_ms`) : un
//!   itinéraire n'attend jamais derrière une génération. Fenêtres, espaces de travail et budget de mémoire sont partagés.
//! - **Préférence de voie facultative** (api 1.16, `use_roads` rapporté à `use_roads_neutral`, le réglage du profil de
//!   l'appelant) : des multiplicateurs par type de voie ([`ord_loops::way_factors`], note `ord-route-lot1.md` §8), avec
//!   ou sans équipe ; sans ces champs, les poids sont ceux de 1.15 au bit près. `avoid_main_roads` (api 1.16) renchérit
//!   les grandes routes par-dessus ([`ord_loops::avoid_main_roads`]), cumulable avec `use_roads`.
//!
//! Rien de privé n'est journalisé : ni coordonnée, ni polyligne, ni clé, ni équipe ; une ligne par requête (type, mode,
//! bande de vol d'oiseau, issue, durée, `ns8`, et le sens de la préférence de voie, jamais sa valeur). Les `///` des types sérialisés vont au contrat : ils sont en anglais.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use ord_graph_match::geo::encode_polyline6;
use ord_loops::{
    route_geometry_spans, route_in, LatLon, Mode, RideType, RouteFailure, RouteRequest, RouteTune, Routed,
    TeamCtx,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::crypto::Credentials;
use super::cursor::{self, Applied};
use super::generate::{mode_v1, queue_slot, r1, ride_name, unavailable, CancelOnDrop};
use super::routes::{
    credentials, point, GeoPointV1, ModeV1, OverlayInfoV1, RideTypeV1, KEY_HEADER, TEAM_HEADER,
};
use super::store::{Entry, ReadyError};
use super::windows::{AcquireError, TeamRef};
use super::{Ready, GENERATE_BODY_MAX};
use crate::app::AppState;
use crate::problem::{CoverageSource, FieldError, GenerationReason, Problem, ProblemCode, ProblemDetails};
use crate::strict_json::read_json;

/// What the service publishes about routes from A to B (`/v1/capabilities`, api 1.15; `use_roads` and
/// `avoid_main_roads` since 1.16).
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct RouterCapabilities {
    /// `POST /v1/router/route` answers; otherwise 503 `data_unavailable`, like the `/v1/generator/*` routes. Follows
    /// `generator.enabled`; the area covered is `generator.coverage`.
    pub enabled: bool,
    /// Accepted ride types (`ride_type`).
    pub ride_types: Vec<&'static str>,
    /// Longest distance from the start to the end as the crow flies, in metres: beyond, 400 `invalid_input` (pointer
    /// `/end`).
    pub crow_m_max: f64,
    /// Longest distance from a point to the way it is snapped to, in metres: beyond, 422 `generation_failed` (reason
    /// `start_too_far` or `end_too_far`).
    pub snap_m: f64,
    /// The request takes `use_roads` and `use_roads_neutral` (api 1.16). Absent (older services): do not send them,
    /// the request would be refused as carrying an unknown field.
    pub use_roads: bool,
    /// The request takes `avoid_main_roads` (api 1.16). Absent (older services): do not send it.
    pub avoid_main_roads: bool,
}

impl RouterCapabilities {
    pub(super) fn new(enabled: bool) -> Self {
        let tune = RouteTune::default();
        Self {
            enabled,
            ride_types: vec!["road", "gravel", "mtb"],
            crow_m_max: tune.crow_max_m,
            snap_m: tune.snap_m,
            use_roads: true,
            avoid_main_roads: true,
        }
    }
}

/// Bicycle route request from a start to an end.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct RouterRequestV1 {
    pub start: GeoPointV1,
    pub end: GeoPointV1,
    pub ride_type: RideTypeV1,
    /// With the team headers only (`Ord-Namespace-Key` and `Ord-Team`), as for a generation (see
    /// `capabilities.generator.cursor`), except that strict mode is not offered: 0 routes team first with the highest
    /// off-team cost. Absent: 1 (discovery; the team's share is still measured). Ignored without the team headers, but
    /// still validated within [0, 1].
    #[serde(default)]
    pub cursor: Option<f64>,
    /// Preference between tracks and roads (api 1.16, see `capabilities.router.use_roads`), within [0, 1]: 0 favours
    /// tracks, cycleways and small ways, 1 roads. Applies with or without the team headers, in every mode. Requires
    /// `use_roads_neutral`: without it, 400 `invalid_input` (pointer `/use_roads_neutral`). Both absent: no preference.
    /// The weights are an estimate, not fitted to measured rides.
    #[serde(default)]
    pub use_roads: Option<f64>,
    /// The caller's profile setting of `use_roads`, within [0, 1], for which the route keeps the weights it has without
    /// a preference: `use_roads` acts as its distance from this value. Alone, it has no effect (but is validated).
    #[serde(default)]
    pub use_roads_neutral: Option<f64>,
    /// Main roads cost more (api 1.16, see `capabilities.router.avoid_main_roads`): `secondary` twice, `primary` and
    /// `trunk` one and a half times, on top of `use_roads` if given. They are not forbidden and are still ridden where
    /// nothing else goes. Applies with or without the team headers. Absent, null or false: no effect. An estimate, tuned
    /// on no measure.
    #[serde(default)]
    pub avoid_main_roads: Option<bool>,
}

/// A route from the start to the end. Distances are measured on the graph, in metres.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct RouterResponseV1 {
    pub graph_id: String,
    /// Mode applied: `discovery` without the team headers, or when the team overlay is absent or stale (then `overlay`
    /// is null); never `strict`.
    pub mode: ModeV1,
    /// Google polyline, precision 6 (the one `/v1/route-details` takes), from the point snapped near the start to the
    /// point snapped near the end.
    pub polyline: String,
    pub distance_m: f64,
    /// Length on edges the team has ridden, and on short holes bridged between them (0 without a team).
    pub team_m: f64,
    pub bridged_m: f64,
    /// (team_m + bridged_m) / distance_m.
    pub team_share: f64,
    /// Length off pavement (surface tag, then `tracktype`, then road class: the rule of `/v1/route-details`).
    pub offroad_m: f64,
    /// Ranges of polyline point indices ridden off pavement (same rule as `offroad_m`): `[i, j]` covers points `i` to `j`
    /// included; adjacent unpaved edges make one range.
    pub unpaved: Vec<[u32; 2]>,
    /// Distance from the asked start and end to the graph, in metres.
    pub start_snap_m: f64,
    pub end_snap_m: f64,
    /// The route takes at least one sharp turn (150 degrees or more at a junction), or a U-turn at a dead end: the
    /// generator refuses them, a route pays for them (200 to 250 m) rather than go a long way around.
    pub sharp_relaxed: bool,
    /// The team overlay the route was weighted and measured with; null without one.
    pub overlay: Option<OverlayInfoV1>,
}

/// Une demande validée.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RouteInput {
    pub start: LatLon,
    pub end: LatLon,
    pub ride: RideType,
    /// Curseur en centièmes (100 sans valeur).
    pub cursor: u32,
    /// Vol d'oiseau (m), pour la bande journalisée.
    pub crow_m: f64,
    /// Multiplicateurs par voie : [`ord_loops::WAY_NEUTRAL`] sans préférence.
    pub way_q8: [u16; ord_loops::WAY_NEUTRAL.len()],
    /// Sens de la préférence, seule trace journalisée du réglage.
    pub use_roads: UseRoads,
    /// Grandes routes renchéries ([`ord_loops::avoid_main_roads`], déjà dans `way_q8`).
    pub avoid_main_roads: bool,
}

/// Le réglage de voie comparé à son neutre : une étiquette à quatre valeurs, jamais le nombre reçu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UseRoads {
    /// Pas de `use_roads`.
    None,
    /// Plus de pistes que le profil.
    Less,
    Same,
    /// Plus de routes que le profil.
    More,
}

impl UseRoads {
    fn of(asked: f64, neutral: f64) -> Self {
        match asked.partial_cmp(&neutral) {
            Some(std::cmp::Ordering::Less) => UseRoads::Less,
            Some(std::cmp::Ordering::Greater) => UseRoads::More,
            _ => UseRoads::Same,
        }
    }

    fn name(self) -> &'static str {
        match self {
            UseRoads::None => "none",
            UseRoads::Less => "less",
            UseRoads::Same => "same",
            UseRoads::More => "more",
        }
    }
}

/// Un réglage de voie dans [0 ; 1], fini.
fn unit(raw: Option<f64>, pointer: &str, errors: &mut Vec<FieldError>) -> Option<f64> {
    let v = raw?;
    if v.is_finite() && (0.0..=1.0).contains(&v) {
        Some(v)
    } else {
        errors.push(FieldError::new(pointer, "must be within [0, 1]"));
        None
    }
}

fn validate(req: &RouterRequestV1) -> Result<RouteInput, Problem> {
    let mut errors = Vec::new();
    let start = point(req.start, "/start", &mut errors);
    let end = point(req.end, "/end", &mut errors);
    let h = match req.cursor {
        None => Some(100),
        Some(c) => cursor::hundredths(c),
    };
    if h.is_none() {
        errors.push(FieldError::new("/cursor", "must be within [0, 1]"));
    }
    let ride = match req.ride_type {
        RideTypeV1::Road => RideType::Road,
        RideTypeV1::Gravel => RideType::Gravel,
        RideTypeV1::Mtb => RideType::Mtb,
    };
    // Le neutre dépend du profil de l'appelant : le service ne le devine pas.
    let asked = unit(req.use_roads, "/use_roads", &mut errors);
    let neutral = unit(req.use_roads_neutral, "/use_roads_neutral", &mut errors);
    if req.use_roads.is_some() && req.use_roads_neutral.is_none() {
        errors.push(FieldError::new("/use_roads_neutral", "required with use_roads"));
    }
    let (way_q8, use_roads) = match (asked, neutral) {
        (Some(u), Some(n)) => (ord_loops::way_factors(ride, u, n), UseRoads::of(u, n)),
        _ => (ord_loops::WAY_NEUTRAL, UseRoads::None),
    };
    // Par-dessus la préférence de voie, ou sur les poids neutres.
    let avoid_main_roads = req.avoid_main_roads == Some(true);
    let way_q8 = if avoid_main_roads { ord_loops::avoid_main_roads(way_q8) } else { way_q8 };
    let mut crow_m = 0.0;
    if errors.is_empty() {
        // La borne du moteur, vérifiée ici : un 400 sur l'entrée passe avant la couverture et la file.
        let probe = RouteRequest {
            start,
            end,
            ride_type: ride,
            mode: Mode::Discovery,
            familiarity: cursor::FAMILIARITY,
            off_team_cost_q8: 256,
            way_q8: ord_loops::WAY_NEUTRAL,
        };
        crow_m = probe.crow_m();
        if crow_m > RouteTune::default().crow_max_m {
            errors.push(too_long());
        }
    }
    if !errors.is_empty() {
        return Err(Problem::invalid_input("invalid request", errors));
    }
    Ok(RouteInput { start, end, ride, cursor: h.unwrap_or(100), crow_m, way_q8, use_roads, avoid_main_roads })
}

/// La limite dans le message, jamais la valeur reçue ni la distance mesurée.
fn too_long() -> FieldError {
    FieldError::new(
        "/end",
        format!(
            "farther than {} m from the start as the crow flies (capabilities.router.crow_m_max)",
            RouteTune::default().crow_max_m
        ),
    )
}

fn timeout() -> Problem {
    Problem::new(ProblemCode::Timeout, "route budget exceeded")
}

/// 503 `overloaded` : la file des itinéraires est pleine, ou plus de place dans le budget de mémoire.
fn overloaded() -> Problem {
    let mut p = Problem::new(ProblemCode::Overloaded, "too many routes in progress");
    p.retry_after = Some(Duration::from_secs(1));
    p
}

/// 422 `generation_failed` : le code de la génération, avec les raisons propres à l'itinéraire.
fn failed(reason: GenerationReason) -> Problem {
    Problem::with_details(
        ProblemCode::GenerationFailed,
        "no route could be found from the start to the end",
        ProblemDetails { reason: Some(reason), ..Default::default() },
    )
}

fn problem_of(f: RouteFailure) -> Problem {
    match f {
        RouteFailure::StartTooFar => failed(GenerationReason::StartTooFar),
        RouteFailure::EndTooFar => failed(GenerationReason::EndTooFar),
        RouteFailure::NoRoute => failed(GenerationReason::NoRoute),
        RouteFailure::Stopped => timeout(),
        // Jamais attendu : validé avant la file, avec la même borne.
        RouteFailure::TooLong => Problem::invalid_input("invalid request", vec![too_long()]),
        // Jamais attendus : le mode d'équipe n'est demandé qu'avec les agrégats de l'équipe ; l'échappement est rejoué.
        RouteFailure::NoTeamData | RouteFailure::WindowEscape => Problem::internal(),
    }
}

/// Bande du vol d'oiseau, seule mesure de la requête qui soit journalisée.
fn band(crow_m: f64) -> &'static str {
    match crow_m {
        d if d < 5_000.0 => "<5km",
        d if d < 20_000.0 => "5-20km",
        d if d < 50_000.0 => "20-50km",
        _ => ">=50km",
    }
}

fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::Strict => "strict",
        Mode::TeamFirst => "team_first",
        Mode::Discovery => "discovery",
    }
}

/// Ce que le calcul rend au handler : la réponse et de quoi la journaliser.
struct Outcome {
    response: RouterResponseV1,
    mode: Mode,
    zone: usize,
}

impl Ready {
    /// Une place de la file des itinéraires, dans le budget.
    async fn router_slot(&self, deadline: Instant) -> Result<tokio::sync::OwnedSemaphorePermit, Problem> {
        queue_slot(&self.router_slots, &self.router_waiting, self.cfg.router_max_queued, deadline)
            .await
            .map_err(|full| if full { overloaded() } else { timeout() })
    }

    pub(crate) async fn route(
        self: &Arc<Self>,
        input: RouteInput,
        creds: Option<Credentials>,
    ) -> Result<RouterResponseV1, Problem> {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(self.cfg.router_budget_ms);
        let ns8 = creds.as_ref().map(|c| c.ns8().to_string());
        // Le mode demandé, pour les lignes d'un échec ; une réussite dit celui qui a servi.
        let asked = if creds.is_some() { cursor::apply_route(input.cursor).mode } else { Mode::Discovery };
        let outcome = self.route_inner(input, creds, deadline).await;
        let duration = started.elapsed();
        let (mode, zone, sharp) = match &outcome {
            Ok(o) => (o.mode, o.zone.to_string(), o.response.sharp_relaxed),
            Err(_) => (asked, String::new(), false),
        };
        let mode = mode_name(mode);
        let ride = ride_name(input.ride);
        let use_roads = input.use_roads.name();
        let avoid_main_roads = if input.avoid_main_roads { "true" } else { "false" };
        metrics::histogram!("ord_router_duration_seconds", "mode" => mode, "ride_type" => ride)
            .record(duration.as_secs_f64());
        let (result, reason) = match &outcome {
            Ok(_) => ("ok", String::new()),
            Err(p) => {
                let reason = p
                    .details
                    .as_ref()
                    .and_then(|d| d.reason.map(|r| serde_json::to_value(r).ok()))
                    .flatten()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                (p.code.as_str(), reason)
            }
        };
        metrics::counter!("ord_router_requests_total", "outcome" => result, "reason" => reason.clone(), "use_roads" => use_roads, "avoid_main_roads" => avoid_main_roads)
            .increment(1);
        if outcome.is_ok() {
            metrics::counter!("ord_router_routes_total", "zone" => zone.clone(), "sharp_relaxed" => if sharp { "true" } else { "false" })
                .increment(1);
        }
        tracing::info!(
            mode,
            ride_type = ride,
            distance_band = band(input.crow_m),
            use_roads,
            avoid_main_roads = input.avoid_main_roads,
            outcome = result,
            reason = reason.as_str(),
            zone = zone.as_str(),
            sharp_relaxed = sharp,
            duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
            ns8 = ns8.as_deref().unwrap_or(""),
            "route from A to B"
        );
        outcome.map(|o| o.response)
    }

    async fn route_inner(
        self: &Arc<Self>,
        input: RouteInput,
        creds: Option<Credentials>,
        deadline: Instant,
    ) -> Result<Outcome, Problem> {
        if !self.in_coverage(input.start) || !self.in_coverage(input.end) {
            return Err(Problem::with_details(
                ProblemCode::OutOfCoverage,
                "start or end is outside the area the router covers",
                ProblemDetails { coverage: Some(CoverageSource::Graph), ..Default::default() },
            ));
        }
        let permit = self.router_slot(deadline).await?;
        let cancel = Arc::new(AtomicBool::new(false));
        let _guard = CancelOnDrop(Arc::clone(&cancel));
        let me = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            me.compute_route(&input, creds.as_ref(), deadline, &cancel)
        })
        .await
        .map_err(|_| Problem::internal())?
    }

    /// Le calcul, sur un fil bloquant : l'équipe si elle est servable, puis les zones.
    fn compute_route(
        &self,
        input: &RouteInput,
        creds: Option<&Credentials>,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<Outcome, Problem> {
        let entry: Option<Arc<Entry>> = match creds {
            Some(c) => match self.team_entry(c, deadline, cancel) {
                Ok(e) => Some(e),
                // Comme une génération : sans la mémoire de l'équipe, la réponse dépendrait de la charge du service.
                Err(ReadyError::Overloaded) => return Err(overloaded()),
                Err(ReadyError::Stopped) => return Err(timeout()),
                Err(ReadyError::Failed) => return Err(Problem::internal()),
                // Absente ou périmée : découverte, et la réponse le dit (`overlay: null`).
                Err(ReadyError::State(_)) => None,
            },
            None => None,
        };
        if Instant::now() >= deadline {
            return Err(timeout());
        }
        let applied: Applied =
            if entry.is_some() { cursor::apply_route(input.cursor) } else { cursor::discovery() };
        let rr = RouteRequest {
            start: input.start,
            end: input.end,
            ride_type: input.ride,
            mode: applied.mode,
            familiarity: applied.familiarity,
            off_team_cost_q8: applied.off_team_cost_q8,
            way_q8: input.way_q8,
        };
        let tune = RouteTune::default();
        let mut stop = || cancel.load(Ordering::Relaxed) || Instant::now() >= deadline;
        let (routed, zone) = self.route_windowed(&rr, &tune, entry.as_deref(), deadline, &mut stop)?;
        Ok(Outcome {
            response: self.render_route(&routed, applied.mode, entry.as_deref()),
            mode: applied.mode,
            zone,
        })
    }

    /// Les zones successives : la seconde seulement sur `NoRoute`. Dans chaque zone, la fenêtre du cache, puis, si la
    /// recherche en touche le bord (jamais attendu), une fois de plus dans une fenêtre élargie de la plus longue chaîne
    /// qui touche les tuiles d'accroche et de 10 km, comme une génération ; un second échappement rend une erreur interne.
    fn route_windowed(
        &self,
        rr: &RouteRequest,
        tune: &RouteTune,
        entry: Option<&Entry>,
        deadline: Instant,
        stop: &mut dyn FnMut() -> bool,
    ) -> Result<(Routed, usize), Problem> {
        let max_m = self.engine.params().max_distance_m;
        for zone in 0..tune.reach.len() {
            let Some(reach_m) = rr.reach_m(tune, zone) else { break };
            if reach_m > max_m {
                // Jamais sous la borne du vol d'oiseau (250 km au plus pour 400) : la zone est hors de l'index du moteur.
                break;
            }
            let mut extra_m = 0.0;
            let mut no_route = false;
            for attempt in 0..2 {
                let plan = self.engine.plan_route(rr, reach_m, self.cfg.window_radius_step_m, extra_m);
                let team =
                    entry.map(|e| TeamRef { serial: e.serial, data: &e.team, taste: e.profile.as_ref() });
                let held = match self.windows.acquire(&self.engine, &plan, team, deadline, stop) {
                    Ok(h) => h,
                    Err(AcquireError::Stopped) => return Err(timeout()),
                    Err(AcquireError::Overloaded) => return Err(overloaded()),
                };
                let mut ws = self.take_workspace();
                let ctx = entry.zip(held.wt.as_deref()).map(|(e, wt)| TeamCtx { data: &e.team, win: wt });
                let result = route_in(&self.engine, &held.win, ctx, rr, tune, reach_m, &mut ws, stop);
                self.give_workspace(ws);
                self.windows.release(held);
                match result {
                    Ok(r) => return Ok((r, zone)),
                    Err(RouteFailure::WindowEscape) => {
                        metrics::counter!("ord_router_window_escapes_total").increment(1);
                        tracing::warn!(attempt, zone, "router window escape");
                        extra_m = plan.snap_lmax_m + 10_000.0;
                    }
                    Err(RouteFailure::NoRoute) => {
                        no_route = true;
                        break;
                    }
                    Err(f) => return Err(problem_of(f)),
                }
            }
            if !no_route {
                return Err(Problem::internal());
            }
        }
        Err(failed(GenerationReason::NoRoute))
    }

    fn render_route(&self, routed: &Routed, mode: Mode, entry: Option<&Entry>) -> RouterResponseV1 {
        let r = &routed.route;
        let (points, unpaved) = route_geometry_spans(self.engine.base(), r);
        let points: Vec<[f64; 2]> = points.iter().map(|p| [p.lat, p.lon]).collect();
        let len = r.length_m.max(1.0);
        RouterResponseV1 {
            graph_id: self.graph_id.clone(),
            mode: mode_v1(mode),
            polyline: encode_polyline6(&points),
            distance_m: r1(r.length_m),
            team_m: r1(r.team_m),
            bridged_m: r1(r.bridged_m),
            team_share: ((r.team_m + r.bridged_m) / len * 1000.0).round() / 1000.0,
            offroad_m: r1(r.offroad_m),
            unpaved,
            start_snap_m: r1(routed.start_snap_m),
            end_snap_m: r1(routed.end_snap_m),
            sharp_relaxed: routed.sharp_relaxed,
            overlay: entry.map(|e| OverlayInfoV1 { deposited_at: super::iso8601(e.meta.deposited_at) }),
        }
    }
}

// Itinéraire de A à B.
#[utoipa::path(
    post,
    path = "/v1/router/route",
    summary = "Bicycle route from A to B",
    description = "One route from a start to an end for a ride type, on the generator's graph and with its weights \
(road class, surface, access, taste). Without the team headers the route is a discovery with the common taste. With \
them, `cursor` maps to the engine as for a generation, strict mode excepted; an absent or stale overlay does not \
block: the route is a discovery and the answer says so (`mode`, `overlay: null`). `use_roads`, relative to the caller's \
`use_roads_neutral`, scales the weights by way type (api 1.16, with or without a team); `avoid_main_roads` makes main roads \
cost more, without forbidding them; without these fields, the route is the one of api 1.15. The search widens once if the first area holds no path. Routes have their own queue and budget, apart from the generations. Nothing from the request is \
logged.",
    operation_id = "postRouterRoute",
    tag = "router",
    params(
        ("Ord-Namespace-Key" = Option<String>, Header, description = "Environment secret; with `Ord-Team`, weights the route with the team overlay. Never logged."),
        ("Ord-Team" = Option<String>, Header, description = "Opaque team reference, [A-Za-z0-9_-]{1,64}; with `Ord-Namespace-Key`. Never logged.")
    ),
    request_body(content = RouterRequestV1, content_type = "application/json"),
    responses(
        (status = 200, description = "Route", body = RouterResponseV1),
        (status = 400, description = "Invalid input (fields listed by JSON pointer, headers by `/headers/<name>`; an end farther than `capabilities.router.crow_m_max` from the start: pointer `/end`; `use_roads` without `use_roads_neutral`: pointer `/use_roads_neutral`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 413, description = "Request body above 64 KiB", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 415, description = "Content-Type is not application/json", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 422, description = "Out of coverage (`out_of_coverage`, `details.coverage = graph`) or no route (`generation_failed`, `details.reason` = `start_too_far`, `end_too_far` or `no_route`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 503, description = "Router unavailable (`data_unavailable`, `details.source = graph`) or queue full (`overloaded`); with `Retry-After`", body = crate::problem::ProblemBody, content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "Seconds to wait before retrying"))),
        (status = 504, description = "Route budget exceeded (`timeout`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
    )
)]
pub(crate) async fn post_route(State(state): State<AppState>, req: Request) -> Result<Response, Problem> {
    let ready = Arc::clone(state.generator().ready().ok_or_else(unavailable)?);
    let headers = req.headers().clone();
    let body: RouterRequestV1 = read_json(req, GENERATE_BODY_MAX).await?;
    let input = validate(&body)?;
    // Facultatifs, mais validés dès que l'un est là : une clé sans équipe, ou mal formée, est un 400.
    let creds = if headers.contains_key(KEY_HEADER) || headers.contains_key(TEAM_HEADER) {
        Some(credentials(&headers, true)?)
    } else {
        None
    };
    ready.route(input, creds).await.map(|r| Json(r).into_response())
}

/// Chemins des handlers pour la dérive OpenAPI.
pub(crate) mod paths {
    pub(crate) use super::__path_post_route;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_never_carry_the_distance() {
        assert_eq!(band(0.0), "<5km");
        assert_eq!(band(5_000.0), "5-20km");
        assert_eq!(band(49_999.0), "20-50km");
        assert_eq!(band(100_000.0), ">=50km");
    }

    #[test]
    fn the_road_setting_is_logged_as_a_direction_only() {
        assert_eq!(UseRoads::of(0.2, 0.25).name(), "less");
        assert_eq!(UseRoads::of(0.25, 0.25).name(), "same");
        assert_eq!(UseRoads::of(0.9, 0.25).name(), "more");
        assert_eq!(UseRoads::None.name(), "none");
    }

    #[test]
    fn failures_map_to_the_generators_problems() {
        let reason = |f| problem_of(f).details.and_then(|d| d.reason);
        assert_eq!(reason(RouteFailure::StartTooFar), Some(GenerationReason::StartTooFar));
        assert_eq!(reason(RouteFailure::EndTooFar), Some(GenerationReason::EndTooFar));
        assert_eq!(reason(RouteFailure::NoRoute), Some(GenerationReason::NoRoute));
        assert_eq!(problem_of(RouteFailure::NoRoute).code, ProblemCode::GenerationFailed);
        assert_eq!(problem_of(RouteFailure::Stopped).code, ProblemCode::Timeout);
        assert_eq!(problem_of(RouteFailure::TooLong).code, ProblemCode::InvalidInput);
        assert_eq!(problem_of(RouteFailure::WindowEscape).code, ProblemCode::Internal);
    }
}
