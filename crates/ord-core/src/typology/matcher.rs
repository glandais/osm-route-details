//! Map-matching HMM (Newson & Krumm, à la meili) sur le graphe local, sans Valhalla (T03 §5).
//!
//! Repris d'osm-surface (`src/matcher.rs`, Apache-2.0, © Gabriel Landais) : candidats, Viterbi en
//! coûts, Dijkstra borné à tableaux réutilisés, sauts en ligne droite. Écarts : observations
//! prises sur les stations du parcours (avec leur distance), observations sans donnée (`NoData`)
//! près d'une tuile absente ou en panne, chaînes de moins de `min_chain_m` rejetées.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use ord_domain::budget::Budget;
use ord_domain::geo::{dist_units, haversine_m, project_on_segment};
use ord_domain::LatLon;
use rustc_hash::FxHashSet;

use super::graph::{SegId, StreetGraph};
use super::TypologyError;
use crate::rules::MatchParams;

/// Une observation : station du parcours.
#[derive(Clone, Copy, PartialEq)]
pub struct Obs {
    /// Unités globales.
    pub p: [f64; 2],
    pub at: LatLon,
    pub d_m: f64,
    /// Pas de candidat par construction : le disque de recherche touche une tuile absente
    /// (`Some(false)`) ou en panne (`Some(true)`).
    pub no_data: Option<bool>,
}

impl std::fmt::Debug for Obs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Obs").field("d_m", &self.d_m).field("no_data", &self.no_data).finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cand {
    pub seg: SegId,
    /// Position sur le segment, du nœud `a` au nœud `b`.
    pub t: f64,
    pub pos: [f64; 2],
    pub dist_m: f64,
}

/// Morceau de segment parcouru de `t0` à `t1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub seg: SegId,
    pub t0: f64,
    pub t1: f64,
}

/// Passage d'une observation à la suivante dans une même chaîne.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Route(Vec<Piece>),
    /// En ligne droite, entre deux rues que le réseau ne relie pas.
    Jump,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ObsState {
    Matched {
        cand: Cand,
        chain: u32,
    },
    Unmatched,
    /// `failed` : tuile en panne (sinon absente de l'emprise).
    NoData {
        failed: bool,
    },
}

#[derive(Clone, Debug)]
pub struct MatchedPath {
    pub obs: Vec<Obs>,
    pub state: Vec<ObsState>,
    /// `steps[t]` : de `t` à `t + 1`, quand les deux sont appariées dans la même chaîne.
    pub steps: Vec<Option<Step>>,
}

#[derive(Clone, Copy, PartialEq)]
struct HeapItem(f64, u32);
impl Eq for HeapItem {}
impl Ord for HeapItem {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0).then_with(|| other.1.cmp(&self.1))
    }
}
impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

const NO_PRED: u32 = u32::MAX;

/// Dijkstra borné qui réutilise ses tableaux d'un appel à l'autre.
struct Dijkstra {
    dist: Vec<f64>,
    pred: Vec<u32>,
    stamp: Vec<u32>,
    settled: Vec<u32>,
    run: u32,
    heap: BinaryHeap<HeapItem>,
}

impl Dijkstra {
    fn new(n: usize) -> Self {
        Self {
            dist: vec![0.0; n],
            pred: vec![NO_PRED; n],
            stamp: vec![0; n],
            settled: vec![0; n],
            run: 0,
            heap: BinaryHeap::new(),
        }
    }

    /// Depuis les deux bouts du segment du candidat, jusqu'à ce que toutes les cibles soient
    /// atteintes ou que la distance dépasse `limit` (relâchement jusqu'à 1,5 × `limit`).
    fn run_from(&mut self, g: &StreetGraph, c: &Cand, targets: &[u32], limit: f64) {
        self.run += 1;
        self.heap.clear();
        let s = g.segs[c.seg as usize];
        self.relax(s.a, c.t * s.len_m, NO_PRED);
        self.relax(s.b, (1.0 - c.t) * s.len_m, NO_PRED);
        let mut left = targets.len();
        while let Some(HeapItem(d, n)) = self.heap.pop() {
            if self.settled[n as usize] == self.run || d > self.dist[n as usize] {
                continue;
            }
            self.settled[n as usize] = self.run;
            if targets.contains(&n) {
                left -= 1;
                if left == 0 {
                    break;
                }
            }
            if d > limit {
                break;
            }
            for &(seg, other) in g.neighbours(n) {
                let nd = d + g.segs[seg as usize].len_m;
                if nd <= limit * 1.5 {
                    self.relax(other, nd, seg);
                }
            }
        }
    }

    fn relax(&mut self, node: u32, d: f64, pred: u32) {
        let i = node as usize;
        if self.stamp[i] != self.run || d < self.dist[i] {
            self.stamp[i] = self.run;
            self.dist[i] = d;
            self.pred[i] = pred;
            self.heap.push(HeapItem(d, node));
        }
    }

