//! Géométrie partagée (T03 §3.1). La grille d'unités globales est reprise d'osm-surface
//! (`src/geo.rs`, Apache-2.0, © Gabriel Landais).

use crate::request::MERCATOR_MAX_LAT;
use crate::tiles::TileKey;
use crate::LatLon;

/// Circonférence équatoriale de la sphère Web Mercator (EPSG:3857), en mètres.
const MERCATOR_CIRCUMFERENCE_M: f64 = 40_075_016.685_578_5;

/// Rayon terrestre moyen (IUGG), en mètres.
pub const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// Distance orthodromique entre deux points, en mètres.
pub fn haversine_m(a: LatLon, b: LatLon) -> f64 {
    let (phi1, phi2) = (a.lat.to_radians(), b.lat.to_radians());
    let dphi = phi2 - phi1;
    let dlambda = (b.lon - a.lon).to_radians();
    let h = (dphi / 2.0).sin().powi(2) + phi1.cos() * phi2.cos() * (dlambda / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().min(1.0).asin()
}

/// Grille de tuiles en unités globales : `extent` unités par tuile au zoom `zoom`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    pub zoom: u8,
    pub extent: u32,
}

impl Grid {
    pub const fn new(zoom: u8, extent: u32) -> Self {
        Self { zoom, extent }
    }

    /// Côté du monde en unités globales.
    pub fn world(&self) -> f64 {
        (1u64 << self.zoom) as f64 * f64::from(self.extent)
    }

    /// Degrés → unités globales (x vers l'est, y vers le sud).
    pub fn to_units(&self, p: LatLon) -> [f64; 2] {
        let w = self.world();
        let lat = p.lat.clamp(-MERCATOR_MAX_LAT, MERCATOR_MAX_LAT).to_radians();
        let x = (p.lon + 180.0) / 360.0 * w;
        let y = (1.0 - lat.tan().asinh() / std::f64::consts::PI) / 2.0 * w;
        [x, y]
    }

    /// Unités globales → degrés.
    pub fn to_latlon(&self, p: [f64; 2]) -> LatLon {
        let w = self.world();
        let lon = p[0] / w * 360.0 - 180.0;
        let n = std::f64::consts::PI * (1.0 - 2.0 * p[1] / w);
        LatLon::new(n.sinh().atan().to_degrees(), lon)
    }

    /// Mètres par unité à une latitude (Mercator est conforme : même échelle en x et en y).
    pub fn metres_per_unit(&self, lat: f64) -> f64 {
        MERCATOR_CIRCUMFERENCE_M * lat.to_radians().cos() / self.world()
    }

    /// Tuile qui contient une position en unités globales.
    pub fn tile_of(&self, p: [f64; 2]) -> TileKey {
        let n = 1u32 << self.zoom;
        let e = f64::from(self.extent);
        let x = ((p[0] / e).floor().max(0.0) as u32).min(n - 1);
        let y = ((p[1] / e).floor().max(0.0) as u32).min(n - 1);
        TileKey { z: self.zoom, x, y }
    }

    /// Origine (coin nord-ouest) d'une tuile, en unités globales.
    pub fn tile_origin(&self, key: TileKey) -> [f64; 2] {
        let e = f64::from(self.extent);
        [f64::from(key.x) * e, f64::from(key.y) * e]
    }
}

/// Distance euclidienne en unités.
pub fn dist_units(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Point à la fraction `t` de `a` vers `b`.
pub fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// Projection orthogonale d'un point sur un segment, en coordonnées planes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projection {
    /// Position sur le segment, dans [0, 1].
    pub t: f64,
    pub point: [f64; 2],
    /// Carré de la distance du point à sa projection.
    pub dist2: f64,
}

pub fn project_on_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> Projection {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let point = [a[0] + t * dx, a[1] + t * dy];
    let (ex, ey) = (p[0] - point[0], p[1] - point[1]);
    Projection { t, point, dist2: ex * ex + ey * ey }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine_one_degree_of_meridian() {
        let d = haversine_m(LatLon::new(45.0, 3.0), LatLon::new(46.0, 3.0));
        assert!((d - EARTH_RADIUS_M.to_radians()).abs() < 1e-6, "{d}");
    }

    #[test]
    fn units_round_trip_and_scale() {
        let g = Grid::new(14, 4096);
        let p = g.to_units(LatLon::new(47.2057, -1.5457));
        let back = g.to_latlon(p);
        assert!((back.lon + 1.5457).abs() < 1e-9 && (back.lat - 47.2057).abs() < 1e-9);
        assert_eq!(g.tile_of(p), TileKey { z: 14, x: 8121, y: 5748 });
        // ≈ 0,406 m par unité à 47° N.
        assert!((g.metres_per_unit(47.2) - 0.4057).abs() < 0.001);
    }

    #[test]
    fn projection_is_clamped() {
        let p = project_on_segment([3.0, 1.0], [0.0, 0.0], [2.0, 0.0]);
        assert_eq!(p.t, 1.0);
        assert_eq!(p.point, [2.0, 0.0]);
        assert_eq!(p.dist2, 2.0);
        let q = project_on_segment([1.0, 1.0], [0.0, 0.0], [0.0, 0.0]);
        assert_eq!(q.t, 0.0);
    }
}
