//! Marche du baro, référence E1 incertaine (README §4 ter.11, 3f8a810 ; criteria
//! `E1.reference_incertaine_si_marche_baro`) : une marche brutale du résidu baro corrigé − MNT,
//! gardée au moins 2 km, sans marche du MNT au même endroit, rend la référence E1 de la trace
//! incertaine (rapportée, non jugée). Toutes séries, toutes strates ; E2 et E2b inchangés.
//!
//! Sur la grille de 5 m : r(s) = baro(s + δ) − MNT(s), MNT = mode `distance` sans correction ;
//! stations exclues (r = NaN) sur un ouvrage, dans une fenêtre masquée ou sans baro, en s comme en
//! s + δ. Δ court = médiane de r sur [s + 100, s + 300] − médiane sur [s − 300, s − 100] (au moins
//! 20 stations chacune) ; Δ long = même différence sur [s + 100, s + 2 100] et [s − 2 100, s − 100],
//! bornées à la trace (au moins 20 stations). Marche : |Δ court| > 10 m et |Δ long| > 10 m, de même
//! signe, et amplitude du MNT ≤ 30 m sur [s − 300, s + 300]. La plus grande marche (|Δ court|) est
//! rapportée.

use serde_json::{json, Value};

use crate::metrics::median;

/// Seuil de |Δ| (m).
pub const SEUIL_M: f64 = 10.0;
/// Fenêtres de Δ court (m), séparées par une transition de 2 × `TRANSITION_M`.
pub const COURT_M: f64 = 200.0;
/// Fenêtres de Δ long (m).
pub const LONG_M: f64 = 2_000.0;
/// Demi-transition entre les deux fenêtres (m).
pub const TRANSITION_M: f64 = 100.0;
/// Stations avec résidu exigées par fenêtre.
pub const STATIONS_MIN: usize = 20;
/// Amplitude maximale du MNT (max − min) sur [s − 300, s + 300] (m).
pub const MNT_AMPLITUDE_MAX_M: f64 = 30.0;
/// Demi-largeur de la fenêtre du MNT lisse (m).
pub const MNT_DEMI_M: f64 = 300.0;

/// Marche retenue : abscisse (grille), Δ court, Δ long, amplitude du MNT.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marche {
    pub s_m: f64,
    pub court_m: f64,
    pub long_m: f64,
    pub amplitude_mnt_m: f64,
}

impl Marche {
    pub fn to_json(&self) -> Value {
        let r = |x: f64, k: f64| (x * k).round() / k;
        json!({"abscisse_km": r(self.s_m / 1000.0, 1000.0), "delta_court_m": r(self.court_m, 10.0),
            "delta_long_m": r(self.long_m, 10.0), "amplitude_mnt_m": r(self.amplitude_mnt_m, 10.0)})
    }
}

/// Zones de pente du baro sans relief (README §4 quater.3, 2744983 et 633255f ; criteria
/// `E1.zone_masquee_si_pente_baro`) : longueur sur laquelle les pentes sont lues (m).
pub const PENTE_SUR_M: f64 = 50.0;
/// Pente du baro corrigé au-delà de laquelle (exclu) une zone est masquée.
pub const PENTE_BARO_MIN: f64 = 0.15;
/// Pente du MNT en deçà de laquelle (exclu) une zone est masquée.
pub const PENTE_MNT_MAX: f64 = 0.10;
/// Masque : [s − 300, s + 50 + 300].
pub const ZONE_MARGE_M: f64 = 300.0;

