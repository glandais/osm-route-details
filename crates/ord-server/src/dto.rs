//! DTO v1 de l'entrée (doc 02 §3). Le DTO de réponse arrive avec l'orchestrateur (incrément 1).
//!
//! Les `///` de ce module sont recopiés dans le contrat OpenAPI : ils sont en anglais.

use std::fmt;

use ord_domain::request::GainPreset;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Body of `POST /v1/route-details`: exactly one of `points` or `polyline`.
#[derive(Deserialize, ToSchema)]
pub struct RouteDetailsRequestV1 {
    /// Route points, in travel order.
    pub points: Option<Vec<PointV1>>,
    /// Encoded polyline (Google algorithm), at precision `polyline_precision`.
    pub polyline: Option<String>,
    /// Precision of `polyline` (5 or 6); required with `polyline`, refused with `points`.
    #[schema(minimum = 5, maximum = 6)]
    pub polyline_precision: Option<u8>,
    pub options: Option<OptionsV1>,
}

impl fmt::Debug for RouteDetailsRequestV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteDetailsRequestV1")
            .field("points", &self.points.as_ref().map(Vec::len))
            .field("polyline", &self.polyline.as_ref().map(String::len))
            .field("polyline_precision", &self.polyline_precision)
            .field("options", &self.options)
            .finish()
    }
}

/// Input point; an optional altitude (`ele` or `alt`) is accepted and ignored.
#[derive(Deserialize, ToSchema)]
pub struct PointV1 {
    #[schema(minimum = -85.05112878, maximum = 85.05112878)]
    pub lat: f64,
    #[schema(minimum = -180, maximum = 180)]
    pub lon: f64,
    /// Ignored.
    pub ele: Option<f64>,
    /// Ignored.
    pub alt: Option<f64>,
}

impl fmt::Debug for PointV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PointV1(..)")
    }
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct OptionsV1 {
    /// Response sections to return; default: all.
    pub sections: Option<Vec<SectionV1>>,
    pub sampling: Option<SamplingV1>,
    pub elevation: Option<ElevationOptionsV1>,
    /// Default: `true`.
    pub structure_correction: Option<bool>,
    /// Default: `false`.
    pub accept_partial: Option<bool>,
    /// Internal use only; refused unless the deployment allows it.
    pub debug: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SectionV1 {
    Elevation,
    Typology,
    Summary,
}

impl SectionV1 {
    pub const ALL: [Self; 3] = [Self::Elevation, Self::Typology, Self::Summary];
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct SamplingV1 {
    /// Default: `input_with_breakpoints`.
    pub mode: Option<SamplingModeV1>,
    /// Profile step in `distance` mode, in metres (bounds: `/v1/capabilities`).
    pub step_m: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SamplingModeV1 {
    InputWithBreakpoints,
    Input,
    Distance,
}

impl SamplingModeV1 {
    pub const ALL: [Self; 3] = [Self::InputWithBreakpoints, Self::Input, Self::Distance];
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct ElevationOptionsV1 {
    /// 0 (disabled) or within the published bounds, in metres.
    pub smoothing_m: Option<f64>,
    /// Default: `dem`.
    pub gain_preset: Option<GainPreset>,
}
