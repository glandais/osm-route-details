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
