//! Lecture du terrain (T02 §6.2–6.3) : MNT IGN LiDAR HD (WMS BIL, z15) avec repli point par point
//! sur Mapterhorn (Terrarium, z12), origine qualifiée par le référentiel de couverture.
//!
//! Bilinéaire au **centre** du pixel : un pixel `i` vaut pour `i + ½` (vcyclist,
//! `ElevationCalculator.kt`). x est linéaire en longitude ; y suit Mercator pour Terrarium et est
//! **linéaire en latitude dans l'emprise de la tuile** pour l'IGN (raster EPSG:4326, écart
//! < 0,005 px à z15). Lecture en trois phases : planifier les tuiles touchées (voisines comprises),
//! les charger par `get_many`, échantillonner de façon synchrone.

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::sync::Arc;

use ord_domain::coverage::CoverageRef;
use ord_domain::ctx::RequestCtx;
use ord_domain::elevation::{
    DemKind, DsmSample, Missing, Plausibility, TerrainMode, TerrainOrigin, TerrainReader, TerrainSample,
};
use ord_domain::tiles::{ElevationTile, TileKey, TileOutcome, TileStore};
use ord_domain::LatLon;

/// Circonférence équatoriale de la sphère Web Mercator, en mètres.
const MERCATOR_CIRCUMFERENCE_M: f64 = 40_075_016.686;

/// Convention de pixel. `Center` est la seule du service ; `Corner` (l'entier `i` pris pour la
/// position de l'échantillon, lecture d'avant `elevation-half-pixel` chez vcyclist) ne sert qu'à
/// mesurer la correction du demi-pixel (test de cohérence `test4`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PixelConvention {
    #[default]
    Center,
    Corner,
}

/// Disposition des lignes d'une tuile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowMode {
    /// Lignes régulières en y Mercator (Terrarium).
    Mercator,
    /// Lignes régulières en latitude entre les bords nord et sud de la tuile (WMS en EPSG:4326).
    LatLinear,
}

fn mercator_y(lat_deg: f64, n: f64) -> f64 {
    let lat = lat_deg.to_radians();
    (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / PI) / 2.0 * n
}

fn lat_of_row(y: f64, n: f64) -> f64 {
    (PI * (1.0 - 2.0 * y / n)).sinh().atan().to_degrees()
}

/// Position en pixels globaux (Web Mercator) au zoom `z`, tuiles de `size` pixels.
pub fn global_pixel(p: LatLon, zoom: u8, size: u32) -> (f64, f64) {
    let n = f64::from(size) * f64::from(1u32 << zoom);
    let x = (p.lon + 180.0) / 360.0 * n;
    (x, mercator_y(p.lat, n))
}

/// Position en pixels globaux selon la disposition des lignes.
fn global_pixel_rows(p: LatLon, zoom: u8, size: u32, rows: RowMode) -> (f64, f64) {
    match rows {
        RowMode::Mercator => global_pixel(p, zoom, size),
        RowMode::LatLinear => {
            let tiles = f64::from(1u32 << zoom);
            let x = (p.lon + 180.0) / 360.0 * tiles * f64::from(size);
            let ty = mercator_y(p.lat, tiles).floor().clamp(0.0, tiles - 1.0);
            let (north, south) = (lat_of_row(ty, tiles), lat_of_row(ty + 1.0, tiles));
            let fy = (north - p.lat) / (north - south);
            (x, (ty + fy) * f64::from(size))
        }
    }
}

/// Les quatre voisins bilinéaires d'un point : pixels globaux et poids `(dx, dy)`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Neighbours {
    px: [(u64, u64); 4],
    /// Pixel nord-ouest du carré, avant bouclage et blocage (couronne du garde-fou).
    origin: (i64, i64),
    dx: f64,
    dy: f64,
}

fn neighbours(p: LatLon, zoom: u8, size: u32, rows: RowMode, convention: PixelConvention) -> Neighbours {
    let (x, y) = global_pixel_rows(p, zoom, size, rows);
    let (fx, fy) = match convention {
        PixelConvention::Center => (x - 0.5, y - 0.5),
        PixelConvention::Corner => (x, y),
    };
    let (x0, y0) = (fx.floor(), fy.floor());
    let (dx, dy) = (fx - x0, fy - y0);
    let n = i64::from(size) * (1i64 << zoom);
    // Débordement vers la tuile voisine ; x boucle autour du monde, y est bloqué à ses bords.
    let wrap = |v: i64| v.rem_euclid(n) as u64;
    let clamp = |v: i64| v.clamp(0, n - 1) as u64;
    let (x0, y0) = (x0 as i64, y0 as i64);
    Neighbours {
        px: [
            (wrap(x0), clamp(y0)),
            (wrap(x0 + 1), clamp(y0)),
            (wrap(x0), clamp(y0 + 1)),
            (wrap(x0 + 1), clamp(y0 + 1)),
        ],
        origin: (x0, y0),
        dx,
        dy,
    }
}

/// Valeur d'un pixel, ou raison de son absence.
enum Pixel {
    Value(f64),
    Absent,
    Failed,
}

/// Échec d'un échantillon : no-data dans la tuile, tuile absente (404 au-delà du zoom local,
/// hors emprise) ou tuile en échec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SampleError {
    NoData,
    Absent,
    Failed,
}

impl From<SampleError> for Missing {
    fn from(e: SampleError) -> Self {
        match e {
            SampleError::NoData | SampleError::Absent => Missing::NoData,
            SampleError::Failed => Missing::Failed,
        }
    }
}

