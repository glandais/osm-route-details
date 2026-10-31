//! Port de lecture du terrain (T02 §6.3), implémenté dans `ord-data::elevation`.

use serde::Serialize;

use crate::ctx::RequestCtx;
use crate::LatLon;

/// Origine du terrain lu (doc 02 §4.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum TerrainOrigin {
    IgnLidarHd,
    MapterhornMeasured,
    Mapterhorn,
    None,
}

impl TerrainOrigin {
    pub const ALL: [Self; 4] = [Self::IgnLidarHd, Self::MapterhornMeasured, Self::Mapterhorn, Self::None];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemKind {
    Dtm,
    Glo30,
    /// Couture ≈ 150 m autour d'une plage GLO-30 (interne).
    Seam,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    /// Absent par construction (hors emprise).
    NoData,
    /// Panne après repli.
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainSample {
    pub ele: Option<f32>,
    pub origin: TerrainOrigin,
    /// Repli subi (tuile IGN en échec), pas un repli hors couverture.
    pub fallback_after_error: bool,
    pub dem_kind: DemKind,
    /// `Some` si et seulement si `ele` est `None`.
    pub missing: Option<Missing>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DsmSample {
    Value(f32),
    NoData,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainMode {
    Full,
    /// IGN lu seulement sur l'emprise des ponts.
    Sober,
}

#[async_trait::async_trait]
pub trait TerrainReader: Send + Sync {
    /// Sol, avec repli Mapterhorn point par point.
    async fn sample_terrain(&self, pts: &[LatLon], ctx: &RequestCtx) -> Vec<TerrainSample>;
    /// MNT IGN seul, sans repli : `None` = no-data ou échec.
    async fn sample_ign_dtm(&self, pts: &[LatLon], ctx: &RequestCtx) -> Vec<Option<f32>>;
    /// MNS IGN seul, jamais de repli.
    async fn sample_dsm(&self, pts: &[LatLon], ctx: &RequestCtx) -> Vec<DsmSample>;
    /// Taille de pixel au sol de la source lue.
    fn pixel_size_m(&self, p: LatLon, origin: TerrainOrigin) -> f64;
}
