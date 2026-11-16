//! Le cœur de l'agrégation, commun à l'outil (qui y ajoute son rapport) et au service : jugement d'une arête d'équipe,
//! recouvrements d'une chaîne, comptage « une fois par parcours, par arête et par sens ».
//!
//! Les causes de non-appariement sont jugées dans l'ordre de la note de conception du lot 1 §4 ; « hors région » est
//! compté à part, ce n'est pas un échec.

use ord_graph::mmap::MappedGraph;
use rustc_hash::FxHashMap;
use xxhash_rust::xxh3::xxh3_64;

use crate::doc::TYPES;
use crate::job::{Job, Outcome, Region};
use crate::matcher::{self, Chain, Fail};

/// Causes d'échec dans l'ordre où elles sont jugées (hors région exclu : ce n'est pas un échec).
pub const CAUSES: [&str; 8] = [
    "border",
    "invalid",
    "no_osm_edge",
    "chain_not_connected",
    "geometry",
    "ambiguous_parallel",
    "way_kind",
    "direction_closed",
];
pub const BORDER: usize = 0;
pub const INVALID: usize = 1;
pub const WAY_KIND: usize = 6;
pub const DIRECTION_CLOSED: usize = 7;

fn cause_of(f: Fail) -> usize {
    match f {
        Fail::NoOsmEdge => 2,
        Fail::ChainNotConnected => 3,
        Fail::Geometry => 4,
        Fail::Ambiguous => 5,
    }
}

pub const N_TYPES: usize = TYPES.len();
/// Comptes par arête : `type * 2 + sens`.
pub const STRIDE: usize = 2 * N_TYPES;

#[derive(Clone, Copy, Debug, Default)]
pub struct EdgeAcc {
    pub counts: [u32; STRIDE],
    /// Parcours distincts, tous types et sens confondus.
    pub any: u32,
}

/// Un recouvrement d'arête de base par un parcours, dans un sens (0 = a→b, 1 = b→a).
#[derive(Clone, Copy, Debug)]
pub struct Cov {
    pub edge: u32,
    pub dir: u8,
    pub lo: f32,
    pub hi: f32,
    pub elen: f32,
}

/// Seuil de couverture : la moitié de l'arête, 50 m au plus.
pub fn coverage_threshold(elen: f64) -> f64 {
    (0.5 * elen).min(50.0)
}

/// Union des recouvrements par (arête, sens) : `f(arête, sens, mètres couverts, longueur)`.
pub fn union_coverage(covs: &mut [Cov], mut f: impl FnMut(u32, u8, f64, f64)) {
    covs.sort_by(|a, b| (a.edge, a.dir).cmp(&(b.edge, b.dir)).then(a.lo.total_cmp(&b.lo)));
    let mut i = 0;
    while i < covs.len() {
        let (edge, dir, elen) = (covs[i].edge, covs[i].dir, covs[i].elen);
        let (mut covered, mut cur_lo, mut cur_hi) = (0.0f64, f64::from(covs[i].lo), f64::from(covs[i].hi));
        let mut j = i + 1;
        while j < covs.len() && covs[j].edge == edge && covs[j].dir == dir {
            let (lo, hi) = (f64::from(covs[j].lo), f64::from(covs[j].hi));
            if lo <= cur_hi {
                cur_hi = cur_hi.max(hi);
            } else {
                covered += cur_hi - cur_lo;
                (cur_lo, cur_hi) = (lo, hi);
            }
            j += 1;
        }
        covered += cur_hi - cur_lo;
        f(edge, dir, covered, f64::from(elen));
        i = j;
    }
}

pub fn wk_family_mismatch(wk: u8, chain: &Chain) -> bool {
    let total = f64::from(chain.road_m + chain.nonroad_m);
    if total <= 0.0 {
        return false;
    }
    match wk {
        1 | 2 => f64::from(chain.nonroad_m) > total / 2.0,
        3..=7 => f64::from(chain.road_m) > total / 2.0,
        _ => false,
    }
}

/// Jugement d'une arête d'équipe.
#[derive(Clone, Copy, Debug)]
pub enum Verdict<'a> {
    /// Hors de la région : compté à part.
    Outside,
    /// Échec de cause `cause` (indice de [`CAUSES`]) ; `chain` quand le matcher en avait trouvé une.
    Fail { cause: usize, chain: Option<&'a Chain> },
    /// Appariée, retenue.
    Counted(&'a Chain),
}

/// Juge une arête d'équipe de famille `wk` dont la géométrie a donné `job`.
pub fn verdict(wk: u8, job: &Job) -> Verdict<'_> {
    match (&job.region, &job.outcome) {
        (Region::Outside, _) => Verdict::Outside,
        (Region::Border, _) => Verdict::Fail { cause: BORDER, chain: None },
        (_, Outcome::Invalid | Outcome::NotTried) => Verdict::Fail { cause: INVALID, chain: None },
        (_, Outcome::Unmatched(f)) => Verdict::Fail { cause: cause_of(*f), chain: None },
        (_, Outcome::Matched(chain)) => {
            if wk_family_mismatch(wk, chain) {
                Verdict::Fail { cause: WAY_KIND, chain: Some(chain) }
            } else if chain.closed_m > 0.0 {
                Verdict::Fail { cause: DIRECTION_CLOSED, chain: Some(chain) }
            } else {
                Verdict::Counted(chain)
            }
        }
    }
}

