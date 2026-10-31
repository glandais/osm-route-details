//! Décision du mode de réponse (T01 §5.3), fonction pure : elle ne décide que du **rejet** ; le
//! `status` d'une réponse 200 est fixé par l'assembleur.

use std::time::Duration;

use ord_domain::coverage::SourceOutcome;

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
}
