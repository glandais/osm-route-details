//! D+/D− à hystérésis (T04 §5.4) : accumulateur à points de retournement de vcyclist
//! (`ElevationGain.accumulate`, `vc:gpx/.../ElevationGain.kt`), prologue `dir == 0` et jambe
//! d'ouverture compris, sur une copie privée lissée à l'échelle du preset.
//!
//! Le résultat ne dépend que des extrema : il ne dépend pas de la densité d'échantillonnage.

use ord_domain::request::GainPreset;
use serde::Serialize;

use super::smooth::smooth_profile;
use crate::rules::PresetParams;

/// Plage GLO-30 où le seuil a été adapté (T04 §5.4), sérialisée avec `"dsm": "glo30"`.
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
    /// Extrémités des jambes comptées (T04 §6.1), réutilisées par la sélection des points de
    /// rupture.
    pub turning_points: Vec<TurningPoint>,
}

/// Extrémité d'une jambe comptée par l'accumulateur (T04 §6.1) : sommet ou creux confirmé, ou
/// bout d'une suite de stations lues. Les extrémités successives d'une suite délimitent ses
/// jambes ; `gain_m` est la somme des montées entre elles, sur le profil lissé du preset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TurningPoint {
    /// Rang de la station (dans le profil entier pour `gain_turns_on_runs`).
    pub station: usize,
    /// `true` : sommet ; `false` : creux.
    pub max: bool,
    /// Altitude lissée à cette station.
    pub value: f64,
}

/// Accumulateur à points de retournement sur un profil déjà lissé ; renvoie `(gain, perte)`,
/// la perte en valeur positive. `threshold ≤ 0` : somme brute des montées et descentes.
pub fn accumulate(h: &[f64], threshold: f64) -> (f64, f64) {
    accumulate_with(h, |_| threshold)
}

/// Accumulateur à seuil variable par station (adaptation GLO-30, T04 §5.4) : une jambe est
/// confirmée quand le retour depuis l'extremum atteint `max(τ(extremum), τ(courant))`, si bien
/// qu'à cheval sur une frontière le seuil le plus prudent s'applique. Avec un seuil constant,
/// identique à `accumulate`. `τ(0) ≤ 0` : somme brute des montées et descentes.
pub fn accumulate_with(h: &[f64], tau: impl Fn(usize) -> f64) -> (f64, f64) {
    let (gain, loss, _) = accumulate_turns(h, tau);
    (gain, loss)
}

/// Comme `accumulate_with`, avec les extrémités des jambes comptées, dans l'ordre : départ de
/// la suite, extrema confirmés, fin de la dernière jambe (extremum courant en fin de profil).
/// Aucune jambe (profil plat sous le seuil) : liste vide.
pub fn accumulate_turns(h: &[f64], tau: impl Fn(usize) -> f64) -> (f64, f64, Vec<TurningPoint>) {
    let mut turns = Vec::new();
    if h.len() < 2 {
        return (0.0, 0.0, turns);
    }
    let turn = |station: usize, max: bool| TurningPoint { station, max, value: h[station] };
    let threshold = tau(0);
    if threshold.is_nan() || threshold <= 0.0 {
        // Somme brute : chaque changement de sens est une extrémité de jambe.
        let (mut gain, mut loss) = (0.0, 0.0);
        let mut dir = 0i8;
        for (i, w) in h.windows(2).enumerate() {
            let d = w[1] - w[0];
            let s = if d > 0.0 {
                1
            } else if d < 0.0 {
                -1
            } else {
                0
            };
            if d > 0.0 {
                gain += d;
            } else {
                loss -= d;
            }
            if s != 0 && s != dir {
                // Départ (dir = 0) ou retournement à la station i.
                turns.push(turn(i, s < 0));
                dir = s;
            }
        }
        if dir != 0 {
            turns.push(turn(h.len() - 1, dir > 0));
        }
        return (gain, loss, turns);
    }
    let (mut gain, mut loss) = (0.0, 0.0);
    let (mut hi, mut lo) = (h[0], h[0]);
    let (mut i_hi, mut i_lo) = (0usize, 0usize);
    let mut dir = 0i8;
    let (mut reference, mut ext) = (h[0], h[0]);
    let mut i_ext = 0usize;
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
                if hi - lo >= tau(i_hi).max(tau(i_lo)) {
                    // Jambe d'ouverture : de h[0] au premier point de retournement, quelle que
                    // soit sa taille (fermeture gain − perte = dénivelé net à un seuil près).
                    if i_hi > i_lo {
                        turns.push(turn(0, i_lo > 0));
                        if lo < h[0] {
                            loss += h[0] - lo;
                            turns.push(turn(i_lo, false));
                        }
                        dir = 1;
                        reference = lo;
                        ext = hi;
                        i_ext = i_hi;
                    } else {
                        turns.push(turn(0, i_hi == 0));
                        if hi > h[0] {
                            gain += hi - h[0];
                            turns.push(turn(i_hi, true));
                        }
                        dir = -1;
                        reference = hi;
                        ext = lo;
                        i_ext = i_lo;
                    }
                }
            }
            1 => {
                if e > ext {
                    ext = e;
                    i_ext = i;
                } else if ext - e >= tau(i_ext).max(tau(i)) {
                    gain += ext - reference;
                    turns.push(turn(i_ext, true));
                    reference = ext;
                    ext = e;
                    i_ext = i;
                    dir = -1;
                }
            }
            _ => {
                if e < ext {
                    ext = e;
                    i_ext = i;
                } else if e - ext >= tau(i_ext).max(tau(i)) {
                    loss += reference - ext;
                    turns.push(turn(i_ext, false));
                    reference = ext;
                    ext = e;
                    i_ext = i;
                    dir = 1;
                }
            }
        }
    }
    // Jambe encore ouverte en fin de profil : une montée finale n'est pas perdue.
    if dir > 0 && ext > reference {
        gain += ext - reference;
        turns.push(turn(i_ext, true));
    } else if dir < 0 && ext < reference {
        loss += reference - ext;
        turns.push(turn(i_ext, false));
    }
    (gain, loss, turns)
}

