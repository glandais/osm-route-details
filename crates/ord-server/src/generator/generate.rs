//! Génération : le moteur, puis le contrôleur indépendant sur chaque parcours avant la réponse (note lot 3a §5–§8).
//!
//! Le calcul est du CPU pur : `spawn_blocking` sous un sémaphore (`max_concurrent`) et une file bornée (`max_queued`),
//! dans un budget qui inclut l'attente. Le `stop` du moteur lit l'échéance et un drapeau levé quand le client part ;
//! jamais de parcours partiel. Rien de privé n'est journalisé : ni coordonnée, ni polyligne, ni clé, ni équipe.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ord_graph_match::geo::encode_polyline6;
use ord_loops::{
    generate, route_geometry, Bearing8, Failure, Family, LatLon, Mode, NoRouteDetail, Request, RideType,
    Route, Workspace,
};
use ord_loops_check::{Checker, Config};

use super::crypto::Credentials;
use super::cursor::Applied;
use super::routes::{CoverageV1, GenerateResponseV1, ModeV1, OverlayInfoV1, RouteV1};
use super::store::{Entry, StaleReason, State};
use super::Ready;
use crate::problem::{
    CoverageSource, GenerationReason, OverlayProblemState, Problem, ProblemCode, ProblemDetails,
    ProblemSource,
};

/// Une demande validée.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GenInput {
    pub start: LatLon,
    pub end: Option<LatLon>,
    pub bearing: Option<Bearing8>,
    pub distance_m: f64,
    pub ride: RideType,
    pub applied: Applied,
    pub seed: u64,
}

/// Une place dans la file d'attente de calcul ; rendue à la chute.
struct QueueSlot<'a>(&'a std::sync::atomic::AtomicUsize);

impl Drop for QueueSlot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Lève le drapeau quand le futur du handler est lâché (client parti) : le moteur s'arrête à son prochain contrôle.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

struct Computed {
    routes: Vec<RouteV1>,
    dropped: u32,
    overlay: Option<OverlayInfoV1>,
}

fn band(distance_m: f64) -> &'static str {
    match distance_m {
        d if d < 20_000.0 => "<20km",
        d if d < 50_000.0 => "20-50km",
        d if d < 100_000.0 => "50-100km",
        _ => ">=100km",
    }
}

fn ride_name(r: RideType) -> &'static str {
    match r {
        RideType::Road => "road",
        RideType::Gravel => "gravel",
        RideType::Mtb => "mtb",
    }
}

fn mode_v1(m: Mode) -> ModeV1 {
    match m {
        Mode::Strict => ModeV1::Strict,
        Mode::TeamFirst => ModeV1::TeamFirst,
        Mode::Discovery => ModeV1::Discovery,
    }
}

fn failed(reason: GenerationReason) -> Problem {
    Problem::with_details(
        ProblemCode::GenerationFailed,
        "no route could be generated for this request",
        ProblemDetails { reason: Some(reason), ..Default::default() },
    )
}

fn reason_of(f: Failure) -> Result<GenerationReason, Problem> {
    Ok(match f {
        Failure::StartTooFar => GenerationReason::StartTooFar,
        Failure::EndTooFar => GenerationReason::EndTooFar,
        Failure::DistanceTooShort => GenerationReason::DistanceTooShort,
        Failure::NetworkTooSmall => GenerationReason::NetworkTooSmall,
        Failure::NoRoute(NoRouteDetail::EndUnreachable) => GenerationReason::EndUnreachable,
        Failure::NoRoute(NoRouteDetail::NoWayBack) => GenerationReason::NoWayBack,
        Failure::NoRoute(NoRouteDetail::TailTooLong) => GenerationReason::TailTooLong,
        Failure::NoRoute(NoRouteDetail::NoCandidate) => GenerationReason::NoCandidate,
        Failure::NoRoute(NoRouteDetail::Budget) => GenerationReason::Budget,
        Failure::NoTeamData => return Err(overlay_required(OverlayProblemState::Absent)),
        Failure::Stopped => return Err(Problem::new(ProblemCode::Timeout, "generation budget exceeded")),
        // Jamais attendus : la fenêtre contient les tuiles nécessaires et `distance_m_max` reste sous la borne du moteur.
        Failure::WindowEscape | Failure::TooLong => return Err(Problem::internal()),
    })
}

/// 409 : le même corps, au `instance` près, pour une équipe inconnue, une autre clé ou une surcouche jamais déposée.
pub(crate) fn overlay_required(state: OverlayProblemState) -> Problem {
    Problem::with_details(
        ProblemCode::OverlayRequired,
        "the team overlay is required for this cursor value: deposit it first",
        ProblemDetails { state: Some(state), ..Default::default() },
    )
}

pub(crate) fn unavailable() -> Problem {
    let mut p = Problem::with_details(
        ProblemCode::DataUnavailable,
        "the route generator is not available",
        ProblemDetails { source: Some(ProblemSource::Graph), ..Default::default() },
    );
    p.retry_after = Some(Duration::from_secs(3600));
    p
}

