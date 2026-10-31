//! Règles versionnées (T01 §9) : tout paramètre qui change une sortie vit ici, sérialisable, haché
//! par l'empreinte des règles et publié dans `/v1/capabilities`.
//!
//! Incrément 2 : règles d'élévation du MNT seul (pas des stations, presets de D+, adaptation du
//! seuil sur GLO-30) et du référentiel de couverture (couture). Incrément 3 : typologie
//! (paramètres du HMM, couloir, chaînes et segments) ; les tables de la classification sont des
//! constantes (`classification::tables`), couvertes par la même empreinte. Incrément 4 : ouvrages
//! (extension, doutes), correction des ouvrages, constructeur de tablier, comblement.

use ord_domain::request::GainPreset;
use serde::Serialize;

/// Version des règles (`AAAA.MM.n`, doc 02 §6), montée à la main à chaque changement de règle.
// 2026.10.2 : IGN LiDAR HD et repli, adaptation du seuil de D+ sur GLO-30, couture de 150 m.
// 2026.10.3 : typologie (classification v1, HMM tiles-only, segments homogènes).
// 2026.10.4 : ouvrages OSM (graphe des ouvrages, extension, tablier MNS, interpolation, tunnels,
// comblement des trous courts).
pub const RULES_VERSION: &str = "2026.10.4";

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Rules {
    pub elevation: ElevationRules,
    pub coverage: CoverageRules,
    pub typology: TypologyRules,
}

/// Paramètres du map-matching HMM (T03 §5.2), repris d'osm-surface (`src/matcher.rs`) sauf
/// `min_chain_m` [à calibrer].
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MatchParams {
    /// Rayon de recherche des candidats.
    pub search_radius_m: f64,
    pub max_candidates: usize,
    /// Écart type de la distance observation–rue (émission `0,5·(d/σ)²`).
    pub sigma_z: f64,
    /// Échelle de la transition `|d_réseau − d_obs| / β`.
    pub beta: f64,
    /// Pas des observations le long du parcours.
    pub step_m: f64,
    /// Coût d'un saut en ligne droite entre deux rues que le réseau ne relie pas.
    pub jump_cost: f64,
    pub max_jump_m: f64,
    /// Malus d'émission d'un trottoir, d'un escalier ou d'une rue piétonne.
    pub footway_cost: f64,
    /// Chaîne plus courte rejetée en non appariée.
    pub min_chain_m: f64,
}

impl Default for MatchParams {
    fn default() -> Self {
        Self {
            search_radius_m: 40.0,
            max_candidates: 8,
            sigma_z: 5.0,
            beta: 3.0,
            step_m: 10.0,
            jump_cost: 12.0,
            max_jump_m: 60.0,
            footway_cost: 0.75,
            min_chain_m: 50.0,
        }
    }
}

/// Règles de la typologie (T03 §4–§8).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TypologyRules {
    pub matcher: MatchParams,
    /// Marge du couloir au-delà du rayon de recherche (`search_radius_m + 60 m`, osm-surface).
    pub corridor_margin_m: f64,
    /// Tronçon au sol apparié plus court absorbé par son voisin le plus long (`MIN_SPAN_M` de tribly).
    pub min_segment_m: f64,
    pub structures: StructureRules,
}

impl Default for TypologyRules {
    fn default() -> Self {
        Self {
            matcher: MatchParams::default(),
            corridor_margin_m: 60.0,
            min_segment_m: 20.0,
            structures: StructureRules::default(),
        }
    }
}

/// Ouvrages et extrémités (T03 §6.2–§6.3).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StructureRules {
    /// Longueur maximale d'un ouvrage suivi dans le graphe des ouvrages (Fréjus routier 12,9 km)
    /// [à calibrer].
    pub max_structure_extent_m: f64,
    /// Tours de lecture de tuiles voisines au plus [à calibrer].
    pub max_extension_rounds: u32,
    /// Tuiles voisines lues au plus, tous tours confondus [à calibrer].
    pub max_extension_tiles: usize,
    /// Voie au sol suivie au-delà d'une culée (marge de vcyclist autour d'un portail).
    pub approach_m: f64,
    /// Une continuation d'ouvrage ou de voie d'approche plus coudée n'est pas suivie [à calibrer].
    pub max_turn_deg: f64,
    /// Portion non appariée à moins de cette distance d'un passage : doute [à calibrer].
    pub doubt_guard_m: f64,
    /// Distance moyenne observation–axe au-delà de laquelle le passage est douteux (3·σ).
    pub far_from_axis_m: f64,
    /// Saut plus long à la culée : doute [à calibrer].
    pub max_end_jump_m: f64,
    /// Un passage atteint une extrémité à moins de cette distance d'axe.
    pub end_reached_m: f64,
}

