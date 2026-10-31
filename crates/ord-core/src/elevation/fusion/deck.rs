//! Constructeur de tablier (T04 §3.2) : profil du tablier d'un pont OSM lu dans le MNS LiDAR.
//!
//! Port libre de `DeckProfileBuilder` et des outils de `DeckDetector` de vcyclist
//! (`gpx/src/commonMain/kotlin/io/github/glandais/engine/path/`, 6.11, Apache-2.0,
//! © Gabriel Landais) : ancres propres (rugosité du MNS), MNS aux ancres et linéaire entre elles,
//! médiane glissante qui ne lit jamais au-delà des ancres, plafond de pente qui ne fait que
//! remonter. L'emprise OSM remplace le détecteur : on ne cherche pas le pont, on le lit.
//!
//! Écarts : fenêtres en mètres sur des stations de pas quelconque (les points d'entrée sont des
//! stations exactes), sans rembourrage aux bords (la fenêtre rétrécit) ; pont sans sursol
//! (`kept_ground`) et contrôles de cohérence MNS/MNT de T04 §3.2, étapes 3 et 8.

use crate::rules::DeckBuilderRules;

/// Sonde de culée : chaînage et MNT, juste au-delà de l'extrémité ; MNS au même point (`NaN` si
/// absent), lu seulement pour juger une rampe, jamais écrit (inv. 9).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probe {
    pub c: f64,
    pub z: f64,
    pub mns: f64,
}

