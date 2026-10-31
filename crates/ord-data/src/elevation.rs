//! Lecture du terrain (T02 §6.2–6.3), limitée à Mapterhorn à l'incrément 1 : origine `mapterhorn`,
//! pas de repli IGN, pas de référentiel de couverture (zone France et GLO-30 : incrément 2).
//!
//! Bilinéaire au **centre** du pixel : un pixel `i` vaut pour `i + ½` (vcyclist,
//! `ElevationCalculator.kt`). Lecture en trois phases : planifier les tuiles touchées (voisines
//! comprises), les charger par `get_many`, échantillonner de façon synchrone.

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::sync::Arc;

use ord_domain::ctx::RequestCtx;
use ord_domain::elevation::{DemKind, DsmSample, Missing, TerrainOrigin, TerrainReader, TerrainSample};
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

/// Position en pixels globaux (Web Mercator) au zoom `z`, tuiles de `size` pixels.
pub fn global_pixel(p: LatLon, zoom: u8, size: u32) -> (f64, f64) {
    let n = f64::from(size) * f64::from(1u32 << zoom);
    let x = (p.lon + 180.0) / 360.0 * n;
    let lat = p.lat.to_radians();
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / PI) / 2.0 * n;
    (x, y)
}

/// Les quatre voisins bilinéaires d'un point : pixels globaux et poids `(dx, dy)`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Neighbours {
    px: [(u64, u64); 4],
    dx: f64,
    dy: f64,
}

fn neighbours(p: LatLon, zoom: u8, size: u32, convention: PixelConvention) -> Neighbours {
    let (x, y) = global_pixel(p, zoom, size);
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

/// Échantillonneur synchrone sur des tuiles déjà chargées.
#[derive(Debug)]
struct TileGrid<'a> {
    pub zoom: u8,
    pub size: u32,
    pub tiles: &'a BTreeMap<TileKey, TileOutcome<ElevationTile>>,
}

