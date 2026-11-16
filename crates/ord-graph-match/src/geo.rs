//! Géométrie plane locale : repère équirectangulaire en mètres, distances point-polyligne, Hausdorff.
//!
//! Utilisé par l'appariement. Le contrôle indépendant (`checks.rs`) n'en partage rien : il travaille en
//! haversine sur les degrés.

/// Rayon moyen de la Terre, en mètres.
pub const EARTH_R: f64 = 6_371_008.8;
/// Mètres par degré de latitude.
const M_PER_DEG: f64 = EARTH_R * std::f64::consts::PI / 180.0;

pub type P2 = [f64; 2];

/// Repère local : x vers l'est, y vers le nord, en mètres, origine en `(lat0, lon0)`.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    lat0: f64,
    lon0: f64,
    kx: f64,
}

impl Frame {
    pub fn new(lat0: f64, lon0: f64) -> Self {
        Self { lat0, lon0, kx: M_PER_DEG * lat0.to_radians().cos() }
    }

    pub fn xy(&self, lat: f64, lon: f64) -> P2 {
        [(lon - self.lon0) * self.kx, (lat - self.lat0) * M_PER_DEG]
    }

    pub fn xy_e7(&self, lat_e7: i32, lon_e7: i32) -> P2 {
        self.xy(f64::from(lat_e7) * 1e-7, f64::from(lon_e7) * 1e-7)
    }

    /// Inverse de [`Frame::xy`] : `(lat, lon)` en degrés.
    pub fn latlon(&self, p: P2) -> (f64, f64) {
        (self.lat0 + p[1] / M_PER_DEG, self.lon0 + p[0] / self.kx)
    }
}

/// Distance haversine en mètres entre deux `(lat, lon)` en degrés.
pub fn haversine(a: P2, b: P2) -> f64 {
    let (la, lb) = (a[0].to_radians(), b[0].to_radians());
    let dlat = lb - la;
    let dlon = (b[1] - a[1]).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la.cos() * lb.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_R * h.sqrt().min(1.0).asin()
}

/// Décode une polyligne Google de précision 6 en `(lat, lon)` degrés ; `None` si elle est illisible.
pub fn decode_polyline6(s: &str) -> Option<Vec<P2>> {
    let b = s.as_bytes();
    let (mut i, mut lat, mut lon) = (0usize, 0i64, 0i64);
    let mut out = Vec::with_capacity(b.len() / 6);
    let next = |i: &mut usize| -> Option<i64> {
        let (mut result, mut shift) = (0i64, 0u32);
        loop {
            let c = i64::from(*b.get(*i)?) - 63;
            *i += 1;
            if !(0..64).contains(&c) || shift > 60 {
                return None;
            }
            result |= (c & 0x1f) << shift;
            shift += 5;
            if c < 0x20 {
                break;
            }
        }
        Some(if result & 1 != 0 { !(result >> 1) } else { result >> 1 })
    };
    while i < b.len() {
        lat += next(&mut i)?;
        lon += next(&mut i)?;
        let p = [lat as f64 * 1e-6, lon as f64 * 1e-6];
        if !(-90.0..=90.0).contains(&p[0]) || !(-180.0..=180.0).contains(&p[1]) {
            return None;
        }
        out.push(p);
    }
    Some(out)
}

pub fn dist(a: P2, b: P2) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Distance d'un point à un segment, et abscisse (en mètres depuis `a`) du pied.
pub fn seg_project(p: P2, a: P2, b: P2) -> (f64, f64) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let foot = [a[0] + t * dx, a[1] + t * dy];
    (dist(p, foot), t * l2.sqrt())
}

pub fn dist_to_polyline(p: P2, line: &[P2]) -> f64 {
    match line {
        [] => f64::INFINITY,
        [a] => dist(p, *a),
        _ => line.windows(2).map(|w| seg_project(p, w[0], w[1]).0).fold(f64::INFINITY, f64::min),
    }
}

