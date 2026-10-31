//! Calculs du banc (bench §2.2, §3) : grille commune, masques, E1 et E2. Fonctions pures.

use std::ops::Range;

use ord_core::elevation::derive::gain_on_runs;
use ord_core::rules::PresetParams;
use ord_domain::geo::haversine_m;
use ord_domain::LatLon;

/// Médiane (moyenne des deux valeurs centrales) ; `NaN` si vide.
pub fn median(values: &[f64]) -> f64 {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| !x.is_nan()).collect();
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

/// Percentile à interpolation linéaire (convention par défaut de numpy).
pub fn percentile(values: &[f64], p: f64) -> f64 {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| !x.is_nan()).collect();
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let rank = p / 100.0 * (v.len() - 1) as f64;
    let (lo, hi) = (rank.floor() as usize, rank.ceil() as usize);
    v[lo] + (rank - lo as f64) * (v[hi] - v[lo])
}

/// Interpolation linéaire de la série `(xs, ys)` (xs croissants) aux abscisses `query` ; `NaN`
/// hors de la série ou entre deux échantillons distants de plus de `gap_max`.
pub fn interpolate(xs: &[f64], ys: &[f64], query: &[f64], gap_max: f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(query.len());
    let mut j = 0;
    for &q in query {
        while j + 1 < xs.len() && xs[j + 1] < q {
            j += 1;
        }
        let v = if xs.is_empty() || q < xs[0] || q > xs[xs.len() - 1] {
            f64::NAN
        } else if j + 1 < xs.len() && xs[j + 1] == q {
            ys[j + 1]
        } else if j + 1 >= xs.len() || xs[j] >= q {
            if (xs[j] - q).abs() < 1e-9 {
                ys[j]
            } else {
                f64::NAN
            }
        } else {
            let span = xs[j + 1] - xs[j];
            if span > gap_max {
                f64::NAN
            } else if span <= 0.0 {
                ys[j]
            } else {
                ys[j] + (q - xs[j]) / span * (ys[j + 1] - ys[j])
            }
        };
        out.push(v);
    }
    out
}

/// Médiane glissante centrée de largeur `window`, calculée tous les `step` mètres et interpolée
/// linéairement entre deux calculs (bench §2.2, étape 4 : dérive lente > 5 km).
pub fn rolling_median(d: &[f64], r: &[f64], window: f64, step: f64) -> Vec<f64> {
    if d.is_empty() {
        return Vec::new();
    }
    let (start, end) = (d[0], d[d.len() - 1]);
    let anchors: Vec<f64> = (0..).map(|k| start + k as f64 * step).take_while(|a| *a <= end + step).collect();
    let half = window / 2.0;
    let (mut lo, mut hi) = (0usize, 0usize);
    let mut medians = Vec::with_capacity(anchors.len());
    for &a in &anchors {
        while lo < d.len() && d[lo] < a - half {
            lo += 1;
        }
        while hi < d.len() && d[hi] <= a + half {
            hi += 1;
        }
        medians.push(median(&r[lo..hi.max(lo)]));
    }
    // Ancres sans valeur : reprises de la plus proche ancre définie.
    let defined: Vec<(f64, f64)> =
        anchors.iter().zip(&medians).filter(|(_, m)| !m.is_nan()).map(|(a, m)| (*a, *m)).collect();
    if defined.is_empty() {
        return vec![f64::NAN; d.len()];
    }
    let (ax, ay): (Vec<f64>, Vec<f64>) = defined.into_iter().unzip();
    d.iter()
        .map(|&x| {
            if x <= ax[0] {
                ay[0]
            } else if x >= ax[ax.len() - 1] {
                ay[ay.len() - 1]
            } else {
                interpolate(&ax, &ay, &[x], f64::INFINITY)[0]
            }
        })
        .collect()
}

/// E1 : RMSE et P95 de (service − baro) après retrait d'une médiane glissante (qui absorbe le
/// décalage global et la dérive lente) ; `offset_m` = médiane(baro − service), en diagnostic.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct E1 {
    pub rmse_m: f64,
    pub p95_m: f64,
    pub offset_m: f64,
    /// Plus grand écart absolu, et part des stations à plus de 10 m (diagnostics).
    pub max_abs_m: f64,
    pub share_over_10m: f64,
    pub stations: usize,
}

