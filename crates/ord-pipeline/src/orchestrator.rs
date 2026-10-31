//! Orchestrateur (T01 §5.1, étapes 2, 3, 4a ∥ 4b, 5, 6, 7, 8, 9).

use std::sync::Arc;
use std::time::Duration;

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

use crate::decision::decide_outcome;
use crate::elevation::sample_raw;
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
}

impl std::fmt::Debug for Orchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Orchestrator").field("api_version", &self.api_version).finish_non_exhaustive()
    }
}

fn join_error(e: tokio::task::JoinError) -> PipelineError {
    PipelineError::Internal(format!("computation task failed: {e}"))
}

impl Orchestrator {
    pub fn new(sources: Sources, rules: Rules, api_version: &'static str, retry: RetryAfterPolicy) -> Self {
        Self { sources, rules: Arc::new(rules), api_version, retry }
    }

    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    /// Calcule le rapport ; l'échéance du contexte borne tout le pipeline (504, jamais de réponse
    /// tronquée).
    pub async fn run(&self, req: ValidatedRequest, ctx: &RequestCtx) -> Result<RouteReport, PipelineError> {
        let deadline = tokio::time::Instant::from_std(ctx.deadline.at);
        match tokio::time::timeout_at(deadline, self.run_inner(req, ctx)).await {
            Ok(result) => result,
            Err(_) => Err(PipelineError::Timeout),
        }
    }

    async fn run_inner(&self, req: ValidatedRequest, ctx: &RequestCtx) -> Result<RouteReport, PipelineError> {
        // Contrôle coopératif aux frontières d'étape (T01 §3.3).
        let check = || if ctx.deadline.expired() { Err(PipelineError::Timeout) } else { Ok(()) };
        check()?;
        let options = req.options.clone();
        let step = self.rules.elevation.station_step_m;
        // 2. Préparation et 3. zonage (bloquants).
        let points = req.points;
        let coverage = Arc::clone(&self.sources.coverage);
        let (route, cov) = tokio::task::spawn_blocking(move || {
            let route = prepare(&points, step);
            let in_zone: Vec<bool> = route.samples.iter().map(|s| coverage.in_primary_zone(s.at)).collect();
            let cov = zone_ranges(&route, &in_zone);
            (route, cov)
        })
        .await
        .map_err(join_error)?;
        let route = Arc::new(route);

        check()?;
        // 4a ∥ 4b. Typologie et MNT aux stations : `join!`, pas `try_join!` (la réponse partielle a
        // besoin des deux branches, T01 §5.2).
        let (typo, raw) = tokio::join!(
            self.sources.typology.run(Arc::clone(&route), ctx),
            sample_raw(&route, self.sources.terrain.as_ref(), ctx)
        );
        let (typo, raw) = (typo?, raw?);

        // 5. Décision. Un repli Mapterhorn absorbé est une donnée (`terrain_fallback`), pas une panne.
        let elevation = if raw.terrain.iter().any(|t| t.missing == Some(Missing::Failed)) {
            SourceOutcome::Failed { retry_after: Some(self.retry_after(ctx)) }
        } else {
            SourceOutcome::Ok(())
        };
        let typology = if typo.coverage.unavailable.is_empty() {
            SourceOutcome::Ok(())
        } else {
            SourceOutcome::Failed { retry_after: Some(self.retry_after(ctx)) }
        };
        let typology_failed = match decide_outcome(&typology, &elevation, options.accept_partial) {
            Outcome::Reject(e) => return Err(e),
            Outcome::Proceed { typology_failed, .. } => typology_failed,
        };

        let versions = Versions {
            api_version: self.api_version,
            rules_version: RULES_VERSION,
            data_versions: self.sources.catalog.data_versions(&ctx.log()),
        };
        let attribution = self.sources.catalog.attributions();

        check()?;
        // 6 à 9 : une seule tâche bloquante.
        let rules = Arc::clone(&self.rules);
        tokio::task::spawn_blocking(move || {
            let profile = fuse(&route, &raw, &options, &rules)?;
            let segments = build_segments(&typo.runs, &typo.classes, &route, rules.typology.min_segment_m);
            let points = sample_output(&route, &profile, &options.sampling);
            let part = TypologyPart {
                output: &typo,
                segments: &segments,
                failed: typology_failed,
                debug: options.debug,
            };
            Ok(assemble(&route, &cov, part, profile, points, versions, attribution))
        })
        .await
        .map_err(join_error)?
    }

    /// `max(Retry-After amont du journal)`, borné ; sans indice, la valeur par défaut (T01 §4.4).
    fn retry_after(&self, ctx: &RequestCtx) -> Duration {
        let upstream = ctx.log().failures.iter().filter_map(|f| f.retry_after).max();
        upstream.map_or(self.retry.default, |d| d.clamp(self.retry.min, self.retry.max))
    }
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
                    let missing = if i == 1 { self.0 } else { None };
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
}