/// Au dixième ; `-0.0` devient `0.0` (un parcours sans arête d'équipe rendait `"team_m":-0.0`, relevé sur la région).
fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0 + 0.0
}

impl Ready {
    fn take_workspace(&self) -> Workspace {
        self.workspaces.lock().unwrap_or_else(std::sync::PoisonError::into_inner).pop().unwrap_or_default()
    }

    fn give_workspace(&self, ws: Workspace) {
        let mut pool = self.workspaces.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if pool.len() < self.cfg.max_concurrent {
            pool.push(ws);
        }
    }

    pub(crate) fn in_coverage(&self, p: LatLon) -> bool {
        self.poly.contains(p.lon, p.lat)
    }

    /// Une place de calcul, dans le budget ; 503 `overloaded` si la file est pleine, 504 si le budget s'écoule.
    pub(crate) async fn slot(&self, deadline: Instant) -> Result<tokio::sync::OwnedSemaphorePermit, Problem> {
        let overloaded = || {
            let mut p = Problem::new(ProblemCode::Overloaded, "too many route generations in progress");
            p.retry_after = Some(Duration::from_secs(2));
            p
        };
        if let Ok(p) = Arc::clone(&self.gen_slots).try_acquire_owned() {
            return Ok(p);
        }
        if self.gen_waiting.fetch_add(1, Ordering::AcqRel) >= self.cfg.max_queued {
            self.gen_waiting.fetch_sub(1, Ordering::AcqRel);
            return Err(overloaded());
        }
        // Libérée en sortie d'attente, y compris quand le futur est lâché (client parti, délai du proxy).
        let _queued = QueueSlot(&self.gen_waiting);
        let wait = deadline.saturating_duration_since(Instant::now());
        let got = tokio::time::timeout(wait, Arc::clone(&self.gen_slots).acquire_owned()).await;
        match got {
            Ok(Ok(p)) => Ok(p),
            Ok(Err(_)) => Err(overloaded()),
            Err(_) => Err(Problem::new(ProblemCode::Timeout, "generation budget exceeded while waiting")),
        }
    }

    pub(crate) async fn generate(
        self: &Arc<Self>,
        input: GenInput,
        creds: Option<Credentials>,
    ) -> Result<GenerateResponseV1, Problem> {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(self.cfg.budget_ms);
        let ns8 = creds.as_ref().map(|c| c.ns8().to_string());
        let outcome = self.generate_inner(input, creds, deadline).await;
        let duration = started.elapsed();
        let mode = match input.applied.mode {
            Mode::Strict => "strict",
            Mode::TeamFirst => "team_first",
            Mode::Discovery => "discovery",
        };
        metrics::histogram!("ord_generator_duration_seconds", "mode" => mode, "ride_type" => ride_name(input.ride))
            .record(duration.as_secs_f64());
        let (result, reason, routes, dropped) = match &outcome {
            Ok(r) => ("ok", String::new(), r.routes.len(), r.dropped),
            Err(p) => {
                let reason = p
                    .details
                    .as_ref()
                    .and_then(|d| d.reason.map(|r| serde_json::to_value(r).ok()))
                    .flatten()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                (p.code.as_str(), reason, 0, 0)
            }
        };
        metrics::counter!("ord_generator_requests_total", "outcome" => result, "reason" => reason.clone())
            .increment(1);
        tracing::info!(
            mode,
            ride_type = ride_name(input.ride),
            distance_band = band(input.distance_m),
            outcome = result,
            reason = reason.as_str(),
            routes,
            dropped,
            duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
            ns8 = ns8.as_deref().unwrap_or(""),
            "route generation"
        );
        outcome
    }