/// D+/D− d'un profil à trous (`NaN` = altitude absente) : l'accumulateur redémarre après chaque
/// trou et les sommes s'additionnent (T04 §5.4, « Trous »). Renvoie `(gain, perte, partiel)`.
pub fn gain_on_runs(d: &[f64], ele: &[f64], params: PresetParams) -> (f64, f64, bool) {
    gain_on_runs_with(d, ele, params, None)
}

/// Comme `gain_on_runs`, avec un seuil par station (`tau`, même longueur que `ele`) : adaptation
/// GLO-30. `None` : seuil du preset partout.
pub fn gain_on_runs_with(
    d: &[f64],
    ele: &[f64],
    params: PresetParams,
    tau: Option<&[f64]>,
) -> (f64, f64, bool) {
    let (gain, loss, partial, _) = gain_turns_on_runs(d, ele, params, tau);
    (gain, loss, partial)
}

/// Comme `gain_on_runs_with`, avec les extrémités des jambes comptées (`TurningPoint`, rangs
/// dans le profil entier), suite par suite, dans l'ordre des distances.
pub fn gain_turns_on_runs(
    d: &[f64],
    ele: &[f64],
    params: PresetParams,
    tau: Option<&[f64]>,
) -> (f64, f64, bool, Vec<TurningPoint>) {
    assert_eq!(d.len(), ele.len(), "distances et altitudes de même longueur");
    let (mut gain, mut loss, mut partial) = (0.0, 0.0, false);
    let mut turns = Vec::new();
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
        let (g, l, t) = match tau {
            Some(t) => {
                let t = &t[start..i];
                accumulate_turns(&smoothed, |k| t[k])
            }
            None => accumulate_turns(&smoothed, |_| params.threshold_m),
        };
        gain += g;
        loss += l;
        turns.extend(t.into_iter().map(|p| TurningPoint { station: p.station + start, ..p }));
    }
    (gain, loss, partial, turns)
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
    fn variable_threshold_uses_the_most_prudent_side() {
        // Bosse de 6 m : comptée à 3 m, ignorée si l'un de ses côtés est sur GLO-30 (10 m).
        let h = [0.0, 6.0, 0.0, 6.0, 0.0];
        assert_eq!(accumulate_with(&h, |_| 3.0), accumulate(&h, 3.0));
        let (g, _) = accumulate_with(&h, |i| if i >= 2 { 10.0 } else { 3.0 });
        assert!(g < accumulate(&h, 3.0).0, "{g}");
        let (g10, _) = accumulate_with(&h, |_| 10.0);
        assert_eq!(g10, 0.0);
        // Montée de 50 m suivie d'une descente de 50 m : comptée sous tous les seuils.
        let big = [0.0, 50.0, 0.0];
        assert_eq!(accumulate_with(&big, |i| if i == 1 { 10.0 } else { 3.0 }), (50.0, 50.0));
    }

    #[test]
    fn turning_points_bound_every_counted_leg() {
        // Descente d'ouverture, montée, bosse sous le seuil, descente finale ouverte.
        let h = [10.0, 5.0, 20.0, 18.5, 21.0, 12.0, 11.0];
        let (g, l, t) = accumulate_turns(&h, |_| 3.0);
        let st: Vec<(usize, bool)> = t.iter().map(|p| (p.station, p.max)).collect();
        assert_eq!(st, [(0, true), (1, false), (4, true), (6, false)]);
        // Les jambes entre extrémités successives redonnent le gain et la perte.
        let up: f64 = t.windows(2).map(|w| (w[1].value - w[0].value).max(0.0)).sum();
        let down: f64 = t.windows(2).map(|w| (w[0].value - w[1].value).max(0.0)).sum();
        assert_eq!((g, l), (up, down));
        // Profil plat sous le seuil : aucune jambe.
        assert!(accumulate_turns(&[1.0, 2.0, 1.0], |_| 3.0).2.is_empty());
        // Somme brute (`raw`) : chaque changement de sens.
        let (_, _, t) = accumulate_turns(&[0.0, 1.0, 1.0, 0.5, 2.0], |_| 0.0);
        let st: Vec<(usize, bool)> = t.iter().map(|p| (p.station, p.max)).collect();
        assert_eq!(st, [(0, false), (2, true), (3, false), (4, true)]);
    }

    #[test]
    fn turning_points_on_runs_use_absolute_stations() {
        let d: Vec<f64> = (0..9).map(|i| i as f64 * 5.0).collect();
        let e = [0.0, 10.0, 0.0, f64::NAN, f64::NAN, 50.0, 40.0, 60.0, 61.0];
        let p = PresetParams { threshold_m: 3.0, smoothing_m: 0.0 };
        let (g, _, partial, t) = gain_turns_on_runs(&d, &e, p, None);
        assert!(partial);
        assert_eq!(g, 31.0);
        let st: Vec<usize> = t.iter().map(|p| p.station).collect();
        assert_eq!(st, [0, 1, 2, 5, 6, 8]);
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