impl Default for StructureRules {
    fn default() -> Self {
        Self {
            max_structure_extent_m: 20_000.0,
            max_extension_rounds: 4,
            max_extension_tiles: 32,
            approach_m: 100.0,
            max_turn_deg: 60.0,
            doubt_guard_m: 20.0,
            far_from_axis_m: 15.0,
            max_end_jump_m: 20.0,
            end_reached_m: 15.0,
        }
    }
}

impl TypologyRules {
    /// Rayon du couloir de tuiles et de cellules.
    pub fn corridor_radius_m(&self) -> f64 {
        self.matcher.search_radius_m + self.corridor_margin_m
    }
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
    pub structures: CorrectionRules,
    pub deck: DeckBuilderRules,
    /// Suite de stations sans altitude comblée linéairement jusqu'à cette longueur (T04 §5.1)
    /// [à calibrer].
    pub gap_fill_max_m: f64,
}

impl Default for ElevationRules {
    fn default() -> Self {
        Self {
            station_step_m: 5.0,
            gain: GainRules::default(),
            structures: CorrectionRules::default(),
            deck: DeckBuilderRules::default(),
            gap_fill_max_m: 100.0,
        }
    }
}

/// Correction des ouvrages OSM (T04 §3.1, §3.3).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CorrectionRules {
    /// Sonde de culée et de portail au-delà de chaque extrémité : `max(min, pixels × pixel lu)`
    /// [à calibrer] (10 m sur IGN z15, ≈ 30 m sur Mapterhorn z12 ; `refreshM` de vcyclist).
    pub abutment_probe_min_m: f64,
    pub abutment_probe_pixels: f64,
    /// Pente au-delà de laquelle la droite d'un tunnel est signalée (`maxChordGrade` de vcyclist).
    pub max_tunnel_chord_grade: f64,
    /// Pente au-delà de laquelle la droite d'un pont est signalée (`maxSlopeP95` de vcyclist).
    pub max_bridge_chord_grade: f64,
}

impl Default for CorrectionRules {
    fn default() -> Self {
        Self {
            abutment_probe_min_m: 10.0,
            abutment_probe_pixels: 2.0,
            max_tunnel_chord_grade: 0.12,
            max_bridge_chord_grade: 0.10,
        }
    }
}

/// Constructeur de tablier (T04 §3.2), portage de `DeckProfileBuilder` de vcyclist ; valeurs de
/// `DeckDetectorOptions.DEFAULT` sauf mention.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DeckBuilderRules {
    /// MNH au-dessus duquel une station a un sursol (`deckMnhM`).
    pub deck_mnh_m: f64,
    /// Part minimale de stations avec sursol pour lire un tablier [à calibrer].
    pub min_deck_presence: f64,
    /// Creux du MNT sous l'axe, sous la plus basse des sondes, qui fait un canyon (`minCanyonDropM`).
    pub min_canyon_drop_m: f64,
    /// Tolérance de `MNT > MNS` (station traitée comme haute ; `mntAboveMnsToleranceM`).
    pub mnt_above_mns_tolerance_m: f64,
    /// Fenêtre de la médiane du MNS (`surfaceMedianWindowM`).
    pub surface_median_window_m: f64,
    /// Fenêtre de la rugosité, RMS de `MNS − médiane` (`roughnessWindowM`).
    pub roughness_window_m: f64,
    /// Station propre : rugosité maximale sur la fenêtre sous ce seuil (`maxRoughnessM`).
    pub max_roughness_m: f64,
    /// Plafond de pente du profil (`maxSlopeP95`).
    pub max_slope: f64,
    /// Rampe sonde–ancre plus raide : profil abandonné [à calibrer].
    pub max_ramp_grade: f64,
    /// Part maximale de stations dont le tablier passe sous `MNT − tolérance` [à calibrer].
    pub max_below_ground_share: f64,
}

impl Default for DeckBuilderRules {
    fn default() -> Self {
        Self {
            deck_mnh_m: 1.0,
            min_deck_presence: 0.30,
            min_canyon_drop_m: 3.0,
            mnt_above_mns_tolerance_m: 0.5,
            surface_median_window_m: 25.0,
            roughness_window_m: 35.0,
            max_roughness_m: 0.8,
            max_slope: 0.10,
            max_ramp_grade: 0.15,
            max_below_ground_share: 0.10,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Empreinte des règles et des tables de classification : une modification sans changement de
    /// `RULES_VERSION` casse ce test (T03 §7). Mettre à jour les deux dans le même commit.
    #[test]
    fn rules_fingerprint() {
        let text = format!("{:?}|{}", Rules::default(), crate::classification::tables::canonical());
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in text.bytes() {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        assert_eq!((RULES_VERSION, format!("{hash:016x}")), ("2026.10.4", "ec416ccecef82f15".to_owned()));
    }
}