/// Échantillonneur synchrone sur des tuiles déjà chargées.
#[derive(Debug)]
struct TileGrid<'a> {
    pub zoom: u8,
    pub size: u32,
    pub tiles: &'a BTreeMap<TileKey, TileOutcome<ElevationTile>>,
    /// Garde-fou de plausibilité (couches IGN) ; `None` : valeurs lues telles quelles.
    pub plausibility: Option<Plausibility>,
}

impl TileGrid<'_> {
    /// Valeur d'un pixel ; sous le plancher de plausibilité, no-data (`NaN`).
    fn pixel(&self, at: (u64, u64)) -> Pixel {
        match (self.raw_pixel(at), self.plausibility) {
            (Pixel::Value(v), Some(p)) if v < p.floor_m => Pixel::Value(f64::NAN),
            (px, _) => px,
        }
    }

    fn raw_pixel(&self, (gx, gy): (u64, u64)) -> Pixel {
        let size = u64::from(self.size);
        let key = TileKey { z: self.zoom, x: (gx / size) as u32, y: (gy / size) as u32 };
        match self.tiles.get(&key) {
            Some(TileOutcome::Data { tile, .. } | TileOutcome::Empty(tile)) => match tile.as_ref() {
                ElevationTile::NoData => Pixel::Value(f64::NAN),
                ElevationTile::Grid { size: s, values, .. } => {
                    let s = u64::from(*s);
                    let i = (gy % size) * s + (gx % size);
                    Pixel::Value(values.get(i as usize).map_or(f64::NAN, |v| f64::from(*v)))
                }
            },
            Some(TileOutcome::Absent) => Pixel::Absent,
            Some(TileOutcome::Failed(_)) | None => Pixel::Failed,
        }
    }

    /// Garde-fou des pixels mêlés de no-data (règles 2026.11.7) : les 4 voisins, tous valides,
    /// s'écartent de plus de `mixed_spread_m` et un pixel no-data est dans la couronne de
    /// `mixed_ring_px` pixels autour d'eux. Seules les tuiles déjà chargées sont lues : une tuile
    /// absente, en échec ou non planifiée ne compte pas comme no-data.
    fn mixed_with_nodata(&self, n: &Neighbours, v: &[f64; 4]) -> bool {
        let Some(p) = self.plausibility else { return false };
        let (lo, hi) =
            v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| (lo.min(x), hi.max(x)));
        if hi - lo <= p.mixed_spread_m {
            return false;
        }
        let r = i64::from(p.mixed_ring_px);
        let world = i64::from(self.size) * (1i64 << self.zoom);
        let (x0, y0) = n.origin;
        (y0 - r..=y0 + 1 + r).any(|y| {
            (x0 - r..=x0 + 1 + r).any(|x| {
                let at = (x.rem_euclid(world) as u64, y.clamp(0, world - 1) as u64);
                matches!(self.pixel(at), Pixel::Value(h) if h.is_nan())
            })
        })
    }

    /// Bilinéaire ; un voisin no-data, même de poids nul, rend le point no-data (T02 §6.2), comme
    /// une valeur invraisemblable (garde-fou de plausibilité des couches IGN).
    fn sample(&self, n: &Neighbours) -> Result<f64, SampleError> {
        let mut v = [0.0; 4];
        let mut absent = false;
        for (slot, &px) in v.iter_mut().zip(&n.px) {
            match self.pixel(px) {
                Pixel::Value(x) => *slot = x,
                Pixel::Absent => absent = true,
                Pixel::Failed => return Err(SampleError::Failed),
            }
        }
        if absent {
            return Err(SampleError::Absent);
        }
        if v.iter().any(|x| x.is_nan()) || self.mixed_with_nodata(n, &v) {
            return Err(SampleError::NoData);
        }
        let top = v[0] * (1.0 - n.dx) + v[1] * n.dx;
        let bottom = v[2] * (1.0 - n.dx) + v[3] * n.dx;
        let h = top * (1.0 - n.dy) + bottom * n.dy;
        if h.is_nan() {
            Err(SampleError::NoData)
        } else {
            Ok(h)
        }
    }
}

/// Tuiles touchées par les points (voisines comprises), triées et sans doublon.
fn plan(neighbours: &[Neighbours], zoom: u8, size: u32) -> Vec<TileKey> {
    let size = u64::from(size);
    let mut keys: Vec<TileKey> = neighbours
        .iter()
        .flat_map(|n| {
            n.px.iter().map(move |&(gx, gy)| TileKey {
                z: zoom,
                x: (gx / size) as u32,
                y: (gy / size) as u32,
            })
        })
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// Une couche d'altitude lue par le port de tuiles.
#[derive(Clone)]
pub struct ElevationLayer {
    pub store: Arc<dyn TileStore<ElevationTile>>,
    pub zoom: u8,
    /// Côté des tuiles, en pixels.
    pub size: u32,
    pub rows: RowMode,
    /// Garde-fou de plausibilité (IGN LiDAR HD, MNT et MNS) ; `None` pour Mapterhorn, dont le
    /// décodeur borne déjà les valeurs.
    pub plausibility: Option<Plausibility>,
}

impl std::fmt::Debug for ElevationLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElevationLayer")
            .field("zoom", &self.zoom)
            .field("size", &self.size)
            .field("rows", &self.rows)
            .field("plausibility", &self.plausibility)
            .finish_non_exhaustive()
    }
}

