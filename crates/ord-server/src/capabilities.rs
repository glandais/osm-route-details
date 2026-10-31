//! `/v1/capabilities` et `/v1/attribution` (T07 §4, T02 §9), sérialisés une fois au démarrage.

use ord_core::RULES_VERSION;
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
    /// Valeurs appliquées quand une option est absente.
    pub defaults: OptionDefaults,
    pub option_bounds: OptionBounds,
    pub enums: EnumCatalog,
    pub primary_zone: &'static str,
    pub data: DataFeatures,
    /// Sources d'élévation configurées, dans l'ordre de priorité.
    pub elevation_sources: Vec<DataVersion>,
}

/// Bornes d'entrée (doc 02 §7) ; au-delà, 413.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Limits {
    pub max_points: usize,
    pub max_length_m: f64,
    pub max_body_bytes: usize,
    pub station_step_m: f64,
    /// Taille densifiée maximale qui découle des bornes ; ce n'est pas une borne.
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
    /// `0` désactive le lissage ; sinon dans [min ; max].
    pub smoothing_m: NumberBounds,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct NumberBounds {
    pub min: f64,
    pub max: f64,
    pub default: f64,
}

/// Toutes les valeurs publiques des enums du contrat.
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
        // Les listes en dur seront remplacées par les enums d'ord-core quand ils existeront
        // (classification : incrément 3 ; élévation et points de rupture : incréments 1, 4, 5).
        Self {
            sections: SectionV1::ALL.to_vec(),
            sampling_modes: SamplingModeV1::ALL.to_vec(),
            gain_presets: GainPreset::ALL.to_vec(),
            status: vec!["complete", "partial"],
            structure: vec!["ground", "bridge", "tunnel", "unknown"],
            structure_source: vec!["osm", "heuristic", "osm_heuristic"],
            surface_class: vec!["paved", "unpaved", "unknown"],
            surface_source: vec!["tagged", "inferred_tracktype", "inferred_highway", "none"],
            way_kind: vec![
                "major_road",
                "minor_road",
                "track",
                "path",
                "cycleway",
                "footway",
                "steps",
                "unknown",
            ],
            ele_method: vec!["dem", "dsm_deck", "interpolated_structure", "gap_filled", "none"],
            terrain_origin: TerrainOrigin::ALL.to_vec(),
            point_origin: vec!["input", "inserted"],
            breakpoint_reasons: vec![
                "local_max",
                "local_min",
                "grade_change",
                "structure_start",
                "structure_end",
                "surface_change",
                "terrain_source_change",
                "elevation_gap_start",
                "elevation_gap_end",
            ],
            structure_method: vec!["dsm_deck", "interpolated_structure", "kept_ground", "none"],
            structure_correction_status: vec!["applied", "partial", "none_found", "disabled", "unavailable"],
            warning_codes: WarningCode::ALL.to_vec(),
            problem_codes: ProblemCode::ALL.to_vec(),
        }
    }
}

/// Capacités des données actives.
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
                bridge_dsm: config.elevation.ign.enabled,
                heuristics: false,
            },
            elevation_sources: catalog.elevation_sources(),
        }
    }
}

/// Corps de `/v1/attribution`.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AttributionDoc {
    /// Mention courte à relayer, toutes sources configurées.
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
