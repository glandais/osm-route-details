//! Requête validée : seule chose que l'orchestrateur reçoit (T01 §4.2).

use std::fmt;

use serde::{Deserialize, Serialize};

/// Latitude maximale de la projection Web Mercator, en degrés (doc 02 §5.2).
pub const MERCATOR_MAX_LAT: f64 = 85.051_128_78;

/// Coordonnée WGS84 en degrés.
#[derive(Clone, Copy, PartialEq)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

impl LatLon {
    pub const fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }
}

impl fmt::Debug for LatLon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LatLon(..)")
    }
}

/// Parcours tel que donné par le client, avant décodage.
#[derive(Clone, PartialEq)]
pub enum RouteInput {
    Points(Vec<LatLon>),
    Polyline { encoded: String, precision: u8 },
}

impl fmt::Debug for RouteInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Points(p) => write!(f, "Points({} points)", p.len()),
            Self::Polyline { encoded, precision } => {
                write!(f, "Polyline({} chars, precision {precision})", encoded.len())
            }
        }
    }
}

/// Requête validée : points d'entrée dans l'ordre du client (doublons compris), options résolues.
#[derive(Clone, PartialEq)]
pub struct ValidatedRequest {
    pub points: Vec<LatLon>,
    /// Nombre de points d'entrée fournis par le client.
    pub input_len: usize,
    pub options: RequestOptions,
}

impl fmt::Debug for ValidatedRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValidatedRequest")
            .field("points", &self.points.len())
            .field("input_len", &self.input_len)
            .field("options", &self.options)
            .finish()
    }
}

/// Options résolues, défauts appliqués (doc 02 §3.2).
#[derive(Clone, Debug, PartialEq)]
pub struct RequestOptions {
    pub sections: SectionSet,
    pub sampling: Sampling,
    /// 0 : lissage désactivé.
    pub smoothing_m: f64,
    pub gain_preset: GainPreset,
    pub structure_correction: bool,
    pub accept_partial: bool,
    pub debug: bool,
    /// Profil de traitement (api 1.5) : `Full` par défaut.
    pub profile: Profile,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            sections: SectionSet::ALL,
            sampling: Sampling::InputWithBreakpoints,
            smoothing_m: 0.0,
            gain_preset: GainPreset::Dem,
            structure_correction: true,
            accept_partial: false,
            debug: false,
            profile: Profile::Full,
        }
    }
}

// Doc 02 §3.2 (api 1.5) ; ce `///` est recopié dans le contrat OpenAPI, donc en anglais.
/// Processing profile. `full` (default): IGN LiDAR HD in France with its surface model, all
/// structure corrections and heuristics. `fast`: low latency for interactive use, Mapterhorn
/// only (no IGN read, own queue, never waits behind `full` requests), OSM bridges and tunnels
/// interpolated between their ends, no heuristic detection. Same response shape; the effective
/// profile is in `meta.profile`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    #[default]
    Full,
    Fast,
}

impl Profile {
    pub const ALL: [Self; 2] = [Self::Full, Self::Fast];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Fast => "fast",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sampling {
    InputWithBreakpoints,
    Input,
    Distance { step_m: f64 },
}

/// Parties de la réponse demandées ; la typologie tourne quand même (doc 02 §3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionSet {
    pub elevation: bool,
    pub typology: bool,
    pub summary: bool,
}

impl SectionSet {
    pub const ALL: Self = Self { elevation: true, typology: true, summary: true };
    pub const NONE: Self = Self { elevation: false, typology: false, summary: false };
}

// Valeurs : T04 §5.4.
/// Elevation gain preset: threshold and smoothing scale of the gain computation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum GainPreset {
    Raw,
    Dem,
    Barometric,
    Gps,
}

impl GainPreset {
    pub const ALL: [Self; 4] = [Self::Raw, Self::Dem, Self::Barometric, Self::Gps];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_coordinates() {
        let p = LatLon::new(47.218_4, -1.553_6);
        let req = ValidatedRequest { points: vec![p, p], input_len: 2, options: RequestOptions::default() };
        let out = format!("{p:?} {req:?} {:?}", RouteInput::Points(vec![p]));
        assert!(!out.contains("47.2"), "{out}");
        assert!(!out.contains("1.55"), "{out}");
    }
}
