//! DTO v1 (doc 02 §3–§4) : entrée, et réponse 200 de l'incrément 1 (élévation, résumé partiel,
//! provenance du terrain). Segments et répartitions typologiques arrivent à l'incrément 3 : ce
//! seront des ajouts au contrat, pas des ruptures.
//!
//! Les `///` de ce module sont recopiés dans le contrat OpenAPI : ils sont en anglais ; les notes
//! en français sont en `//`.

use std::collections::BTreeMap;
use std::fmt;

use ord_core::assemble::{RouteReport, Status};
use ord_core::elevation::{EleMethod, PointOrigin};
use ord_domain::budget::RequestId;
use ord_domain::catalog::DataVersions;
use ord_domain::elevation::TerrainOrigin;
use ord_domain::report::WarningCode;
use ord_domain::request::{GainPreset, SectionSet};
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

// ---------------------------------------------------------------------------------------------
// Réponse 200 (doc 02 §4)
// ---------------------------------------------------------------------------------------------

/// Response of `POST /v1/route-details`. `meta`, `status`, `quality` and `attribution` are always
/// present; `points` and `summary` follow the requested `sections`.
#[derive(Debug, Serialize, ToSchema)]
pub struct RouteDetailsResponseV1 {
    pub meta: MetaV1,
    pub status: StatusV1,
    /// Elevation profile (section `elevation`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<OutputPointV1>>,
    /// Route aggregates (section `summary`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<SummaryV1>,
    pub quality: QualityV1,
    pub attribution: AttributionRefV1,
}

/// Versions and request identity: equal requests with equal versions give equal responses, apart
/// from `request_id` and `duration_ms`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MetaV1 {
    pub api_version: &'static str,
    pub rules_version: &'static str,
    /// Version of each data source used by this request.
    pub data_versions: DataVersions,
    pub request_id: String,
    pub duration_ms: u64,
}

/// `partial` when a source gave no data on part of the route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusV1 {
    Complete,
    Partial,
}

/// Elevation method of a point or station.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EleMethodV1 {
    Dem,
    DsmDeck,
    InterpolatedStructure,
    GapFilled,
    None,
}

impl From<EleMethod> for EleMethodV1 {
    fn from(m: EleMethod) -> Self {
        match m {
            EleMethod::Dem => Self::Dem,
            EleMethod::DsmDeck => Self::DsmDeck,
            EleMethod::InterpolatedStructure => Self::InterpolatedStructure,
            EleMethod::GapFilled => Self::GapFilled,
            EleMethod::None => Self::None,
        }
    }
}

/// `input` for a client point, `inserted` for a point added by the service.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PointOriginV1 {
    Input,
    Inserted,
}

/// A point of the elevation profile.
#[derive(Serialize, ToSchema)]
pub struct OutputPointV1 {
    /// Cumulative distance from the start, in metres.
    pub d_m: f64,
    /// Client index of the input point, or of the last input point before it.
    pub index: u32,
    /// Absent in `distance` mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<PointOriginV1>,
    /// Only on inserted points and in `distance` mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    /// Only on inserted points and in `distance` mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    /// Elevation after structure correction, not smoothed; `null` when missing.
    #[schema(required = true)]
    pub ele: Option<f64>,
    /// Only when smoothing is requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ele_smoothed: Option<f64>,
    pub ele_method: EleMethodV1,
}

impl fmt::Debug for OutputPointV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutputPointV1").field("d_m", &self.d_m).finish_non_exhaustive()
    }
}

/// Route aggregates, computed on the internal 5 m profile.
#[derive(Debug, Serialize, ToSchema)]
pub struct SummaryV1 {
    pub length_m: f64,
    pub gain_m: f64,
    pub loss_m: f64,
    pub gain_params: GainParamsV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ele_min_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ele_max_m: Option<f64>,
    /// Length per terrain origin, in metres (keys: `terrain_origin` values).
    pub terrain_m: BTreeMap<String, f64>,
}

/// Effective parameters of the elevation gain.
#[derive(Debug, Serialize, ToSchema)]
pub struct GainParamsV1 {
    pub preset: GainPreset,
    pub threshold_m: f64,
    pub smoothing_m: f64,
    /// GLO-30 ranges where the threshold was adapted.
    pub adapted: Vec<AdaptedRangeV1>,
    /// Some elevation ranges are missing: the gain sums the ranges that were read.
    pub partial: bool,
}