impl ElevationLayer {
    /// Échantillonne les points au zoom `zoom` ; une tuile pour laquelle `may_fetch` répond non
    /// est lue comme no-data, sans réseau, disque ni cache (contour IGN, T02 §3).
    async fn sample(
        &self,
        pts: &[LatLon],
        zoom: u8,
        convention: PixelConvention,
        ctx: &RequestCtx,
        may_fetch: &(dyn Fn(TileKey) -> bool + Sync),
    ) -> Vec<Result<f64, SampleError>> {
        if pts.is_empty() {
            return Vec::new();
        }
        let neighbours: Vec<Neighbours> =
            pts.iter().map(|&p| neighbours(p, zoom, self.size, self.rows, convention)).collect();
        let (fetch, skipped): (Vec<TileKey>, Vec<TileKey>) =
            plan(&neighbours, zoom, self.size).into_iter().partition(|k| may_fetch(*k));
        let outcomes = self.store.get_many(&fetch, ctx).await;
        let mut tiles: BTreeMap<TileKey, TileOutcome<ElevationTile>> =
            fetch.into_iter().zip(outcomes).collect();
        let nodata = Arc::new(ElevationTile::NoData);
        for k in skipped {
            tiles.insert(k, TileOutcome::Empty(Arc::clone(&nodata)));
        }
        let grid = TileGrid { zoom, size: self.size, tiles: &tiles, plausibility: self.plausibility };
        neighbours.iter().map(|n| grid.sample(n)).collect()
    }

    fn pixel_size_m(&self, p: LatLon) -> f64 {
        let along = MERCATOR_CIRCUMFERENCE_M * p.lat.to_radians().cos();
        along / (f64::from(self.size) * f64::from(1u32 << self.zoom))
    }
}

/// Mapterhorn : couche Terrarium et redescente de zoom après un 404 (T02 §6.3).
#[derive(Clone, Debug)]
pub struct MapterhornLayer {
    pub layer: ElevationLayer,
    /// Plus bas zoom de la redescente : à z > 12, un 404 signifie « zoom local dépassé » ; le
    /// lecteur redescend d'un zoom à la fois jusqu'à une tuile (au plus jusqu'à z12).
    pub min_zoom: u8,
}

impl MapterhornLayer {
    pub fn new(layer: ElevationLayer) -> Self {
        let min_zoom = layer.zoom.min(12);
        Self { layer, min_zoom }
    }

    async fn sample(
        &self,
        pts: &[LatLon],
        convention: PixelConvention,
        ctx: &RequestCtx,
    ) -> Vec<Result<f64, SampleError>> {
        let all = |_: TileKey| true;
        let mut zoom = self.layer.zoom;
        let mut out = self.layer.sample(pts, zoom, convention, ctx, &all).await;
        while zoom > self.min_zoom {
            let absent: Vec<usize> = (0..out.len()).filter(|&i| out[i] == Err(SampleError::Absent)).collect();
            if absent.is_empty() {
                break;
            }
            zoom -= 1;
            let sub: Vec<LatLon> = absent.iter().map(|&i| pts[i]).collect();
            for (i, r) in absent.into_iter().zip(self.layer.sample(&sub, zoom, convention, ctx, &all).await) {
                out[i] = r;
            }
        }
        out
    }
}

/// Lecteur de terrain du service : MNT IGN (mode `Full`) avec repli Mapterhorn point par point,
/// MNS IGN sans repli, origine qualifiée par le référentiel de couverture.
pub struct LayeredTerrainReader {
    ign_dtm: Option<ElevationLayer>,
    ign_dsm: Option<ElevationLayer>,
    mapterhorn: MapterhornLayer,
    coverage: Arc<dyn CoverageRef>,
    mode: TerrainMode,
    convention: PixelConvention,
}

impl std::fmt::Debug for LayeredTerrainReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayeredTerrainReader")
            .field("ign_dtm", &self.ign_dtm)
            .field("ign_dsm", &self.ign_dsm)
            .field("mapterhorn", &self.mapterhorn)
            .field("mode", &self.mode)
            .field("convention", &self.convention)
            .finish_non_exhaustive()
    }
}

impl LayeredTerrainReader {
    pub fn new(
        ign_dtm: Option<ElevationLayer>,
        ign_dsm: Option<ElevationLayer>,
        mapterhorn: MapterhornLayer,
        coverage: Arc<dyn CoverageRef>,
        mode: TerrainMode,
        convention: PixelConvention,
    ) -> Self {
        Self { ign_dtm, ign_dsm, mapterhorn, coverage, mode, convention }
    }

    async fn sample_ign(
        &self,
        layer: &ElevationLayer,
        pts: &[LatLon],
        ctx: &RequestCtx,
    ) -> Vec<Result<f64, SampleError>> {
        let coverage = Arc::clone(&self.coverage);
        let may_fetch = move |k: TileKey| coverage.ign_may_have_data(k);
        layer.sample(pts, layer.zoom, self.convention, ctx, &may_fetch).await
    }
}

