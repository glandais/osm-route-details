//! Port de lecture du terrain (T02 §6.3), implémenté dans `ord-data::elevation`.

use serde::Serialize;

use crate::ctx::RequestCtx;
use crate::LatLon;

// Doc 02 §4.5 ; ce `///` est recopié dans le contrat OpenAPI, donc en anglais.
/// Origin of the terrain data.
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

/// Garde-fou de plausibilité d'une couche IGN LiDAR HD (règles 2026.11.7, décision de
/// l'utilisateur du 2026-11-02) : le rééchantillonnage du WMS mêle le no-data `−9999` aux pixels
/// valides voisins, qui sortent du format comme des altitudes (−297 à −947 m sur un terrain à
/// 349 m). Une valeur rejetée est lue comme du no-data : le point se replie comme sur un vrai
/// no-data (Mapterhorn pour le MNT, `NoData` pour le MNS).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Plausibility {
    /// Pixel sous ce plancher rejeté.
    pub floor_m: f64,
    /// Interpolation rejetée quand l'écart entre le plus haut et le plus bas des 4 voisins dépasse
    /// ce seuil **et** qu'un pixel no-data (ou sous le plancher) est à moins de `mixed_ring_px`
    /// pixels du carré des voisins : signature d'un pixel mêlé de no-data.
    pub mixed_spread_m: f64,
    pub mixed_ring_px: u32,
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
