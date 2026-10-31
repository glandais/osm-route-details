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

/// Preset de D+ (valeurs : T04 §5.4).
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