/// Zones de pente du baro sans relief, sur la grille `d` : là où |baro(s + 50) − baro(s)| / 50 >
/// 15 % et |MNT(s + 50) − MNT(s)| / 50 < 10 %, baro = baro corrigé du δ (référence E1), MNT = mode
/// `distance` sans correction ; les deux stations avec baro et MNT, hors ouvrage et fenêtre masquée
/// (`excluded`). Zone = [s − 300, s + 350], zones qui se chevauchent réunies, bornées à la trace.
pub fn slope_zones(d: &[f64], baro: &[f64], mnt: &[f64], excluded: &[bool]) -> Vec<std::ops::Range<f64>> {
    let (Some(&start), Some(&end)) = (d.first(), d.last()) else { return Vec::new() };
    let n = d.len();
    let ok = |i: usize| !excluded[i] && baro[i].is_finite() && mnt[i].is_finite();
    let mut out: Vec<std::ops::Range<f64>> = Vec::new();
    for i in (0..n).filter(|&i| ok(i)) {
        let x = d[i] + PENTE_SUR_M;
        // Station de s + 50 : la plus proche, à un demi-pas (2,5 m) au plus.
        let k = d.partition_point(|v| *v < x);
        let j = [k.checked_sub(1), (k < n).then_some(k)]
            .into_iter()
            .flatten()
            .filter(|&j| j > i && (d[j] - x).abs() <= 2.5)
            .min_by(|&a, &b| (d[a] - x).abs().total_cmp(&(d[b] - x).abs()));
        let Some(j) = j.filter(|&j| ok(j)) else { continue };
        let slope = |z: &[f64]| (z[j] - z[i]).abs() / PENTE_SUR_M;
        if slope(baro) > PENTE_BARO_MIN && slope(mnt) < PENTE_MNT_MAX {
            let z = (d[i] - ZONE_MARGE_M).max(start)..(x + ZONE_MARGE_M).min(end);
            match out.last_mut() {
                Some(last) if z.start <= last.end => last.end = last.end.max(z.end),
                _ => out.push(z),
            }
        }
    }
    out
}

/// Stations de la grille dans une des zones.
pub fn in_zones(d: &[f64], zones: &[std::ops::Range<f64>]) -> Vec<bool> {
    d.iter().map(|s| zones.iter().any(|z| z.start <= *s && *s <= z.end)).collect()
}

/// Zones rapportées par trace : nombre, km masqués, abscisses (km), stations comparées masquées.
pub fn zones_json(zones: &[std::ops::Range<f64>], stations_comparees_masquees: usize) -> Value {
    let r = |x: f64| (x * 1000.0).round() / 1000.0;
    json!({
        "zones": zones.len(),
        "km_masques": r(zones.iter().map(|z| z.end - z.start).sum::<f64>() / 1000.0),
        "abscisses_km": zones.iter().map(|z| [r(z.start / 1000.0), r(z.end / 1000.0)]).collect::<Vec<_>>(),
        "stations_comparees_masquees": stations_comparees_masquees,
    })
}

/// Résidu r(s) = baro corrigé − MNT aux stations gardées : `excluded[i]` (ouvrage, fenêtre
/// masquée) s'applique en s comme en s + δ (`shift`, station la plus proche de la grille).
pub fn residual(d: &[f64], baro_corr: &[f64], mnt: &[f64], excluded: &[bool], shift: f64) -> Vec<f64> {
    let n = d.len();
    let at = |x: f64| -> Option<usize> {
        if n == 0 || x < d[0] - 2.5 || x > d[n - 1] + 2.5 {
            return None;
        }
        let k = d.partition_point(|v| *v < x).min(n - 1);
        Some(if k > 0 && (x - d[k - 1]).abs() <= (d[k] - x).abs() { k - 1 } else { k })
    };
    (0..n)
        .map(|i| {
            let shifted_out = at(d[i] + shift).is_none_or(|j| excluded[j]);
            if excluded[i] || shifted_out || !baro_corr[i].is_finite() || !mnt[i].is_finite() {
                f64::NAN
            } else {
                baro_corr[i] - mnt[i]
            }
        })
        .collect()
}

/// Médiane des résidus finis de [a, b], `None` sous `STATIONS_MIN` stations.
fn window_median(d: &[f64], r: &[f64], a: f64, b: f64) -> Option<f64> {
    let (i, j) = (d.partition_point(|v| *v < a), d.partition_point(|v| *v <= b));
    let v: Vec<f64> = r[i..j].iter().copied().filter(|x| x.is_finite()).collect();
    (v.len() >= STATIONS_MIN).then(|| median(&v))
}

