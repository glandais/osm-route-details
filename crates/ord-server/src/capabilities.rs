//! `/v1/capabilities` et `/v1/attribution` (T07 §4, T02 §9), sérialisés une fois au démarrage.
//!
//! Les `///` des types sérialisés sont recopiés dans le contrat OpenAPI : ils sont en anglais.

use ord_core::classification::{StructureClass, StructureSource, SurfaceClass, SurfaceSource, WayKind};
use ord_core::elevation::BreakpointReason;
use ord_core::{Rules, RULES_VERSION};
use ord_data::catalog::StaticCatalog;
use ord_domain::catalog::{Attribution, DataCatalog, DataVersion};
use ord_domain::elevation::TerrainOrigin;
use ord_domain::report::WarningCode;
use ord_domain::request::GainPreset;
use serde::Serialize;
use utoipa::ToSchema;

use crate::config::Config;
use crate::dto::{SamplingModeV1, SectionV1};
use crate::problem::ProblemCode;
use crate::API_VERSION;

/// Pas des stations internes (règle `ElevationRules::station_step_m`, T01 §5.1.1).
pub const STATION_STEP_M: f64 = 5.0;

/// Zone de qualité maximale (contour : T02 §7.2).
pub const PRIMARY_ZONE: &str = "FR-metropole+corse";

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Capabilities {
    pub api_version: &'static str,
    pub rules_version: &'static str,
    pub limits: Limits,
    /// Values applied when an option is absent.
    pub defaults: OptionDefaults,
    pub option_bounds: OptionBounds,
    pub enums: EnumCatalog,
    pub primary_zone: &'static str,
    pub data: DataFeatures,
    /// Configured elevation sources, in priority order.
    pub elevation_sources: Vec<DataVersion>,
    /// Elevation rules that shape the output.
    pub elevation_rules: ElevationRulesV1,
    /// Typology rules that shape the output.
    pub typology_rules: TypologyRulesV1,
}

// Paramètres effectifs du HMM et des segments (T03 §5.2, §8) : un ajout au contrat.
/// Map matching and segmentation rules of `rules_version`.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TypologyRulesV1 {
    /// Street tile schema read.
    pub schema: &'static str,
    /// Zoom of the street tiles.
    pub zoom: u8,
    /// Candidate search radius, in metres.
    pub search_radius_m: f64,
    pub max_candidates: usize,
    /// Standard deviation of the distance between an observation and its street, in metres.
    pub sigma_z_m: f64,
    /// Scale of the transition cost, in metres.
    pub beta_m: f64,
    /// Spacing of the observations along the route, in metres.
    pub step_m: f64,
    /// Cost of a straight jump between two streets the network does not join.
    pub jump_cost: f64,
    /// Longest straight jump, in metres.
    pub max_jump_m: f64,
    /// Extra emission cost of a footway, steps or pedestrian street.
    pub footway_cost: f64,
    /// A matched chain shorter than this is dropped as unmatched, in metres.
    pub min_chain_m: f64,
    /// A matched ground stretch shorter than this is merged into its longest neighbour, in metres.
    pub min_segment_m: f64,
}

impl TypologyRulesV1 {
    fn new(rules: &Rules, zoom: u8) -> Self {
        let t = &rules.typology;
        let m = &t.matcher;
        Self {
            schema: ord_adapters::Shortbread::ID,
            zoom,
            search_radius_m: m.search_radius_m,
            max_candidates: m.max_candidates,
            sigma_z_m: m.sigma_z,
            beta_m: m.beta,
            step_m: m.step_m,
            jump_cost: m.jump_cost,
            max_jump_m: m.max_jump_m,
            footway_cost: m.footway_cost,
            min_chain_m: m.min_chain_m,
            min_segment_m: t.min_segment_m,
        }
    }
}

// Règles publiées (T04 §5.4, T07 §4) : un ajout au contrat.
/// Elevation rules of `rules_version`.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ElevationRulesV1 {
    /// Spacing of the internal stations, in metres.
    pub station_step_m: f64,
    /// Hysteresis threshold and smoothing half-width of each gain preset, in metres.
    pub gain_presets: Vec<GainPresetRuleV1>,
    /// Threshold applied on GLO-30 ranges (a surface model), in metres.
    pub glo30_threshold_m: f64,
    /// GLO-30 ranges shorter than this are not adapted, and shorter gaps are closed, in metres.
    pub glo30_min_range_m: f64,
    /// Breakpoint selection of the `input_with_breakpoints` mode.
    pub breakpoints: BreakpointRulesV1,
}