#[async_trait::async_trait]
impl TerrainReader for LayeredTerrainReader {
    /// (1) mode `Full` et tuile dans le contour → MNT IGN ; (2) tuile IGN en échec → point entier
    /// relu dans Mapterhorn, `fallback_after_error` ; (3) no-data IGN sur un des 4 voisins → point
    /// entier relu dans Mapterhorn, repli normal ; (4) origine Mapterhorn par le référentiel :
    /// `mapterhorn_measured` en zone France hors GLO-30 et coutures. Jamais 0 m.
    async fn sample_terrain(&self, pts: &[LatLon], ctx: &RequestCtx) -> Vec<TerrainSample> {
        let ign: Vec<Result<f64, SampleError>> = match (&self.ign_dtm, self.mode) {
            (Some(layer), TerrainMode::Full) => self.sample_ign(layer, pts, ctx).await,
            _ => vec![Err(SampleError::Absent); pts.len()],
        };
        let mut out: Vec<Option<TerrainSample>> = ign
            .iter()
            .map(|r| {
                r.ok().map(|h| TerrainSample {
                    ele: Some(h as f32),
                    origin: TerrainOrigin::IgnLidarHd,
                    fallback_after_error: false,
                    dem_kind: DemKind::Dtm,
                    missing: None,
                })
            })
            .collect();
        let fallback: Vec<usize> = (0..pts.len()).filter(|&i| out[i].is_none()).collect();
        if !fallback.is_empty() {
            let sub: Vec<LatLon> = fallback.iter().map(|&i| pts[i]).collect();
            let values = self.mapterhorn.sample(&sub, self.convention, ctx).await;
            let with_value: Vec<LatLon> =
                sub.iter().zip(&values).filter(|(_, v)| v.is_ok()).map(|(p, _)| *p).collect();
            let mut kinds = self.coverage.dem_kinds(&with_value, ctx).await.into_iter();
            for ((&i, p), v) in fallback.iter().zip(&sub).zip(values) {
                let after_error = ign[i] == Err(SampleError::Failed);
                out[i] = Some(match v {
                    Ok(h) => {
                        let dem_kind = kinds.next().unwrap_or(DemKind::Unknown);
                        let measured = self.coverage.in_primary_zone(*p)
                            && !matches!(dem_kind, DemKind::Glo30 | DemKind::Seam);
                        TerrainSample {
                            ele: Some(h as f32),
                            origin: if measured {
                                TerrainOrigin::MapterhornMeasured
                            } else {
                                TerrainOrigin::Mapterhorn
                            },
                            fallback_after_error: after_error,
                            dem_kind,
                            missing: None,
                        }
                    }
                    Err(e) => TerrainSample {
                        ele: None,
                        origin: TerrainOrigin::None,
                        fallback_after_error: false,
                        dem_kind: DemKind::Unknown,
                        missing: Some(e.into()),
                    },
                });
            }
        }
        out.into_iter()
            .map(|s| {
                s.unwrap_or(TerrainSample {
                    ele: None,
                    origin: TerrainOrigin::None,
                    fallback_after_error: false,
                    dem_kind: DemKind::Unknown,
                    missing: Some(Missing::Failed),
                })
            })
            .collect()
    }

    async fn sample_ign_dtm(&self, pts: &[LatLon], ctx: &RequestCtx) -> Vec<Option<f32>> {
        match &self.ign_dtm {
            Some(layer) => {
                self.sample_ign(layer, pts, ctx).await.into_iter().map(|r| r.ok().map(|h| h as f32)).collect()
            }
            None => vec![None; pts.len()],
        }
    }

    /// MNS IGN seul, **jamais de repli** (doc 01 §4, §5.5 invariant 10).
    async fn sample_dsm(&self, pts: &[LatLon], ctx: &RequestCtx) -> Vec<DsmSample> {
        match &self.ign_dsm {
            Some(layer) => self
                .sample_ign(layer, pts, ctx)
                .await
                .into_iter()
                .map(|r| match r {
                    Ok(h) => DsmSample::Value(h as f32),
                    Err(SampleError::Failed) => DsmSample::Unavailable,
                    Err(_) => DsmSample::NoData,
                })
                .collect(),
            None => vec![DsmSample::NoData; pts.len()],
        }
    }

    fn pixel_size_m(&self, p: LatLon, origin: TerrainOrigin) -> f64 {
        match (origin, &self.ign_dtm) {
            (TerrainOrigin::IgnLidarHd, Some(layer)) => layer.pixel_size_m(p),
            _ => self.mapterhorn.layer.pixel_size_m(p),
        }
    }
}

/// Centre géographique d'un pixel global (inverse de `global_pixel` au centre `i + ½`).
pub fn pixel_center(gx: u64, gy: u64, zoom: u8, size: u32) -> LatLon {
    latlon_of_global(gx as f64 + 0.5, gy as f64 + 0.5, zoom, size)
}