impl TileGrid<'_> {
    fn pixel(&self, (gx, gy): (u64, u64)) -> Pixel {
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

    /// Bilinéaire ; un voisin no-data, même de poids nul, rend le point no-data (T02 §6.2).
    fn sample(&self, n: &Neighbours) -> Result<f64, Missing> {
        let mut v = [0.0; 4];
        let mut absent = false;
        for (slot, &px) in v.iter_mut().zip(&n.px) {
            match self.pixel(px) {
                Pixel::Value(x) => *slot = x,
                Pixel::Absent => absent = true,
                Pixel::Failed => return Err(Missing::Failed),
            }
        }
        if absent {
            return Err(Missing::NoData);
        }
        let top = v[0] * (1.0 - n.dx) + v[1] * n.dx;
        let bottom = v[2] * (1.0 - n.dx) + v[3] * n.dx;
        let h = top * (1.0 - n.dy) + bottom * n.dy;
        if h.is_nan() {
            Err(Missing::NoData)
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

/// Lecteur de terrain Mapterhorn seul (Terrarium, `tile_size` pixels au zoom `zoom`).
pub struct MapterhornTerrainReader {
    store: Arc<dyn TileStore<ElevationTile>>,
    zoom: u8,
    tile_size: u32,
    convention: PixelConvention,
}

impl std::fmt::Debug for MapterhornTerrainReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MapterhornTerrainReader")
            .field("zoom", &self.zoom)
            .field("tile_size", &self.tile_size)
            .field("convention", &self.convention)
            .finish_non_exhaustive()
    }
}

impl MapterhornTerrainReader {
    pub fn new(
        store: Arc<dyn TileStore<ElevationTile>>,
        zoom: u8,
        tile_size: u32,
        convention: PixelConvention,
    ) -> Self {
        Self { store, zoom, tile_size, convention }
    }
}

#[async_trait::async_trait]
impl TerrainReader for MapterhornTerrainReader {
    async fn sample_terrain(&self, pts: &[LatLon], ctx: &RequestCtx) -> Vec<TerrainSample> {
        let neighbours: Vec<Neighbours> =
            pts.iter().map(|&p| neighbours(p, self.zoom, self.tile_size, self.convention)).collect();
        let keys = plan(&neighbours, self.zoom, self.tile_size);
        let outcomes = self.store.get_many(&keys, ctx).await;
        let tiles: BTreeMap<TileKey, TileOutcome<ElevationTile>> = keys.into_iter().zip(outcomes).collect();
        let grid = TileGrid { zoom: self.zoom, size: self.tile_size, tiles: &tiles };
        neighbours
            .iter()
            .map(|n| match grid.sample(n) {
                Ok(h) => TerrainSample {
                    ele: Some(h as f32),
                    origin: TerrainOrigin::Mapterhorn,
                    fallback_after_error: false,
                    dem_kind: DemKind::Unknown,
                    missing: None,
                },
                Err(missing) => TerrainSample {
                    ele: None,
                    origin: TerrainOrigin::None,
                    fallback_after_error: false,
                    dem_kind: DemKind::Unknown,
                    missing: Some(missing),
                },
            })
            .collect()
    }

    async fn sample_ign_dtm(&self, pts: &[LatLon], _ctx: &RequestCtx) -> Vec<Option<f32>> {
        // IGN non configuré à l'incrément 1.
        vec![None; pts.len()]
    }

    async fn sample_dsm(&self, pts: &[LatLon], _ctx: &RequestCtx) -> Vec<DsmSample> {
        vec![DsmSample::NoData; pts.len()]
    }

    fn pixel_size_m(&self, p: LatLon, _origin: TerrainOrigin) -> f64 {
        MERCATOR_CIRCUMFERENCE_M * p.lat.to_radians().cos()
            / (f64::from(self.tile_size) * f64::from(1u32 << self.zoom))
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
        let grid = TileGrid { zoom: Z, size: S, tiles: &tiles };
        for (gx, gy) in [(5, 9), (12, 3), (4, 4), (7, 8)] {
            let p = pixel_center(gx, gy, Z, S);
            let n = neighbours(p, Z, S, PixelConvention::Center);
            let h = grid.sample(&n).unwrap();
            assert!((h - (gx * 100 + gy) as f64).abs() < 1e-6, "({gx}, {gy}) : {h}");
        }
    }

    #[test]
    fn continuity_across_a_tile_edge_and_plane_reproduced() {
        // Plan en pixels globaux : la bilinéaire au centre le reproduit sans décalage.
        let tiles = all_tiles(|x, y| (2.0 * x as f64 + 0.5 * y as f64) as f32);
        let grid = TileGrid { zoom: Z, size: S, tiles: &tiles };
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
        let grid = TileGrid { zoom: Z, size: S, tiles: &tiles };
        let p = pixel_center(10, 10, Z, S);
        let centre = grid.sample(&neighbours(p, Z, S, PixelConvention::Center)).unwrap();
        let corner = grid.sample(&neighbours(p, Z, S, PixelConvention::Corner)).unwrap();
        assert!((centre - 10.0).abs() < 1e-9);
        assert!((corner - 10.5).abs() < 1e-9);
    }

    #[test]
    fn nodata_failure_and_absence() {
        let mut tiles = all_tiles(|x, y| if (x, y) == (9, 9) { f32::NAN } else { 1.0 });
        let grid = TileGrid { zoom: Z, size: S, tiles: &tiles };
        // Centre exact du pixel (8, 9) : le voisin (9, 9) a un poids nul mais est no-data.
        let n = neighbours(pixel_center(8, 9, Z, S), Z, S, PixelConvention::Center);
        assert_eq!(grid.sample(&n), Err(Missing::NoData));
        let k = TileKey { z: Z, x: 0, y: 0 };
        tiles.insert(k, TileOutcome::Absent);
        let n0 = neighbours(pixel_center(1, 1, Z, S), Z, S, PixelConvention::Center);
        assert_eq!(TileGrid { zoom: Z, size: S, tiles: &tiles }.sample(&n0), Err(Missing::NoData));
        tiles.insert(
            k,
            TileOutcome::Failed(TileFailure {
                source: SourceId::Mapterhorn,
                tile: k,
                kind: FailureKind::Server,
                status: Some(500),
                retry_after: None,
                message: String::new(),
            }),
        );
        assert_eq!(TileGrid { zoom: Z, size: S, tiles: &tiles }.sample(&n0), Err(Missing::Failed));
    }

    #[test]
    fn plan_includes_neighbour_tiles_and_world_edges() {
        // Moitié ouest du pixel 4, premier pixel de la tuile x = 1 : le voisin ouest est dans la tuile 0.
        let near_edge = neighbours(latlon_of_global(4.2, 5.5, Z, S), Z, S, PixelConvention::Center);
        let keys = plan(&[near_edge], Z, S);
        assert_eq!(keys.iter().map(|k| k.x).collect::<Vec<_>>(), [0, 1]);
        let west = neighbours(LatLon::new(0.0, -179.99999), Z, S, PixelConvention::Center);
        assert!(plan(&[west], Z, S).iter().any(|k| k.x == (1 << Z) - 1), "x boucle à l'antiméridien");
        let north = neighbours(LatLon::new(85.05, 0.0), Z, S, PixelConvention::Center);
        assert!(plan(&[north], Z, S).iter().all(|k| k.y == 0), "y bloqué au bord du monde");
    }

    #[test]
    fn pixel_size_at_z12() {
        struct NoStore;
        #[async_trait::async_trait]
        impl TileStore<ElevationTile> for NoStore {
            async fn get_many(&self, _: &[TileKey], _: &RequestCtx) -> Vec<TileOutcome<ElevationTile>> {
                Vec::new()
            }
            fn data_version(&self) -> ord_domain::catalog::DataVersion {
                unreachable!()
            }
        }
        let r = MapterhornTerrainReader::new(Arc::new(NoStore), 12, 512, PixelConvention::Center);
        let px = r.pixel_size_m(LatLon::new(45.0, 6.0), TerrainOrigin::Mapterhorn);
        assert!((px - 13.5).abs() < 0.1, "{px}");
    }
}
