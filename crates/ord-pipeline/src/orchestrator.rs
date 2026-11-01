//! Orchestrateur (T01 §5.1, étapes 2, 3, 4a ∥ 4b, 5, 4c, 6, 7, 8, 9).

use std::sync::Arc;
use std::time::{Duration, Instant};

use ord_core::assemble::{assemble, RouteReport, TypologyPart};
use ord_core::coverage::zone_ranges;
use ord_core::elevation::{fuse, sample_output};
use ord_core::prepare::prepare;
use ord_core::typology::build_segments;
use ord_core::{Rules, RULES_VERSION};
use ord_domain::budget::Budget;
use ord_domain::catalog::{DataCatalog, Versions};
use ord_domain::coverage::{CoverageRef, SourceOutcome};
use ord_domain::ctx::RequestCtx;
use ord_domain::elevation::{Missing, TerrainReader};
use ord_domain::request::ValidatedRequest;
use ord_domain::tiles::SourceId;
use tokio::sync::Semaphore;

use crate::admission::{Admission, AdmissionLimits, CancelOnDrop};
use crate::decision::{decide_outcome, out_of_coverage};
use crate::elevation::{sample_dsm_hull, sample_raw, sample_structures};
use crate::typology::TypologyChain;
use crate::{Outcome, PipelineError};

/// Implémentations des ports, assemblées par `ord-server` (T01 §2.1).
#[derive(Clone)]
pub struct Sources {
    pub typology: Arc<dyn TypologyChain>,
    pub terrain: Arc<dyn TerrainReader>,
    pub coverage: Arc<dyn CoverageRef>,
    pub catalog: Arc<dyn DataCatalog>,
}

impl std::fmt::Debug for Sources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sources").finish_non_exhaustive()
    }
}

/// Bornes de `Retry-After` du 503 `data_unavailable` (T01 §4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryAfterPolicy {
    pub min: Duration,
    pub max: Duration,
    pub default: Duration,
}

impl Default for RetryAfterPolicy {
    fn default() -> Self {
        Self { min: Duration::from_secs(5), max: Duration::from_secs(120), default: Duration::from_secs(30) }
    }
}

pub struct Orchestrator {
    sources: Sources,
    rules: Arc<Rules>,
    api_version: &'static str,
    retry: RetryAfterPolicy,
    admission: Admission,
    /// `compute_permits` (T01 §3.1) : chaque tâche bloquante du cœur en prend un, déplacé dans la
    /// tâche.
    compute: Arc<Semaphore>,
}

impl std::fmt::Debug for Orchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Orchestrator").field("api_version", &self.api_version).finish_non_exhaustive()
    }
}

fn join_error(e: tokio::task::JoinError) -> PipelineError {
    PipelineError::Internal(format!("computation task failed: {e}"))
}

/// Nombre de CPU, défaut des jetons d'admission et de calcul (T07 `budget`).
pub fn cpus() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

/// Erreur de budget : client parti (`Cancelled`) ou échéance (`Timeout`).
fn budget_error(ctx: &RequestCtx) -> Option<PipelineError> {
    if ctx.deadline.is_cancelled() {
        Some(PipelineError::Cancelled)
    } else if ctx.deadline.expired() {
        Some(PipelineError::Timeout)
    } else {
        None
    }
}

fn stage(name: &'static str, started: Instant) {
    metrics::histogram!("ord_stage_duration_seconds", "stage" => name)
        .record(started.elapsed().as_secs_f64());
}

impl Orchestrator {
    pub fn new(sources: Sources, rules: Rules, api_version: &'static str, retry: RetryAfterPolicy) -> Self {
        Self {
            sources,
            rules: Arc::new(rules),
            api_version,
            retry,
            admission: Admission::new(AdmissionLimits::default()),
            compute: Arc::new(Semaphore::new(cpus())),
        }
    }

    /// File d'admission (`max_in_flight`, `max_queued`).
    pub fn with_admission(mut self, admission: Admission) -> Self {
        self.admission = admission;
        self
    }

    /// Jetons de calcul, partagés avec la chaîne typologie.
    pub fn with_compute_permits(mut self, compute: Arc<Semaphore>) -> Self {
        self.compute = compute;
        self
    }

    pub fn admission(&self) -> &Admission {
        &self.admission
    }

