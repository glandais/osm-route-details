//! Géométrie partagée (T03 §3.1).

use crate::LatLon;

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
    fn projection_is_clamped() {
        let p = project_on_segment([3.0, 1.0], [0.0, 0.0], [2.0, 0.0]);
        assert_eq!(p.t, 1.0);
        assert_eq!(p.point, [2.0, 0.0]);
        assert_eq!(p.dist2, 2.0);
        let q = project_on_segment([1.0, 1.0], [0.0, 0.0], [0.0, 0.0]);
        assert_eq!(q.t, 0.0);
    }
}
