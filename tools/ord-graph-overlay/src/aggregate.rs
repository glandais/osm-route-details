//! Agrégation séquentielle (donc déterministe) : causes, kilomètres, comptage par parcours, continuité.
//!
//! Les causes de non-appariement sont jugées dans l'ordre de la note de conception §4 ; « hors région » est
//! compté à part, ce n'est pas un échec.

use ord_graph::mmap::MappedGraph;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::checks::{adjacency, internal_gaps, Adjacency};
use crate::export::{OW_NAMES, TYPES, WK_NAMES};
use crate::matcher::{Chain, Fail};
use crate::pipeline::{EdgeRec, Ingest, Job, Outcome, Region};

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
const BORDER: usize = 0;
const INVALID: usize = 1;
const WAY_KIND: usize = 6;
const DIRECTION_CLOSED: usize = 7;

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
const N_WK: usize = WK_NAMES.len();

/// Tolérance de contiguïté de deux arêtes d'équipe sur l'axe de la trace, en mètres.
const CONTIGUITY_M: f32 = 1.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct EdgeAcc {
    pub counts: [u32; STRIDE],
    /// Parcours distincts, tous types et sens confondus.
    pub any: u32,
}

#[derive(Debug, Default)]
pub struct Continuity {
    pub pairs: u64,
    pub adjacent: u64,
    pub same_edge: u64,
    /// Paires contiguës, toutes deux dans la région, dont une chaîne n'est pas appariée.
    pub skipped_unmatched: u64,
    /// Paires contiguës dont une arête est hors région ou à cheval sur sa limite.
    pub skipped_outside: u64,
    pub gaps_m: Vec<f32>,
    /// Raccords non adjacents à l'intérieur de chaînes distinctes (devrait rester 0).
    pub internal_chains: u64,
    pub internal_gaps: u64,
}

/// Contrôle des champs `fo` / `to` / `p` (note §3) sur les arêtes appariées.
#[derive(Debug, Default)]
pub struct PartialCheck {
    /// Arêtes appariées portant `fo`, `to` et `trv`.
    pub checked: u64,
    /// `to - fo` s'écarte de `trv` de plus de max(5 m, 5 %).
    pub offsets_vs_trv: u64,
    /// Arêtes dont `fo` est au nœud (≤ 1,5 m) : le premier morceau devrait partir d'un bout d'arête.
    pub start_expected_node: u64,
    pub start_not_at_node: u64,
    /// Idem pour la sortie : `len - to` ≤ 1,5 m.
    pub end_expected_node: u64,
    pub end_not_at_node: u64,
}