/// Issue du constructeur sur un pont.
#[derive(Clone, Debug, PartialEq)]
pub enum DeckOutcome {
    /// Tablier : une valeur par station de `first..=last` (ancres extrêmes), `NaN` ailleurs ;
    /// les stations entre une sonde et la première ancre sont laissées à la rampe.
    Deck { z: Vec<f64>, first: usize, last: usize },
    /// Pas de sursol : le sol est gardé.
    KeptGround,
    /// Profil abandonné : le pont passe en interpolation.
    Fallback(DeckFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeckFailure {
    TooFewAnchors,
    RampTooSteep,
    BelowGround,
}

/// Médiane des valeurs finies de `x` dont le chaînage est à moins de `half` de chaque station.
pub fn rolling_nan_median(c: &[f64], x: &[f64], half: f64) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    let mut window: Vec<f64> = Vec::new();
    let (mut lo, mut hi) = (0usize, 0usize);
    for k in 0..n {
        while c[lo] < c[k] - half - 1e-9 {
            lo += 1;
        }
        while hi < n && c[hi] <= c[k] + half + 1e-9 {
            hi += 1;
        }
        window.clear();
        window.extend(x[lo..hi].iter().copied().filter(|v| !v.is_nan()));
        if window.is_empty() {
            continue;
        }
        window.sort_by(f64::total_cmp);
        let m = window.len();
        out[k] = if m % 2 == 1 { window[m / 2] } else { 0.5 * (window[m / 2 - 1] + window[m / 2]) };
    }
    out
}

/// Fenêtre glissante de rayon `half` : `f` sur les valeurs, `NaN` si une valeur est `NaN`.
fn rolling(c: &[f64], x: &[f64], half: f64, f: impl Fn(&[f64]) -> f64) -> Vec<f64> {
    let n = x.len();
    let (mut lo, mut hi) = (0usize, 0usize);
    (0..n)
        .map(|k| {
            while c[lo] < c[k] - half - 1e-9 {
                lo += 1;
            }
            while hi < n && c[hi] <= c[k] + half + 1e-9 {
                hi += 1;
            }
            let w = &x[lo..hi];
            if w.iter().any(|v| v.is_nan()) {
                f64::NAN
            } else {
                f(w)
            }
        })
        .collect()
}

/// Stations propres : rugosité (RMS de `MNS − médiane`) maximale sur la fenêtre sous le seuil.
/// Une station dont la fenêtre touche un MNS absent n'est jamais propre (`!(x < t)`).
pub fn clean_stations(c: &[f64], mns: &[f64], r: &DeckBuilderRules) -> Vec<bool> {
    let mf = rolling_nan_median(c, mns, 0.5 * r.surface_median_window_m);
    let sq: Vec<f64> = mns.iter().zip(&mf).map(|(a, m)| (a - m) * (a - m)).collect();
    let half = 0.5 * r.roughness_window_m;
    let rough = rolling(c, &sq, half, |w| (w.iter().sum::<f64>() / w.len() as f64).sqrt());
    let worst = rolling(c, &rough, half, |w| w.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    worst.iter().map(|&v| v < r.max_roughness_m).collect()
}

/// Plus petit profil au-dessus de `z` dont la pente entre deux stations ne dépasse jamais
/// `max_slope` (dilatation par un cône, deux passes linéaires ; `capSlope` de vcyclist).
pub fn cap_slope(c: &[f64], z: &mut [f64], max_slope: f64) {
    for k in 1..z.len() {
        z[k] = z[k].max(z[k - 1] - max_slope * (c[k] - c[k - 1]));
    }
    for k in (0..z.len().saturating_sub(1)).rev() {
        z[k] = z[k].max(z[k + 1] - max_slope * (c[k + 1] - c[k]));
    }
}

/// Profil de tablier sur les stations d'axe `c` (croissant), avec MNT et MNS (`NaN` = absent),
/// et les sondes de chaque côté (`None` : côté tenu plat, jonction avec un tunnel).
pub fn build_deck(
    c: &[f64],
    mnt: &[f64],
    mns: &[f64],
    probes: [Option<Probe>; 2],
    r: &DeckBuilderRules,
) -> DeckOutcome {
    let n = c.len();
    if n == 0 {
        return DeckOutcome::Fallback(DeckFailure::TooFewAnchors);
    }
    // Étape 3 : pont sans sursol. `MNT > MNS + tolérance` (physiquement impossible) compte comme
    // haut ; pas de canyon si le MNT sous l'axe ne descend pas sous la plus basse des sondes.
    let tall = (0..n)
        .filter(|&k| mnt[k] > mns[k] + r.mnt_above_mns_tolerance_m || mns[k] - mnt[k] > r.deck_mnh_m)
        .count();
    let probe_min = probes.iter().flatten().map(|p| p.z).fold(f64::INFINITY, f64::min);
    let mnt_min = mnt.iter().copied().filter(|v| !v.is_nan()).fold(f64::INFINITY, f64::min);
    let canyon = probe_min.is_finite() && mnt_min.is_finite() && probe_min - mnt_min > r.min_canyon_drop_m;
    if (tall as f64) < r.min_deck_presence * n as f64 && !canyon {
        return DeckOutcome::KeptGround;
    }
    // Étapes 4–5 : ancres propres, MNS fini et pas sous le MNT.
    let clean = clean_stations(c, mns, r);
    let anchors: Vec<usize> = (0..n)
        .filter(|&k| clean[k] && !mns[k].is_nan() && mns[k] >= mnt[k] - r.mnt_above_mns_tolerance_m)
        .collect();
    if anchors.len() < 2 {
        return DeckOutcome::Fallback(DeckFailure::TooFewAnchors);
    }
    let (u, v) = (anchors[0], anchors[anchors.len() - 1]);
    // Étape 6 : MNS aux ancres, linéaire entre elles, `NaN` au-delà ; médiane qui ne lit jamais
    // au-delà des ancres ; plafond de pente.
    let mut x = vec![f64::NAN; n];
    for w in anchors.windows(2) {
        let (a, b) = (w[0], w[1]);
        for k in a..=b {
            let t = if c[b] > c[a] { (c[k] - c[a]) / (c[b] - c[a]) } else { 0.0 };
            x[k] = mns[a] + (mns[b] - mns[a]) * t;
        }
    }
    let med = rolling_nan_median(c, &x, 0.5 * r.surface_median_window_m);
    let mut deck: Vec<f64> = med[u..=v].to_vec();
    cap_slope(&c[u..=v], &mut deck, r.max_slope);
    // Étape 8 : cohérence MNS/MNT. Rampe trop raide (ancre sur une superstructure lisse, pont
    // au-dessus de la trace) ou tablier sous le sol sur trop de stations. Exception : le MNS de la
    // sonde prolonge le tablier sans rupture (pente sous `max_ramp_grade`) ; la surface lue
    // continue au-delà du way, c'est un ouvrage qui se prolonge sans tag (accès nord du pont de
    // Saint-Nazaire), pas une superstructure : le tablier est gardé.
    let mut held = [false, false];
    for (side, (probe, k)) in [(probes[0], u), (probes[1], v)].into_iter().enumerate() {
        if let Some(p) = probe {
            let run = (c[k] - p.c).abs();
            let z = deck[k - u];
            let steep = run > 0.0 && (z - p.z).abs() / run > r.max_ramp_grade;
            // MNS de la sonde absent (`NaN`) : pas de prolongement.
            let continues = run > 0.0 && (z - p.mns).abs() / run <= r.max_ramp_grade;
            if steep && !continues {
                return DeckOutcome::Fallback(DeckFailure::RampTooSteep);
            }
            // L'ouvrage continue : tablier tenu plat jusqu'à l'extrémité du way, la rampe ne
            // commence qu'au-delà (comme à la jonction d'un tunnel).
            held[side] = steep;
        }
    }
    let below = (u..=v).filter(|&k| deck[k - u] < mnt[k] - r.mnt_above_mns_tolerance_m).count();
    if below as f64 > r.max_below_ground_share * (v - u + 1) as f64 {
        return DeckOutcome::Fallback(DeckFailure::BelowGround);
    }
    let mut z = vec![f64::NAN; n];
    z[u..=v].copy_from_slice(&deck);
    let (mut first, mut last) = (u, v);
    if held[0] {
        z[..u].fill(deck[0]);
        first = 0;
    }
    if held[1] {
        z[v + 1..].fill(deck[deck.len() - 1]);
        last = n - 1;
    }
    DeckOutcome::Deck { z, first, last }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(n: usize) -> Vec<f64> {
        (0..n).map(|k| k as f64 * 5.0).collect()
    }

    fn rules() -> DeckBuilderRules {
        DeckBuilderRules::default()
    }

    #[test]
    fn median_ignores_nan_and_shrinks_at_the_ends() {
        let c = grid(5);
        let m = rolling_nan_median(&c, &[1.0, f64::NAN, 3.0, 10.0, 5.0], 5.0);
        assert_eq!(m[0], 1.0);
        assert_eq!(m[2], 6.5, "fenêtre {{NaN, 3, 10}} : médiane de 3 et 10");
        assert_eq!(m[4], 7.5);
    }

    #[test]
    fn slope_cap_is_lipschitz_idempotent_and_only_raises() {
        let c = grid(41);
        let raw: Vec<f64> = (0..41)
            .map(|k| if (15..25).contains(&k) { 0.0 } else { 20.0 + (k as f64 * 0.7).sin() })
            .collect();
        let mut z = raw.clone();
        cap_slope(&c, &mut z, 0.10);
        assert!(z.iter().zip(&raw).all(|(a, b)| a >= b), "ne fait que remonter");
        assert!(z.windows(2).all(|w| (w[1] - w[0]).abs() <= 0.5 + 1e-9), "10 % sur 5 m");
        let mut again = z.clone();
        cap_slope(&c, &mut again, 0.10);
        assert_eq!(again, z, "idempotent");
        // Le creux de 50 m est comblé à 10 % depuis chaque bord : au plus 2,5 m sous le bord.
        assert!(z[20] > 16.5, "{}", z[20]);
    }

    /// Pont de 200 m à 30 m au-dessus d'une vallée de 25 m de profondeur ; sol à 0 aux sondes.
    fn valley(n: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let c = grid(n);
        let mnt: Vec<f64> = c.iter().map(|&x| -25.0 * (std::f64::consts::PI * x / c[n - 1]).sin()).collect();
        let mns: Vec<f64> = c.iter().map(|&x| 5.0 + 0.02 * x).collect();
        (c, mnt, mns)
    }

    #[test]
    fn deck_follows_the_mns_and_ramps_stay_to_the_caller() {
        let (c, mnt, mns) = valley(41);
        let probes = [
            Some(Probe { c: -10.0, z: 4.0, mns: f64::NAN }),
            Some(Probe { c: 210.0, z: 9.5, mns: f64::NAN }),
        ];
        match build_deck(&c, &mnt, &mns, probes, &rules()) {
            DeckOutcome::Deck { z, first, last } => {
                assert_eq!((first, last), (0, 40));
                // La médiane rétrécit aux bouts (elle ne lit jamais au-delà des ancres) : 0,1 m au plus.
                assert!(z.iter().zip(&mns).all(|(a, b)| (a - b).abs() <= 0.1 + 1e-9), "{z:?}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn fewer_than_two_anchors_falls_back() {
        let (c, mnt, _) = valley(41);
        // MNS rugueux partout (treillis) : aucune ancre. Un motif de période 2 serait lisse pour
        // une médiane sur 5 stations ; période 5.
        let mns: Vec<f64> = (0..41).map(|k| [10.0, 14.0, 12.0, 15.0, 9.0][k % 5]).collect();
        let probes = [
            Some(Probe { c: -10.0, z: 0.0, mns: f64::NAN }),
            Some(Probe { c: 210.0, z: 0.0, mns: f64::NAN }),
        ];
        assert_eq!(
            build_deck(&c, &mnt, &mns, probes, &rules()),
            DeckOutcome::Fallback(DeckFailure::TooFewAnchors)
        );
    }

    #[test]
    fn steep_ramp_onto_an_untagged_continuation_keeps_the_deck() {
        // Le way s'arrête en l'air (viaduc d'accès non tagué) : la sonde lit le sol 30 m plus bas,
        // mais le MNS y prolonge le tablier.
        let (c, mnt, mns) = valley(41);
        let mns: Vec<f64> = mns.iter().map(|z| z + 25.0).collect();
        let probes =
            [Some(Probe { c: -10.0, z: 29.0, mns: 30.0 }), Some(Probe { c: 210.0, z: 0.0, mns: 34.0 })];
        match build_deck(&c, &mnt, &mns, probes, &rules()) {
            DeckOutcome::Deck { first, last, .. } => {
                assert_eq!((first, last), (0, 40), "tenu plat jusqu'au bout")
            }
            other => panic!("{other:?}"),
        }
        let probes =
            [Some(Probe { c: -10.0, z: 29.0, mns: 30.0 }), Some(Probe { c: 210.0, z: 0.0, mns: 0.2 })];
        assert_eq!(
            build_deck(&c, &mnt, &mns, probes, &rules()),
            DeckOutcome::Fallback(DeckFailure::RampTooSteep)
        );
    }

    #[test]
    fn deck_without_overground_keeps_the_ground() {
        let c = grid(11);
        let mnt = vec![3.0; 11];
        let mns = vec![3.2; 11];
        let probes = [Some(Probe { c: -10.0, z: 3.0, mns: 3.2 }), Some(Probe { c: 60.0, z: 3.0, mns: 3.2 })];
        assert_eq!(build_deck(&c, &mnt, &mns, probes, &rules()), DeckOutcome::KeptGround);
    }

    #[test]
    fn steep_ramp_or_deck_under_the_ground_falls_back() {
        let (c, mnt, mns) = valley(41);
        let probes =
            [Some(Probe { c: -10.0, z: -20.0, mns: -20.0 }), Some(Probe { c: 210.0, z: 0.0, mns: f64::NAN })];
        assert_eq!(
            build_deck(&c, &mnt, &mns, probes, &rules()),
            DeckOutcome::Fallback(DeckFailure::RampTooSteep)
        );
        // Sol au-dessus du MNS sur 11 stations (25 %) : pas d'ancre, tablier interpolé sous le sol.
        let mnt_high: Vec<f64> =
            (0..41).map(|k| if (10..=20).contains(&k) { 30.0 } else { mnt[k] }).collect();
        let probes = [
            Some(Probe { c: -10.0, z: 5.0, mns: f64::NAN }),
            Some(Probe { c: 210.0, z: 9.0, mns: f64::NAN }),
        ];
        assert_eq!(
            build_deck(&c, &mnt_high, &mns, probes, &rules()),
            DeckOutcome::Fallback(DeckFailure::BelowGround)
        );
    }

    #[test]
    fn a_smooth_sag_is_filled_by_the_slope_cap() {
        // Creux lisse de 20 m de large et 15 m de profondeur au milieu d'un tablier plat à 30 m :
        // ancré (lisse) mais comblé par le plafond, à 1 m du tablier au plus.
        let c = grid(61);
        let mnt = vec![0.0; 61];
        let mns: Vec<f64> = c
            .iter()
            .map(|&x| {
                let d = (x - 150.0).abs();
                if d < 10.0 {
                    30.0 - 15.0 * (1.0 - d / 10.0)
                } else {
                    30.0
                }
            })
            .collect();
        let probes = [
            Some(Probe { c: -10.0, z: 29.5, mns: f64::NAN }),
            Some(Probe { c: 310.0, z: 29.5, mns: f64::NAN }),
        ];
        match build_deck(&c, &mnt, &mns, probes, &rules()) {
            DeckOutcome::Deck { z, .. } => assert!(z[30] > 29.0, "{}", z[30]),
            other => panic!("{other:?}"),
        }
    }
}
