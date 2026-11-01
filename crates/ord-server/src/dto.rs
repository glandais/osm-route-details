//! DTO v1 (doc 02 §3–§4) : entrée, et réponse 200 (élévation, segments typés, résumé, qualité,
//! provenance du terrain). Chaque incrément ajoute des champs au contrat, sans rupture.
//!
//! Les `///` de ce module sont recopiés dans le contrat OpenAPI : ils sont en anglais ; les notes
//! en français sont en `//`.

use std::collections::BTreeMap;
use std::fmt;

use ord_core::assemble::{ReportSegment, RouteReport, Status};
use ord_core::classification::{StructureClass, StructureSource, SurfaceClass, SurfaceSource, WayKind};
use ord_core::elevation::fusion::{CorrectionStatus, OutcomeMethod};
use ord_core::elevation::{BreakpointReason, EleMethod, PointOrigin};
use ord_core::typology::StructureKind;
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
    /// Contiguous homogeneous stretches covering the whole route (section `typology`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<SegmentV1>>,
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

// Raisons d'insertion (doc 02 §4.2) : liste ouverte, un client ignore une raison inconnue.
/// Why a point was inserted (open list: clients ignore unknown values).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BreakpointReasonV1 {
    LocalMax,
    LocalMin,
    GradeChange,
    StructureStart,
    StructureEnd,
    SurfaceChange,
    TerrainSourceChange,
    ElevationGapStart,
    ElevationGapEnd,
}

impl From<BreakpointReason> for BreakpointReasonV1 {
    fn from(r: BreakpointReason) -> Self {
        match r {
            BreakpointReason::LocalMax => Self::LocalMax,
            BreakpointReason::LocalMin => Self::LocalMin,
            BreakpointReason::GradeChange => Self::GradeChange,
            BreakpointReason::StructureStart => Self::StructureStart,
            BreakpointReason::StructureEnd => Self::StructureEnd,
            BreakpointReason::SurfaceChange => Self::SurfaceChange,
            BreakpointReason::TerrainSourceChange => Self::TerrainSourceChange,
            BreakpointReason::ElevationGapStart => Self::ElevationGapStart,
            BreakpointReason::ElevationGapEnd => Self::ElevationGapEnd,
        }
    }
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
    /// Insertion reasons (`input_with_breakpoints` mode): always on an inserted point, on an input
    /// point only when a breakpoint falls on it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<BreakpointReasonV1>,
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
    /// Length per `structure` value, in metres; zero values are omitted.
    pub by_structure_m: BTreeMap<String, f64>,
    /// Length per `surface.class` value, in metres; zero values are omitted.
    pub by_surface_class_m: BTreeMap<String, f64>,
    /// Length per `surface.source` value, in metres; zero values are omitted.
    pub by_surface_source_m: BTreeMap<String, f64>,
    /// Length per `way_kind` value, in metres; zero values are omitted.
    pub by_way_kind_m: BTreeMap<String, f64>,
    /// Length not matched on the street network, in metres.
    pub unknown_m: f64,
    /// Bridges and tunnels met on the bicycle network, sorted by distance: one entry per run of
    /// elements of the same kind on each passage. Structures of a way closed to bicycles are
    /// not listed (segments only, warning `bicycle_forbidden`).
    pub structures: Vec<StructureEntryV1>,
}

/// Kind of a structure entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StructureKindV1 {
    Bridge,
    Tunnel,
}

/// Method applied to a structure: `kept_ground` is a bridge with no deck above the ground (counted
/// as corrected), `none` an uncorrected structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StructureMethodV1 {
    DsmDeck,
    InterpolatedStructure,
    KeptGround,
    None,
}

/// A bridge or tunnel met on the route.
#[derive(Debug, Serialize, ToSchema)]
pub struct StructureEntryV1 {
    pub kind: StructureKindV1,
    pub from_m: f64,
    pub to_m: f64,
    pub length_m: f64,
    pub structure_source: StructureSourceV1,
    pub method: StructureMethodV1,
}

/// Correction status of one kind of structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionStatusV1 {
    Applied,
    Partial,
    NoneFound,
    Disabled,
    Unavailable,
}

/// Structure correction: a status per kind, and counters of `summary.structures[]` entries.
#[derive(Debug, Serialize, ToSchema)]
pub struct StructureCorrectionV1 {
    pub bridges: CorrectionStatusV1,
    pub tunnels: CorrectionStatusV1,
    /// Entries per method (`dsm_deck`, `interpolated_structure`, `kept_ground`, `uncorrected`);
    /// zero counters are omitted.
    pub counts: BTreeMap<String, u32>,
    /// Entries per `structure_source`; zero counters are omitted.
    pub counts_by_source: BTreeMap<String, u32>,
}

/// Structure of a segment; `ford` is reserved, `unknown` is an unmatched stretch with no known
/// way. On a way closed to bicycles (warning `bicycle_forbidden`), the structure of that way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StructureV1 {
    Ground,
    Bridge,
    Tunnel,
    Ford,
    Unknown,
}

