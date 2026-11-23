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
    generate_steered_in, route_geometry, Bearing8, Failure, Family, LatLon, Mode, NoRouteDetail,
    OffroadTarget, Request, RideType, Route, Steer, SteeredGeneration, TeamCtx, Workspace,
};
use ord_loops_check::{Checker, Config};

use super::crypto::Credentials;
use super::cursor::Applied;
use super::routes::{
    CoverageV1, FiguresV1, GenerateResponseV1, ModeV1, OffroadResultV1, OverlayInfoV1, RouteV1, TasteV1,
};
use super::store::{Entry, ReadyError, StaleReason, State};
use super::windows::{AcquireError, TeamRef};
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
    pub figures: FiguresV1,
    /// Part de non-revêtu visée (validée dans [0 ; 0,8], jamais en route), lot 5.
    pub offroad_target: Option<f64>,
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
    /// Le goût servi : celui de l'équipe (profil appris) ou la table commune.
    taste: TasteV1,
    /// `None` sans cible.
    offroad: Option<OffroadResultV1>,
}

/// La cible est tenue : la part (au millième, celle que le client lit) est dans la tolérance. La même marge que le moteur.
fn within(share: f64, target: f64, tolerance: f64) -> bool {
    (share - target).abs() <= tolerance + 1e-9
}

/// La consigne rendue au client : `reached` se lit sur la **première proposition gardée par le contrôleur** (le moteur a pu
/// voir sa première proposition jetée), `None` quand rien n'est gardé.
fn offroad_result(target: f64, tolerance: f64, steps: u8, first_kept_share: Option<f64>) -> OffroadResultV1 {
    OffroadResultV1 {
        target,
        tolerance,
        reached: first_kept_share.is_some_and(|s| within(s, target, tolerance)),
        steps: u32::from(steps),
    }
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
        // `OffroadOnRoad` : le service ne passe aucune cible en ROAD (400 avant le moteur).
        Failure::WindowEscape | Failure::TooLong | Failure::OffroadOnRoad => return Err(Problem::internal()),
    })
}