pub fn polyline_len(line: &[P2]) -> f64 {
    line.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// Points de `line` : sommets et points intermédiaires tous les `step` mètres au plus.
pub fn densify(line: &[P2], step: f64, mut f: impl FnMut(P2)) {
    let Some(&first) = line.first() else { return };
    f(first);
    for w in line.windows(2) {
        let l = dist(w[0], w[1]);
        let n = (l / step).ceil() as usize;
        for k in 1..=n.max(1) {
            let t = k as f64 / n.max(1) as f64;
            f([w[0][0] + t * (w[1][0] - w[0][0]), w[0][1] + t * (w[1][1] - w[0][1])]);
        }
    }
}

/// Hausdorff dirigé `a → b` : plus grande distance d'un point de `a` (échantillonné au pas `step`) à `b`.
/// S'arrête dès que `cap` est dépassé : la valeur rendue est alors un minorant supérieur à `cap`.
pub fn directed_hausdorff(a: &[P2], b: &[P2], step: f64, cap: f64) -> f64 {
    let mut worst = 0.0f64;
    let mut over = false;
    densify(a, step, |p| {
        if !over {
            worst = worst.max(dist_to_polyline(p, b));
            over = worst > cap;
        }
    });
    worst
}

/// Hausdorff exact à `step` près, rendu tel quel ou, s'il dépasse `cap`, un minorant plus grand que `cap`.
pub fn hausdorff(a: &[P2], b: &[P2], step: f64, cap: f64) -> f64 {
    let h1 = directed_hausdorff(a, b, step, cap);
    if h1 > cap {
        return h1;
    }
    h1.max(directed_hausdorff(b, a, step, cap))
}

/// Encodeur de polyligne de précision 6 (tests seulement : l'outil ne fait que décoder).
pub fn encode_polyline6(points: &[P2]) -> String {
    let (mut out, mut plat, mut plon) = (String::new(), 0i64, 0i64);
    for p in points {
        let (la, lo) = ((p[0] * 1e6).round() as i64, (p[1] * 1e6).round() as i64);
        for v in [la - plat, lo - plon] {
            let mut v = if v < 0 { !(v << 1) } else { v << 1 };
            while v >= 0x20 {
                out.push((((v & 0x1f) | 0x20) as u8 + 63) as char);
                v >>= 5;
            }
            out.push((v as u8 + 63) as char);
        }
        (plat, plon) = (la, lo);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polyline_round_trip_and_garbage() {
        let pts = [[47.218371, -1.553621], [47.2190, -1.5521], [-33.5, 151.25]];
        let back = decode_polyline6(&encode_polyline6(&pts)).unwrap();
        assert_eq!(back.len(), 3);
        for (b, p) in back.iter().zip(pts) {
            assert!((b[0] - p[0]).abs() < 1e-6 && (b[1] - p[1]).abs() < 1e-6);
        }
        assert!(decode_polyline6("\u{7f}").is_none());
        assert_eq!(decode_polyline6("").unwrap().len(), 0);
        assert!(decode_polyline6("_p~iF").is_none(), "latitude sans longitude");
    }

    #[test]
    fn projection_and_distance() {
        let (d, s) = seg_project([5.0, 3.0], [0.0, 0.0], [10.0, 0.0]);
        assert!((d - 3.0).abs() < 1e-12 && (s - 5.0).abs() < 1e-12);
        let (d, s) = seg_project([-4.0, 3.0], [0.0, 0.0], [10.0, 0.0]);
        assert!((d - 5.0).abs() < 1e-12 && s == 0.0);
        let line = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        assert!((dist_to_polyline([12.0, 5.0], &line) - 2.0).abs() < 1e-12);
        assert!((polyline_len(&line) - 20.0).abs() < 1e-12);
    }

    #[test]
    fn hausdorff_of_known_shapes() {
        let a = [[0.0, 0.0], [100.0, 0.0]];
        let b = [[0.0, 2.0], [100.0, 2.0]];
        assert!((hausdorff(&a, &b, 0.25, 10.0) - 2.0).abs() < 1e-9);
        // Un bout plus long : la plus grande distance est celle du bout dépassant.
        let c = [[0.0, 0.0], [100.0, 0.0], [110.0, 0.0]];
        assert!((hausdorff(&a, &c, 0.25, 50.0) - 10.0).abs() < 1e-9);
        // Coupure : au-delà du plafond, un minorant plus grand que lui.
        assert!(hausdorff(&a, &c, 0.25, 3.0) > 3.0);
    }

    #[test]
    fn frame_is_metric_near_nantes() {
        let f = Frame::new(47.2, -1.55);
        let p = f.xy(47.2, -1.55 + 0.001);
        assert!((p[0] - 75.5).abs() < 0.5, "{p:?}");
        let h = haversine([47.2, -1.55], [47.2, -1.549]);
        assert!((h - p[0]).abs() < 0.2);
    }
}