// Seuils de sélection des points de rupture (T04 §6.2, doc 02 §2) : un ajout au contrat.
/// Breakpoint selection thresholds; distances in metres.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct BreakpointRulesV1 {
    /// Candidate breakpoints closer than this are merged into one point.
    pub merge_m: f64,
    /// A breakpoint this close to an input point is carried by it, without a new point.
    pub snap_m: f64,
    /// The same for a bridge crossing above the route (`overpass_crossing`, api 1.4).
    pub overpass_snap_m: f64,
    /// A summit, low point or grade change closer than this to a kept point is dropped.
    pub min_spacing_m: f64,
    /// Inserted points over any sliding kilometre, at most; structure, gap, terrain, overpass and
    /// surface breakpoints are never dropped.
    pub max_inserted_per_km: usize,
    /// Inserted points over the whole route, at most.
    pub max_inserted_total: usize,
    /// Vertical tolerance of the grade change search.
    pub grade_tolerance_m: f64,
    /// Both stretches around a grade change are at least this long.
    pub grade_min_run_m: f64,
    /// Grades around a grade change differ by at least this, in percentage points.
    pub grade_min_delta_pct: f64,
}

/// Parameters of one gain preset.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct GainPresetRuleV1 {
    pub preset: GainPreset,
    pub threshold_m: f64,
    pub smoothing_m: f64,
}

impl ElevationRulesV1 {
    fn new(rules: &Rules) -> Self {
        let g = &rules.elevation.gain;
        Self {
            station_step_m: rules.elevation.station_step_m,
            gain_presets: GainPreset::ALL
                .iter()
                .map(|&preset| {
                    let p = g.preset(preset);
                    GainPresetRuleV1 { preset, threshold_m: p.threshold_m, smoothing_m: p.smoothing_m }
                })
                .collect(),
            glo30_threshold_m: g.glo30_threshold_m,
            glo30_min_range_m: g.glo30_min_range_m,
            breakpoints: {
                let b = &rules.elevation.breakpoints;
                BreakpointRulesV1 {
                    merge_m: b.merge_m,
                    snap_m: b.snap_m,
                    overpass_snap_m: b.overpass_snap_m,
                    min_spacing_m: b.min_spacing_m,
                    max_inserted_per_km: b.max_inserted_per_km,
                    max_inserted_total: b.max_inserted_total,
                    grade_tolerance_m: b.grade_tolerance_m,
                    grade_min_run_m: b.grade_min_run_m,
                    grade_min_delta_pct: b.grade_min_delta_pct,
                }
            },
        }
    }
}