    /// Calcul bloquant du cœur sous un jeton de calcul, déplacé dans la tâche : un client qui part
    /// ne rend pas un jeton encore utilisé (T01 §3.1).
    async fn blocking<R: Send + 'static>(
        &self,
        f: impl FnOnce() -> R + Send + 'static,
    ) -> Result<R, PipelineError> {
        let permit = Arc::clone(&self.compute)
            .acquire_owned()
            .await
            .map_err(|_| PipelineError::Internal("compute permits closed".into()))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            f()
        })
        .await
        .map_err(join_error)
    }

    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    /// Sources du service (diagnostics du banc : étapes du pipeline rejouées une à une).
    pub fn sources(&self) -> &Sources {
        &self.sources
    }

    /// Admission (file bornée), puis calcul du rapport. L'échéance du contexte borne l'attente en
    /// file et tout le pipeline (504, jamais de réponse tronquée). Si le futur est lâché avant la
    /// fin (client parti) ou interrompu par l'échéance, le `Deadline` est annulé : les tâches
    /// bloquantes s'arrêtent au prochain contrôle, et les jetons d'admission et de calcul sont
    /// rendus (T01 §3.2–3.3).
    pub async fn run(&self, req: ValidatedRequest, ctx: &RequestCtx) -> Result<RouteReport, PipelineError> {
        let guard = CancelOnDrop::new(&ctx.deadline);
        let result = async {
            let _permit = self.admission.admit(&ctx.deadline).await?;
            let deadline = tokio::time::Instant::from_std(ctx.deadline.at);
            match tokio::time::timeout_at(deadline, self.run_inner(req, ctx)).await {
                Ok(result) => result,
                Err(_) => Err(PipelineError::Timeout),
            }
        }
        .await;
        if result.as_ref().err() == Some(&PipelineError::Timeout) {
            // Échéance : le calcul en cours s'arrête au prochain contrôle.
            ctx.deadline.cancel();
        }
        guard.disarm();
        result
    }

    async fn run_inner(&self, req: ValidatedRequest, ctx: &RequestCtx) -> Result<RouteReport, PipelineError> {
        // Contrôle coopératif aux frontières d'étape (T01 §3.3).
        let check = || budget_error(ctx).map_or(Ok(()), Err);
        check()?;
        let options = req.options.clone();
        let step = self.rules.elevation.station_step_m;
        // 2. Préparation et 3. zonage (bloquants).
        let points = req.points;
        let coverage = Arc::clone(&self.sources.coverage);
        let started = Instant::now();
        let (route, cov) = self
            .blocking(move || {
                let route = prepare(&points, step);
                let in_zone: Vec<bool> =
                    route.samples.iter().map(|s| coverage.in_primary_zone(s.at)).collect();
                let cov = zone_ranges(&route, &in_zone);
                (route, cov)
            })
            .await?;
        stage("prepare", started);
        // 422 dès le zonage si aucune source ne couvre rien (T01 §5.3).
        if out_of_coverage(&cov, route.length_m) {
            return Err(PipelineError::OutOfCoverage);
        }
        let route = Arc::new(route);

        check()?;
        let heuristics = self.rules.elevation.heuristics && options.structure_correction;
        // 4a ∥ 4b. Typologie et MNT aux stations : `join!`, pas `try_join!` (la réponse partielle a
        // besoin des deux branches, T01 §5.2).
        let started = Instant::now();
        let (typo, raw) = tokio::join!(
            async {
                let r = self.sources.typology.run(Arc::clone(&route), ctx).await;
                stage("typology", started);
                r
            },
            async {
                let mut r = sample_raw(&route, self.sources.terrain.as_ref(), ctx).await;
                stage("terrain", started);
                // V1 : MNS sur toute l'enveloppe LiDAR pour les heuristiques, en parallèle de la
                // typologie (`DsmScope::LidarHull`, T04 §2.3), seulement si la correction est
                // demandée.
                if heuristics {
                    if let Ok(raw) = r.as_mut() {
                        let dsm_started = Instant::now();
                        if let Err(e) = sample_dsm_hull(raw, &route, self.sources.terrain.as_ref(), ctx).await
                        {
                            r = Err(e);
                        }
                        stage("dsm_hull", dsm_started);
                    }
                }
                r
            }
        );
        let (typo, mut raw) = (typo?, raw?);
        check()?;

        // 5. Décision (règle stricte). Un repli Mapterhorn absorbé est une donnée
        // (`terrain_fallback`), pas une panne ; une station hors emprise non plus.
        let elevation = if raw.terrain.iter().any(|t| t.missing == Some(Missing::Failed)) {
            SourceOutcome::Failed { retry_after: Some(self.retry_after(ctx, DataSideFilter::Elevation)) }
        } else if raw.terrain.iter().any(|t| t.fallback_after_error) {
            SourceOutcome::Fallback(())
        } else if raw.terrain.iter().any(|t| t.missing == Some(Missing::NoData)) {
            SourceOutcome::Missing(())
        } else {
            SourceOutcome::Ok(())
        };
        let typology = if !typo.coverage.unavailable.is_empty() {
            SourceOutcome::Failed { retry_after: Some(self.retry_after(ctx, DataSideFilter::Typology)) }
        } else if !typo.coverage.missing.is_empty() {
            SourceOutcome::Missing(())
        } else {
            SourceOutcome::Ok(())
        };
        let typology_failed = match decide_outcome(&typology, &elevation, options.accept_partial) {
            Outcome::Reject(e) => return Err(e),
            Outcome::Proceed { typology_failed, .. } => typology_failed,
        };

        // 4c. Lectures des ouvrages (MNS des ponts, extensions hors du parcours), seulement si la
        // correction est demandée.
        if options.structure_correction {
            check()?;
            let started = Instant::now();
            sample_structures(&mut raw, &route, &typo, self.sources.terrain.as_ref(), ctx, &self.rules)
                .await?;
            stage("structures", started);
        }

        let versions = Versions {
            api_version: self.api_version,
            rules_version: RULES_VERSION,
            data_versions: self.sources.catalog.data_versions(&ctx.log()),
        };
        let attribution = self.sources.catalog.attributions();

        check()?;
        // 6 à 9 : une seule tâche bloquante.
        let rules = Arc::clone(&self.rules);
        let started = Instant::now();
        let report = self
            .blocking(move || {
                let profile = fuse(&route, &raw, Some(&typo), &options, &rules)?;
                let segments = build_segments(
                    &typo.runs,
                    &typo.classes,
                    &profile.verdicts,
                    &route,
                    rules.typology.min_segment_m,
                );
                let points = sample_output(
                    &route,
                    &profile,
                    &options.sampling,
                    &segments,
                    &rules.elevation.breakpoints,
                );
                let part = TypologyPart {
                    output: &typo,
                    segments: &segments,
                    failed: typology_failed,
                    debug: options.debug,
                };
                Ok::<_, PipelineError>(assemble(&route, &cov, part, profile, points, versions, attribution))
            })
            .await?;
        stage("assemble", started);
        report
    }

    /// `Retry-After` d'un 503 `data_unavailable` (T01 §4.4) : le plus grand de `Retry-After` amont
    /// et de l'échéance restante du cache négatif, parmi les tuiles en échec du côté en panne ;
    /// borné à `[min, max]` ; sans indice, la valeur par défaut.
    fn retry_after(&self, ctx: &RequestCtx, side: DataSideFilter) -> Duration {
        retry_after_hint(&ctx.log().failures, side, Instant::now())
            .map_or(self.retry.default, |d| d.clamp(self.retry.min, self.retry.max))
    }
}