/// Point géographique d'une position en pixels globaux (inverse de `global_pixel`).
pub fn latlon_of_global(x: f64, y: f64, zoom: u8, size: u32) -> LatLon {
    let n = f64::from(size) * f64::from(1u32 << zoom);
    let lon = x / n * 360.0 - 180.0;
    let lat = (PI * (1.0 - 2.0 * y / n)).sinh().atan().to_degrees();
    LatLon::new(lat, lon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ord_domain::ctx::{FailureKind, TileFailure};
    use ord_domain::tiles::SourceId;

    const Z: u8 = 3;
    const S: u32 = 4;

    fn grid_tile(f: impl Fn(u64, u64) -> f32, tx: u32, ty: u32) -> TileOutcome<ElevationTile> {
        let s = u64::from(S);
        let values: Box<[f32]> =
            (0..s * s).map(|i| f(u64::from(tx) * s + i % s, u64::from(ty) * s + i / s)).collect();
        TileOutcome::Data {
            tile: Arc::new(ElevationTile::Grid { size: S as u16, values, all_valid: true }),
            stale: false,
        }
    }

    fn all_tiles(f: impl Fn(u64, u64) -> f32 + Copy) -> BTreeMap<TileKey, TileOutcome<ElevationTile>> {
        let n = 1u32 << Z;
        let mut m = BTreeMap::new();
        for x in 0..n {
            for y in 0..n {
                m.insert(TileKey { z: Z, x, y }, grid_tile(f, x, y));
            }
        }
        m
    }

    #[test]
    fn pixel_centre_returns_the_pixel_itself() {
        let tiles = all_tiles(|x, y| (x * 100 + y) as f32);
        let grid = TileGrid { zoom: Z, size: S, tiles: &tiles, plausibility: None };
        for (gx, gy) in [(5, 9), (12, 3), (4, 4), (7, 8)] {
            let p = pixel_center(gx, gy, Z, S);
            let n = neighbours(p, Z, S, RowMode::Mercator, PixelConvention::Center);
            let h = grid.sample(&n).unwrap();
            assert!((h - (gx * 100 + gy) as f64).abs() < 1e-6, "({gx}, {gy}) : {h}");
        }
    }

    #[test]
    fn continuity_across_a_tile_edge_and_plane_reproduced() {
        // Plan en pixels globaux : la bilinéaire au centre le reproduit sans décalage.
        let tiles = all_tiles(|x, y| (2.0 * x as f64 + 0.5 * y as f64) as f32);
        let grid = TileGrid { zoom: Z, size: S, tiles: &tiles, plausibility: None };
        let edge = pixel_center(8, 10, Z, S); // pixel 8 = premier pixel de la tuile x = 2
        let (x, y) = global_pixel(edge, Z, S);
        for dx in [-0.6, -0.5, -0.01, 0.0, 0.01, 0.5] {
            let n = neighbours(
                {
                    let nn = f64::from(S) * f64::from(1u32 << Z);
                    let lon = (x - 0.5 + dx) / nn * 360.0 - 180.0;
                    LatLon::new(edge.lat, lon)
                },
                Z,
                S,
                RowMode::Mercator,
                PixelConvention::Center,
            );
            let h = grid.sample(&n).unwrap();
            let expected = 2.0 * (x - 0.5 + dx - 0.5) + 0.5 * (y - 0.5);
            assert!((h - expected).abs() < 1e-4, "dx {dx}: {h} vs {expected}");
        }
    }

    #[test]
    fn corner_convention_reads_half_a_pixel_south_east() {
        let tiles = all_tiles(|x, _| x as f32);
        let grid = TileGrid { zoom: Z, size: S, tiles: &tiles, plausibility: None };
        let p = pixel_center(10, 10, Z, S);
        let centre = grid.sample(&neighbours(p, Z, S, RowMode::Mercator, PixelConvention::Center)).unwrap();
        let corner = grid.sample(&neighbours(p, Z, S, RowMode::Mercator, PixelConvention::Corner)).unwrap();
        assert!((centre - 10.0).abs() < 1e-9);
        assert!((corner - 10.5).abs() < 1e-9);
    }

    #[test]
    fn nodata_failure_and_absence() {
        let mut tiles = all_tiles(|x, y| if (x, y) == (9, 9) { f32::NAN } else { 1.0 });
        let grid = TileGrid { zoom: Z, size: S, tiles: &tiles, plausibility: None };
        // Centre exact du pixel (8, 9) : le voisin (9, 9) a un poids nul mais est no-data.
        let n = neighbours(pixel_center(8, 9, Z, S), Z, S, RowMode::Mercator, PixelConvention::Center);
        assert_eq!(grid.sample(&n), Err(SampleError::NoData));
        let k = TileKey { z: Z, x: 0, y: 0 };
        tiles.insert(k, TileOutcome::Absent);
        let n0 = neighbours(pixel_center(1, 1, Z, S), Z, S, RowMode::Mercator, PixelConvention::Center);
        assert_eq!(
            TileGrid { zoom: Z, size: S, tiles: &tiles, plausibility: None }.sample(&n0),
            Err(SampleError::Absent)
        );
        tiles.insert(
            k,
            TileOutcome::Failed(TileFailure {
                source: SourceId::Mapterhorn,
                tile: k,
                kind: FailureKind::Server,
                status: Some(500),
                retry_after: None,
                negative_until: None,
                message: String::new(),
            }),
        );
        assert_eq!(
            TileGrid { zoom: Z, size: S, tiles: &tiles, plausibility: None }.sample(&n0),
            Err(SampleError::Failed)
        );
    }

    #[test]
    fn plan_includes_neighbour_tiles_and_world_edges() {
        // Moitié ouest du pixel 4, premier pixel de la tuile x = 1 : le voisin ouest est dans la tuile 0.
        let near_edge =
            neighbours(latlon_of_global(4.2, 5.5, Z, S), Z, S, RowMode::Mercator, PixelConvention::Center);
        let keys = plan(&[near_edge], Z, S);
        assert_eq!(keys.iter().map(|k| k.x).collect::<Vec<_>>(), [0, 1]);
        let west = neighbours(LatLon::new(0.0, -179.99999), Z, S, RowMode::Mercator, PixelConvention::Center);
        assert!(plan(&[west], Z, S).iter().any(|k| k.x == (1 << Z) - 1), "x boucle à l'antiméridien");
        let north = neighbours(LatLon::new(85.05, 0.0), Z, S, RowMode::Mercator, PixelConvention::Center);
        assert!(plan(&[north], Z, S).iter().all(|k| k.y == 0), "y bloqué au bord du monde");
    }

    /// Magasin en mémoire : tuiles par clé, compteur de clés demandées.
    struct MemStore {
        tiles: BTreeMap<TileKey, TileOutcome<ElevationTile>>,
        asked: std::sync::Mutex<Vec<TileKey>>,
    }

    impl MemStore {
        fn new(tiles: BTreeMap<TileKey, TileOutcome<ElevationTile>>) -> Arc<Self> {
            Arc::new(Self { tiles, asked: std::sync::Mutex::new(Vec::new()) })
        }
    }

    fn clone_outcome(o: &TileOutcome<ElevationTile>) -> TileOutcome<ElevationTile> {
        match o {
            TileOutcome::Data { tile, stale } => TileOutcome::Data { tile: Arc::clone(tile), stale: *stale },
            TileOutcome::Empty(t) => TileOutcome::Empty(Arc::clone(t)),
            TileOutcome::Absent => TileOutcome::Absent,
            TileOutcome::Failed(f) => TileOutcome::Failed(f.clone()),
        }
    }

    #[async_trait::async_trait]
    impl TileStore<ElevationTile> for MemStore {
        async fn get_many(&self, keys: &[TileKey], _: &RequestCtx) -> Vec<TileOutcome<ElevationTile>> {
            self.asked.lock().unwrap().extend_from_slice(keys);
            keys.iter().map(|k| self.tiles.get(k).map_or(TileOutcome::Absent, clone_outcome)).collect()
        }
        fn data_version(&self) -> ord_domain::catalog::DataVersion {
            unreachable!()
        }
    }

    fn ctx() -> RequestCtx {
        use ord_domain::budget::{Deadline, RequestId};
        RequestCtx::new(RequestId("t".into()), Deadline::after(std::time::Duration::from_secs(5)))
    }

    fn constant(size: u16, v: f32) -> TileOutcome<ElevationTile> {
        let n = usize::from(size) * usize::from(size);
        TileOutcome::Data {
            tile: Arc::new(ElevationTile::Grid { size, values: vec![v; n].into(), all_valid: !v.is_nan() }),
            stale: false,
        }
    }

    fn failed(k: TileKey) -> TileOutcome<ElevationTile> {
        TileOutcome::Failed(TileFailure {
            source: SourceId::IgnDtm,
            tile: k,
            kind: FailureKind::Server,
            status: Some(503),
            retry_after: None,
            negative_until: None,
            message: String::new(),
        })
    }

    /// Tuiles IGN z15 (256 px) et Mapterhorn z12 (512 px) autour d'un point, à valeur constante.
    fn tiles_around(
        p: LatLon,
        zoom: u8,
        size: u16,
        rows: RowMode,
        v: impl Fn(TileKey) -> TileOutcome<ElevationTile>,
    ) -> BTreeMap<TileKey, TileOutcome<ElevationTile>> {
        let n = neighbours(p, zoom, u32::from(size), rows, PixelConvention::Center);
        plan(&[n], zoom, u32::from(size)).into_iter().map(|k| (k, v(k))).collect()
    }

    fn reader(ign: Arc<MemStore>, mt: Arc<MemStore>, mt_zoom: u8) -> LayeredTerrainReader {
        LayeredTerrainReader::new(
            Some(ElevationLayer {
                store: ign,
                zoom: 15,
                size: 256,
                rows: RowMode::LatLinear,
                plausibility: Some(IGN_PLAUSIBILITY),
            }),
            None,
            MapterhornLayer::new(ElevationLayer {
                store: mt,
                zoom: mt_zoom,
                size: 512,
                rows: RowMode::Mercator,
                plausibility: None,
            }),
            Arc::new(crate::coverage::MapterhornCoverage::hull_only()),
            TerrainMode::Full,
            PixelConvention::Center,
        )
    }

    #[test]
    fn lat_linear_rows_are_continuous_across_tile_edges() {
        let (z, size) = (15u8, 256u32);
        let k = TileKey { z, x: 16_000, y: 11_500 };
        let (_, south, _, north) = k.bounds();
        let y_north = global_pixel_rows(LatLon::new(north - 1e-12, 0.0), z, size, RowMode::LatLinear).1;
        let y_south = global_pixel_rows(LatLon::new(south + 1e-12, 0.0), z, size, RowMode::LatLinear).1;
        assert!((y_north - f64::from(k.y * 256)).abs() < 1e-3);
        assert!((y_south - f64::from((k.y + 1) * 256)).abs() < 1e-3);
        // Écart avec Mercator < 0,005 px au milieu d'une tuile z15 (T02 §6.2).
        let mid = LatLon::new(0.5 * (north + south), 0.0);
        let dm = global_pixel_rows(mid, z, size, RowMode::Mercator).1
            - global_pixel_rows(mid, z, size, RowMode::LatLinear).1;
        assert!(dm.abs() < 0.005, "{dm}");
    }

    #[tokio::test]
    async fn ign_then_fallbacks_with_their_flags() {
        let france = LatLon::new(47.2184, -1.5536);
        let mt = MemStore::new(tiles_around(france, 12, 512, RowMode::Mercator, |_| constant(512, 20.0)));
        // IGN lisible : origine IGN.
        let ign = MemStore::new(tiles_around(france, 15, 256, RowMode::LatLinear, |_| constant(256, 10.0)));
        let s = reader(ign, Arc::clone(&mt), 12).sample_terrain(&[france], &ctx()).await;
        assert_eq!(
            (s[0].ele, s[0].origin, s[0].fallback_after_error),
            (Some(10.0), TerrainOrigin::IgnLidarHd, false)
        );
        // IGN en échec : repli subi, Mapterhorn mesuré en zone France (sans fichier de couverture).
        let ign = MemStore::new(tiles_around(france, 15, 256, RowMode::LatLinear, failed));
        let s = reader(ign, Arc::clone(&mt), 12).sample_terrain(&[france], &ctx()).await;
        assert_eq!(s[0].ele, Some(20.0));
        assert_eq!(s[0].origin, TerrainOrigin::MapterhornMeasured);
        assert!(s[0].fallback_after_error);
        // IGN no-data : repli normal.
        let ign =
            MemStore::new(tiles_around(france, 15, 256, RowMode::LatLinear, |_| constant(256, f32::NAN)));
        let s = reader(ign, mt, 12).sample_terrain(&[france], &ctx()).await;
        assert_eq!((s[0].ele, s[0].fallback_after_error), (Some(20.0), false));
    }

    #[tokio::test]
    async fn no_ign_tile_is_asked_outside_the_hull() {
        let krk = LatLon::new(45.2464, 14.5692);
        let ign = MemStore::new(BTreeMap::new());
        let mt = MemStore::new(tiles_around(krk, 12, 512, RowMode::Mercator, |_| constant(512, 50.0)));
        let s = reader(Arc::clone(&ign), mt, 12).sample_terrain(&[krk], &ctx()).await;
        assert!(ign.asked.lock().unwrap().is_empty(), "aucune tuile IGN demandée hors contour");
        assert_eq!((s[0].ele, s[0].origin), (Some(50.0), TerrainOrigin::Mapterhorn));
    }

    #[tokio::test]
    async fn mapterhorn_zoom_falls_back_after_a_404() {
        let krk = LatLon::new(45.2464, 14.5692);
        // z13 absente (404 : zoom local dépassé), z12 présente.
        let mut tiles = tiles_around(krk, 13, 512, RowMode::Mercator, |_| TileOutcome::Absent);
        tiles.extend(tiles_around(krk, 12, 512, RowMode::Mercator, |_| constant(512, 7.0)));
        let mt = MemStore::new(tiles);
        let s =
            reader(MemStore::new(BTreeMap::new()), Arc::clone(&mt), 13).sample_terrain(&[krk], &ctx()).await;
        assert_eq!(s[0].ele, Some(7.0));
        assert!(mt.asked.lock().unwrap().iter().any(|k| k.z == 12));
        // Au zoom plancher, une tuile absente reste une absence par construction.
        let none = MemStore::new(BTreeMap::new());
        let s = reader(MemStore::new(BTreeMap::new()), none, 12).sample_terrain(&[krk], &ctx()).await;
        assert_eq!((s[0].ele, s[0].missing), (None, Some(Missing::NoData)));
    }

    #[tokio::test]
    async fn failed_fallback_is_a_failure_without_fallback_flag() {
        let france = LatLon::new(47.2184, -1.5536);
        let ign = MemStore::new(tiles_around(france, 15, 256, RowMode::LatLinear, failed));
        let mt = MemStore::new(tiles_around(france, 12, 512, RowMode::Mercator, failed));
        let s = reader(ign, mt, 12).sample_terrain(&[france], &ctx()).await;
        assert_eq!((s[0].ele, s[0].missing, s[0].fallback_after_error), (None, Some(Missing::Failed), false));
    }

    /// Garde-fou des règles (MNT) : plancher −50 m, écart 20 m, couronne de 2 pixels.
    const IGN_PLAUSIBILITY: Plausibility =
        Plausibility { floor_m: -50.0, mixed_spread_m: 20.0, mixed_ring_px: 2 };

    /// Tuile IGN z15 dont chaque pixel vaut `f(x, y)` en pixels globaux.
    fn ign_tile(k: TileKey, f: &impl Fn(i64, i64) -> f32) -> TileOutcome<ElevationTile> {
        let values: Box<[f32]> = (0..256 * 256)
            .map(|i| f(i64::from(k.x) * 256 + i % 256, i64::from(k.y) * 256 + i / 256))
            .collect();
        let all_valid = values.iter().all(|v| !v.is_nan());
        TileOutcome::Data {
            tile: Arc::new(ElevationTile::Grid { size: 256, values, all_valid }),
            stale: false,
        }
    }

    /// Point IGN au milieu d'une tuile et son carré de voisins.
    fn ign_point() -> (LatLon, (i64, i64)) {
        let p = LatLon::new(47.2184, -1.5536);
        let n = neighbours(p, 15, 256, RowMode::LatLinear, PixelConvention::Center);
        let (x0, y0) = n.origin;
        assert!((4..250).contains(&x0.rem_euclid(256)) && (4..250).contains(&y0.rem_euclid(256)));
        (p, n.origin)
    }

    #[tokio::test]
    async fn band_mixed_with_nodata_falls_back_to_mapterhorn() {
        let (p, (_, y0)) = ign_point();
        let mt = MemStore::new(tiles_around(p, 12, 512, RowMode::Mercator, |_| constant(512, 351.0)));
        // Bande du WMS (perso-53) : une ligne sous le plancher (−9999 mêlé à 1/8), la ligne du point
        // mêlée plus faiblement (202 m, au-dessus du plancher), terrain vers 349 m autour.
        let band = move |_: i64, y: i64| match y - y0 {
            -1 => -945.0,
            0 => 202.0,
            _ => 349.0,
        };
        let ign = MemStore::new(tiles_around(p, 15, 256, RowMode::LatLinear, |k| ign_tile(k, &band)));
        let s = reader(ign, Arc::clone(&mt), 12).sample_terrain(&[p], &ctx()).await;
        assert_eq!(
            (s[0].ele, s[0].origin, s[0].fallback_after_error),
            (Some(351.0), TerrainOrigin::MapterhornMeasured, false),
            "repli normal, comme sur un vrai no-data"
        );
        // Le point voisin dont un des 4 voisins est sous le plancher se replie aussi.
        let below = |_: i64, y: i64| if y == y0 { -297.0 } else { 349.0 };
        let ign = MemStore::new(tiles_around(p, 15, 256, RowMode::LatLinear, |k| ign_tile(k, &below)));
        let s = reader(ign, Arc::clone(&mt), 12).sample_terrain(&[p], &ctx()).await;
        assert_eq!((s[0].ele, s[0].origin), (Some(351.0), TerrainOrigin::MapterhornMeasured));
        // MNT IGN seul (sans repli) : no-data.
        let ign = MemStore::new(tiles_around(p, 15, 256, RowMode::LatLinear, |k| ign_tile(k, &band)));
        assert_eq!(reader(ign, mt, 12).sample_ign_dtm(&[p], &ctx()).await, vec![None]);
    }

    #[tokio::test]
    async fn plausible_tiles_are_unchanged() {
        let (p, (_, y0)) = ign_point();
        let mt = MemStore::new(tiles_around(p, 12, 512, RowMode::Mercator, |_| constant(512, 351.0)));
        let n = neighbours(p, 15, 256, RowMode::LatLinear, PixelConvention::Center);
        // Falaise de 147 m sans no-data autour : lue telle quelle, interpolation comprise.
        let cliff = move |_: i64, y: i64| if y <= y0 { 202.0 } else { 349.0 };
        let ign = MemStore::new(tiles_around(p, 15, 256, RowMode::LatLinear, |k| ign_tile(k, &cliff)));
        let s = reader(ign, Arc::clone(&mt), 12).sample_terrain(&[p], &ctx()).await;
        let expected = 202.0 + n.dy * 147.0;
        assert_eq!(s[0].origin, TerrainOrigin::IgnLidarHd);
        assert!((f64::from(s[0].ele.unwrap()) - expected).abs() < 1e-3, "{:?} vs {expected}", s[0].ele);
        // Terrain bas mais plausible (polder à −3 m) au bord d'un vrai no-data : écart faible, lu.
        let polder = move |_: i64, y: i64| if y < y0 - 1 { f32::NAN } else { -3.0 + (y - y0) as f32 };
        let ign = MemStore::new(tiles_around(p, 15, 256, RowMode::LatLinear, |k| ign_tile(k, &polder)));
        let s = reader(ign, mt, 12).sample_terrain(&[p], &ctx()).await;
        assert_eq!(s[0].origin, TerrainOrigin::IgnLidarHd);
        assert!((f64::from(s[0].ele.unwrap()) - (-3.0 + n.dy)).abs() < 1e-3);
    }

    #[test]
    fn plausibility_floor_and_mixed_pixels_on_the_grid() {
        let guard = Some(IGN_PLAUSIBILITY);
        let p = pixel_center(9, 9, Z, S);
        let n = neighbours(p, Z, S, RowMode::Mercator, PixelConvention::Center);
        // Sous le plancher : no-data ; sans garde-fou (Mapterhorn) : lu tel quel.
        let tiles = all_tiles(|x, y| if (x, y) == (9, 9) { -60.0 } else { 100.0 });
        assert_eq!(
            TileGrid { zoom: Z, size: S, tiles: &tiles, plausibility: guard }.sample(&n),
            Err(SampleError::NoData)
        );
        let near = |r: Result<f64, SampleError>, v: f64| r.is_ok_and(|h| (h - v).abs() < 1e-6);
        assert!(near(TileGrid { zoom: Z, size: S, tiles: &tiles, plausibility: None }.sample(&n), -60.0));
        let low = all_tiles(|_, _| -40.0);
        assert!(
            near(TileGrid { zoom: Z, size: S, tiles: &low, plausibility: guard }.sample(&n), -40.0),
            "au-dessus du plancher"
        );
        // Pixel mêlé au-dessus du plancher, à deux pixels d'un no-data : rejeté ; sans no-data
        // dans la couronne (falaise) ou à trois pixels du carré `[9, 10]²`, gardé.
        let mixed = |nodata: (u64, u64)| {
            all_tiles(move |x, y| match (x, y) {
                (9, 9) => 30.0,
                q if q == nodata => f32::NAN,
                _ => 100.0,
            })
        };
        for (nodata, ok) in [
            ((11, 9), false),
            ((7, 7), false),
            ((12, 12), false),
            ((13, 9), true),
            ((6, 9), true),
            ((0, 0), true),
        ] {
            let tiles = mixed(nodata);
            let r = TileGrid { zoom: Z, size: S, tiles: &tiles, plausibility: guard }.sample(&n);
            assert_eq!(r.is_ok(), ok, "no-data en {nodata:?} : {r:?}");
        }
        // Écart sous le seuil, no-data voisin : gardé.
        let tiles = all_tiles(|x, y| match (x, y) {
            (9, 9) => 85.0,
            (11, 9) => f32::NAN,
            _ => 100.0,
        });
        assert!(near(TileGrid { zoom: Z, size: S, tiles: &tiles, plausibility: guard }.sample(&n), 85.0));
    }

    #[test]
    fn pixel_size_at_z12_and_z15() {
        let r = reader(MemStore::new(BTreeMap::new()), MemStore::new(BTreeMap::new()), 12);
        let px = r.pixel_size_m(LatLon::new(45.0, 6.0), TerrainOrigin::Mapterhorn);
        assert!((px - 13.5).abs() < 0.1, "{px}");
        let px = r.pixel_size_m(LatLon::new(45.0, 6.0), TerrainOrigin::IgnLidarHd);
        assert!((px - 3.38).abs() < 0.05, "{px}");
    }
}
