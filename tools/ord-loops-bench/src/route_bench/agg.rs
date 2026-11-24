//! Agrégats du banc d'itinéraire : moyennes, médianes, quantiles, intervalles de confiance par bootstrap **par trace**
//! (les tronçons d'une même trace ne sont pas indépendants), différences appariées à la variante neutre, parts des
//! kilomètres par case de voie.

use std::collections::BTreeMap;

use rayon::prelude::*;
use serde::Serialize;

use crate::likeness::shape::{quantile_sorted, sorted};

/// Nombre de tirages du bootstrap.
pub const BOOT: usize = 1000;

/// Ce qu'un agrégat lit d'un tronçon pour une variante.
#[derive(Clone, Copy, Debug)]
pub struct Obs {
    /// Grappe du bootstrap : la trace.
    pub cluster: usize,
    pub ok: bool,
    /// Jaccard (0 si échec).
    pub jaccard: f64,
    /// Jaccard de la variante neutre sur le même tronçon (0 si échec).
    pub jaccard_neutral: f64,
    pub precision: f64,
    pub recall: f64,
    pub detour: f64,
    pub ms: f64,
    pub km: [f64; 7],
    pub offroad_km: f64,
    pub ref_km: [f64; 7],
    pub ref_offroad_km: f64,
}

/// Les agrégats d'un groupe de tronçons pour une variante.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Stats {
    pub n: usize,
    pub clusters: usize,
    pub fails: usize,
    pub fail_rate: f64,
    pub jac_mean: f64,
    pub jac_mean_ci: [f64; 2],
    pub jac_median: f64,
    pub jac_median_ci: Option<[f64; 2]>,
    /// Moyenne appariée de `J(variante) − J(neutre)` sur les mêmes tronçons, et son intervalle.
    pub d_neutral: f64,
    pub d_neutral_ci: [f64; 2],
    /// Sur les tronçons trouvés.
    pub precision_mean: f64,
    pub recall_mean: f64,
    pub detour_p50: f64,
    pub detour_p90: f64,
    pub ms_p50: f64,
    pub ms_p90: f64,
    /// Parts des kilomètres par case (itinéraire et référence, sur les tronçons trouvés), et part non revêtue.
    pub route_share: [f64; 7],
    pub ref_share: [f64; 7],
    pub route_offroad_share: f64,
    pub ref_offroad_share: f64,
    pub route_km: f64,
    pub ref_km: f64,
}

/// Générateur reproductible (splitmix64).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Regroupe des valeurs par grappe : `(somme, nombre)` par grappe, dans l'ordre des grappes.
pub fn by_cluster(obs: &[(usize, f64)]) -> Vec<(f64, f64)> {
    let mut m: BTreeMap<usize, (f64, f64)> = BTreeMap::new();
    for &(c, v) in obs {
        let e = m.entry(c).or_default();
        e.0 += v;
        e.1 += 1.0;
    }
    m.into_values().collect()
}

/// Intervalle à 95 % (percentiles 2,5 et 97,5) de la moyenne par bootstrap des grappes.
pub fn boot_mean_ci(groups: &[(f64, f64)], b: usize, seed: u64) -> [f64; 2] {
    if groups.is_empty() {
        return [f64::NAN, f64::NAN];
    }
    let mut rng = Rng::new(seed);
    let mut means = Vec::with_capacity(b);
    for _ in 0..b {
        let (mut s, mut n) = (0.0, 0.0);
        for _ in 0..groups.len() {
            let g = groups[rng.below(groups.len())];
            s += g.0;
            n += g.1;
        }
        means.push(if n > 0.0 { s / n } else { f64::NAN });
    }
    let s = sorted(&means);
    [quantile_sorted(&s, 0.025).unwrap_or(f64::NAN), quantile_sorted(&s, 0.975).unwrap_or(f64::NAN)]
}

