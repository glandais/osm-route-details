//! Tirages de la livraison B (note : `ord-retouche-lot0.md` §4.3, §5.3 et §7.3) : les points `E′` de `move_end`, les
//! fenêtres `(i, j)` de `resize` et l'échantillon de traces. Tout est déterministe : une graine par essai, jamais l'ordre
//! d'exécution. Aucune position ne sort du banc.

use ord_graph::util::SplitMix64;
use ord_loops_check::haversine_m;
use xxhash_rust::xxh3::xxh3_64;

use super::posed::Posed;

const EARTH_R: f64 = 6_371_008.8;

/// Le point à `dist_m` de `(lat, lon)` dans la direction `bearing_deg` (formule directe sur la sphère).
pub fn destination(lat: f64, lon: f64, bearing_deg: f64, dist_m: f64) -> (f64, f64) {
    let (phi1, lam1, th, d) =
        (lat.to_radians(), lon.to_radians(), bearing_deg.to_radians(), dist_m / EARTH_R);
    let phi2 = (phi1.sin() * d.cos() + phi1.cos() * d.sin() * th.cos()).asin();
    let lam2 = lam1 + (th.sin() * d.sin() * phi1.cos()).atan2(d.cos() - phi1.sin() * phi2.sin());
    (phi2.to_degrees(), (lam2.to_degrees() + 540.0).rem_euclid(360.0) - 180.0)
}

/// Un générateur par essai : la graine mélange l'empreinte de la trace, la graine du banc et une étiquette.
pub fn rng_for(id_hash: u64, seed: u64, tag: u64) -> SplitMix64 {
    let mut r = SplitMix64(id_hash ^ seed ^ tag.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    r.next_u64();
    r
}

/// Rang de tirage d'une trace pour l'échantillon (la règle stable de `holdout::select`).
pub fn sample_rank(id_hash: u64, seed: u64) -> u64 {
    xxh3_64(&(id_hash ^ seed).to_le_bytes())
}

/// Nature d'un essai de `move_end`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrialKind {
    /// À `r` km de l'arrivée d'origine, au cap tiré numéro `k`.
    Ring { r_km: f64, k: u8 },
    /// L'arrivée d'une autre trace de la même équipe, à 30 km ou moins.
    TeamEnd,
    /// `E′` = le départ d'un aller simple (information : écarté par D5, hors critères).
    CloseLoop,
}

impl TrialKind {
    pub fn label(&self) -> String {
        match self {
            Self::Ring { r_km, k } => format!("r{r_km}_b{k}"),
            Self::TeamEnd => "team_end".into(),
            Self::CloseLoop => "close_loop".into(),
        }
    }
}

/// Un essai : nature et point `(lat, lon)`. **Privé** : n'est jamais écrit.
#[derive(Clone, Copy, Debug)]
pub struct Trial {
    pub kind: TrialKind,
    pub pt: (f64, f64),
}

#[derive(Clone, Debug)]
pub struct MoveEndDraw {
    pub radii_km: Vec<f64>,
    pub bearings: u8,
    pub team_ends: bool,
    pub close_loop: bool,
}

/// Les essais d'une trace : `radii × bearings` points autour de l'arrivée, un lieu d'équipe, et pour un aller simple la
/// fermeture en boucle. `team_ends` : arrivées des autres traces de l'équipe (toutes, avec leur rang).
pub fn move_end_trials(
    p: &Posed,
    team_ends: &[(u32, (f64, f64))],
    d: &MoveEndDraw,
    seed: u64,
    one_way: bool,
) -> Vec<Trial> {
    let Some(&end) = p.line.pts.last() else { return Vec::new() };
    let mut out = Vec::new();
    for (ri, &r_km) in d.radii_km.iter().enumerate() {
        for k in 0..d.bearings {
            let mut rng = rng_for(p.id_hash, seed, 0x100 + (ri as u64) * 16 + u64::from(k));
            let bearing = rng.next_u64() as f64 / u64::MAX as f64 * 360.0;
            out.push(Trial {
                kind: TrialKind::Ring { r_km, k },
                pt: destination(end.0, end.1, bearing, r_km * 1000.0),
            });
        }
    }
    if d.team_ends {
        let near: Vec<(f64, f64)> = team_ends
            .iter()
            .filter(|(rank, pt)| {
                *rank != p.rank && {
                    let dm = haversine_m(end, *pt);
                    dm > 500.0 && dm <= 30_000.0
                }
            })
            .map(|x| x.1)
            .collect();
        if !near.is_empty() {
            let mut rng = rng_for(p.id_hash, seed, 0x200);
            out.push(Trial { kind: TrialKind::TeamEnd, pt: near[rng.below(near.len())] });
        }
    }
    if d.close_loop && one_way {
        if let Some(&start) = p.line.pts.first() {
            out.push(Trial { kind: TrialKind::CloseLoop, pt: start });
        }
    }
    out
}