/// Origin of a structure: OSM tags, heuristic detection, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StructureSourceV1 {
    Osm,
    Heuristic,
    OsmHeuristic,
}

/// Surface class, frozen in v1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceClassV1 {
    Paved,
    Unpaved,
    Unknown,
}

/// How the surface class was obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceSourceV1 {
    Tagged,
    InferredTracktype,
    InferredHighway,
    None,
}

/// Kind of way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WayKindV1 {
    MajorRoad,
    MinorRoad,
    Track,
    Path,
    Cycleway,
    Footway,
    Steps,
    Unknown,
}

impl From<StructureClass> for StructureV1 {
    fn from(v: StructureClass) -> Self {
        match v {
            StructureClass::Ground => Self::Ground,
            StructureClass::Bridge => Self::Bridge,
            StructureClass::Tunnel => Self::Tunnel,
            StructureClass::Ford => Self::Ford,
            StructureClass::Unknown => Self::Unknown,
        }
    }
}

impl From<StructureSource> for StructureSourceV1 {
    fn from(v: StructureSource) -> Self {
        match v {
            StructureSource::Osm => Self::Osm,
            StructureSource::Heuristic => Self::Heuristic,
            StructureSource::OsmHeuristic => Self::OsmHeuristic,
        }
    }
}

impl From<SurfaceClass> for SurfaceClassV1 {
    fn from(v: SurfaceClass) -> Self {
        match v {
            SurfaceClass::Paved => Self::Paved,
            SurfaceClass::Unpaved => Self::Unpaved,
            SurfaceClass::Unknown => Self::Unknown,
        }
    }
}

impl From<SurfaceSource> for SurfaceSourceV1 {
    fn from(v: SurfaceSource) -> Self {
        match v {
            SurfaceSource::Tagged => Self::Tagged,
            SurfaceSource::InferredTracktype => Self::InferredTracktype,
            SurfaceSource::InferredHighway => Self::InferredHighway,
            SurfaceSource::None => Self::None,
        }
    }
}

impl From<WayKind> for WayKindV1 {
    fn from(v: WayKind) -> Self {
        match v {
            WayKind::MajorRoad => Self::MajorRoad,
            WayKind::MinorRoad => Self::MinorRoad,
            WayKind::Track => Self::Track,
            WayKind::Path => Self::Path,
            WayKind::Cycleway => Self::Cycleway,
            WayKind::Footway => Self::Footway,
            WayKind::Steps => Self::Steps,
            WayKind::Unknown => Self::Unknown,
        }
    }
}

/// Surface of a segment.
#[derive(Debug, Serialize, ToSchema)]
pub struct SurfaceV1 {
    pub class: SurfaceClassV1,
    pub source: SurfaceSourceV1,
}

/// A contiguous homogeneous stretch of the route.
#[derive(Debug, Serialize, ToSchema)]
pub struct SegmentV1 {
    pub from_m: f64,
    pub to_m: f64,
    /// Client index of the last input point at or before `from_m`.
    pub from_index: u32,
    /// Client index of the first input point at or after `to_m`.
    pub to_index: u32,
    pub structure: StructureV1,
    /// Only on a bridge or a tunnel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structure_source: Option<StructureSourceV1>,
    pub surface: SurfaceV1,
    pub way_kind: WayKindV1,
    /// `false` on a stretch not matched on the bicycle street network. Since api 1.1, a stretch
    /// that follows a way closed to bicycles (motorway, trunk, `bicycle=no` road...) stays
    /// `false` but carries the `structure`, `surface` and `way_kind` of that way, with the
    /// warnings `unmatched_range` and `bicycle_forbidden`; its structures are not corrected.
    pub matched: bool,
    /// Dominant elevation method on the stretch.
    pub ele_method: EleMethodV1,
    /// Elevation at `from_m`; `null` when missing.
    #[schema(required = true)]
    pub ele_start: Option<f64>,
    /// Elevation at `to_m`; `null` when missing.
    #[schema(required = true)]
    pub ele_end: Option<f64>,
    /// Mean grade, in percent; `null` when an end has no elevation.
    #[schema(required = true)]
    pub grade_pct: Option<f64>,
    /// Raw source attributes, only with `debug`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub osm: Option<BTreeMap<String, String>>,
}