/// Intervalle à 95 % de la médiane par bootstrap des grappes (valeurs de chaque grappe).
pub fn boot_median_ci(groups: &[Vec<f64>], b: usize, seed: u64) -> [f64; 2] {
    if groups.is_empty() {
        return [f64::NAN, f64::NAN];
    }
    let mut rng = Rng::new(seed);
    let mut meds = Vec::with_capacity(b);
    let mut buf: Vec<f64> = Vec::new();
    for _ in 0..b {
        buf.clear();
        for _ in 0..groups.len() {
            buf.extend_from_slice(&groups[rng.below(groups.len())]);
        }
        buf.sort_by(f64::total_cmp);
        meds.push(quantile_sorted(&buf, 0.5).unwrap_or(f64::NAN));
    }
    let s = sorted(&meds);
    [quantile_sorted(&s, 0.025).unwrap_or(f64::NAN), quantile_sorted(&s, 0.975).unwrap_or(f64::NAN)]
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        f64::NAN
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn q(v: &[f64], p: f64) -> f64 {
    quantile_sorted(&sorted(v), p).unwrap_or(f64::NAN)
}

/// Les agrégats d'un groupe. `median_ci` : calculer aussi l'intervalle de la médiane (plus coûteux).
pub fn stats(obs: &[Obs], seed: u64, median_ci: bool) -> Stats {
    let n = obs.len();
    if n == 0 {
        return Stats::default();
    }
    let fails = obs.iter().filter(|o| !o.ok).count();
    let jac: Vec<f64> = obs.iter().map(|o| o.jaccard).collect();
    let jc: Vec<(usize, f64)> = obs.iter().map(|o| (o.cluster, o.jaccard)).collect();
    let dc: Vec<(usize, f64)> = obs.iter().map(|o| (o.cluster, o.jaccard - o.jaccard_neutral)).collect();
    let groups = by_cluster(&jc);
    let ok: Vec<&Obs> = obs.iter().filter(|o| o.ok).collect();
    let col = |f: &dyn Fn(&Obs) -> f64| ok.iter().map(|o| f(o)).collect::<Vec<f64>>();
    let (mut rk, mut fk) = ([0.0; 7], [0.0; 7]);
    let (mut ro, mut fo) = (0.0, 0.0);
    for o in &ok {
        for k in 0..7 {
            rk[k] += o.km[k];
            fk[k] += o.ref_km[k];
        }
        ro += o.offroad_km;
        fo += o.ref_offroad_km;
    }
    let (rt, ft): (f64, f64) = (rk.iter().sum(), fk.iter().sum());
    let share = |a: [f64; 7], t: f64| a.map(|x| if t > 0.0 { x / t } else { f64::NAN });
    let median_ci = median_ci.then(|| {
        let mut m: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
        for o in obs {
            m.entry(o.cluster).or_default().push(o.jaccard);
        }
        boot_median_ci(&m.into_values().collect::<Vec<_>>(), BOOT, seed ^ 0x6d65_6469)
    });
    Stats {
        n,
        clusters: groups.len(),
        fails,
        fail_rate: fails as f64 / n as f64,
        jac_mean: mean(&jac),
        jac_mean_ci: boot_mean_ci(&groups, BOOT, seed),
        jac_median: q(&jac, 0.5),
        jac_median_ci: median_ci,
        d_neutral: mean(&dc.iter().map(|x| x.1).collect::<Vec<_>>()),
        d_neutral_ci: boot_mean_ci(&by_cluster(&dc), BOOT, seed ^ 0x64),
        precision_mean: mean(&col(&|o| o.precision)),
        recall_mean: mean(&col(&|o| o.recall)),
        detour_p50: q(&col(&|o| o.detour), 0.5),
        detour_p90: q(&col(&|o| o.detour), 0.9),
        ms_p50: q(&col(&|o| o.ms), 0.5),
        ms_p90: q(&col(&|o| o.ms), 0.9),
        route_share: share(rk, rt),
        ref_share: share(fk, ft),
        route_offroad_share: if rt > 0.0 { ro / rt } else { f64::NAN },
        ref_offroad_share: if ft > 0.0 { fo / ft } else { f64::NAN },
        route_km: rt,
        ref_km: ft,
    }
}

/// Les agrégats de plusieurs groupes en parallèle (même ordre).
pub fn stats_many(groups: Vec<(Vec<Obs>, u64, bool)>) -> Vec<Stats> {
    groups.into_par_iter().map(|(o, seed, m)| stats(&o, seed, m)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(cluster: usize, ok: bool, j: f64, jn: f64) -> Obs {
        Obs {
            cluster,
            ok,
            jaccard: j,
            jaccard_neutral: jn,
            precision: j,
            recall: j,
            detour: 1.1,
            ms: 10.0,
            km: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            offroad_km: 0.5,
            ref_km: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0],
            ref_offroad_km: 0.0,
        }
    }

    #[test]
    fn means_failures_and_shares() {
        let o = vec![obs(0, true, 0.8, 0.6), obs(0, true, 0.4, 0.4), obs(1, false, 0.0, 0.2)];
        let s = stats(&o, 1, true);
        assert_eq!((s.n, s.clusters, s.fails), (3, 2, 1));
        assert!((s.jac_mean - 0.4).abs() < 1e-12);
        assert!((s.d_neutral - (0.2 + 0.0 - 0.2) / 3.0).abs() < 1e-12);
        // Précision et rappel sur les trouvés seulement.
        assert!((s.precision_mean - 0.6).abs() < 1e-12);
        assert!((s.route_share[0] - 0.5).abs() < 1e-12 && (s.ref_share[6] - 1.0).abs() < 1e-12);
        assert!((s.route_offroad_share - 0.25).abs() < 1e-12);
        assert!(s.jac_mean_ci[0] <= s.jac_mean && s.jac_mean <= s.jac_mean_ci[1]);
        assert!(s.jac_median_ci.is_some());
    }

    #[test]
    fn bootstrap_is_by_cluster_and_reproducible() {
        // Deux grappes très différentes : l'intervalle couvre l'écart entre elles, pas celui d'un tirage par tronçon.
        let g = vec![(10.0, 10.0), (0.0, 10.0)];
        let ci = boot_mean_ci(&g, 2000, 3);
        assert!(ci[0] <= 0.05 && ci[1] >= 0.95, "{ci:?}");
        assert_eq!(ci, boot_mean_ci(&g, 2000, 3));
        // Une seule grappe : l'intervalle est un point.
        let one = boot_mean_ci(&[(3.0, 6.0)], 100, 1);
        assert_eq!(one, [0.5, 0.5]);
        assert_eq!(by_cluster(&[(2, 1.0), (0, 2.0), (2, 3.0)]), vec![(2.0, 1.0), (4.0, 2.0)]);
    }
}