    fn dist_to(&self, node: u32) -> Option<f64> {
        let i = node as usize;
        (self.stamp[i] == self.run).then_some(self.dist[i])
    }

    /// Longueur de route du candidat source à `c`, et par quel bout de son segment (`true` : `a`).
    fn route_to(&self, g: &StreetGraph, from: &Cand, c: &Cand) -> Option<(f64, Option<bool>)> {
        let s = g.segs[c.seg as usize];
        let mut best: Option<(f64, Option<bool>)> = None;
        if from.seg == c.seg {
            best = Some(((c.t - from.t).abs() * s.len_m, None));
        }
        for (node, via_a, off) in [(s.a, true, c.t * s.len_m), (s.b, false, (1.0 - c.t) * s.len_m)] {
            if let Some(d) = self.dist_to(node) {
                let total = d + off;
                if best.is_none_or(|(b, _)| total < b - 1e-9) {
                    best = Some((total, Some(via_a)));
                }
            }
        }
        best
    }

    /// Morceaux parcourus du candidat source à `c`, en entrant par `via_a`.
    fn pieces_to(&self, g: &StreetGraph, from: &Cand, c: &Cand, via_a: Option<bool>) -> Vec<Piece> {
        let Some(via_a) = via_a else {
            return vec![Piece { seg: c.seg, t0: from.t, t1: c.t }];
        };
        let s = g.segs[c.seg as usize];
        let mut node = if via_a { s.a } else { s.b };
        let mut middle: Vec<Piece> = Vec::new();
        loop {
            let p = self.pred[node as usize];
            if p == NO_PRED {
                break;
            }
            let ps = g.segs[p as usize];
            let (t0, t1, prev) = if ps.b == node { (0.0, 1.0, ps.a) } else { (1.0, 0.0, ps.b) };
            middle.push(Piece { seg: p, t0, t1 });
            node = prev;
        }
        middle.reverse();
        let fs = g.segs[from.seg as usize];
        let exit_t = if node == fs.a { 0.0 } else { 1.0 };
        let mut out = Vec::with_capacity(middle.len() + 2);
        out.push(Piece { seg: from.seg, t0: from.t, t1: exit_t });
        out.extend(middle);
        out.push(Piece { seg: c.seg, t0: if via_a { 0.0 } else { 1.0 }, t1: c.t });
        out.retain(|p| p.t0 != p.t1);
        out
    }
}

/// Candidats d'une observation : projections les plus proches, une par position (¼ d'unité).
pub fn candidates(g: &StreetGraph, o: &Obs, params: &MatchParams) -> Vec<Cand> {
    let mpu = g.grid.metres_per_unit(o.at.lat);
    let r = params.search_radius_m / mpu;
    let mut out: Vec<Cand> = g
        .index
        .locate_within_distance(o.p, r * r)
        .map(|item| {
            let (a, b) = g.seg_points(item.data);
            let pr = project_on_segment(o.p, a, b);
            Cand { seg: item.data, t: pr.t, pos: pr.point, dist_m: dist_units(o.p, pr.point) * mpu }
        })
        .filter(|c| c.dist_m <= params.search_radius_m)
        .collect();
    out.sort_by(|a, b| a.dist_m.total_cmp(&b.dist_m).then(a.seg.cmp(&b.seg)));
    let mut seen: FxHashSet<(i64, i64)> = FxHashSet::default();
    out.retain(|c| seen.insert(((c.pos[0] * 4.0).round() as i64, (c.pos[1] * 4.0).round() as i64)));
    out.truncate(params.max_candidates);
    out
}

