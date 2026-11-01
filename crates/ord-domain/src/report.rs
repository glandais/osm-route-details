//! Avertissements de la réponse (doc 02 §4.5).

use std::ops::Range;

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    UnmatchedRange,
    StructureUncorrected,
    StructureSlopeImplausible,
    ElevationGapFilled,
    ElevationMissing,
    TypologyMissing,
    TerrainFallback,
    DsmUnavailable,
    DsmSource,
    OutsidePrimaryZone,
    TypologyUnavailable,
    ElevationUnavailable,
    // V1 : ouvrage détecté par heuristique.
    StructureUntagged,
    // V1 (api 1.1) : route fermée au vélo suivie, typologie rendue (état « hors graphe »).
    BicycleForbidden,
}

impl WarningCode {
    pub const ALL: [Self; 14] = [
        Self::UnmatchedRange,
        Self::StructureUncorrected,
        Self::StructureSlopeImplausible,
        Self::ElevationGapFilled,
        Self::ElevationMissing,
        Self::TypologyMissing,
        Self::TerrainFallback,
        Self::DsmUnavailable,
        Self::DsmSource,
        Self::OutsidePrimaryZone,
        Self::TypologyUnavailable,
        Self::ElevationUnavailable,
        Self::StructureUntagged,
        Self::BicycleForbidden,
    ];
}

/// Avertissement ; `message` est un gabarit anglais fixe, sans coordonnées.
#[derive(Clone, Debug, PartialEq)]
pub struct Warning {
    pub code: WarningCode,
    pub message: String,
    pub range: Option<Range<f64>>,
}