/// Une fenêtre de `resize` : les coutures `i` et `j` (indices dans `Posed::seams`) et `ℓ = s_j − s_i`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Win {
    pub i: usize,
    pub j: usize,
    pub ell_m: f64,
    /// Le tronçon visé vaut 1,5 ou 3 fois `|Δ|` (rallonger), ou le rang du `j` (raccourcir) : pour le rendement.
    pub family: u8,
}

/// Les fenêtres d'une demande `δ` (note §5.3) : au plus `max` ; rien dans les 5 % des bouts ; `ℓ + Δ ≥ 1 km` et
/// `|Δ| ≥ 1 km`. `cand` : les coutures valides des deux côtés, `crow(i, j)` : vol d'oiseau entre deux coutures, m.
pub fn windows(
    p: &Posed,
    cand: &[usize],
    delta: f64,
    max: usize,
    seed: u64,
    crow: &dyn Fn(usize, usize) -> f64,
) -> Vec<Win> {
    let l = p.l_m;
    let big_delta = delta * l;
    if l <= 0.0 || big_delta.abs() < 1000.0 {
        return Vec::new();
    }
    let s = |k: usize| p.seams[k].s_m;
    let inner: Vec<usize> = cand.iter().copied().filter(|&k| s(k) >= 0.05 * l && s(k) <= 0.95 * l).collect();
    let tag = (delta * 1000.0).round() as i64 as u64;
    let mut out: Vec<Win> = Vec::new();
    if delta > 0.0 {
        for (fi, mult) in [1.5, 3.0].into_iter().enumerate() {
            let ell_t = mult * big_delta;
            let (lo, hi) = (0.05 * l, 0.95 * l - ell_t);
            if hi <= lo {
                continue;
            }
            for b in 0..6 {
                let (b0, b1) = (lo + (hi - lo) * b as f64 / 6.0, lo + (hi - lo) * (b + 1) as f64 / 6.0);
                let in_box: Vec<usize> = inner.iter().copied().filter(|&k| s(k) >= b0 && s(k) < b1).collect();
                if in_box.is_empty() {
                    continue;
                }
                let mut rng =
                    rng_for(p.id_hash, seed, 0x300 ^ tag.wrapping_mul(131) ^ (fi as u64 * 8 + b as u64));
                let i = in_box[rng.below(in_box.len())];
                let j = inner
                    .iter()
                    .copied()
                    .filter(|&j| s(j) > s(i) && (0.8 * ell_t..=1.25 * ell_t).contains(&(s(j) - s(i))))
                    .min_by(|&a, &b| (s(a) - s(i) - ell_t).abs().total_cmp(&(s(b) - s(i) - ell_t).abs()));
                if let Some(j) = j {
                    out.push(Win { i, j, ell_m: s(j) - s(i), family: fi as u8 });
                }
            }
        }
    } else {
        let abs = -big_delta;
        let (lo, hi) = (0.05 * l, 0.95 * l);
        for b in 0..6 {
            let (b0, b1) = (lo + (hi - lo) * b as f64 / 6.0, lo + (hi - lo) * (b + 1) as f64 / 6.0);
            let in_box: Vec<usize> = inner.iter().copied().filter(|&k| s(k) >= b0 && s(k) < b1).collect();
            if in_box.is_empty() {
                continue;
            }
            let mut rng = rng_for(p.id_hash, seed, 0x400 ^ tag.wrapping_mul(131) ^ b as u64);
            let i = in_box[rng.below(in_box.len())];
            let mut js: Vec<(f64, usize)> = inner
                .iter()
                .copied()
                .filter(|&j| s(j) > s(i) && s(j) - s(i) >= 1.5 * abs && s(j) - s(i) + big_delta >= 1000.0)
                .filter_map(|j| {
                    let c = crow(i, j).max(1.0);
                    let ratio = (s(j) - s(i) + big_delta) / c;
                    (ratio >= 1.3).then_some(((ratio - 1.6).abs(), j))
                })
                .collect();
            js.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            for (rk, &(_, j)) in js.iter().take(2).enumerate() {
                out.push(Win { i, j, ell_m: s(j) - s(i), family: rk as u8 });
            }
        }
    }
    out.truncate(max);
    out
}