pub fn e1(d: &[f64], service: &[f64], baro: &[f64], window: f64, step: f64) -> E1 {
    let (mut dd, mut rr) = (Vec::new(), Vec::new());
    for i in 0..d.len() {
        if !service[i].is_nan() && !baro[i].is_nan() {
            dd.push(d[i]);
            rr.push(service[i] - baro[i]);
        }
    }
    let offset_m = -median(&rr);
    let trend = rolling_median(&dd, &rr, window, step);
    let e: Vec<f64> = rr.iter().zip(&trend).map(|(r, t)| r - t).collect();
    let n = e.len();
    let rmse_m = if n == 0 { f64::NAN } else { (e.iter().map(|x| x * x).sum::<f64>() / n as f64).sqrt() };
    let abs: Vec<f64> = e.iter().map(|x| x.abs()).collect();
    let max_abs_m = abs.iter().copied().fold(f64::NAN, f64::max);
    let share_over_10m = abs.iter().filter(|x| **x > 10.0).count() as f64 / n.max(1) as f64;
    E1 { rmse_m, p95_m: percentile(&abs, 95.0), offset_m, max_abs_m, share_over_10m, stations: n }
}

/// D+ d'un profil à trous (`NaN`), sommé sur les tronçons restants (même accumulateur que le
/// service).
pub fn dplus(d: &[f64], ele: &[f64], preset: PresetParams) -> f64 {
    gain_on_runs(d, ele, preset).0
}

/// Fenêtres masquées autour des passages d'un ouvrage : stations à moins de `tol` d'un de ses
/// points de référence, regroupées en passages (écart > 1 km), étendues de `half` de chaque côté.
pub fn passage_windows(d: &[f64], pts: &[LatLon], refs: &[LatLon], tol: f64, half: f64) -> Vec<Range<f64>> {
    let near: Vec<f64> = d
        .iter()
        .zip(pts)
        .filter(|(_, p)| refs.iter().any(|r| haversine_m(**p, *r) <= tol))
        .map(|(d, _)| *d)
        .collect();
    let mut out: Vec<Range<f64>> = Vec::new();
    for x in near {
        match out.last_mut() {
            Some(w) if x - (w.end - half) <= 1_000.0 => w.end = x + half,
            _ => out.push((x - half)..(x + half)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_and_percentile() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0, 5.0], 95.0), 4.8);
        assert!(median(&[f64::NAN]).is_nan());
    }

    #[test]
    fn interpolation_respects_gaps_and_bounds() {
        let xs = [0.0, 10.0, 200.0];
        let ys = [0.0, 10.0, 20.0];
        let v = interpolate(&xs, &ys, &[-1.0, 0.0, 5.0, 10.0, 50.0, 200.0, 201.0], 100.0);
        assert!(v[0].is_nan() && v[4].is_nan() && v[6].is_nan());
        assert_eq!((v[1], v[2], v[3], v[5]), (0.0, 5.0, 10.0, 20.0));
    }

    #[test]
    fn e1_removes_offset_and_slow_drift() {
        let d: Vec<f64> = (0..4_000).map(|i| i as f64 * 5.0).collect();
        let service: Vec<f64> = d.iter().map(|x| (x / 300.0).sin() * 20.0).collect();
        // Baro = service + décalage de 12 m + dérive lente de 3 m sur 20 km + bruit ±0,5 m.
        let baro: Vec<f64> = d
            .iter()
            .zip(&service)
            .enumerate()
            .map(|(i, (x, s))| s + 12.0 + x / 20_000.0 * 3.0 + if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let m = e1(&d, &service, &baro, 5_000.0, 50.0);
        assert!((m.rmse_m - 0.5).abs() < 0.1, "{m:?}");
        assert!((m.p95_m - 0.5).abs() < 0.2, "{m:?}");
        assert!((m.offset_m - 13.5).abs() < 0.6, "{m:?}");
    }

    #[test]
    fn windows_merge_close_hits() {
        let d: Vec<f64> = (0..100).map(|i| i as f64 * 5.0).collect();
        let pts: Vec<LatLon> = d.iter().map(|x| LatLon::new(45.0 + x / 111_195.0, 6.0)).collect();
        let w = passage_windows(&d, &pts, &[pts[50]], 12.0, 500.0);
        assert_eq!(w.len(), 1);
        assert!((w[0].start - (240.0 - 500.0)).abs() < 1e-6 && (w[0].end - (260.0 + 500.0)).abs() < 1e-6);
    }
}
