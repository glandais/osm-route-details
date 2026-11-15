//! Géométrie du contrôleur : écrite ici, sans rien emprunter au moteur ni à `ord-graph`.
//!
//! Les points sont en degrés `(lat, lon)` ; les calculs d'angle et de distance courte se font dans un repère
//! équirectangulaire local (`Proj`, mètres est `x` et nord `y`).

pub const R_EARTH_M: f64 = 6_371_008.8;

pub type LatLon = (f64, f64);

/// Distance haversine, en mètres.
pub fn haversine_m(a: LatLon, b: LatLon) -> f64 {
    let (la1, la2) = (a.0.to_radians(), b.0.to_radians());
    let dlat = la2 - la1;
    let dlon = (b.1 - a.1).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R_EARTH_M * h.sqrt().min(1.0).asin()
}

/// Repère équirectangulaire autour d'un point.
#[derive(Clone, Copy, Debug)]
pub struct Proj {
    lat0: f64,
    lon0: f64,
    kx: f64,
    ky: f64,
}

impl Proj {
    pub fn new(origin: LatLon) -> Self {
        let ky = R_EARTH_M.to_radians();
        Self { lat0: origin.0, lon0: origin.1, kx: ky * origin.0.to_radians().cos(), ky }
    }

    pub fn xy(&self, p: LatLon) -> (f64, f64) {
        ((p.1 - self.lon0) * self.kx, (p.0 - self.lat0) * self.ky)
    }
}

pub fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

pub fn poly_len(pts: &[(f64, f64)]) -> f64 {
    pts.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// Longueurs cumulées haversine d'une polyligne `(lat, lon)` : `cum[i]` = abscisse du point `i`.
pub fn cumulative_m(pts: &[LatLon]) -> Vec<f64> {
    let mut cum = Vec::with_capacity(pts.len());
    let mut acc = 0.0;
    for (i, p) in pts.iter().enumerate() {
        if i > 0 {
            acc += haversine_m(pts[i - 1], *p);
        }
        cum.push(acc);
    }
    cum
}

/// Point d'abscisse `s` (mètres haversine) sur une polyligne `(lat, lon)` de cumul `cum`.
pub fn point_at(pts: &[LatLon], cum: &[f64], s: f64) -> LatLon {
    let total = *cum.last().unwrap_or(&0.0);
    if pts.is_empty() {
        return (0.0, 0.0);
    }
    if s <= 0.0 || pts.len() == 1 {
        return pts[0];
    }
    if s >= total {
        return pts[pts.len() - 1];
    }
    let i = cum.partition_point(|&c| c <= s).clamp(1, pts.len() - 1);
    let (c0, c1) = (cum[i - 1], cum[i]);
    let t = if c1 > c0 { (s - c0) / (c1 - c0) } else { 0.0 };
    (pts[i - 1].0 + t * (pts[i].0 - pts[i - 1].0), pts[i - 1].1 + t * (pts[i].1 - pts[i - 1].1))
}

/// Sous-polyligne entre les abscisses `lo <= hi`, dans le sens de l'abscisse croissante.
pub fn clip(pts: &[LatLon], cum: &[f64], lo: f64, hi: f64) -> Vec<LatLon> {
    let mut out = vec![point_at(pts, cum, lo)];
    for (i, p) in pts.iter().enumerate() {
        if cum[i] > lo && cum[i] < hi {
            out.push(*p);
        }
    }
    out.push(point_at(pts, cum, hi));
    out
}

/// Point d'abscisse `offset` sur une polyligne `(x, y)`.
pub fn along(pts: &[(f64, f64)], offset: f64) -> (f64, f64) {
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let seg = dist(w[0], w[1]);
        if acc + seg >= offset && seg > 0.0 {
            let t = (offset - acc) / seg;
            return (w[0].0 + t * (w[1].0 - w[0].0), w[0].1 + t * (w[1].1 - w[0].1));
        }
        acc += seg;
    }
    *pts.last().unwrap_or(&(0.0, 0.0))
}

/// Cap en degrés `[0, 360)`, 0 = nord, sens horaire, d'un vecteur `(dx est, dy nord)`.
pub fn bearing_deg(dx: f64, dy: f64) -> f64 {
    let d = dx.atan2(dy).to_degrees();
    if d < 0.0 {
        d + 360.0
    } else {
        d
    }
}

/// Angle de virage, de 0 (tout droit) à 180 (demi-tour).
pub fn turn_angle_deg(h_in: f64, h_out: f64) -> f64 {
    let d = (h_out - h_in).rem_euclid(360.0);
    if d > 180.0 {
        360.0 - d
    } else {
        d
    }
}

/// Cap d'un chemin à son début ou à sa fin, sur la fenêtre `[near, far]` mètres (`RouteGuarantees.pathHeading`) :
/// `far = min(far_m, longueur)`, `near = min(near_m, far / 2)` ; au début la fenêtre est `[near, far]`, à la
/// fin `[total - far, total - near]` ; deux points à moins de 0,5 m : cap de la corde entière.
pub fn path_heading(pts: &[(f64, f64)], at_start: bool, near_m: f64, far_m: f64) -> f64 {
    let total = poly_len(pts);
    let far = far_m.min(total);
    let near = near_m.min(far / 2.0);
    let (from, to) = if at_start { (near, far) } else { (total - far, total - near) };
    let mut a = along(pts, from);
    let mut b = along(pts, to);
    if dist(a, b) < 0.5 {
        a = pts[0];
        b = pts[pts.len() - 1];
    }
    bearing_deg(b.0 - a.0, b.1 - a.1)
}