/// 503 `overloaded` : trop de générations, ou plus de place dans le budget de mémoire.
pub(crate) fn overloaded() -> Problem {
    let mut p = Problem::new(ProblemCode::Overloaded, "too many route generations in progress");
    p.retry_after = Some(Duration::from_secs(2));
    p
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
    pub(super) fn take_workspace(&self) -> Workspace {
        self.windows.take_workspace()
    }

    /// Rend un espace de travail à la réserve du gestionnaire : un trop gros est libéré (§9), ceux gardés comptent dans
    /// le budget et s'évincent en premier.
    pub(super) fn give_workspace(&self, ws: Workspace) {
        self.windows.give_workspace(ws, self.cfg.workspace_keep_mb * 1_000_000, self.cfg.max_concurrent);
    }

    /// Le moteur dans une fenêtre du cache : une fois, puis, si la recherche touche le bord de la fenêtre (le
    /// garde-fou du §5.2, jamais attendu), une fois de plus dans une fenêtre élargie de la plus longue chaîne qui
    /// touche les tuiles d'accroche et de 10 km ; un second échappement rend une erreur interne sans coordonnée.
    fn generate_windowed(
        &self,
        req: &Request,
        entry: Option<&Entry>,
        offroad: Option<f64>,
        deadline: Instant,
        stop: &mut dyn FnMut() -> bool,
    ) -> Result<SteeredGeneration, Problem> {
        let mut extra_m = 0.0;
        for attempt in 0..2 {
            let plan = self.engine.plan_with(req, self.cfg.window_radius_step_m, extra_m);
            let team = entry.map(|e| TeamRef { serial: e.serial, data: &e.team, taste: e.profile.as_ref() });
            let held = match self.windows.acquire(&self.engine, &plan, team, deadline, stop) {
                Ok(h) => h,
                Err(AcquireError::Stopped) => {
                    return Err(Problem::new(ProblemCode::Timeout, "generation budget exceeded"));
                }
                Err(AcquireError::Overloaded) => return Err(overloaded()),
            };
            let mut ws = self.take_workspace();
            let ctx = entry.zip(held.wt.as_deref()).map(|(e, wt)| TeamCtx { data: &e.team, win: wt });
            let steer = Steer {
                offroad: offroad.map(|t| {
                    // Aucun pas de la recherche après `min(échéance − 1 s, offroad_steps_until_ms)` : le pas 0 garde le
                    // budget entier du service, les suivants sont jetés plutôt que de coûter un 504.
                    let left_ms = deadline
                        .saturating_duration_since(Instant::now())
                        .saturating_sub(Duration::from_secs(1))
                        .as_millis();
                    OffroadTarget {
                        target_milli: (t * 1000.0).round() as u16,
                        tolerance_milli: (self.cfg.offroad_tolerance * 1000.0).round() as u16,
                        max_steps: self.cfg.offroad_max_steps,
                        steps_until_ms: u32::try_from(left_ms)
                            .unwrap_or(u32::MAX)
                            .min(self.cfg.offroad_steps_until_ms),
                    }
                }),
            };
            let result = generate_steered_in(&self.engine, &held.win, ctx, req, &steer, &mut ws, stop).0;
            self.give_workspace(ws);
            self.windows.release(held);
            match result {
                Err(Failure::WindowEscape) => {
                    metrics::counter!("ord_generator_window_escapes_total").increment(1);
                    tracing::warn!(attempt, "generator window escape");
                    extra_m = plan.snap_lmax_m + 10_000.0;
                }
                Ok(g) => return Ok(g),
                Err(f) => return Err(reason_of(f).map_or_else(|p| p, failed)),
            }
        }
        Err(Problem::internal())
    }

    pub(crate) fn in_coverage(&self, p: LatLon) -> bool {
        self.poly.contains(p.lon, p.lat)
    }

    /// Une place de calcul, dans le budget ; 503 `overloaded` si la file est pleine, 504 si le budget s'écoule.
    pub(crate) async fn slot(&self, deadline: Instant) -> Result<tokio::sync::OwnedSemaphorePermit, Problem> {
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
        if let Ok(GenerateResponseV1 { offroad: Some(o), .. }) = &outcome {
            let ride = ride_name(input.ride);
            metrics::histogram!("ord_generator_offroad_steps", "ride_type" => ride)
                .record(f64::from(o.steps));
            let band = format!("{:.1}", (o.target * 10.0).round() / 10.0);
            metrics::counter!("ord_generator_offroad_total", "ride_type" => ride, "target_band" => band, "reached" => if o.reached { "true" } else { "false" })
                .increment(1);
        }
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
            taste: computed.taste,
            dropped: computed.dropped,
            figures: input.figures,
            offroad: computed.offroad,
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
            Some(c) => match self.store.ready(c, &self.engine, &self.graph_index, &|bytes| {
                self.windows
                    .reserve_build(bytes, deadline, &|| cancel.load(Ordering::Relaxed))
                    .map(|g| Box::new(g) as Box<dyn Send + '_>)
                    .map_err(|e| match e {
                        AcquireError::Stopped => ReadyError::Stopped,
                        AcquireError::Overloaded => ReadyError::Overloaded,
                    })
            }) {
                Ok(e) => Some(e),
                // Même en découverte : sans la mémoire de l'équipe, la réponse dirait `overlay: null` et `team_m: 0`
                // alors qu'une surcouche existe, et dépendrait de la charge du service. Mieux vaut 503 (`Retry-After`).
                Err(ReadyError::Overloaded) => return Err(overloaded()),
                Err(ReadyError::Stopped) => {
                    return Err(Problem::new(ProblemCode::Timeout, "generation budget exceeded"))
                }
                Err(ReadyError::Failed) => return Err(Problem::internal()),
                Err(ReadyError::State(state)) if need_team => {
                    return Err(overlay_required(match state {
                        State::Stale(StaleReason::Graph | StaleReason::Matcher) => OverlayProblemState::Stale,
                        // Jamais rendu par `Store::ready` : un profil absent ne bloque aucune génération (la table
                        // commune sert). Écrit pour l'exhaustivité ; un test le tient.
                        State::Stale(StaleReason::Profile) | State::Absent | State::Ready => {
                            OverlayProblemState::Absent
                        }
                    }));
                }
                Err(ReadyError::State(_)) => None,
            },
            None if need_team => return Err(overlay_required(OverlayProblemState::Absent)),
            None => None,
        };
        if Instant::now() >= deadline {
            return Err(Problem::new(ProblemCode::Timeout, "generation budget exceeded"));
        }
        // Les attracteurs (4d) : les poids généraux du type quand le réglage est `on` et que les deux fichiers sont là, sinon aucun
        // (le moteur rend alors 1.12 au bit près).
        let attr = if self.attractors.enabled {
            ord_loops::AttrWeights::general(input.ride)
        } else {
            ord_loops::AttrWeights::NONE
        };
        *self.hooks.last_attr_weights.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(attr);
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
            figures: match input.figures {
                FiguresV1::Classic => ord_loops::Figures::Classic,
                // Le mélange de l'équipe (ses formes au dépôt) s'il existe, sinon le général ; les réglages du service.
                FiguresV1::Organic => ord_loops::Figures::Organic(ord_loops::Organic {
                    mix: ord_graph_match::profile::shape::mix_for(
                        entry.as_ref().and_then(|e| e.shapes.as_ref()),
                        input.ride,
                    ),
                    tune: ord_loops::OrganicTune::default(),
                    attr,
                }),
            },
        };
        let mut stop = || cancel.load(Ordering::Relaxed) || Instant::now() >= deadline;
        let steered =
            self.generate_windowed(&req, entry.as_deref(), input.offroad_target, deadline, &mut stop)?;
        let generation = steered.generation;

        let checker = Checker::with_tables(
            self.engine.base(),
            entry.as_ref().map(|e| &e.overlay),
            Config {
                rideability: match self.cfg.rideability {
                    crate::config::GeneratorRideability::On => ord_loops_check::Rideability::On,
                    crate::config::GeneratorRideability::Off => ord_loops_check::Rideability::Off,
                },
                ..Config::default()
            },
            &self.graph_index,
            entry.as_ref().map(|e| &e.check),
        );
        let (mut routes, mut dropped) = (Vec::new(), 0u32);
        let snaps = (generation.diagnostics.start_snap_m, generation.diagnostics.end_snap_m);
        let mut first_kept_share: Option<f64> = None;
        let corrupt_first = self.hooks.corrupt_first_route.load(Ordering::Relaxed);
        for (i, mut route) in generation.routes.into_iter().enumerate() {
            if (self.hooks.corrupt_routes.load(Ordering::Relaxed) || (corrupt_first && i == 0))
                && route.steps.len() > 1
            {
                route.steps.remove(0);
            }
            let report = checker.check(&req, &route);
            if report.ok() {
                let rendered = self.render(&route, &report, snaps);
                first_kept_share.get_or_insert(rendered.offroad_share);
                routes.push(rendered);
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
        for family in ["turnaround", "triangle"] {
            let n = routes.iter().filter(|r| r.family == family).count() as u64;
            if n > 0 {
                metrics::counter!("ord_generator_routes_total", "family" => family).increment(n);
            }
        }
        Ok(Computed {
            routes,
            dropped,
            taste: if entry.as_ref().is_some_and(|e| e.profile.is_some()) {
                TasteV1::Team
            } else {
                TasteV1::Common
            },
            offroad: input.offroad_target.map(|t| {
                // Sans consigne rendue par le moteur (jamais : elle suit la cible), un seul pas.
                let steps = steered.offroad.map_or(1, |o| o.steps);
                offroad_result(t, self.cfg.offroad_tolerance, steps, first_kept_share)
            }),
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
            offroad_m: r1(report.offroad_m),
            offroad_share: (report.offroad_m / len * 1000.0).round() / 1000.0,
            bearing_deviation_deg: route.bearing_deviation_deg.map(r1),
            family: match route.family {
                Family::Turnaround => "turnaround",
                Family::Polygon => "polygon",
                Family::Triangle => "triangle",
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

    /// Lot 5, §8.2 : `reached` se lit sur la première proposition que le contrôleur a gardée, avec sa part à lui.
    #[test]
    fn reached_is_read_on_the_first_route_the_checker_kept() {
        // Le moteur visait 0,47 et avait trouvé 0,46 en première ; le contrôleur la jette, la seconde (0,30) est la première
        // rendue : la cible n'est plus tenue.
        let kept = [0.30, 0.44];
        assert!(!offroad_result(0.47, 0.05, 2, kept.first().copied()).reached);
        // L'inverse : la première jetée était loin, la suivante est dans la tolérance.
        assert!(offroad_result(0.47, 0.05, 2, Some(0.50)).reached);
        // Les bords sont inclus ; rien de gardé : jamais atteinte (la réponse est alors une erreur).
        assert!(offroad_result(0.47, 0.05, 1, Some(0.52)).reached);
        assert!(offroad_result(0.47, 0.05, 1, Some(0.42)).reached);
        assert!(!offroad_result(0.47, 0.05, 1, Some(0.419)).reached);
        assert!(!offroad_result(0.47, 0.05, 1, None).reached);
        let o = offroad_result(0.25, 0.05, 3, Some(0.25));
        assert_eq!((o.target, o.tolerance, o.steps), (0.25, 0.05, 3));
    }
}
