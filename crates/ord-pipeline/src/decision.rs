//! Décision du mode de réponse (T01 §5.3), fonction pure : elle ne décide que du **rejet** ; le
//! `status` d'une réponse 200 est fixé par l'assembleur.

use std::ops::Range;
use std::time::Duration;

use ord_domain::coverage::{CoverageRanges, SourceOutcome};

use crate::{DataSide, Outcome, PipelineError};

/// Règle stricte, sans seuil : une seule plage en panne suffit. Les deux côtés en panne donnent
/// toujours un 503 ; un seul côté, un 503 sauf `accept_partial`.
pub fn decide_outcome(t: &SourceOutcome<()>, e: &SourceOutcome<()>, accept_partial: bool) -> Outcome {
    let failed = |o: &SourceOutcome<()>| match o {
        SourceOutcome::Failed { retry_after } => Some(*retry_after),
        _ => None,
    };
    let max = |a: Option<Duration>, b: Option<Duration>| match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, y) => x.or(y),
    };
    match (failed(t), failed(e)) {
        (Some(rt), Some(re)) => {
            Outcome::Reject(PipelineError::Unavailable { source: DataSide::Both, retry_after: max(rt, re) })
        }
        (Some(retry_after), None) if !accept_partial => {
            Outcome::Reject(PipelineError::Unavailable { source: DataSide::Typology, retry_after })
        }
        (None, Some(retry_after)) if !accept_partial => {
            Outcome::Reject(PipelineError::Unavailable { source: DataSide::Elevation, retry_after })
        }
        (tf, ef) => Outcome::Proceed { typology_failed: tf.is_some(), elevation_failed: ef.is_some() },
    }
}

/// 422 `out_of_coverage`, décidé dès le zonage (T01 §5.3) : hors emprise de la typologie **et**
/// de l'élévation sur tout `[0, L]`. Avec les sources mondiales du MVP (`Extent::World`), les plages
/// d'emprise sont vides et ce cas ne se présente pas.
pub fn out_of_coverage(cov: &CoverageRanges, length_m: f64) -> bool {
    covers(&cov.typology_absent, length_m) && covers(&cov.elevation_absent, length_m)
}

/// Les plages, triées ou non, couvrent `[0, length]` sans trou.
fn covers(ranges: &[Range<f64>], length: f64) -> bool {
    let mut sorted: Vec<&Range<f64>> = ranges.iter().collect();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut reached = 0.0_f64;
    for r in sorted {
        if r.start > reached {
            return false;
        }
        reached = reached.max(r.end);
    }
    !ranges.is_empty() && reached >= length
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: SourceOutcome<()> = SourceOutcome::Ok(());
    const FB: SourceOutcome<()> = SourceOutcome::Fallback(());
    const MISSING: SourceOutcome<()> = SourceOutcome::Missing(());

    fn failed(s: u64) -> SourceOutcome<()> {
        SourceOutcome::Failed { retry_after: Some(Duration::from_secs(s)) }
    }

    fn reject(source: DataSide, s: u64) -> Outcome {
        Outcome::Reject(PipelineError::Unavailable { source, retry_after: Some(Duration::from_secs(s)) })
    }

    /// Table du doc 01 §6, ligne à ligne.
    #[test]
    fn decision_table() {
        let proceed = |t, e| Outcome::Proceed { typology_failed: t, elevation_failed: e };
        for partial in [false, true] {
            assert_eq!(decide_outcome(&OK, &OK, partial), proceed(false, false));
            assert_eq!(decide_outcome(&OK, &FB, partial), proceed(false, false), "repli absorbé : 200");
            assert_eq!(decide_outcome(&MISSING, &MISSING, partial), proceed(false, false));
            assert_eq!(
                decide_outcome(&failed(5), &failed(9), partial),
                reject(DataSide::Both, 9),
                "deux pannes : 503"
            );
        }
        assert_eq!(decide_outcome(&OK, &failed(7), false), reject(DataSide::Elevation, 7));
        assert_eq!(decide_outcome(&failed(7), &OK, false), reject(DataSide::Typology, 7));
        assert_eq!(decide_outcome(&OK, &failed(7), true), proceed(false, true));
        assert_eq!(decide_outcome(&failed(7), &FB, true), proceed(true, false));
    }

    #[test]
    fn out_of_coverage_needs_both_sides_absent_everywhere() {
        let cov = |t: Vec<Range<f64>>, e: Vec<Range<f64>>| CoverageRanges {
            typology_absent: t,
            elevation_absent: e,
            ..CoverageRanges::default()
        };
        assert!(out_of_coverage(&cov(vec![0.0..100.0], vec![50.0..100.0, 0.0..50.0]), 100.0));
        assert!(!out_of_coverage(&cov(vec![0.0..100.0], vec![0.0..40.0, 50.0..100.0]), 100.0), "trou");
        assert!(!out_of_coverage(&cov(vec![0.0..100.0], vec![]), 100.0), "élévation présente");
        assert!(!out_of_coverage(&CoverageRanges::default(), 100.0), "sources mondiales");
    }
}
