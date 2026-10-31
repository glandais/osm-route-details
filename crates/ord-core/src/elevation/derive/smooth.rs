//! Noyau triangulaire **métrique** (portage de `ElevationSmoother.smoothProfile` de vcyclist,
//! `vc:elevation/.../ElevationSmoother.kt`) : intégrale exacte de l'interpolant linéaire contre le
//! noyau, renormalisée aux bouts. Insérer des points sur un segment droit du profil ne change pas
//! la sortie aux points d'origine.

/// En dessous, le profil est rendu tel quel (`MIN_SMOOTHING_POINTS` de vcyclist).
pub const MIN_SMOOTHING_POINTS: usize = 3;

/// Lisse `ele` (aux distances croissantes `d`) avec un noyau de demi-largeur `window_m`.
/// `window_m ≤ 0` ou moins de 3 points : copie de l'entrée.
pub fn smooth_profile(d: &[f64], ele: &[f64], window_m: f64) -> Vec<f64> {
    assert_eq!(d.len(), ele.len(), "distances et altitudes de même longueur");
    let n = ele.len();
    if n < MIN_SMOOTHING_POINTS || window_m.is_nan() || window_m <= 0.0 {
        return ele.to_vec();
    }
    let mut out = vec![0.0; n];
    let (mut start, mut end) = (0usize, 0usize);
    for i in 0..n {
        let current = d[i];
        while current - d[start] > window_m {
            start += 1;
        }
        if end < i {
            end = i;
        }
        while end < n - 1 && d[end + 1] - current <= window_m {
            end += 1;
        }
        let (lower, upper) = (current - window_m, current + window_m);
        let (mut total_weight, mut weighted) = (0.0, 0.0);
        for j in start.saturating_sub(1)..=end.min(n - 2) {
            let (d0, d1) = (d[j], d[j + 1]);
            let length = d1 - d0;
            if length <= 0.0 {
                continue;
            }
            let a = d0.max(lower);
            let b = d1.min(upper);
            if b <= a {
                continue;
            }
            let e0 = ele[j];
            let slope = (ele[j + 1] - e0) / length;
            let m = 0.5 * (a + b);
            let ka = 1.0 - (a - current).abs() / window_m;
            let km = 1.0 - (m - current).abs() / window_m;
            let kb = 1.0 - (b - current).abs() / window_m;
            let fa = e0 + slope * (a - d0);
            let fm = e0 + slope * (m - d0);
            let fb = e0 + slope * (b - d0);
            // `current` est un sommet : sur [a, b], noyau et interpolant sont affines, leur produit
            // est quadratique et la règle de Simpson est exacte.
            total_weight += 0.5 * (b - a) * (ka + kb);
            weighted += (b - a) / 6.0 * (fa * ka + 4.0 * fm * km + fb * kb);
        }
        out[i] = if total_weight > 0.0 { weighted / total_weight } else { ele[i] };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_kept() {
        let d: Vec<f64> = (0..50).map(|i| i as f64 * 5.0).collect();
        let e: Vec<f64> = d.iter().map(|x| 100.0 + 0.05 * x).collect();
        let s = smooth_profile(&d, &e, 30.0);
        // Au milieu, une droite est inchangée ; aux bouts la fenêtre est renormalisée d'un côté.
        for i in 10..40 {
            assert!((s[i] - e[i]).abs() < 1e-9, "{i}: {} vs {}", s[i], e[i]);
        }
    }

    #[test]
    fn inserting_points_on_a_straight_segment_changes_nothing() {
        let d = [0.0, 40.0, 100.0, 130.0, 200.0];
        let e = [0.0, 10.0, 4.0, 8.0, 1.0];
        let base = smooth_profile(&d, &e, 30.0);
        let d2 = [0.0, 20.0, 40.0, 70.0, 100.0, 130.0, 200.0];
        let e2 = [0.0, 5.0, 10.0, 7.0, 4.0, 8.0, 1.0];
        let dense = smooth_profile(&d2, &e2, 30.0);
        for (i, j) in [(0, 0), (1, 2), (2, 4), (3, 5), (4, 6)] {
            assert!((base[i] - dense[j]).abs() < 1e-9, "{i}");
        }
    }

    #[test]
    fn no_window_is_a_copy() {
        let d = [0.0, 1.0, 2.0];
        let e = [1.0, 5.0, 2.0];
        assert_eq!(smooth_profile(&d, &e, 0.0), e);
        assert_eq!(smooth_profile(&d[..2], &e[..2], 30.0), e[..2]);
    }
}