/// Ajoute à `covs` les recouvrements d'une chaîne retenue ; rend les kilomètres de morceaux tenus hors de la surcouche.
///
/// Un morceau de moins de `MIN_PIECE_M` dans un sens fermé ne rend pas la chaîne `direction_closed` (bruit de recalage
/// aux carrefours), mais il ne doit pas non plus compter dans la surcouche : celle-ci ne porte jamais un sens fermé
/// au vélo.
pub fn push_chain_covs(gf: &MappedGraph, chain: &Chain, covs: &mut Vec<Cov>) -> f64 {
    let mut skipped_km = 0.0;
    for p in &chain.pieces {
        let attrs = gf.edge(p.edge).attrs;
        if !(if p.forward() { attrs.fwd_ok() } else { attrs.bwd_ok() }) {
            skipped_km += p.len() / 1000.0;
            continue;
        }
        let (lo, hi) = (p.from.min(p.to), p.from.max(p.to));
        covs.push(Cov { edge: p.edge, dir: u8::from(!p.forward()), lo, hi, elen: p.elen });
    }
    skipped_km
}

/// Compte un parcours de type `ty` : une fois par arête et par sens recouvert au moins au seuil. Rend les
/// kilomètres comptés et les kilomètres sous le seuil. `covs` est consommé (trié sur place).
pub fn count_track(covs: &mut [Cov], ty: usize, counts: &mut FxHashMap<u32, EdgeAcc>) -> (f64, f64) {
    let mut counted_edges: FxHashMap<u32, [bool; 2]> = FxHashMap::default();
    let (mut counted_km, mut below_km) = (0.0f64, 0.0f64);
    union_coverage(covs, |edge, dir, covered, elen| {
        if covered + 1e-6 >= coverage_threshold(elen) {
            counted_km += covered / 1000.0;
            counted_edges.entry(edge).or_default()[usize::from(dir)] = true;
        } else {
            below_km += covered / 1000.0;
        }
    });
    for (edge, dirs) in counted_edges {
        let e = counts.entry(edge).or_default();
        for (d, on) in dirs.into_iter().enumerate() {
            if on {
                e.counts[ty * 2 + d] += 1;
            }
        }
        e.any += 1;
    }
    (counted_km, below_km)
}

/// Empreinte des paramètres d'appariement : change dès qu'une tolérance change.
pub fn params_hash() -> u64 {
    let s = format!(
        "corridor={};node={};edge={};hd={};lenabs={};lenrel={};amb={};piece={}",
        matcher::CORRIDOR_M,
        matcher::NODE_SNAP_M,
        matcher::EDGE_SNAP_M,
        matcher::HAUSDORFF_MAX_M,
        matcher::LEN_TOL_ABS_M,
        matcher::LEN_TOL_REL,
        matcher::AMBIGUITY_M,
        matcher::MIN_PIECE_M
    );
    xxh3_64(s.as_bytes())
}

/// Arêtes triées et comptes à plat (saturés à 65 535) d'une équipe, prêts pour l'écriture.
pub fn overlay_arrays(counts: &FxHashMap<u32, EdgeAcc>) -> (Vec<u32>, Vec<u16>) {
    let mut keys: Vec<u32> = counts.keys().copied().collect();
    keys.sort_unstable();
    let mut flat = Vec::with_capacity(keys.len() * STRIDE);
    for k in &keys {
        flat.extend(counts[k].counts.iter().map(|&c| c.min(u32::from(u16::MAX)) as u16));
    }
    (keys, flat)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cov(edge: u32, dir: u8, lo: f32, hi: f32) -> Cov {
        Cov { edge, dir, lo, hi, elen: 200.0 }
    }

    #[test]
    fn coverage_is_a_union_not_a_sum() {
        let mut v = vec![
            cov(1, 0, 0.0, 60.0),
            cov(1, 0, 40.0, 100.0),
            cov(1, 0, 150.0, 160.0),
            cov(1, 1, 0.0, 5.0),
            cov(2, 0, 0.0, 30.0),
        ];
        let mut out = Vec::new();
        union_coverage(&mut v, |e, d, c, _| out.push((e, d, c)));
        assert_eq!(out, vec![(1, 0, 110.0), (1, 1, 5.0), (2, 0, 30.0)]);
    }

    #[test]
    fn threshold_is_half_the_edge_capped_at_50_m() {
        assert_eq!(coverage_threshold(200.0), 50.0);
        assert_eq!(coverage_threshold(60.0), 30.0);
        assert_eq!(coverage_threshold(1.0), 0.5);
    }

    #[test]
    fn way_kind_needs_a_majority_of_the_other_family() {
        let mut c = Chain { pieces: vec![], hausdorff: 0.0, road_m: 100.0, nonroad_m: 0.0, closed_m: 0.0 };
        assert!(!wk_family_mismatch(1, &c));
        assert!(wk_family_mismatch(4, &c));
        assert!(!wk_family_mismatch(0, &c), "wk absent : pas de contrôle");
        c.road_m = 40.0;
        c.nonroad_m = 60.0;
        assert!(wk_family_mismatch(2, &c));
        assert!(!wk_family_mismatch(4, &c));
    }

    #[test]
    fn a_track_counts_an_edge_once_per_direction() {
        let mut counts = FxHashMap::default();
        let mut covs = vec![cov(1, 0, 0.0, 120.0), cov(1, 0, 60.0, 200.0), cov(1, 1, 0.0, 10.0)];
        let (counted, below) = count_track(&mut covs, 1, &mut counts);
        assert!((counted - 0.2).abs() < 1e-9 && (below - 0.01).abs() < 1e-9);
        let e = counts[&1];
        assert_eq!((e.counts[2], e.counts[3], e.any), (1, 0, 1));
        let (edges, flat) = overlay_arrays(&counts);
        assert_eq!(edges, vec![1]);
        assert_eq!(flat.len(), STRIDE);
    }
}