/// Composante, transversale à un cap, d'un vecteur `(dx, dy)`.
pub fn across_m(dx: f64, dy: f64, heading_deg: f64) -> f64 {
    let h = heading_deg.to_radians();
    (dx * h.cos() - dy * h.sin()).abs()
}

/// Un segment de direction `seg_deg` est-il à 90 ± `tol` degrés d'un cap (modulo 180) ?
pub fn is_across_deg(seg_deg: f64, heading_deg: f64, tol: f64) -> bool {
    let d = (seg_deg - heading_deg).rem_euclid(180.0);
    (d - 90.0).abs() <= tol
}

/// Réglages de la recherche d'une portion droite « connecteur » (`RouteGuarantees.hasConnectorRun`).
#[derive(Clone, Copy, Debug)]
pub struct RunRule {
    pub min_m: f64,
    pub tol_deg: f64,
    pub bend_deg: f64,
    pub min_piece_m: f64,
}

/// Y a-t-il, dans la polyligne `(x, y)`, une portion droite d'au moins `min_m` à 90 ± `tol` d'un des deux caps ?
/// Les points à moins de `min_piece_m` du dernier gardé sont fondus ; les pièces qui s'écartent d'au plus
/// `bend_deg` du cap où la portion a commencé sont de la même portion.
pub fn has_connector_run(raw: &[(f64, f64)], hin: f64, hout: f64, r: &RunRule) -> bool {
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for &q in raw {
        if pts.last().is_none_or(|&l| dist(l, q) >= r.min_piece_m) {
            pts.push(q);
        }
    }
    let mut a = 0;
    while a + 1 < pts.len() {
        let from = pts[a];
        let first = pts[a + 1];
        let run = bearing_deg(first.0 - from.0, first.1 - from.1);
        let mut b = a + 1;
        while b + 1 < pts.len() {
            let (p0, p1) = (pts[b], pts[b + 1]);
            let next = bearing_deg(p1.0 - p0.0, p1.1 - p0.1);
            if turn_angle_deg(run, next) > r.bend_deg {
                break;
            }
            b += 1;
        }
        let to = pts[b];
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        if dx.hypot(dy) >= r.min_m - 0.5 {
            let seg = bearing_deg(dx, dy);
            if is_across_deg(seg, hin, r.tol_deg) || is_across_deg(seg, hout, r.tol_deg) {
                return true;
            }
        }
        a = b;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_angle_is_symmetric_and_bounded() {
        assert!((turn_angle_deg(10.0, 350.0) - 20.0).abs() < 1e-9);
        assert!((turn_angle_deg(0.0, 180.0) - 180.0).abs() < 1e-9);
        assert!((turn_angle_deg(90.0, 90.0)).abs() < 1e-9);
    }

    #[test]
    fn haversine_one_degree_of_latitude() {
        let d = haversine_m((47.0, -1.0), (48.0, -1.0));
        assert!((d - 111_194.9).abs() < 1.0, "{d}");
    }

    #[test]
    fn heading_window_of_a_long_straight_path() {
        let north = [(0.0, 0.0), (0.0, 100.0)];
        assert!((path_heading(&north, true, 30.0, 50.0)).abs() < 1e-9);
        let east = [(0.0, 0.0), (100.0, 0.0)];
        assert!((path_heading(&east, false, 30.0, 50.0) - 90.0).abs() < 1e-9);
    }

    #[test]
    fn clip_interpolates_inside_a_segment() {
        let pts = [(47.0, -1.0), (47.001, -1.0)];
        let cum = cumulative_m(&pts);
        let c = clip(&pts, &cum, cum[1] * 0.25, cum[1] * 0.75);
        assert_eq!(c.len(), 2);
        assert!((c[0].0 - 47.00025).abs() < 1e-9);
        assert!((c[1].0 - 47.00075).abs() < 1e-9);
    }

    #[test]
    fn connector_run_across_the_heading_is_found() {
        let rule = RunRule { min_m: 15.0, tol_deg: 25.0, bend_deg: 20.0, min_piece_m: 1.0 };
        // 20 m vers l'est, cap nord de part et d'autre : portion droite à 90 degrés
        let across = [(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)];
        assert!(has_connector_run(&across, 0.0, 180.0, &rule));
        // 20 m vers le nord, dans l'axe : pas un connecteur
        let along_it = [(0.0, 0.0), (0.0, 10.0), (0.0, 20.0)];
        assert!(!has_connector_run(&along_it, 0.0, 180.0, &rule));
        // 10 m seulement : trop court
        let short = [(0.0, 0.0), (10.0, 0.0)];
        assert!(!has_connector_run(&short, 0.0, 180.0, &rule));
    }
}