// Attributs neutres de la source, pour `debug` (jamais de coordonnée).
fn osm_attributes(a: &ord_domain::street::WayAttrs) -> BTreeMap<String, String> {
    use ord_domain::street::{PavedGroup, StructureTag, SurfaceTag};
    let mut m = BTreeMap::new();
    m.insert("kind".to_owned(), a.highway.to_string());
    match &a.surface {
        SurfaceTag::Grouped(PavedGroup::Paved) => {
            m.insert("surface".into(), "paved".into());
        }
        SurfaceTag::Grouped(PavedGroup::Unpaved) => {
            m.insert("surface".into(), "unpaved".into());
        }
        SurfaceTag::Raw(v) => {
            m.insert("surface".into(), v.to_string());
        }
        SurfaceTag::Absent => {}
    }
    if let Some(t) = a.tracktype {
        m.insert("tracktype".into(), format!("grade{}", t as u8 + 1));
    }
    for (k, v) in [("bicycle", &a.bicycle), ("service", &a.service)] {
        if let Some(v) = v {
            m.insert(k.into(), v.to_string());
        }
    }
    let structure = match a.structure {
        StructureTag::None => None,
        StructureTag::Bridge => Some("bridge"),
        StructureTag::Tunnel => Some("tunnel"),
        StructureTag::Covered => Some("covered"),
        StructureTag::BuildingPassage => Some("building_passage"),
        StructureTag::Ford => Some("ford"),
    };
    if let Some(s) = structure {
        m.insert(s.into(), "true".into());
    }
    if a.link {
        m.insert("link".into(), "true".into());
    }
    m
}

fn segment_dto(s: &ReportSegment) -> SegmentV1 {
    let t = &s.typed;
    SegmentV1 {
        from_m: t.from_m,
        to_m: t.to_m,
        from_index: t.from_index,
        to_index: t.to_index,
        structure: t.structure.into(),
        structure_source: t.structure_source.map(Into::into),
        surface: SurfaceV1 { class: t.surface_class.into(), source: t.surface_source.into() },
        way_kind: t.way_kind.into(),
        matched: t.matched,
        ele_method: s.ele_method.into(),
        ele_start: s.ele_start,
        ele_end: s.ele_end,
        grade_pct: s.grade_pct,
        osm: s.osm.as_ref().map(osm_attributes),
    }
}

fn keyed<K: Copy>(m: &BTreeMap<K, f64>, name: impl Fn(K) -> &'static str) -> BTreeMap<String, f64> {
    m.iter().map(|(k, v)| (name(*k).to_owned(), *v)).collect()
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
    /// Share of the length matched on the street network, in [0, 1].
    pub matched_ratio: f64,
    /// Share of the length whose surface is inferred (tracktype or way kind), in [0, 1].
    pub inferred_surface_ratio: f64,
    /// Share of the length with an elevation, in [0, 1].
    pub elevation_coverage: f64,
    pub structure_correction: StructureCorrectionV1,
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
                reasons: p.reasons.iter().map(|&r| r.into()).collect(),
                lat: p.latlon.map(|l| l.lat),
                lon: p.latlon.map(|l| l.lon),
                ele: p.ele,
                ele_smoothed: p.ele_smoothed,
                ele_method: p.ele_method.into(),
            })
            .collect()
    });
    let segments = sections.typology.then(|| report.segments.iter().map(segment_dto).collect());
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
        by_structure_m: keyed(&s.by_structure_m, StructureClass::as_str),
        by_surface_class_m: keyed(&s.by_surface_class_m, SurfaceClass::as_str),
        by_surface_source_m: keyed(&s.by_surface_source_m, SurfaceSource::as_str),
        by_way_kind_m: keyed(&s.by_way_kind_m, WayKind::as_str),
        unknown_m: s.unknown_m,
        structures: s
            .structures
            .iter()
            .map(|o| StructureEntryV1 {
                kind: match o.kind {
                    StructureKind::Bridge => StructureKindV1::Bridge,
                    StructureKind::Tunnel => StructureKindV1::Tunnel,
                },
                from_m: o.from_m,
                to_m: o.to_m,
                length_m: ((o.to_m - o.from_m) * 100.0).round() / 100.0,
                structure_source: o.source.into(),
                method: match o.method {
                    OutcomeMethod::DsmDeck => StructureMethodV1::DsmDeck,
                    OutcomeMethod::InterpolatedStructure => StructureMethodV1::InterpolatedStructure,
                    OutcomeMethod::KeptGround => StructureMethodV1::KeptGround,
                    OutcomeMethod::Uncorrected(_) => StructureMethodV1::None,
                },
            })
            .collect(),
    });
    let status = |c: CorrectionStatus| match c {
        CorrectionStatus::Applied => CorrectionStatusV1::Applied,
        CorrectionStatus::Partial => CorrectionStatusV1::Partial,
        CorrectionStatus::NoneFound => CorrectionStatusV1::NoneFound,
        CorrectionStatus::Disabled => CorrectionStatusV1::Disabled,
        CorrectionStatus::Unavailable => CorrectionStatusV1::Unavailable,
    };
    let sc = &report.quality.structure_correction;
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
        segments,
        summary,
        quality: QualityV1 {
            matched_ratio: report.quality.matched_ratio,
            inferred_surface_ratio: report.quality.inferred_surface_ratio,
            elevation_coverage: report.quality.elevation_coverage,
            structure_correction: StructureCorrectionV1 {
                bridges: status(sc.bridges),
                tunnels: status(sc.tunnels),
                counts: sc.counts.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect(),
                counts_by_source: sc.by_source.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect(),
            },
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
