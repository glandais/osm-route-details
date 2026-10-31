//! D+/D− à hystérésis (T04 §5.4) : accumulateur à points de retournement de vcyclist
//! (`ElevationGain.accumulate`, `vc:gpx/.../ElevationGain.kt`), prologue `dir == 0` et jambe
//! d'ouverture compris, sur une copie privée lissée à l'échelle du preset.
//!
//! Le résultat ne dépend que des extrema : il ne dépend pas de la densité d'échantillonnage.

use ord_domain::request::GainPreset;
use serde::Serialize;

use super::smooth::smooth_profile;
use crate::rules::PresetParams;

/// Plage GLO-30 où le seuil a été adapté (incrément 2 ; toujours vide à l'incrément 1).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AdaptedRange {
    pub from_m: f64,
    pub to_m: f64,
    pub threshold_m: f64,
}

/// Paramètres effectifs du D+ (`summary.gain_params`, doc 02 §4.4).
#[derive(Clone, Debug, PartialEq)]
pub struct GainParams {
    pub preset: GainPreset,
    pub threshold_m: f64,
    pub smoothing_m: f64,
    pub adapted: Vec<AdaptedRange>,
    /// Des plages d'altitude manquent : D+ sommé sur les plages lues.
    pub partial: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GainResult {
    /// Dénivelé positif cumulé, ≥ 0.
    pub gain_m: f64,
    /// Dénivelé négatif cumulé, ≥ 0 (valeur absolue, comme le contrat).
    pub loss_m: f64,
    pub params: GainParams,
}

/// Accumulateur à points de retournement sur un profil déjà lissé ; renvoie `(gain, perte)`,
/// la perte en valeur positive. `threshold ≤ 0` : somme brute des montées et descentes.
pub fn accumulate(h: &[f64], threshold: f64) -> (f64, f64) {
    if h.len() < 2 {
        return (0.0, 0.0);
    }
    if threshold.is_nan() || threshold <= 0.0 {
        let (mut gain, mut loss) = (0.0, 0.0);
        for w in h.windows(2) {
            let d = w[1] - w[0];
            if d > 0.0 {
                gain += d;
            } else {
                loss -= d;
            }
        }
        return (gain, loss);
    }
    let (mut gain, mut loss) = (0.0, 0.0);
    let (mut hi, mut lo) = (h[0], h[0]);
    let (mut i_hi, mut i_lo) = (0usize, 0usize);
    let mut dir = 0i8;
    let (mut reference, mut ext) = (h[0], h[0]);
    for (i, &e) in h.iter().enumerate().skip(1) {
        match dir {
            0 => {
                if e > hi {
                    hi = e;
                    i_hi = i;
                }
                if e < lo {
                    lo = e;
                    i_lo = i;
                }
                if hi - lo >= threshold {
                    // Jambe d'ouverture : de h[0] au premier point de retournement, quelle que
                    // soit sa taille (fermeture gain − perte = dénivelé net à un seuil près).
                    if i_hi > i_lo {
                        if lo < h[0] {
                            loss += h[0] - lo;
                        }
                        dir = 1;
                        reference = lo;
                        ext = hi;
                    } else {
                        if hi > h[0] {
                            gain += hi - h[0];
                        }
                        dir = -1;
                        reference = hi;
                        ext = lo;
                    }
                }
            }
            1 => {
                if e > ext {
                    ext = e;
                } else if ext - e >= threshold {
                    gain += ext - reference;
                    reference = ext;
                    ext = e;
                    dir = -1;
                }
            }
            _ => {
                if e < ext {
                    ext = e;
                } else if e - ext >= threshold {
                    loss += reference - ext;
                    reference = ext;
                    ext = e;
                    dir = 1;
                }
            }
        }
    }
    // Jambe encore ouverte en fin de profil : une montée finale n'est pas perdue.
    if dir > 0 && ext > reference {
        gain += ext - reference;
    } else if dir < 0 && ext < reference {
        loss += reference - ext;
    }
    (gain, loss)
}

/// D+/D− d'un profil à trous (`NaN` = altitude absente) : l'accumulateur redémarre après chaque
/// trou et les sommes s'additionnent (T04 §5.4, « Trous »). Renvoie `(gain, perte, partiel)`.
pub fn gain_on_runs(d: &[f64], ele: &[f64], params: PresetParams) -> (f64, f64, bool) {
    assert_eq!(d.len(), ele.len(), "distances et altitudes de même longueur");
    let (mut gain, mut loss, mut partial) = (0.0, 0.0, false);
    let mut i = 0;
    while i < ele.len() {
        if ele[i].is_nan() {
            partial = true;
            i += 1;
            continue;
        }
        let start = i;
        while i < ele.len() && !ele[i].is_nan() {
            i += 1;
        }
        let smoothed = smooth_profile(&d[start..i], &ele[start..i], params.smoothing_m);
        let (g, l) = accumulate(&smoothed, params.threshold_m);
        gain += g;
        loss += l;
    }
    (gain, loss, partial)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEM: PresetParams = PresetParams { threshold_m: 3.0, smoothing_m: 30.0 };

    #[test]
    fn opening_leg_keeps_closure() {
        // Cas de vcyclist : [0, −2,9, 100, 97,1] à 3 m → gain 102,9, perte 2,9 + 2,9 non confirmée.
        let (g, l) = accumulate(&[0.0, -2.9, 100.0, 97.1], 3.0);
        assert!((g - 102.9).abs() < 1e-9, "{g}");
        assert!((l - 2.9).abs() < 1e-9, "{l}");
        assert!(((g - l) - 97.1).abs() < 3.0, "fermeture à un seuil près");
    }

    #[test]
    fn smooth_climb_sampled_densely_is_counted() {
        let h: Vec<f64> = (0..=250).map(|i| i as f64 * 2.0).collect();
        assert_eq!(accumulate(&h, 3.0), (500.0, 0.0));
    }

    #[test]
    fn bumps_below_threshold_are_dropped_with_their_descent() {
        let (g, l) = accumulate(&[0.0, 10.0, 8.0, 10.0, 8.5, 20.0], 3.0);
        assert_eq!((g, l), (20.0, 0.0));
        let (g, l) = accumulate(&[0.0, 10.0, 6.0, 12.0], 3.0);
        assert_eq!((g, l), (16.0, 4.0));
    }

    #[test]
    fn properties_of_the_accumulator() {
        let h: Vec<f64> =
            (0..400).map(|i| (i as f64 * 0.07).sin() * 25.0 + (i as f64 * 0.31).cos() * 4.0).collect();
        let (g3, l3) = accumulate(&h, 3.0);
        let (g10, _) = accumulate(&h, 10.0);
        assert!(g3 >= 0.0 && l3 >= 0.0);
        assert!(g10 <= g3, "le gain décroît quand le seuil croît");
        let shifted: Vec<f64> = h.iter().map(|x| x + 1000.0).collect();
        let (gs, ls) = accumulate(&shifted, 3.0);
        assert!((gs - g3).abs() < 1e-9 && (ls - l3).abs() < 1e-9, "constante ajoutée : inchangé");
        let negated: Vec<f64> = h.iter().map(|x| -x).collect();
        let (gn, ln) = accumulate(&negated, 3.0);
        assert!((gn - l3).abs() < 1e-9 && (ln - g3).abs() < 1e-9, "profil inversé : échange");
    }

    #[test]
    fn gaps_restart_the_accumulator_and_mark_partial() {
        let d: Vec<f64> = (0..9).map(|i| i as f64 * 5.0).collect();
        let e = [0.0, 5.0, 10.0, f64::NAN, f64::NAN, 50.0, 55.0, 60.0, 61.0];
        let (g, l, partial) = gain_on_runs(&d, &e, PresetParams { threshold_m: 3.0, smoothing_m: 0.0 });
        assert!(partial);
        assert_eq!((g, l), (21.0, 0.0), "le saut de 10 à 50 m à travers le trou n'est pas compté");
        let (_, _, partial) = gain_on_runs(&d[..3], &e[..3], DEM);
        assert!(!partial);
    }

    #[test]
    fn independent_of_sampling_on_a_straight_climb() {
        let coarse_d: Vec<f64> = (0..=20).map(|i| i as f64 * 50.0).collect();
        let coarse_e: Vec<f64> =
            coarse_d.iter().map(|d| (d / 1000.0 * std::f64::consts::PI).sin() * 80.0).collect();
        let fine_d: Vec<f64> = (0..=200).map(|i| i as f64 * 5.0).collect();
        // Points colinéaires ajoutés sur chaque segment du profil grossier.
        let fine_e: Vec<f64> = fine_d
            .iter()
            .map(|&d| {
                let j = ((d / 50.0) as usize).min(19);
                let t = (d - coarse_d[j]) / 50.0;
                coarse_e[j] + t * (coarse_e[j + 1] - coarse_e[j])
            })
            .collect();
        let (gc, _, _) = gain_on_runs(&coarse_d, &coarse_e, DEM);
        let (gf, _, _) = gain_on_runs(&fine_d, &fine_e, DEM);
        assert!((gc - gf).abs() / gc < 0.01, "{gc} vs {gf}");
    }
}
