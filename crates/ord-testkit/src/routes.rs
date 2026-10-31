//! Parcours synthétiques, sans aucune trace réelle.

use ord_domain::geo::EARTH_RADIUS_M;
use ord_domain::LatLon;
use serde_json::{json, Value};

/// Point de départ des parcours synthétiques (Nantes).
pub const START: LatLon = LatLon::new(47.2184, -1.5536);

/// Parcours vers le nord le long d'un méridien, `n` points également espacés (n ≥ 2), de
/// longueur `length_m` (exacte au sens de `haversine_m`).
pub fn meridian(start: LatLon, length_m: f64, n: usize) -> Vec<LatLon> {
    assert!(n >= 2, "un parcours a au moins deux points");
    let dlat = (length_m / EARTH_RADIUS_M).to_degrees();
    (0..n).map(|i| LatLon::new(start.lat + dlat * i as f64 / (n - 1) as f64, start.lon)).collect()
}

/// Corps JSON `{"points": [...]}` d'une requête, options éventuelles fusionnées.
pub fn request_body(points: &[LatLon], options: Option<Value>) -> Value {
    let pts: Vec<Value> = points.iter().map(|p| json!({ "lat": p.lat, "lon": p.lon })).collect();
    let mut body = json!({ "points": pts });
    if let Some(options) = options {
        body["options"] = options;
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use ord_domain::geo::haversine_m;

    #[test]
    fn meridian_has_requested_length() {
        let pts = meridian(START, 201_000.0, 202);
        let total: f64 = pts.windows(2).map(|w| haversine_m(w[0], w[1])).sum();
        assert!((total - 201_000.0).abs() < 1e-3, "{total}");
    }
}
