//! Zonage et emprises des sources (T01 §5.1 étape 3, T02 §7).

use std::ops::Range;
use std::time::Duration;

use crate::ctx::RequestCtx;
use crate::elevation::DemKind;
use crate::tiles::{SourceId, TileKey};
use crate::LatLon;

/// Plages en distance le long du parcours, issues du zonage.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoverageRanges {
    pub outside_primary_zone: Vec<Range<f64>>,
    pub typology_absent: Vec<Range<f64>>,
    pub elevation_absent: Vec<Range<f64>>,
}

/// État d'une source pour une requête, lu par `decide_outcome`.
#[derive(Clone, Debug, PartialEq)]
pub enum SourceOutcome<T> {
    Ok(T),
    Fallback(T),
    Missing(T),
    Failed { retry_after: Option<Duration> },
}

/// Rectangle en degrés.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

/// Polygones `[lon, lat]` : anneau extérieur puis trous.
pub type MultiPolygon = Vec<Vec<Vec<[f64; 2]>>>;

/// Emprise d'une source configurée.
#[derive(Clone, Debug, PartialEq)]
pub enum Extent {
    World,
    Bbox(Rect),
    Polygons(MultiPolygon),
}

#[async_trait::async_trait]
pub trait CoverageRef: Send + Sync {
    fn ign_may_have_data(&self, tile: TileKey) -> bool;
    fn in_primary_zone(&self, p: LatLon) -> bool;
    async fn dem_kinds(&self, pts: &[LatLon], ctx: &RequestCtx) -> Vec<DemKind>;
    fn extent(&self, source: SourceId) -> &Extent;
}
