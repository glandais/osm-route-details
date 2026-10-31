//! Règles versionnées (T01 §9) : tout paramètre qui change une sortie vit ici, sérialisable, haché
//! par l'empreinte des règles et publié dans `/v1/capabilities`.
//!
//! Incrément 2 : règles d'élévation du MNT seul (pas des stations, presets de D+, adaptation du
//! seuil sur GLO-30) et du référentiel de couverture (couture). `RuleSet` (classification) et
//! `MatchParams` (HMM) arrivent à l'incrément 3.

use ord_domain::request::GainPreset;
use serde::Serialize;

/// Version des règles (`AAAA.MM.n`, doc 02 §6), montée à la main à chaque changement de règle.
// 2026.10.2 : IGN LiDAR HD et repli, adaptation du seuil de D+ sur GLO-30, couture de 150 m.
pub const RULES_VERSION: &str = "2026.10.2";

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Rules {
    pub elevation: ElevationRules,
    pub coverage: CoverageRules,
}

/// Règles du référentiel de couverture (T02 §7.3).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CoverageRules {
    /// Couture : point de MNT national à moins de cette distance d'une plage GLO-30 [mesuré :
    /// bande de mélange de Mapterhorn, `MACROTILE_BUFFER_3857`, exploration 05 §1.3].
    pub seam_m: f64,
}

impl Default for CoverageRules {
    fn default() -> Self {
        Self { seam_m: 150.0 }
    }
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
    /// Seuil sur GLO-30, un MNS (BRouter ≈ 10 m sur SRTM, exploration 04 §5) [à calibrer].
    pub glo30_threshold_m: f64,
    /// Plages GLO-30 : trous plus courts fermés, plages plus courtes écartées [à calibrer].
    pub glo30_min_range_m: f64,
}

impl Default for GainRules {
    fn default() -> Self {
        Self {
            raw: PresetParams { threshold_m: 0.0, smoothing_m: 0.0 },
            dem: PresetParams { threshold_m: 3.0, smoothing_m: 30.0 },
            barometric: PresetParams { threshold_m: 2.0, smoothing_m: 15.0 },
            gps: PresetParams { threshold_m: 10.0, smoothing_m: 50.0 },
            glo30_threshold_m: 10.0,
            glo30_min_range_m: 200.0,
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