// Doc 02 §7.
/// Input bounds; beyond them, the request gets a 413.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Limits {
    pub max_points: usize,
    pub max_length_m: f64,
    pub max_body_bytes: usize,
    pub station_step_m: f64,
    /// Maximum number of internal stations implied by the bounds; not a bound itself.
    pub densified_max: usize,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OptionDefaults {
    pub sections: Vec<SectionV1>,
    pub sampling: SamplingDefaults,
    pub elevation: ElevationDefaults,
    pub structure_correction: bool,
    pub accept_partial: bool,
    pub debug: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct SamplingDefaults {
    pub mode: SamplingModeV1,
    pub step_m: f64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ElevationDefaults {
    pub smoothing_m: f64,
    pub gain_preset: GainPreset,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OptionBounds {
    pub step_m: NumberBounds,
    /// `0` disables smoothing; otherwise within [min, max].
    pub smoothing_m: NumberBounds,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct NumberBounds {
    pub min: f64,
    pub max: f64,
    pub default: f64,
}

/// Every public value of the contract enums.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct EnumCatalog {
    pub sections: Vec<SectionV1>,
    pub sampling_modes: Vec<SamplingModeV1>,
    pub gain_presets: Vec<GainPreset>,
    pub status: Vec<&'static str>,
    pub structure: Vec<&'static str>,
    pub structure_source: Vec<&'static str>,
    pub surface_class: Vec<&'static str>,
    pub surface_source: Vec<&'static str>,
    pub way_kind: Vec<&'static str>,
    pub ele_method: Vec<&'static str>,
    pub terrain_origin: Vec<TerrainOrigin>,
    pub point_origin: Vec<&'static str>,
    pub breakpoint_reasons: Vec<&'static str>,
    pub structure_method: Vec<&'static str>,
    pub structure_correction_status: Vec<&'static str>,
    pub warning_codes: Vec<WarningCode>,
    pub problem_codes: Vec<ProblemCode>,
}

impl EnumCatalog {
    fn new() -> Self {
        // Classification : enums d'ord-core ; élévation et points de rupture : listes en dur.
        Self {
            sections: SectionV1::ALL.to_vec(),
            sampling_modes: SamplingModeV1::ALL.to_vec(),
            gain_presets: GainPreset::ALL.to_vec(),
            status: vec!["complete", "partial"],
            // `ford` est réservé (V2) : pas encore publié.
            structure: StructureClass::ALL
                .iter()
                .filter(|s| **s != StructureClass::Ford)
                .map(|s| s.as_str())
                .collect(),
            structure_source: StructureSource::ALL.iter().map(|s| s.as_str()).collect(),
            surface_class: SurfaceClass::ALL.iter().map(|s| s.as_str()).collect(),
            surface_source: SurfaceSource::ALL.iter().map(|s| s.as_str()).collect(),
            way_kind: WayKind::ALL.iter().map(|s| s.as_str()).collect(),
            ele_method: vec!["dem", "dsm_deck", "interpolated_structure", "gap_filled", "none"],
            terrain_origin: TerrainOrigin::ALL.to_vec(),
            point_origin: vec!["input", "inserted"],
            breakpoint_reasons: BreakpointReason::ALL.iter().map(|r| r.as_str()).collect(),
            structure_method: vec!["dsm_deck", "interpolated_structure", "kept_ground", "none"],
            structure_correction_status: vec!["applied", "partial", "none_found", "disabled", "unavailable"],
            warning_codes: WarningCode::ALL.to_vec(),
            problem_codes: ProblemCode::ALL.to_vec(),
        }
    }
}

/// Capabilities of the active data sources.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DataFeatures {
    pub ford_detectable: bool,
    pub surface_detail: bool,
    pub bridge_dsm: bool,
    pub heuristics: bool,
}

impl Capabilities {
    pub fn build(config: &Config, catalog: &StaticCatalog) -> Self {
        let l = &config.limits;
        let o = &l.options;
        Self {
            api_version: API_VERSION,
            rules_version: RULES_VERSION,
            limits: Limits {
                max_points: l.max_points,
                max_length_m: l.max_length_m,
                max_body_bytes: l.max_body_bytes,
                station_step_m: STATION_STEP_M,
                densified_max: l.max_points + (l.max_length_m / STATION_STEP_M).ceil() as usize,
            },
            defaults: OptionDefaults {
                sections: SectionV1::ALL.to_vec(),
                sampling: SamplingDefaults {
                    mode: SamplingModeV1::InputWithBreakpoints,
                    step_m: o.step_m_default,
                },
                elevation: ElevationDefaults {
                    smoothing_m: o.smoothing_m_default,
                    gain_preset: GainPreset::Dem,
                },
                structure_correction: true,
                accept_partial: false,
                debug: false,
            },
            option_bounds: OptionBounds {
                step_m: NumberBounds { min: o.step_m_min, max: o.step_m_max, default: o.step_m_default },
                smoothing_m: NumberBounds {
                    min: o.smoothing_m_min,
                    max: o.smoothing_m_max,
                    default: o.smoothing_m_default,
                },
            },
            enums: EnumCatalog::new(),
            primary_zone: PRIMARY_ZONE,
            data: DataFeatures {
                // Shortbread (VersaTiles) : revêtement groupé, pas de gué (T03 §3).
                ford_detectable: false,
                surface_detail: false,
                // T07 §4 : MNS des ponts = IGN actif ; heuristiques = `ElevationRules::heuristics`
                // (V1, MNS sur l'enveloppe LiDAR, T04 §2.3), qui demandent elles aussi l'IGN.
                bridge_dsm: config.elevation.ign.enabled,
                heuristics: Rules::default().elevation.heuristics && config.elevation.ign.enabled,
            },
            elevation_sources: catalog.elevation_sources(),
            elevation_rules: ElevationRulesV1::new(&Rules::default()),
            typology_rules: TypologyRulesV1::new(&Rules::default(), config.typology.zoom),
        }
    }
}

/// Body of `/v1/attribution`.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AttributionDoc {
    /// Short notice to display, covering every configured source.
    pub text: String,
    pub sources: Vec<Attribution>,
}

impl AttributionDoc {
    pub fn build(catalog: &dyn DataCatalog) -> Self {
        let sources = catalog.attributions();
        let mut shorts: Vec<&str> = Vec::new();
        for a in &sources {
            if !shorts.contains(&a.short.as_str()) {
                shorts.push(&a.short);
            }
        }
        Self { text: shorts.join(" · "), sources }
    }
}