#[cfg(test)]
mod tests {
    use ord_graph_match::walk::Walk;

    use super::*;
    use crate::retouche::continuity::Shape;
    use crate::retouche::posed::{PSeam, Seam};
    use crate::retouche::project::DocLine;

    /// Une trace synthétique de `n_km` km avec une couture tous les 300 m, sans marche (non lue par les tirages).
    fn fake(n_km: f64) -> Posed {
        let l = n_km * 1000.0;
        let seams = (1..)
            .map(|k| k as f64 * 300.0)
            .take_while(|&s| s < l - 100.0)
            .enumerate()
            .map(|(k, s)| PSeam {
                seam: Seam { join: k, node: k as u32, a_m: s, before_m: 300.0, after_m: 300.0 },
                s_m: s,
            })
            .collect();
        Posed {
            team: 0,
            letter: "B".into(),
            rank: 3,
            id_hash: 0xABCD,
            ty: 0,
            ride: ord_loops::RideType::Road,
            shape: Shape::Loop,
            walk: Walk { legs: vec![], joins: vec![] },
            line: DocLine::from_points(vec![(47.0, -1.5), (47.0 + l / 111_195.0, -1.5)]),
            l_m: l,
            seams,
            cum_end: vec![],
            edge_set: Default::default(),
        }
    }

    #[test]
    fn destination_is_at_the_asked_distance() {
        let p = destination(47.0, -1.5, 73.0, 5000.0);
        assert!((haversine_m((47.0, -1.5), p) - 5000.0).abs() < 1.0);
    }

    #[test]
    fn windows_stay_inside_the_inner_90_percent_are_at_most_twelve_and_stable() {
        let p = fake(40.0);
        let cand: Vec<usize> = (0..p.seams.len()).collect();
        let crow = |i: usize, j: usize| (p.seams[j].s_m - p.seams[i].s_m) * 0.3;
        for delta in [0.1, 0.25, 0.35, -0.1, -0.25, -0.35] {
            let w = windows(&p, &cand, delta, 12, 7, &crow);
            assert!(!w.is_empty() && w.len() <= 12, "{delta} : {}", w.len());
            for x in &w {
                let (si, sj) = (p.seams[x.i].s_m, p.seams[x.j].s_m);
                assert!(si >= 0.05 * p.l_m && sj <= 0.95 * p.l_m && sj > si, "{delta} {x:?}");
                assert!(x.ell_m + delta * p.l_m >= 1000.0);
            }
            assert_eq!(w, windows(&p, &cand, delta, 12, 7, &crow), "graine stable");
        }
        // Un autre graine change au moins un tirage.
        let a = windows(&p, &cand, 0.25, 12, 7, &crow);
        let b = windows(&p, &cand, 0.25, 12, 8, &crow);
        assert!(a != b);
        // Trop petit : |Δ| < 1 km.
        assert!(windows(&fake(5.0), &(0..10).collect::<Vec<_>>(), 0.1, 12, 7, &crow).is_empty());
    }

    #[test]
    fn move_end_trials_are_thirteen_for_a_loop_and_fourteen_for_a_one_way() {
        let p = fake(40.0);
        let d =
            MoveEndDraw { radii_km: vec![2.0, 5.0, 15.0], bearings: 4, team_ends: true, close_loop: true };
        let ends = [(3u32, (47.0, -1.5)), (9, (p.line.pts[1].0 + 0.1, -1.5))];
        let loop_trials = move_end_trials(&p, &ends, &d, 1, false);
        assert_eq!(loop_trials.len(), 13);
        assert_eq!(move_end_trials(&p, &ends, &d, 1, true).len(), 14);
        for t in loop_trials.iter().take(12) {
            let TrialKind::Ring { r_km, .. } = t.kind else { panic!() };
            let end = *p.line.pts.last().unwrap();
            assert!((haversine_m(end, t.pt) - r_km * 1000.0).abs() < 2.0);
        }
    }
}
