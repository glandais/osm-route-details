//! Projection du chemin apparié en distance le long du parcours (T03 §6.1).
//!
//! Chaque intervalle entre deux observations d'une même chaîne reçoit les classes de ses morceaux,
//! au prorata des longueurs réseau ; un saut est coupé en deux, moitié à la rue quittée, moitié à
//! la rue atteinte (osm-surface, `src/trace.rs`). Écart : les changements de classe ne sont pas
//! recalés sur le nœud du graphe projeté sur la trace (prorata seul, erreur ≤ un pas de 10 m).

use super::graph::{SegId, StreetGraph};
use super::matcher::{MatchedPath, ObsState, Step};
use super::{RunMatch, TypedRun};

fn key(g: &StreetGraph, seg: SegId) -> RunMatch {
    let s = g.segs[seg as usize];
    RunMatch::Matched { class: s.class, element: s.level.is_structure().then_some(s.line) }
}

fn push(out: &mut Vec<TypedRun>, from: f64, to: f64, matched: RunMatch) {
    if to <= from {
        return;
    }
    match out.last_mut() {
        Some(last) if last.matched == matched && (last.to_m - from).abs() < 1e-9 => last.to_m = to,
        _ => out.push(TypedRun { from_m: from, to_m: to, matched }),
    }
}

/// Tronçons typés couvrant `[0, length_m]`.
pub fn project_runs(g: &StreetGraph, path: &MatchedPath, length_m: f64) -> Vec<TypedRun> {
    let mut out: Vec<TypedRun> = Vec::new();
    let obs = &path.obs;
    let gap = |a: &ObsState, b: &ObsState| match (a, b) {
        (ObsState::NoData { failed: x }, ObsState::NoData { failed: y }) => {
            RunMatch::NoData { failed: *x || *y }
        }
        (ObsState::NoData { failed }, _) | (_, ObsState::NoData { failed }) => {
            RunMatch::NoData { failed: *failed }
        }
        // Bouts du parcours au-delà d'une observation hors graphe.
        (ObsState::OffGraph { class: x }, ObsState::OffGraph { class: y }) if x == y => {
            RunMatch::OffGraph { class: *x }
        }
        _ => RunMatch::Unmatched,
    };
    if let Some(first) = obs.first() {
        push(&mut out, 0.0, first.d_m, gap(&path.state[0], &path.state[0]));
    }
    for t in 0..obs.len().saturating_sub(1) {
        let (d0, d1) = (obs[t].d_m, obs[t + 1].d_m);
        match (&path.state[t], &path.state[t + 1], &path.steps[t]) {
            (ObsState::Matched { cand: ci, .. }, ObsState::Matched { cand: cj, .. }, Some(step)) => {
                match step {
                    Step::Jump => {
                        let mid = 0.5 * (d0 + d1);
                        push(&mut out, d0, mid, key(g, ci.seg));
                        push(&mut out, mid, d1, key(g, cj.seg));
                    }
                    Step::Route(pieces) => {
                        let w: Vec<f64> = pieces
                            .iter()
                            .map(|p| (p.t1 - p.t0).abs() * g.segs[p.seg as usize].len_m)
                            .collect();
                        let total: f64 = w.iter().sum();
                        if total <= 1e-9 {
                            push(&mut out, d0, d1, key(g, cj.seg));
                            continue;
                        }
                        let mut at = d0;
                        for (k, (p, wk)) in pieces.iter().zip(&w).enumerate() {
                            let to = if k + 1 == pieces.len() { d1 } else { at + (d1 - d0) * wk / total };
                            push(&mut out, at, to, key(g, p.seg));
                            at = to;
                        }
                    }
                }
            }
            // Route fermée au vélo suivie d'un bout à l'autre : sa typologie, moitié à chaque
            // classe si elle change.
            (ObsState::OffGraph { class: a }, ObsState::OffGraph { class: b }, _) => {
                let mid = 0.5 * (d0 + d1);
                push(&mut out, d0, mid, RunMatch::OffGraph { class: *a });
                push(&mut out, mid, d1, RunMatch::OffGraph { class: *b });
            }
            (a, b, _) => push(&mut out, d0, d1, gap(a, b)),
        }
    }
    if let Some(last) = obs.last() {
        let s = path.state[obs.len() - 1];
        push(&mut out, last.d_m, length_m, gap(&s, &s));
    }
    out
}