#[derive(Debug, Default)]
pub struct TeamAcc {
    pub id: u64,
    pub rows: u64,
    pub excluded: u64,
    pub unreadable: u64,
    pub tracks_in: u64,
    pub tracks_partial: u64,
    pub tracks_out: u64,
    pub tracks_empty: u64,
    pub team_edges: u64,
    pub km_total: f64,
    pub km_by_type: [f64; N_TYPES],
    pub km_outside: f64,
    pub km_matched: f64,
    pub km_counted: f64,
    pub km_below_threshold: f64,
    /// Morceaux (< `MIN_PIECE_M`) de chaînes appariées dans un sens fermé au vélo, tenus hors de la surcouche.
    pub km_closed_piece_skipped: f64,
    pub km_unmatched: [f64; CAUSES.len()],
    pub km_unmatched_wk: [[f64; N_WK]; CAUSES.len()],
    pub km_total_wk: [f64; N_WK],
    pub km_closed_ow: [f64; OW_NAMES.len()],
    pub edges_outside: u64,
    pub edges_by_cause: [u64; CAUSES.len()],
    pub geoms_outside: FxHashSet<u32>,
    pub geoms_matched: FxHashSet<u32>,
    pub geoms_by_cause: [FxHashSet<u32>; CAUSES.len()],
    pub counts: FxHashMap<u32, EdgeAcc>,
    pub continuity: Continuity,
    pub partial: PartialCheck,
    /// Arêtes d'équipe appariées jusque dans la surcouche (indices de `Ingest::edges`), pour l'échantillon.
    pub matched_occurrences: Vec<u32>,
    /// Hausdorff de l'appariement par géométrie distincte appariée (mètres).
    pub matcher_hausdorff: Vec<f32>,
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

fn wk_family_mismatch(wk: u8, chain: &Chain) -> bool {
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

/// Distance d'un point d'arête (à `off` m de `a`) au bout le plus proche de l'arête, en mètres.
fn to_nearest_end(off: f32, elen: f32) -> f32 {
    off.abs().min((elen - off).abs())
}

fn check_partial(c: &mut PartialCheck, rec: &EdgeRec, trv: f32, chain: &Chain) {
    if rec.from_off.is_nan() || rec.to_off.is_nan() || rec.trv.is_nan() {
        return;
    }
    c.checked += 1;
    let tol = 5.0f32.max(0.05 * trv);
    if ((rec.to_off - rec.from_off).abs() - trv).abs() > tol {
        c.offsets_vs_trv += 1;
    }
    let (Some(first), Some(last)) = (chain.pieces.first(), chain.pieces.last()) else { return };
    if rec.from_off <= 1.5 {
        c.start_expected_node += 1;
        if to_nearest_end(first.from, first.elen) > 1.5 {
            c.start_not_at_node += 1;
        }
    }
    if !rec.len.is_nan() && rec.len - rec.to_off <= 1.5 {
        c.end_expected_node += 1;
        if to_nearest_end(last.to, last.elen) > 1.5 {
            c.end_not_at_node += 1;
        }
    }
}

fn contiguous(a: &EdgeRec, b: &EdgeRec) -> bool {
    (a.to_node >= 0 && a.to_node == b.from_node)
        || (!a.tm.is_nan() && !b.fm.is_nan() && (a.tm - b.fm).abs() <= CONTIGUITY_M)
}

/// Agrège toutes les traces ; `teams[i]` suit `ing.teams[i]`.
pub fn aggregate(gf: &MappedGraph, ing: &Ingest, jobs: &[Job]) -> Vec<TeamAcc> {
    let mut teams: Vec<TeamAcc> = ing
        .teams
        .iter()
        .map(|t| TeamAcc {
            id: t.id,
            rows: t.rows,
            excluded: t.excluded,
            unreadable: t.unreadable,
            ..Default::default()
        })
        .collect();
    // Chaînes distinctes déjà contrôlées en interne (une fois chacune par équipe suffirait ; une fois tout court).
    let mut internal_done: FxHashSet<u32> = FxHashSet::default();
    let mut covs: Vec<Cov> = Vec::new();
    for tr in &ing.tracks {
        let acc = &mut teams[tr.team];
        let (mut n_in, mut n_out, mut n_border) = (0usize, 0usize, 0usize);
        covs.clear();
        let mut prev: Option<(usize, bool)> = None; // (indice de l'arête, appariée par le matcher)
        for k in tr.lo..tr.hi {
            let rec = &ing.edges[k];
            let job = &jobs[rec.geom as usize];
            let trv = if rec.trv.is_nan() { job.glen } else { rec.trv };
            let km = f64::from(trv) / 1000.0;
            let wk = usize::from(rec.wk);
            acc.team_edges += 1;
            acc.km_total += km;
            acc.km_by_type[usize::from(tr.ty)] += km;
            acc.km_total_wk[wk] += km;
            match job.region {
                Region::Inside => n_in += 1,
                Region::Outside => n_out += 1,
                Region::Border => n_border += 1,
            }
            let fail = |acc: &mut TeamAcc, cause: usize| {
                acc.km_unmatched[cause] += km;
                acc.km_unmatched_wk[cause][wk] += km;
                acc.edges_by_cause[cause] += 1;
                acc.geoms_by_cause[cause].insert(rec.geom);
            };
            let mut matcher_chain: Option<&Chain> = None;
            match (&job.region, &job.outcome) {
                (Region::Outside, _) => {
                    acc.km_outside += km;
                    acc.edges_outside += 1;
                    acc.geoms_outside.insert(rec.geom);
                }
                (Region::Border, _) => fail(acc, BORDER),
                (_, Outcome::Invalid | Outcome::NotTried) => fail(acc, INVALID),
                (_, Outcome::Unmatched(f)) => fail(acc, cause_of(*f)),
                (_, Outcome::Matched(chain)) => {
                    matcher_chain = Some(chain);
                    if wk_family_mismatch(rec.wk, chain) {
                        fail(acc, WAY_KIND);
                    } else if chain.closed_m > 0.0 {
                        fail(acc, DIRECTION_CLOSED);
                        acc.km_closed_ow[usize::from(rec.ow)] += km;
                    } else {
                        acc.km_matched += km;
                        acc.matched_occurrences.push(k as u32);
                        if acc.geoms_matched.insert(rec.geom) {
                            acc.matcher_hausdorff.push(chain.hausdorff);
                        }
                        check_partial(&mut acc.partial, rec, trv, chain);
                        for p in &chain.pieces {
                            // Un morceau de moins de `MIN_PIECE_M` dans un sens fermé ne rend pas la chaîne
                            // `direction_closed` (bruit de recalage aux carrefours), mais il ne doit pas non plus
                            // compter dans la surcouche : celle-ci ne porte jamais un sens fermé au vélo.
                            let attrs = gf.edge(p.edge).attrs;
                            if !(if p.forward() { attrs.fwd_ok() } else { attrs.bwd_ok() }) {
                                acc.km_closed_piece_skipped += p.len() / 1000.0;
                                continue;
                            }
                            let (lo, hi) = (p.from.min(p.to), p.from.max(p.to));
                            covs.push(Cov {
                                edge: p.edge,
                                dir: u8::from(!p.forward()),
                                lo,
                                hi,
                                elen: p.elen,
                            });
                        }
                    }
                }
            }
            // Continuité : arêtes d'équipe contiguës dans le document, toutes deux appariées par le matcher.
            if let Some(chain) = matcher_chain {
                if internal_done.insert(rec.geom) {
                    acc.continuity.internal_chains += 1;
                    acc.continuity.internal_gaps += internal_gaps(gf, chain) as u64;
                }
            }
            if let Some((pk, prev_matched)) = prev {
                if contiguous(&ing.edges[pk], rec) {
                    match (prev_matched, matcher_chain) {
                        (true, Some(next)) => {
                            let Outcome::Matched(before) = &jobs[ing.edges[pk].geom as usize].outcome else {
                                unreachable!("prev_matched garantit une chaîne")
                            };
                            let (last, first) = (before.pieces.last(), next.pieces.first());
                            if let (Some(last), Some(first)) = (last, first) {
                                let c = &mut acc.continuity;
                                c.pairs += 1;
                                match adjacency(gf, last, first) {
                                    Adjacency::SameEdge => {
                                        c.adjacent += 1;
                                        c.same_edge += 1;
                                    }
                                    Adjacency::SharedNode => c.adjacent += 1,
                                    Adjacency::Gap(m) => c.gaps_m.push(m as f32),
                                }
                            }
                        }
                        _ => {
                            let inside = |r: &EdgeRec| jobs[r.geom as usize].region == Region::Inside;
                            if inside(&ing.edges[pk]) && inside(rec) {
                                acc.continuity.skipped_unmatched += 1;
                            } else {
                                acc.continuity.skipped_outside += 1;
                            }
                        }
                    }
                }
            }
            prev = Some((k, matcher_chain.is_some()));
        }
        if n_in + n_out + n_border == 0 {
            acc.tracks_empty += 1;
        } else if n_out == 0 && n_border == 0 {
            acc.tracks_in += 1;
        } else if n_in == 0 && n_border == 0 {
            acc.tracks_out += 1;
        } else {
            acc.tracks_partial += 1;
        }
        // Comptage : une fois par parcours, par arête et par sens.
        let ty = usize::from(tr.ty);
        let mut counted_edges: FxHashMap<u32, [bool; 2]> = FxHashMap::default();
        let (mut counted_km, mut below_km) = (0.0f64, 0.0f64);
        union_coverage(&mut covs, |edge, dir, covered, elen| {
            if covered + 1e-6 >= coverage_threshold(elen) {
                counted_km += covered / 1000.0;
                counted_edges.entry(edge).or_default()[usize::from(dir)] = true;
            } else {
                below_km += covered / 1000.0;
            }
        });
        acc.km_counted += counted_km;
        acc.km_below_threshold += below_km;
        for (edge, dirs) in counted_edges {
            let e = acc.counts.entry(edge).or_default();
            for (d, on) in dirs.into_iter().enumerate() {
                if on {
                    e.counts[ty * 2 + d] += 1;
                }
            }
            e.any += 1;
        }
    }
    teams
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
}
