//! Géométrie locale : positions en degrés E7, projection équirectangulaire, caps et angles.

pub const R_EARTH_M: f64 = 6_371_008.8;
/// Mètres par degré de latitude (sphère de `R_EARTH_M`).
pub const M_PER_DEG: f64 = R_EARTH_M * std::f64::consts::PI / 180.0;

/// Un point en degrés décimaux.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

impl LatLon {
    pub fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }

    pub fn to_e7(self) -> (i32, i32) {
        ((self.lat * 1e7).round() as i32, (self.lon * 1e7).round() as i32)
    }

    pub fn from_e7(p: (i32, i32)) -> Self {
        Self { lat: f64::from(p.0) * 1e-7, lon: f64::from(p.1) * 1e-7 }
    }
}

/// Distance haversine entre deux points E7, en mètres.
pub fn haversine_m(a: (i32, i32), b: (i32, i32)) -> f64 {
    let (la1, la2) = ((f64::from(a.0) * 1e-7).to_radians(), (f64::from(b.0) * 1e-7).to_radians());
    let dlat = la2 - la1;
    let dlon = (f64::from(b.1 - a.1) * 1e-7).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R_EARTH_M * h.sqrt().min(1.0).asin()
}

/// Longueur haversine d'une polyligne E7.
pub fn polyline_m(pts: &[(i32, i32)]) -> f64 {
    pts.windows(2).map(|w| haversine_m(w[0], w[1])).sum()
}

/// Projection équirectangulaire autour d'un point : mètres est (`x`) et nord (`y`).
#[derive(Clone, Copy, Debug)]
pub struct Local {
    lat0: i32,
    lon0: i32,
    kx: f64,
    ky: f64,
}

impl Local {
    pub fn new(origin: (i32, i32)) -> Self {
        let ky = M_PER_DEG * 1e-7;
        let kx = ky * (f64::from(origin.0) * 1e-7).to_radians().cos();
        Self { lat0: origin.0, lon0: origin.1, kx, ky }
    }

    pub fn xy(&self, p: (i32, i32)) -> (f64, f64) {
        (f64::from(p.1 - self.lon0) * self.kx, f64::from(p.0 - self.lat0) * self.ky)
    }
}

/// Cap `[0, 360)` du vecteur `(dx, dy)` (0 = nord, sens horaire).
pub fn heading_deg(dx: f64, dy: f64) -> f64 {
    let d = dx.atan2(dy).to_degrees();
    if d < 0.0 {
        d + 360.0
    } else {
        d
    }
}

/// Angle de virage entre deux caps, `[0, 180]` : 0 tout droit, 180 demi-tour.
pub fn turn_angle_deg(h_in: f64, h_out: f64) -> f64 {
    ((h_out - h_in + 540.0).rem_euclid(360.0) - 180.0).abs()
}

/// Distance d'un point à un segment, et abscisse du pied (0..1) sur le segment ; repère local en mètres.
pub fn point_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 <= 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
    let (qx, qy) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - qx).hypot(p.1 - qy), t)
}