/// A range read on GLO-30 (a surface model), with the threshold applied there.
#[derive(Debug, Serialize, ToSchema)]
pub struct AdaptedRangeV1 {
    pub from_m: f64,
    pub to_m: f64,
    pub dsm: &'static str,
    pub threshold_m: f64,
}

/// Factual provenance and warnings.
#[derive(Debug, Serialize, ToSchema)]
pub struct QualityV1 {
    /// Share of the length with an elevation, in [0, 1].
    pub elevation_coverage: f64,
    /// Terrain provenance by range.
    pub terrain: Vec<TerrainRangeV1>,
    pub warnings: Vec<WarningV1>,
}

/// Terrain read on a range of the route.
#[derive(Debug, Serialize, ToSchema)]
pub struct TerrainRangeV1 {
    pub from_m: f64,
    pub to_m: f64,
    pub origin: TerrainOrigin,
    /// Whether this origin has been measured by a benchmark.
    pub measured: bool,
    /// Fallback after a source failure, as opposed to a normal fallback outside coverage.
    pub fallback_after_error: bool,
}

/// A knowingly degraded or network-dependent output; codes are stable.
#[derive(Debug, Serialize, ToSchema)]
pub struct WarningV1 {
    pub code: WarningCode,
    /// Diagnostic only; clients translate the code.
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_m: Option<f64>,
}

/// Mandatory notice of the configured sources and link to the full list.
#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionRefV1 {
    pub text: String,
    pub url: &'static str,
}

fn origin_key(origin: TerrainOrigin) -> String {
    serde_json::to_value(origin).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// Rapport → DTO v1, filtré par `sections` (T01 §4.2, étape 10).
pub fn to_dto(
    report: &RouteReport,
    sections: SectionSet,
    request_id: &RequestId,
    duration_ms: u64,
    attribution_text: &str,
) -> RouteDetailsResponseV1 {
    let points = sections.elevation.then(|| {
        report
            .points
            .iter()
            .map(|p| OutputPointV1 {
                d_m: p.d_m,
                index: p.index,
                origin: p.origin.map(|o| match o {
                    PointOrigin::Input => PointOriginV1::Input,
                    PointOrigin::Inserted => PointOriginV1::Inserted,
                }),
                lat: p.latlon.map(|l| l.lat),
                lon: p.latlon.map(|l| l.lon),
                ele: p.ele,
                ele_smoothed: p.ele_smoothed,
                ele_method: p.ele_method.into(),
            })
            .collect()
    });
    let s = &report.summary;
    let summary = sections.summary.then(|| SummaryV1 {
        length_m: s.length_m,
        gain_m: s.gain.gain_m,
        loss_m: s.gain.loss_m,
        gain_params: GainParamsV1 {
            preset: s.gain.params.preset,
            threshold_m: s.gain.params.threshold_m,
            smoothing_m: s.gain.params.smoothing_m,
            adapted: s
                .gain
                .params
                .adapted
                .iter()
                .map(|a| AdaptedRangeV1 {
                    from_m: a.from_m,
                    to_m: a.to_m,
                    dsm: "glo30",
                    threshold_m: a.threshold_m,
                })
                .collect(),
            partial: s.gain.params.partial,
        },
        ele_min_m: s.ele_min_m,
        ele_max_m: s.ele_max_m,
        terrain_m: s.terrain_m.iter().map(|(k, v)| (origin_key(*k), *v)).collect(),
    });
    RouteDetailsResponseV1 {
        meta: MetaV1 {
            api_version: report.versions.api_version,
            rules_version: report.versions.rules_version,
            data_versions: report.versions.data_versions.clone(),
            request_id: request_id.0.clone(),
            duration_ms,
        },
        status: match report.status {
            Status::Complete => StatusV1::Complete,
            Status::Partial => StatusV1::Partial,
        },
        points,
        summary,
        quality: QualityV1 {
            elevation_coverage: report.quality.elevation_coverage,
            terrain: report
                .quality
                .terrain
                .iter()
                .map(|r| TerrainRangeV1 {
                    from_m: r.from_m,
                    to_m: r.to_m,
                    origin: r.origin,
                    measured: r.measured,
                    fallback_after_error: r.fallback_after_error,
                })
                .collect(),
            warnings: report
                .warnings
                .iter()
                .map(|w| WarningV1 {
                    code: w.code,
                    message: w.message.clone(),
                    from_m: w.range.as_ref().map(|r| r.start),
                    to_m: w.range.as_ref().map(|r| r.end),
                })
                .collect(),
        },
        attribution: AttributionRefV1 { text: attribution_text.to_owned(), url: "/v1/attribution" },
    }
}
