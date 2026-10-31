//! Orchestrateur : admission, budget, chaînes typologie et élévation, décision (T01 §3, §5).
//!
//! Incrément 3 : préparation → zonage (zone France) → typologie et lecture du MNT en parallèle
//! (`tokio::join!`) → décision → fusion, segments, points de sortie et assemblage.
//! L'admission (file bornée, 503 `overloaded`) arrive à l'incrément 6.

pub mod decision;
pub mod elevation;
mod orchestrator;
pub mod typology;

use std::time::Duration;

use ord_core::elevation::ElevationError;
use ord_core::typology::TypologyError;

pub use decision::decide_outcome;
pub use orchestrator::{Orchestrator, RetryAfterPolicy, Sources};

/// Côté de données en panne (`details.source` du 503 `data_unavailable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataSide {
    Typology,
    Elevation,
    Both,
}

impl DataSide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Typology => "typology",
            Self::Elevation => "elevation",
            Self::Both => "both",
        }
    }
}

/// Erreur de pipeline ; traduite en problème HTTP par `ord-server` seulement (T01 §4.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PipelineError {
    Unavailable {
        source: DataSide,
        retry_after: Option<Duration>,
    },
    OutOfCoverage,
    Timeout,
    Overloaded,
    /// Client parti : rien n'est envoyé, l'événement est journalisé.
    Cancelled,
    Internal(String),
}

// Écrit à la main : thiserror prendrait le champ `source` pour une cause chaînée.
impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable { source, .. } => write!(f, "{} data unavailable", source.as_str()),
            Self::OutOfCoverage => f.write_str("no data from any source on the route"),
            Self::Timeout => f.write_str("request budget exceeded"),
            Self::Overloaded => f.write_str("too many requests in flight"),
            Self::Cancelled => f.write_str("request cancelled"),
            Self::Internal(cause) => write!(f, "internal error: {cause}"),
        }
    }
}

impl std::error::Error for PipelineError {}

impl From<ElevationError> for PipelineError {
    fn from(e: ElevationError) -> Self {
        match e {
            ElevationError::Timeout => Self::Timeout,
            ElevationError::Cancelled => Self::Cancelled,
            ElevationError::Internal(cause) => Self::Internal(cause),
        }
    }
}

impl From<TypologyError> for PipelineError {
    fn from(e: TypologyError) -> Self {
        match e {
            TypologyError::Timeout => Self::Timeout,
            TypologyError::Cancelled => Self::Cancelled,
            TypologyError::Internal(cause) => Self::Internal(cause),
        }
    }
}

/// Décision du mode de réponse (T01 §5.3) : rejet ou poursuite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Proceed { typology_failed: bool, elevation_failed: bool },
    Reject(PipelineError),
}
