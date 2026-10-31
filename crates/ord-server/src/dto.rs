//! DTO v1 de l'entrée (doc 02 §3). Le DTO de réponse arrive avec l'orchestrateur (incrément 1).

use std::fmt;

use ord_domain::request::GainPreset;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Corps de `POST /v1/route-details` : exactement un de `points` ou `polyline`.
#[derive(Deserialize, ToSchema)]
pub struct RouteDetailsRequestV1 {
    /// Points du parcours, dans le sens de parcours.
    pub points: Option<Vec<PointV1>>,
    /// Polyline encodée (algorithme Google), à la précision `polyline_precision`.
    pub polyline: Option<String>,
    /// Précision de `polyline` (5 ou 6), obligatoire avec elle.
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

/// Point d'entrée ; une altitude éventuelle (`ele` ou `alt`) est tolérée et ignorée.
#[derive(Deserialize, ToSchema)]
pub struct PointV1 {
    #[schema(minimum = -85.05112878, maximum = 85.05112878)]
    pub lat: f64,
    #[schema(minimum = -180, maximum = 180)]
    pub lon: f64,
    /// Ignorée.
    pub ele: Option<f64>,
    /// Ignorée.
    pub alt: Option<f64>,
}

impl fmt::Debug for PointV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PointV1(..)")
    }
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct OptionsV1 {
    /// Parties de la réponse ; défaut : toutes.
    pub sections: Option<Vec<SectionV1>>,
    pub sampling: Option<SamplingV1>,
    pub elevation: Option<ElevationOptionsV1>,
    /// Défaut : `true`.
    pub structure_correction: Option<bool>,
    /// Défaut : `false`.
    pub accept_partial: Option<bool>,
    /// Réservé à un usage interne ; refusé si le déploiement ne l'autorise pas.
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
    /// Défaut : `input_with_breakpoints`.
    pub mode: Option<SamplingModeV1>,
    /// Pas du profil en mode `distance`, en mètres (bornes : `/v1/capabilities`).
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
    /// 0 (désactivé) ou dans les bornes publiées, en mètres.
    pub smoothing_m: Option<f64>,
    /// Défaut : `dem`.
    pub gain_preset: Option<GainPreset>,
}