    async fn generate_inner(
        self: &Arc<Self>,
        input: GenInput,
        creds: Option<Credentials>,
        deadline: Instant,
    ) -> Result<GenerateResponseV1, Problem> {
        // Départ et arrivée dans le polygone ; le disque de rayon D/2 qui en sort rend la couverture partielle.
        if !self.in_coverage(input.start) || input.end.is_some_and(|e| !self.in_coverage(e)) {
            return Err(Problem::with_details(
                ProblemCode::OutOfCoverage,
                "start or end is outside the area the generator covers",
                ProblemDetails { coverage: Some(CoverageSource::Graph), ..Default::default() },
            ));
        }
        let radius = input.distance_m / 2.0;
        let inside = self.poly.contains_disc(input.start.lat, input.start.lon, radius)
            && input.end.is_none_or(|e| self.poly.contains_disc(e.lat, e.lon, radius));

        let permit = self.slot(deadline).await?;
        let cancel = Arc::new(AtomicBool::new(false));
        let _guard = CancelOnDrop(Arc::clone(&cancel));
        let me = Arc::clone(self);
        let computed = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            me.compute(&input, creds.as_ref(), deadline, &cancel)
        })
        .await
        .map_err(|_| Problem::internal())??;

        if computed.routes.is_empty() {
            return Err(failed(GenerationReason::CheckerRejected));
        }
        Ok(GenerateResponseV1 {
            graph_id: self.graph_id.clone(),
            mode: mode_v1(input.applied.mode),
            lambda: input.applied.lambda,
            coverage: if inside { CoverageV1::Inside } else { CoverageV1::Partial },
            overlay: computed.overlay,
            dropped: computed.dropped,
            routes: computed.routes,
        })
    }

    /// Le calcul, sur un fil bloquant : l'équipe (si le mode la demande), le moteur, le contrôleur.
    fn compute(
        &self,
        input: &GenInput,
        creds: Option<&Credentials>,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<Computed, Problem> {
        let need_team = input.applied.mode != Mode::Discovery;
        let entry: Option<Arc<Entry>> = match creds {
            Some(c) => match self.store.ready(c, &self.engine) {
                Ok(e) => Some(e),
                Err(state) if need_team => {
                    return Err(overlay_required(match state {
                        State::Stale(StaleReason::Graph | StaleReason::Matcher) => OverlayProblemState::Stale,
                        State::Absent | State::Ready => OverlayProblemState::Absent,
                    }));
                }
                Err(_) => None,
            },
            None if need_team => return Err(overlay_required(OverlayProblemState::Absent)),
            None => None,
        };
        if Instant::now() >= deadline {
            return Err(Problem::new(ProblemCode::Timeout, "generation budget exceeded"));
        }
        let req = Request {
            start: input.start,
            end: input.end,
            bearing: input.bearing,
            distance_m: input.distance_m,
            ride_type: input.ride,
            mode: input.applied.mode,
            familiarity: input.applied.familiarity,
            off_team_cost_q8: input.applied.off_team_cost_q8,
            seed: input.seed,
        };
        let mut ws = self.take_workspace();
        let mut stop = || cancel.load(Ordering::Relaxed) || Instant::now() >= deadline;
        let result = generate(&self.engine, entry.as_ref().map(|e| &e.team), &req, &mut ws, &mut stop);
        self.give_workspace(ws);
        let generation = match result {
            Ok(g) => g,
            Err(f) => return Err(reason_of(f).map_or_else(|p| p, failed)),
        };

        let checker = Checker::new(self.engine.base(), entry.as_ref().map(|e| &e.overlay), Config::default());
        let (mut routes, mut dropped) = (Vec::new(), 0u32);
        let snaps = (generation.diagnostics.start_snap_m, generation.diagnostics.end_snap_m);
        for mut route in generation.routes {
            if self.hooks.corrupt_routes.load(Ordering::Relaxed) && route.steps.len() > 1 {
                route.steps.remove(0);
            }
            let report = checker.check(&req, &route);
            if report.ok() {
                routes.push(self.render(&route, &report, snaps));
            } else {
                dropped += 1;
                let codes: Vec<&str> = report.violations.iter().map(|v| v.code()).collect();
                for code in &codes {
                    metrics::counter!("ord_generator_routes_rejected_total", "violation" => (*code))
                        .increment(1);
                }
                tracing::warn!(
                    violations = codes.join(",").as_str(),
                    mode = ?input.applied.mode,
                    ride_type = ride_name(input.ride),
                    distance_band = band(input.distance_m),
                    "route rejected by the independent checker"
                );
            }
        }
        metrics::counter!("ord_generator_routes_total").increment(routes.len() as u64);
        Ok(Computed {
            routes,
            dropped,
            overlay: entry.map(|e| OverlayInfoV1 { deposited_at: super::iso8601(e.meta.deposited_at) }),
        })
    }

    fn render(&self, route: &Route, report: &ord_loops_check::Report, snaps: (f64, f64)) -> RouteV1 {
        let points: Vec<[f64; 2]> =
            route_geometry(self.engine.base(), route).iter().map(|p| [p.lat, p.lon]).collect();
        let len = report.length_m.max(1.0);
        RouteV1 {
            polyline: encode_polyline6(&points),
            distance_m: r1(report.length_m),
            team_m: r1(report.team_m),
            bridged_m: r1(report.bridged_m),
            team_share: ((report.team_m + report.bridged_m) / len * 1000.0).round() / 1000.0,
            tail_m: r1(report.shared_m),
            crossings: report.crossings,
            unpaved_m: r1(route.unpaved_m),
            unpaved_share: (route.unpaved_m / len * 1000.0).round() / 1000.0,
            presumed_surface_m: r1(route.presumed_surface_m),
            bearing_deviation_deg: route.bearing_deviation_deg.map(r1),
            family: match route.family {
                Family::Turnaround => "turnaround",
                Family::Polygon => "polygon",
            },
            start_snap_m: r1(snaps.0),
            end_snap_m: r1(snaps.1),
            ascent_m: None,
            descent_m: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_never_renders_a_negative_zero() {
        for x in [-0.0, -0.04, 0.0] {
            let v = r1(x);
            assert!(v == 0.0 && v.is_sign_positive(), "{x} -> {v}");
            assert_eq!(serde_json::to_string(&v).unwrap(), "0.0");
        }
        assert_eq!(r1(12.34), 12.3);
        assert_eq!(r1(-1.26), -1.3);
    }
}
