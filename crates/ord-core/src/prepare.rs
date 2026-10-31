//! Préparation du parcours (T01 §5.1.1) : doublons consécutifs, distance cumulée, stations.

use ord_domain::geo::haversine_m;
use ord_domain::route::{PreparedRoute, RouteSample};
use ord_domain::LatLon;

/// Deux points consécutifs plus proches que ce seuil, en degrés sur chaque axe, sont un doublon
/// (≈ 1 cm ; même règle que la validation des bornes).
pub const DUPLICATE_EPS_DEG: f64 = 1e-7;

/// Une station uniforme à moins de cette distance d'un point d'entrée se confond avec lui.
pub const MERGE_EPS_M: f64 = 0.01;

/// Prépare le parcours : `points` dans l'ordre du client, `step_m` le pas des stations uniformes.
///
/// Les stations uniformes sont à `k × step_m` ; chaque point d'entrée dédoublonné est aussi une
/// station. Un parcours sans point donne un parcours vide (la validation l'a refusé avant).
pub fn prepare(points: &[LatLon], step_m: f64) -> PreparedRoute {
    // 1. Doublons consécutifs : le premier garde son indice client.
    let mut kept: Vec<(u32, LatLon)> = Vec::with_capacity(points.len());
    for (i, p) in points.iter().enumerate() {
        let duplicate = kept.last().is_some_and(|(_, q)| {
            (p.lat - q.lat).abs() <= DUPLICATE_EPS_DEG && (p.lon - q.lon).abs() <= DUPLICATE_EPS_DEG
        });
        if !duplicate {
            kept.push((i as u32, *p));
        }
    }
    let Some(&(first_index, first)) = kept.first() else {
        return PreparedRoute { samples: Vec::new(), input_pos: Vec::new(), length_m: 0.0 };
    };

    // 2. Distance cumulée, sommée dans l'ordre.
    let mut cum = Vec::with_capacity(kept.len());
    let mut total = 0.0;
    cum.push(0.0);
    for w in kept.windows(2) {
        total += haversine_m(w[0].1, w[1].1);
        cum.push(total);
    }

    // 3. Stations : chaque point d'entrée, puis les stations uniformes strictement entre lui et
    // le suivant (à plus de 1 cm de l'un et de l'autre).
    let step = if step_m.is_finite() && step_m > 0.0 { step_m } else { f64::INFINITY };
    let estimate = kept.len() + if step.is_finite() { (total / step) as usize + 1 } else { 0 };
    let mut samples = Vec::with_capacity(estimate);
    let mut input_pos = Vec::with_capacity(kept.len());
    let mut k: u64 = 1;
    samples.push(RouteSample {
        at: first,
        d_m: 0.0,
        input_index: Some(first_index),
        prev_input: first_index,
    });
    input_pos.push(0);
    for (seg, w) in kept.windows(2).enumerate() {
        let (a_index, a) = w[0];
        let (b_index, b) = w[1];
        let (d0, d1) = (cum[seg], cum[seg + 1]);
        let len = d1 - d0;
        while step.is_finite() && (k as f64) * step < d1 - MERGE_EPS_M {
            let s = k as f64 * step;
            k += 1;
            if s <= d0 + MERGE_EPS_M {
                continue;
            }
            let t = if len > 0.0 { (s - d0) / len } else { 0.0 };
            let at = LatLon::new(a.lat + t * (b.lat - a.lat), a.lon + t * (b.lon - a.lon));
            samples.push(RouteSample { at, d_m: s, input_index: None, prev_input: a_index });
        }
        // La station uniforme qui tombe à moins de 1 cm de `b` se confond avec lui.
        while step.is_finite() && (k as f64) * step <= d1 + MERGE_EPS_M {
            k += 1;
        }
        input_pos.push(samples.len() as u32);
        samples.push(RouteSample { at: b, d_m: d1, input_index: Some(b_index), prev_input: b_index });
    }
    PreparedRoute { samples, input_pos, length_m: total }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ord_domain::geo::EARTH_RADIUS_M;

    fn north(start: LatLon, m: f64) -> LatLon {
        LatLon::new(start.lat + (m / EARTH_RADIUS_M).to_degrees(), start.lon)
    }

    const O: LatLon = LatLon::new(45.0, 6.0);

    #[test]
    fn consecutive_duplicates_are_merged_and_keep_client_indices() {
        let b = north(O, 12.0);
        let r = prepare(&[O, O, b, b, O], 5.0);
        let inputs: Vec<u32> =
            r.input_pos.iter().map(|&i| r.samples[i as usize].input_index.unwrap()).collect();
        assert_eq!(inputs, [0, 2, 4], "doublons consécutifs fusionnés, non consécutifs gardés");
        assert!((r.length_m - 24.0).abs() < 1e-6);
    }

    #[test]
    fn stations_every_step_plus_inputs() {
        let b = north(O, 12.0);
        let r = prepare(&[O, b], 5.0);
        let d: Vec<f64> = r.samples.iter().map(|s| s.d_m).collect();
        assert_eq!(d.len(), 4);
        assert!((d[1] - 5.0).abs() < 1e-9 && (d[2] - 10.0).abs() < 1e-9);
        assert!((d[3] - 12.0).abs() < 1e-6);
        assert!(r.samples.windows(2).all(|w| w[1].d_m > w[0].d_m), "distances croissantes");
        assert_eq!(r.samples[2].prev_input, 0);
        assert_eq!(r.samples[3].input_index, Some(1));
    }

    #[test]
    fn uniform_station_within_one_centimetre_merges_with_input() {
        let b = north(O, 10.004);
        let r = prepare(&[O, b], 5.0);
        assert_eq!(r.samples.len(), 3, "la station à 10 m se confond avec le point à 10,004 m");
        let c = north(O, 0.02);
        let r = prepare(&[O, c, north(O, 7.0)], 5.0);
        assert_eq!(r.samples.len(), 4);
    }

    #[test]
    fn zero_and_tiny_segments() {
        let r = prepare(&[O, north(O, 0.02), north(O, 0.04)], 5.0);
        assert_eq!(r.samples.len(), 3);
        assert_eq!(r.input_pos, [0, 1, 2]);
        let r = prepare(&[O], 5.0);
        assert_eq!(r.samples.len(), 1);
        assert_eq!(r.length_m, 0.0);
    }

    #[test]
    fn long_route_density() {
        let r = prepare(&[O, north(O, 200_000.0)], 5.0);
        assert_eq!(r.samples.len(), 40_001);
        let last = r.samples.last().unwrap();
        assert!((last.d_m - 200_000.0).abs() < 1e-3);
        let max_gap = r.samples.windows(2).map(|w| w[1].d_m - w[0].d_m).fold(0.0, f64::max);
        assert!(max_gap <= 5.0 + 1e-9);
    }
}