/// Plus grande marche du résidu `r` (même grille `d`, MNT `mnt`), `None` s'il n'y en a pas.
pub fn find(d: &[f64], r: &[f64], mnt: &[f64]) -> Option<Marche> {
    let (Some(&start), Some(&end)) = (d.first(), d.last()) else { return None };
    let mut best: Option<Marche> = None;
    let mut last = f64::NAN;
    for &s in d {
        let delta = |w: f64| -> Option<f64> {
            let after = window_median(d, r, s + TRANSITION_M, (s + TRANSITION_M + w).min(end))?;
            let before = window_median(d, r, (s - TRANSITION_M - w).max(start), s - TRANSITION_M)?;
            Some(after - before)
        };
        let Some(court) = delta(COURT_M).filter(|c| c.abs() > SEUIL_M) else { continue };
        // À |Δ court| égal (plateau d'une marche franche), l'abscisse rapportée est le milieu du
        // plateau : seule sa fin est notée ici.
        let tie = best.is_some_and(|b| (b.court_m.abs() - court.abs()).abs() < 1e-9);
        if best.is_some_and(|b| b.court_m.abs() > court.abs() + 1e-9) {
            continue;
        }
        let (a, b) =
            (d.partition_point(|v| *v < s - MNT_DEMI_M), d.partition_point(|v| *v <= s + MNT_DEMI_M));
        let m: Vec<f64> = mnt[a..b].iter().copied().filter(|x| x.is_finite()).collect();
        if m.is_empty() {
            continue;
        }
        let amplitude = m.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - m.iter().copied().fold(f64::INFINITY, f64::min);
        if amplitude > MNT_AMPLITUDE_MAX_M {
            continue;
        }
        let Some(long) = delta(LONG_M).filter(|l| l.abs() > SEUIL_M && l.signum() == court.signum()) else {
            continue;
        };
        if tie {
            last = s;
            continue;
        }
        best = Some(Marche { s_m: s, court_m: court, long_m: long, amplitude_mnt_m: amplitude });
        last = s;
    }
    best.map(|b| Marche { s_m: (b.s_m + last) / 2.0, ..b })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(km: f64) -> Vec<f64> {
        (0..=(km * 200.0) as usize).map(|k| k as f64 * 5.0).collect()
    }

    /// Marche de −30 m à 16,2 km gardée jusqu'à la fin, MNT plat (`perso-44`) : retenue.
    #[test]
    fn a_kept_step_on_a_flat_dem_is_found() {
        let d = grid(30.0);
        let mnt: Vec<f64> = d.iter().map(|s| 50.0 + (s / 3000.0).sin()).collect();
        let r: Vec<f64> = d.iter().map(|s| if *s < 16_200.0 { 2.0 } else { -28.0 }).collect();
        let m = find(&d, &r, &mnt).expect("marche");
        assert!((m.s_m - 16_200.0).abs() <= 10.0, "{m:?}");
        assert!((m.court_m + 30.0).abs() < 1e-9 && (m.long_m + 30.0).abs() < 1e-9, "{m:?}");
        assert_eq!(m.to_json()["delta_court_m"], -30.0);
    }

    /// Marche de 9 m : sous le seuil ; marche brève (rendue en 1 km) : Δ long trop petit ou de
    /// signe contraire ; marche au sommet d'un col (MNT de plus de 30 m sur 600 m) : écartée.
    #[test]
    fn small_brief_or_mountain_steps_are_not_found() {
        let d = grid(20.0);
        let flat = vec![100.0; d.len()];
        let small: Vec<f64> = d.iter().map(|s| if *s < 10_000.0 { 0.0 } else { 9.0 }).collect();
        assert!(find(&d, &small, &flat).is_none());
        // Bosse de 12 m sur 600 m : Δ court à ±12 m, Δ long d'environ 3 m.
        let brief: Vec<f64> =
            d.iter().map(|s| if (10_000.0..10_600.0).contains(s) { 12.0 } else { 0.0 }).collect();
        assert!(find(&d, &brief, &flat).is_none());
        let col: Vec<f64> = d.iter().map(|s| 500.0 - (s - 10_000.0).abs() * 0.2).collect();
        let step: Vec<f64> = d.iter().map(|s| if *s < 10_000.0 { 0.0 } else { 15.0 }).collect();
        assert!(find(&d, &step, &col).is_none(), "inversion de pente au col");
        assert!(find(&d, &step, &flat).is_some());
    }

    /// Zones de pente (README §4 quater.3) : baro qui monte de 30 m en 150 m (20 %) sur un MNT plat,
    /// masqué sur [s − 300, s + 350] ; même montée lue par le MNT (relief réel) : pas de zone ;
    /// station exclue (ouvrage, fenêtre masquée) en s ou en s + 50 : pas de zone ; zones voisines
    /// réunies, bornées à la trace.
    #[test]
    fn baro_slope_without_relief_is_masked() {
        let d = grid(10.0);
        let n = d.len();
        let ramp = |s: f64, at: f64| (((s - at) / 150.0).clamp(0.0, 1.0)) * 30.0;
        let baro: Vec<f64> = d.iter().map(|s| 100.0 + ramp(*s, 5_000.0)).collect();
        let flat = vec![100.0; n];
        let none = vec![false; n];
        let z = slope_zones(&d, &baro, &flat, &none);
        assert_eq!(z.len(), 1, "{z:?}");
        // Paires en pente (Δ > 7,5 m sur 50 m) : s de 4 990 à 5 110 ; masque [s − 300, s + 350].
        assert_eq!(z[0], 4_690.0..5_460.0, "{z:?}");
        let masked = in_zones(&d, &z);
        assert!(masked[(5_000.0 / 5.0) as usize] && !masked[(4_000.0 / 5.0) as usize]);
        // Relief réel : le MNT monte avec le baro.
        assert!(slope_zones(&d, &baro, &baro, &none).is_empty());
        // Montée sur un ouvrage : stations exclues.
        let on: Vec<bool> = d.iter().map(|s| (4_900.0..=5_200.0).contains(s)).collect();
        assert!(slope_zones(&d, &baro, &flat, &on).is_empty());
        // Deux montées à 400 m : une zone ; près du début : bornée à la trace.
        let two: Vec<f64> = d.iter().map(|s| 100.0 + ramp(*s, 100.0) + ramp(*s, 500.0)).collect();
        let z = slope_zones(&d, &two, &flat, &none);
        assert_eq!(z.len(), 1, "{z:?}");
        assert_eq!(z[0].start, 0.0);
        let j = zones_json(&z, 7);
        assert_eq!(j["zones"], 1);
        assert_eq!(j["stations_comparees_masquees"], 7);
    }

    /// Stations exclues en s ou en s + δ (ouvrage, masque), sans baro ou sans MNT : pas de résidu ;
    /// trop peu de stations dans une fenêtre : pas de Δ.
    #[test]
    fn excluded_stations_have_no_residual() {
        let d = grid(1.0);
        let n = d.len();
        let mut excluded = vec![false; n];
        excluded[100] = true;
        let mut baro = vec![10.0; n];
        baro[50] = f64::NAN;
        let mnt = vec![4.0; n];
        let r = residual(&d, &baro, &mnt, &excluded, 20.0);
        assert!(r[100].is_nan() && r[50].is_nan());
        assert!(r[96].is_nan(), "s + δ sur la station exclue");
        assert!(r[200].is_nan(), "s + δ hors de la trace");
        assert_eq!(r[10], 6.0);
        let sparse: Vec<f64> = (0..n).map(|i| if i % 50 == 0 { 30.0 } else { f64::NAN }).collect();
        assert!(find(&d, &sparse, &mnt).is_none());
    }
}