/// Apparie les observations sur le graphe ; le budget est lu toutes les 1 000 observations.
pub fn match_obs(
    g: &StreetGraph,
    obs: Vec<Obs>,
    params: &MatchParams,
    budget: &dyn Budget,
) -> Result<MatchedPath, TypologyError> {
    let n = obs.len();
    let mut cands: Vec<Vec<Cand>> = Vec::with_capacity(n);
    for (t, o) in obs.iter().enumerate() {
        if t % 1000 == 0 && budget.expired() {
            return Err(TypologyError::Timeout);
        }
        cands.push(if o.no_data.is_some() || g.is_empty() { Vec::new() } else { candidates(g, o, params) });
    }
    let mut dij = Dijkstra::new(g.nodes.len());
    let emission = |c: &Cand| {
        let side = if g.class_of(c.seg).classified.footway_like { params.footway_cost } else { 0.0 };
        0.5 * (c.dist_m / params.sigma_z).powi(2) + side
    };
    // Viterbi en coûts ; `back[t][j]` : meilleur prédécesseur, `u16::MAX` ouvre une chaîne.
    let mut back: Vec<Vec<u16>> = Vec::with_capacity(n);
    let mut prev_cost: Vec<f64> = Vec::new();
    let mut last_costs: Vec<Vec<f64>> = vec![Vec::new(); n];
    let mut targets: Vec<u32> = Vec::new();
    for t in 0..n {
        if t % 1000 == 0 && budget.expired() {
            return Err(TypologyError::Timeout);
        }
        let cur = &cands[t];
        if cur.is_empty() {
            back.push(Vec::new());
            prev_cost.clear();
            continue;
        }
        let mut cost = vec![f64::INFINITY; cur.len()];
        let mut bp = vec![u16::MAX; cur.len()];
        if t > 0 && !prev_cost.is_empty() {
            let gc = haversine_m(obs[t - 1].at, obs[t].at);
            let limit = 2.0 * gc + 60.0;
            targets.clear();
            for c in cur {
                let s = g.segs[c.seg as usize];
                for node in [s.a, s.b] {
                    if !targets.contains(&node) {
                        targets.push(node);
                    }
                }
            }
            let mpu = g.grid.metres_per_unit(obs[t].at.lat);
            for (i, ci) in cands[t - 1].iter().enumerate() {
                if !prev_cost[i].is_finite() {
                    continue;
                }
                dij.run_from(g, ci, &targets, limit);
                for (j, cj) in cur.iter().enumerate() {
                    let tr = match dij.route_to(g, ci, cj) {
                        Some((route, _)) if route <= limit => (route - gc).abs() / params.beta,
                        _ => {
                            let straight = dist_units(ci.pos, cj.pos) * mpu;
                            if straight > params.max_jump_m {
                                continue;
                            }
                            params.jump_cost + (straight - gc).abs() / params.beta
                        }
                    };
                    let c = prev_cost[i] + tr;
                    if c < cost[j] {
                        cost[j] = c;
                        bp[j] = i as u16;
                    }
                }
            }
        }
        if cost.iter().all(|c| !c.is_finite()) {
            cost.iter_mut().for_each(|c| *c = 0.0);
            bp.iter_mut().for_each(|b| *b = u16::MAX);
        }
        for (j, c) in cur.iter().enumerate() {
            cost[j] += emission(c);
        }
        let min = cost.iter().copied().fold(f64::INFINITY, f64::min);
        cost.iter_mut().for_each(|c| *c -= min);
        last_costs[t] = cost.clone();
        prev_cost = cost;
        back.push(bp);
    }

    // Retour arrière de chaque chaîne depuis sa fin.
    let mut chosen: Vec<Option<usize>> = vec![None; n];
    let mut chain_of: Vec<u32> = vec![u32::MAX; n];
    let mut chains: Vec<(usize, usize)> = Vec::new();
    let mut t = n;
    while t > 0 {
        t -= 1;
        if cands[t].is_empty() || chosen[t].is_some() {
            continue;
        }
        let costs = &last_costs[t];
        let mut j =
            (0..costs.len()).min_by(|&a, &b| costs[a].total_cmp(&costs[b]).then(a.cmp(&b))).unwrap_or(0);
        let mut k = t;
        let id = chains.len() as u32;
        loop {
            chosen[k] = Some(j);
            chain_of[k] = id;
            let b = back[k][j];
            if b == u16::MAX || k == 0 {
                break;
            }
            j = b as usize;
            k -= 1;
        }
        chains.push((k, t));
        t = k;
    }
    // Chaînes trop courtes rejetées (sauf si elles couvrent tout le parcours).
    let total = obs.last().map_or(0.0, |o| o.d_m) - obs.first().map_or(0.0, |o| o.d_m);
    let min_chain = params.min_chain_m.min(total);
    for &(first, last) in &chains {
        if obs[last].d_m - obs[first].d_m < min_chain {
            for c in &mut chosen[first..=last] {
                *c = None;
            }
        }
    }

    let mut steps: Vec<Option<Step>> = vec![None; n];
    let mut dst: [u32; 2] = [0; 2];
    for t in 0..n.saturating_sub(1) {
        let (Some(ji), Some(jj)) = (chosen[t], chosen[t + 1]) else {
            continue;
        };
        if chain_of[t] != chain_of[t + 1] || back[t + 1][jj] != ji as u16 {
            continue;
        }
        let (ci, cj) = (cands[t][ji], cands[t + 1][jj]);
        let gc = haversine_m(obs[t].at, obs[t + 1].at);
        let limit = 2.0 * gc + 60.0;
        let s = g.segs[cj.seg as usize];
        dst[0] = s.a;
        dst[1] = s.b;
        dij.run_from(g, &ci, &dst, limit);
        steps[t] = Some(match dij.route_to(g, &ci, &cj) {
            Some((route, via)) if route <= limit => Step::Route(dij.pieces_to(g, &ci, &cj, via)),
            _ => Step::Jump,
        });
    }
    let state = (0..n)
        .map(|t| match (chosen[t], obs[t].no_data) {
            (Some(j), _) => ObsState::Matched { cand: cands[t][j], chain: chain_of[t] },
            (None, Some(failed)) => ObsState::NoData { failed },
            (None, None) => ObsState::Unmatched,
        })
        .collect();
    Ok(MatchedPath { obs, state, steps })
}