/// Sources d'un côté de données.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DataSideFilter {
    Typology,
    Elevation,
}

impl DataSideFilter {
    fn contains(self, source: SourceId) -> bool {
        match self {
            Self::Typology => source == SourceId::Streets,
            Self::Elevation => source != SourceId::Streets,
        }
    }
}

fn retry_after_hint(
    failures: &[ord_domain::ctx::TileFailure],
    side: DataSideFilter,
    now: Instant,
) -> Option<Duration> {
    failures
        .iter()
        .filter(|f| side.contains(f.source))
        .flat_map(|f| [f.retry_after, f.negative_until.map(|t| t.saturating_duration_since(now))])
        .flatten()
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ord_domain::budget::{Deadline, RequestId};
    use ord_domain::catalog::{Attribution, DataVersions, Readiness};
    use ord_domain::ctx::DataLog;
    use ord_domain::elevation::{DemKind, DsmSample, TerrainOrigin, TerrainSample};
    use ord_domain::request::RequestOptions;
    use ord_domain::LatLon;

    struct Flat(Option<Missing>);

    #[async_trait::async_trait]
    impl TerrainReader for Flat {
        async fn sample_terrain(&self, pts: &[LatLon], _: &RequestCtx) -> Vec<TerrainSample> {
            pts.iter()
                .enumerate()
                .map(|(i, _)| {
                    // Panne de la station 1 à la fin : trou non encadré, jamais comblé.
                    let missing = if i >= 1 { self.0 } else { None };
                    TerrainSample {
                        ele: missing.is_none().then_some(42.0),
                        origin: if missing.is_none() {
                            TerrainOrigin::Mapterhorn
                        } else {
                            TerrainOrigin::None
                        },
                        fallback_after_error: false,
                        dem_kind: DemKind::Unknown,
                        missing,
                    }
                })
                .collect()
        }
        async fn sample_ign_dtm(&self, pts: &[LatLon], _: &RequestCtx) -> Vec<Option<f32>> {
            vec![None; pts.len()]
        }
        async fn sample_dsm(&self, pts: &[LatLon], _: &RequestCtx) -> Vec<DsmSample> {
            vec![DsmSample::NoData; pts.len()]
        }
        fn pixel_size_m(&self, _: LatLon, _: TerrainOrigin) -> f64 {
            13.0
        }
    }

    struct NoCatalog;

    impl DataCatalog for NoCatalog {
        fn attributions(&self) -> Vec<Attribution> {
            Vec::new()
        }
        fn data_versions(&self, _: &DataLog) -> DataVersions {
            DataVersions::default()
        }
        fn readiness(&self) -> Readiness {
            Readiness { ready: true, checks: Vec::new() }
        }
    }

    /// Zone France = latitudes au-dessus de 45,0005° (seconde moitié du parcours de test).
    struct HalfZone;

    #[async_trait::async_trait]
    impl CoverageRef for HalfZone {
        fn ign_may_have_data(&self, _: ord_domain::tiles::TileKey) -> bool {
            false
        }
        fn in_primary_zone(&self, p: LatLon) -> bool {
            p.lat > 45.0005
        }
        async fn dem_kinds(&self, pts: &[LatLon], _: &RequestCtx) -> Vec<DemKind> {
            vec![DemKind::Unknown; pts.len()]
        }
        fn extent(&self, _: ord_domain::tiles::SourceId) -> &ord_domain::coverage::Extent {
            &ord_domain::coverage::Extent::World
        }
    }

    /// Typologie sans aucune rue : tout le parcours est non apparié.
    struct NoStreets;

    #[async_trait::async_trait]
    impl TypologyChain for NoStreets {
        async fn run(
            &self,
            route: Arc<ord_domain::route::PreparedRoute>,
            _: &RequestCtx,
        ) -> Result<ord_core::typology::TypologyOutput, ord_core::typology::TypologyError> {
            let classes: Arc<[ord_core::typology::EdgeClass]> = Vec::new().into();
            Ok(ord_core::typology::TypologyOutput {
                runs: vec![ord_core::typology::TypedRun {
                    from_m: 0.0,
                    to_m: route.length_m,
                    matched: ord_core::typology::RunMatch::Unmatched,
                }],
                coverage: Default::default(),
                classes,
                stats: Default::default(),
                structures: Vec::new(),
                passages: Vec::new(),
                overpasses: Vec::new(),
            })
        }
    }

    fn orchestrator(missing: Option<Missing>) -> Orchestrator {
        Orchestrator::new(
            Sources {
                typology: Arc::new(NoStreets),
                terrain: Arc::new(Flat(missing)),
                coverage: Arc::new(HalfZone),
                catalog: Arc::new(NoCatalog),
            },
            Rules::default(),
            "1.0",
            RetryAfterPolicy::default(),
        )
    }

    fn request(accept_partial: bool) -> ValidatedRequest {
        let points = vec![LatLon::new(45.0, 6.0), LatLon::new(45.001, 6.0)];
        ValidatedRequest {
            input_len: 2,
            points,
            options: RequestOptions { accept_partial, ..RequestOptions::default() },
        }
    }

    fn ctx() -> RequestCtx {
        RequestCtx::new(RequestId("t".into()), Deadline::after(Duration::from_secs(10)))
    }

    #[tokio::test]
    async fn failed_station_gives_503_unless_partial_accepted() {
        let err = orchestrator(Some(Missing::Failed)).run(request(false), &ctx()).await.unwrap_err();
        assert_eq!(
            err,
            PipelineError::Unavailable {
                source: crate::DataSide::Elevation,
                retry_after: Some(Duration::from_secs(30))
            }
        );
        let report = orchestrator(Some(Missing::Failed)).run(request(true), &ctx()).await.unwrap();
        assert_eq!(report.status, ord_core::assemble::Status::Partial);
        assert!(report
            .warnings
            .iter()
            .any(|w| w.code == ord_domain::report::WarningCode::ElevationUnavailable));
    }

    #[tokio::test]
    async fn complete_run_and_deadline() {
        let report = orchestrator(None).run(request(false), &ctx()).await.unwrap();
        assert_eq!(report.status, ord_core::assemble::Status::Complete);
        assert_eq!(report.summary.ele_max_m, Some(42.0));
        assert_eq!(report.segments.len(), 1);
        assert!(!report.segments[0].typed.matched);
        assert_eq!(report.quality.matched_ratio, 0.0);
        let outside: Vec<_> = report
            .warnings
            .iter()
            .filter(|w| w.code == ord_domain::report::WarningCode::OutsidePrimaryZone)
            .collect();
        assert_eq!(outside.len(), 1, "zonage : première moitié hors zone France");
        assert_eq!(outside[0].range.as_ref().map(|r| r.start), Some(0.0));
        let expired = RequestCtx::new(RequestId("t".into()), Deadline::after(Duration::ZERO));
        assert_eq!(
            orchestrator(None).run(request(false), &expired).await.unwrap_err(),
            PipelineError::Timeout
        );
    }

    /// Typologie qui calcule (tâche bloquante) jusqu'à ce que le budget l'arrête, et note ce
    /// qu'elle a vu.
    struct Spinning {
        stopped: Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait::async_trait]
    impl TypologyChain for Spinning {
        async fn run(
            &self,
            _: Arc<ord_domain::route::PreparedRoute>,
            ctx: &RequestCtx,
        ) -> Result<ord_core::typology::TypologyOutput, ord_core::typology::TypologyError> {
            let (budget, stopped) = (ctx.deadline.clone(), Arc::clone(&self.stopped));
            let cancelled = ctx.deadline.clone();
            tokio::task::spawn_blocking(move || {
                let t0 = std::time::Instant::now();
                while !budget.expired() && t0.elapsed() < Duration::from_secs(10) {
                    std::thread::sleep(Duration::from_millis(1));
                }
                stopped.store(budget.is_cancelled(), std::sync::atomic::Ordering::SeqCst);
            })
            .await
            .ok();
            Err(if cancelled.is_cancelled() {
                ord_core::typology::TypologyError::Cancelled
            } else {
                ord_core::typology::TypologyError::Timeout
            })
        }
    }

    fn spinning(stopped: &Arc<std::sync::atomic::AtomicBool>, limits: AdmissionLimits) -> Orchestrator {
        Orchestrator::new(
            Sources {
                typology: Arc::new(Spinning { stopped: Arc::clone(stopped) }),
                terrain: Arc::new(Flat(None)),
                coverage: Arc::new(HalfZone),
                catalog: Arc::new(NoCatalog),
            },
            Rules::default(),
            "1.0",
            RetryAfterPolicy::default(),
        )
        .with_admission(Admission::new(limits))
    }

    /// Client parti : le futur de la requête est lâché ; le `Deadline` est annulé, le calcul
    /// bloquant s'arrête au contrôle suivant et le jeton d'admission est rendu.
    #[tokio::test]
    async fn dropped_request_cancels_its_computation_and_frees_its_permit() {
        let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let o = spinning(&stopped, AdmissionLimits { max_in_flight: 1, max_queued: 0 });
        let ctx = ctx();
        let run = tokio::time::timeout(Duration::from_millis(100), o.run(request(false), &ctx)).await;
        assert!(run.is_err(), "toujours en calcul");
        assert!(ctx.deadline.is_cancelled());
        assert_eq!(o.admission().in_flight(), 0, "jeton rendu");
        let t0 = std::time::Instant::now();
        while !stopped.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(t0.elapsed() < Duration::from_secs(5), "le calcul ne s'est pas arrêté");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Temps virtuel : une source qui ne répond qu'après le budget de 90 s donne un 504, sans
    /// attendre 90 s réelles, et le calcul en cours est annulé.
    #[tokio::test(start_paused = true)]
    async fn source_slower_than_the_budget_gives_a_timeout() {
        struct Slow;
        #[async_trait::async_trait]
        impl TypologyChain for Slow {
            async fn run(
                &self,
                route: Arc<ord_domain::route::PreparedRoute>,
                ctx: &RequestCtx,
            ) -> Result<ord_core::typology::TypologyOutput, ord_core::typology::TypologyError> {
                tokio::time::sleep(Duration::from_secs(600)).await;
                NoStreets.run(route, ctx).await
            }
        }
        let o = Orchestrator::new(
            Sources {
                typology: Arc::new(Slow),
                terrain: Arc::new(Flat(None)),
                coverage: Arc::new(HalfZone),
                catalog: Arc::new(NoCatalog),
            },
            Rules::default(),
            "1.0",
            RetryAfterPolicy::default(),
        );
        let ctx = RequestCtx::new(RequestId("t".into()), Deadline::after(Duration::from_secs(90)));
        let started = std::time::Instant::now();
        assert_eq!(o.run(request(false), &ctx).await.unwrap_err(), PipelineError::Timeout);
        assert!(started.elapsed() < Duration::from_secs(30), "temps virtuel");
        assert!(ctx.deadline.is_cancelled(), "calcul en cours annulé");
        assert_eq!(o.admission().in_flight(), 0);
    }

    /// File pleine : 503 `overloaded` tout de suite, sans toucher aux sources.
    #[tokio::test]
    async fn full_queue_gives_overloaded() {
        let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let o = Arc::new(spinning(&stopped, AdmissionLimits { max_in_flight: 1, max_queued: 0 }));
        let first = {
            let o = Arc::clone(&o);
            tokio::spawn(async move {
                let ctx = RequestCtx::new(RequestId("a".into()), Deadline::after(Duration::from_millis(500)));
                o.run(request(false), &ctx).await
            })
        };
        while o.admission().in_flight() == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(o.run(request(false), &ctx()).await.unwrap_err(), PipelineError::Overloaded);
        assert_eq!(first.await.unwrap().unwrap_err(), PipelineError::Timeout);
    }

    #[test]
    fn retry_after_takes_the_largest_hint_of_the_failed_side() {
        use ord_domain::ctx::{FailureKind, TileFailure};
        use ord_domain::tiles::TileKey;
        let now = Instant::now();
        let f = |source, retry_after: Option<u64>, negative_s: Option<u64>| TileFailure {
            source,
            tile: TileKey { z: 1, x: 0, y: 0 },
            kind: FailureKind::Server,
            status: Some(503),
            retry_after: retry_after.map(Duration::from_secs),
            negative_until: negative_s.map(|s| now + Duration::from_secs(s)),
            message: String::new(),
        };
        let log = [
            f(SourceId::Streets, Some(300), None),
            f(SourceId::IgnDtm, Some(7), Some(40)),
            f(SourceId::Mapterhorn, None, Some(55)),
        ];
        let hint = |side| retry_after_hint(&log, side, now);
        assert_eq!(hint(DataSideFilter::Elevation), Some(Duration::from_secs(55)), "cache négatif");
        assert_eq!(hint(DataSideFilter::Typology), Some(Duration::from_secs(300)), "amont");
        assert_eq!(retry_after_hint(&[], DataSideFilter::Elevation, now), None);
        // Bornes de la politique ; sans indice, le défaut.
        let o = orchestrator(None);
        let ctx = ctx();
        ctx.log().failures.extend(log);
        assert_eq!(o.retry_after(&ctx, DataSideFilter::Typology), Duration::from_secs(120));
        assert_eq!(
            o.retry_after(
                &RequestCtx::new(RequestId("t".into()), Deadline::after(Duration::from_secs(1))),
                DataSideFilter::Typology
            ),
            Duration::from_secs(30)
        );
    }
}
