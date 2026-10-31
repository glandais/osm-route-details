//! Règles versionnées (T01 §9) : tout paramètre qui change une sortie vit ici, sérialisable, haché
//! par l'empreinte des règles et publié dans `/v1/capabilities`.
//!
//! Incrément 1 : seules les règles d'élévation utiles au MNT seul (pas des stations, presets de D+).
//! `RuleSet` (classification) et `MatchParams` (HMM) arrivent à l'incrément 3.

use ord_domain::request::GainPreset;
use serde::Serialize;

/// Version des règles (`AAAA.MM.n`, doc 02 §6), montée à la main à chaque changement de règle.
pub const RULES_VERSION: &str = "2026.10.1";

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Rules {
    pub elevation: ElevationRules,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ElevationRules {
    /// Pas des stations internes, en mètres (T04 §2.1, `stepM` de vcyclist).
    pub station_step_m: f64,
    pub gain: GainRules,
}

impl Default for ElevationRules {
    fn default() -> Self {
        Self { station_step_m: 5.0, gain: GainRules::default() }
    }
}

/// Seuil d'hystérésis et demi-largeur du lissage privé de chaque preset (T04 §5.4,
/// `ElevationGainOptions.kt` de vcyclist).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PresetParams {
    pub threshold_m: f64,
    pub smoothing_m: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GainRules {
    pub raw: PresetParams,
    pub dem: PresetParams,
    pub barometric: PresetParams,
    pub gps: PresetParams,
}

impl Default for GainRules {
    fn default() -> Self {
        Self {
            raw: PresetParams { threshold_m: 0.0, smoothing_m: 0.0 },
            dem: PresetParams { threshold_m: 3.0, smoothing_m: 30.0 },
            barometric: PresetParams { threshold_m: 2.0, smoothing_m: 15.0 },
            gps: PresetParams { threshold_m: 10.0, smoothing_m: 50.0 },
        }
    }
}

impl GainRules {
    pub fn preset(&self, preset: GainPreset) -> PresetParams {
        match preset {
            GainPreset::Raw => self.raw,
            GainPreset::Dem => self.dem,
            GainPreset::Barometric => self.barometric,
            GainPreset::Gps => self.gps,
        }
    }
}
