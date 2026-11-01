//! Correction des ouvrages OSM (T04 §3) : tablier lu dans le MNS sur les ponts en terrain IGN,
//! interpolation de repli entre les sondes de culée, droite entre portails pour les tunnels ;
//! issue par suite d'éléments de même nature (`summary.structures[]`) et avertissements.
//!
//! Tout est calculé sur le profil brut ; les stations corrigées reçoivent la méthode de leur suite
//! (rampes et marges de sonde comprises).

pub mod deck;
pub mod heuristic;
pub mod plan;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use ord_domain::elevation::{DsmSample, TerrainOrigin, TerrainSample};
use ord_domain::report::{Warning, WarningCode};
use ord_domain::route::PreparedRoute;

use crate::classification::{reconcile, StructureSource, StructureVerdict};
use crate::rules::Rules;
use crate::typology::{StructureKind, TypologyOutput};
use deck::{build_deck, DeckOutcome, Probe};
use plan::{AxisPoint, GroupPlan, Mapping, StructurePlan, UncorrectedReason};

use super::{EleMethod, StructureSamples};

/// Méthode appliquée à une entrée de `summary.structures[]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OutcomeMethod {
    DsmDeck,
    InterpolatedStructure,
    KeptGround,
    Uncorrected(UncorrectedReason),
}

impl OutcomeMethod {
    /// Valeur publique de `summary.structures[].method`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DsmDeck => "dsm_deck",
            Self::InterpolatedStructure => "interpolated_structure",
            Self::KeptGround => "kept_ground",
            Self::Uncorrected(_) => "none",
        }
    }

    /// Clé des compteurs de `quality.structure_correction`.
    pub fn counter(self) -> &'static str {
        match self {
            Self::Uncorrected(_) => "uncorrected",
            m => m.as_str(),
        }
    }

    pub fn is_corrected(self) -> bool {
        !matches!(self, Self::Uncorrected(_))
    }
}

/// Une entrée de `summary.structures[]` : suite d'éléments de même nature d'un passage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StructureOutcome {
    pub kind: StructureKind,
    pub from_m: f64,
    pub to_m: f64,
    pub source: StructureSource,
    pub method: OutcomeMethod,
}

/// Statut de correction d'une nature d'ouvrage (vocabulaire de vcyclist, contrat §4.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorrectionStatus {
    Applied,
    Partial,
    NoneFound,
    Disabled,
    Unavailable,
}

impl CorrectionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Partial => "partial",
            Self::NoneFound => "none_found",
            Self::Disabled => "disabled",
            Self::Unavailable => "unavailable",
        }
    }
}

/// `quality.structure_correction`.
#[derive(Clone, Debug, PartialEq)]
pub struct StructureCorrectionReport {
    pub bridges: CorrectionStatus,
    pub tunnels: CorrectionStatus,
    /// Entrées par méthode (`dsm_deck`, `interpolated_structure`, `kept_ground`, `uncorrected`).
    pub counts: BTreeMap<&'static str, u32>,
    /// Entrées par `structure_source`.
    pub by_source: BTreeMap<&'static str, u32>,
}

impl StructureCorrectionReport {
    pub fn new(outcomes: &[StructureOutcome], disabled: bool, unavailable: bool) -> Self {
        let status = |kind: StructureKind| {
            let mine: Vec<&StructureOutcome> = outcomes.iter().filter(|o| o.kind == kind).collect();
            if disabled {
                CorrectionStatus::Disabled
            } else if unavailable {
                CorrectionStatus::Unavailable
            } else if mine.is_empty() {
                CorrectionStatus::NoneFound
            } else if mine.iter().any(|o| !o.method.is_corrected()) {
                CorrectionStatus::Partial
            } else {
                CorrectionStatus::Applied
            }
        };
        let mut counts = BTreeMap::new();
        let mut by_source = BTreeMap::new();
        for o in outcomes {
            *counts.entry(o.method.counter()).or_default() += 1;
            *by_source.entry(o.source.as_str()).or_default() += 1;
        }
        Self {
            bridges: status(StructureKind::Bridge),
            tunnels: status(StructureKind::Tunnel),
            counts,
            by_source,
        }
    }
}

/// Résultat de la correction des ouvrages, appliqué par `fuse`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Correction {
    pub outcomes: Vec<StructureOutcome>,
    pub verdicts: Vec<StructureVerdict>,
    pub warnings: Vec<Warning>,
}

/// Valeurs lues pour un groupe : par station d'axe, puis par sonde.
struct Axis {
    c: Vec<f64>,
    mnt: Vec<f64>,
    mns: Vec<f64>,
    ign: Vec<bool>,
    dsm_failed: bool,
    fallback: bool,
}

fn terrain_at<'a>(raw: &'a [TerrainSample], s: &'a StructureSamples, at: AxisPoint) -> &'a TerrainSample {
    match at {
        AxisPoint::Route(i) => &raw[i],
        AxisPoint::Off(k) => &s.off_terrain[k],
    }
}

fn dsm_at(s: &StructureSamples, at: AxisPoint) -> Option<DsmSample> {
    match at {
        AxisPoint::Route(i) => s.plan.route_dsm.binary_search(&i).ok().map(|k| s.route_dsm[k]),
        AxisPoint::Off(k) => s.off_dsm.get(k).copied().flatten(),
    }
}

fn ele(t: &TerrainSample) -> f64 {
    t.ele.map_or(f64::NAN, f64::from)
}

/// Interpolation linéaire de `(c, z)` triés en `x`, bornée aux extrémités.
fn interp(pts: &[(f64, f64)], x: f64) -> f64 {
    let i = pts.partition_point(|p| p.0 <= x);
    if i == 0 {
        return pts[0].1;
    }
    if i >= pts.len() {
        return pts[pts.len() - 1].1;
    }
    let (a, b) = (pts[i - 1], pts[i]);
    if b.0 > a.0 {
        a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0)
    } else {
        b.1
    }
}

fn warn(code: WarningCode, message: &str, from: f64, to: f64) -> Warning {
    Warning { code, message: message.into(), range: Some(from..to) }
}

/// Issue d'une suite d'éléments : méthode, et profil par station d'axe (`NaN` = à interpoler).
struct RunResult {
    method: OutcomeMethod,
    known: Vec<(usize, f64)>,
}

/// Profil d'un groupe : points `(c, z)` connus, triés, et méthode de chaque suite.
type GroupProfile = (Vec<(f64, f64)>, Vec<OutcomeMethod>);

/// Profil d'un groupe corrigeable : `(c, z)` triés, sondes comprises, et méthode de chaque suite.
fn group_profile(
    g: &GroupPlan,
    raw: &[TerrainSample],
    s: &StructureSamples,
    rules: &Rules,
    warnings: &mut Vec<Warning>,
    passage_range: (f64, f64),
) -> Result<GroupProfile, UncorrectedReason> {
    let probes: Vec<(f64, &TerrainSample)> =
        g.probes.iter().flatten().map(|p| (p.c, terrain_at(raw, s, p.at))).collect();
    if probes.len() != 2 {
        return Err(UncorrectedReason::EndUnresolved);
    }
    if probes.iter().any(|p| p.1.ele.is_none()) {
        return Err(UncorrectedReason::NoAltitude);
    }
    let probe_z = [ele(probes[0].1), ele(probes[1].1)];
    let probe_mns: Vec<f64> = g
        .probes
        .iter()
        .flatten()
        .map(|p| match dsm_at(s, p.at) {
            Some(DsmSample::Value(v)) => f64::from(v),
            _ => f64::NAN,
        })
        .collect();
    let probe_ign =
        probes.iter().all(|p| p.1.origin == TerrainOrigin::IgnLidarHd && !p.1.fallback_after_error);
    let probe_fallback = probes.iter().any(|p| p.1.fallback_after_error);
    let axis = Axis {
        c: g.axis.iter().map(|a| a.c).collect(),
        mnt: g.axis.iter().map(|a| ele(terrain_at(raw, s, a.at))).collect(),
        mns: g
            .axis
            .iter()
            .map(|a| match dsm_at(s, a.at) {
                Some(DsmSample::Value(v)) => f64::from(v),
                _ => f64::NAN,
            })
            .collect(),
        ign: g
            .axis
            .iter()
            .map(|a| {
                let t = terrain_at(raw, s, a.at);
                t.origin == TerrainOrigin::IgnLidarHd && !t.fallback_after_error
            })
            .collect(),
        dsm_failed: g.axis.iter().any(|a| matches!(dsm_at(s, a.at), Some(DsmSample::Unavailable))),
        fallback: g.axis.iter().any(|a| terrain_at(raw, s, a.at).fallback_after_error),
    };
    let n_runs = g.runs.len();
    let mut results: Vec<RunResult> = Vec::with_capacity(n_runs);
    for (r, &(c0, c1, kind)) in g.runs.iter().enumerate() {
        let stations: Vec<usize> = (0..axis.c.len())
            .filter(|&k| {
                axis.c[k] >= c0 - 1e-9
                    && (axis.c[k] < c1 - 1e-9 || (r + 1 == n_runs && axis.c[k] <= c1 + 1e-9))
            })
            .collect();
        if kind == StructureKind::Tunnel || stations.is_empty() {
            results.push(RunResult { method: OutcomeMethod::InterpolatedStructure, known: Vec::new() });
            continue;
        }
        let eligible = probe_ign && stations.iter().all(|&k| axis.ign[k]) && !axis.dsm_failed;
        if !eligible {
            if axis.dsm_failed || axis.fallback || probe_fallback {
                warnings.push(warn(
                    WarningCode::DsmUnavailable,
                    "surface model unreadable, bridge interpolated",
                    passage_range.0,
                    passage_range.1,
                ));
            }
            results.push(RunResult { method: OutcomeMethod::InterpolatedStructure, known: Vec::new() });
            continue;
        }
        let first_run = r == 0;
        let last_run = r + 1 == n_runs;
        let side_probe = |side: usize| -> Option<Probe> {
            let at_end = if side == 0 { first_run } else { last_run };
            at_end.then(|| Probe { c: probes[side].0, z: probe_z[side], mns: probe_mns[side] })
        };
        let c: Vec<f64> = stations.iter().map(|&k| axis.c[k]).collect();
        let mnt: Vec<f64> = stations.iter().map(|&k| axis.mnt[k]).collect();
        let mns: Vec<f64> = stations.iter().map(|&k| axis.mns[k]).collect();
        match build_deck(&c, &mnt, &mns, [side_probe(0), side_probe(1)], &rules.elevation.deck) {
            DeckOutcome::Deck { z, first, last } => {
                let mut known: Vec<(usize, f64)> = (first..=last).map(|j| (stations[j], z[j])).collect();
                // Côté tunnel : tablier tenu plat jusqu'à la jonction.
                if !first_run {
                    known.extend((0..first).map(|j| (stations[j], z[first])));
                }
                if !last_run {
                    known.extend((last + 1..stations.len()).map(|j| (stations[j], z[last])));
                }
                results.push(RunResult { method: OutcomeMethod::DsmDeck, known });
            }
            DeckOutcome::KeptGround => {
                let known = stations.iter().map(|&k| (k, axis.mnt[k])).collect();
                results.push(RunResult { method: OutcomeMethod::KeptGround, known });
            }
            DeckOutcome::Fallback(_) => {
                results.push(RunResult { method: OutcomeMethod::InterpolatedStructure, known: Vec::new() });
            }
        }
    }
    // Points connus : sondes, tabliers, sol gardé ; le reste est interpolé entre ses voisins.
    let mut known: Vec<(f64, f64)> = vec![(probes[0].0, probe_z[0]), (probes[1].0, probe_z[1])];
    for r in &results {
        known.extend(r.known.iter().map(|&(k, z)| (axis.c[k], z)));
    }
    known.sort_by(|a, b| a.0.total_cmp(&b.0));
    known.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9);
    if known.iter().any(|p| p.1.is_nan()) {
        return Err(UncorrectedReason::NoAltitude);
    }
    // Plausibilité des droites (signalée, valeur gardée, doc 01 §6).
    for (r, &(c0, c1, kind)) in g.runs.iter().enumerate() {
        if results[r].method != OutcomeMethod::InterpolatedStructure {
            continue;
        }
        let (a, b) = (interp(&known, c0), interp(&known, c1));
        let (left, right) = (
            known.iter().rev().find(|p| p.0 <= c0 + 1e-9).copied().unwrap_or((c0, a)),
            known.iter().find(|p| p.0 >= c1 - 1e-9).copied().unwrap_or((c1, b)),
        );
        let grade = if right.0 > left.0 { (right.1 - left.1).abs() / (right.0 - left.0) } else { 0.0 };
        let max = match kind {
            StructureKind::Tunnel => rules.elevation.structures.max_tunnel_chord_grade,
            StructureKind::Bridge => rules.elevation.structures.max_bridge_chord_grade,
        };
        if grade > max {
            warnings.push(warn(
                WarningCode::StructureSlopeImplausible,
                "interpolated structure grade is implausible",
                passage_range.0,
                passage_range.1,
            ));
        }
    }
    Ok((known, results.into_iter().map(|r| r.method).collect()))
}

/// Corrige les ouvrages en place sur `ele` et `method` (stations du parcours) ; renvoie les issues,
/// verdicts et avertissements.
#[allow(clippy::too_many_arguments)]
pub fn correct(
    route: &PreparedRoute,
    raw: &[TerrainSample],
    samples: Option<&StructureSamples>,
    dsm: Option<&[DsmSample]>,
    typo: &TypologyOutput,
    rules: &Rules,
    ele: &mut [f64],
    method: &mut [EleMethod],
) -> Correction {
    // Heuristiques V1 (T04 §4) : détections sur le profil brut, réconciliées avec les passages OSM.
    let detected = heuristic::detect(route, raw, dsm, typo, rules);
    let verdicts = reconcile(&typo.passages, &typo.structures, &detected);
    let source_of = |pid: u32| {
        verdicts.iter().find(|v| v.passage == Some(pid)).map_or(StructureSource::Osm, |v| v.source)
    };
    let empty = StructureSamples::default();
    let s = samples.unwrap_or(&empty);
    let plan: &StructurePlan = &s.plan;
    let mut outcomes: Vec<StructureOutcome> = Vec::new();
    let mut warnings: Vec<Warning> = Vec::new();
    let d = |i: usize| route.samples[i].d_m;
    for g in &plan.groups {
        let span = g.passages.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &k| {
            (lo.min(typo.passages[k].from_m), hi.max(typo.passages[k].to_m))
        });
        let result = match g.uncorrected {
            Some(r) => Err(r),
            None => group_profile(g, raw, s, rules, &mut warnings, span),
        };
        // Issues par passage et par suite d'éléments, en distance de parcours.
        let methods: Vec<OutcomeMethod> = match &result {
            Ok((_, m)) => m.clone(),
            Err(r) => vec![OutcomeMethod::Uncorrected(*r); g.runs.len()],
        };
        for &k in &g.passages {
            let p = &typo.passages[k];
            let source = source_of(p.id);
            let mut last: Option<(usize, f64)> = None;
            let push = |run: usize, from: f64, to: f64, outcomes: &mut Vec<StructureOutcome>| {
                if to > from {
                    outcomes.push(StructureOutcome {
                        kind: g.runs[run].2,
                        from_m: from,
                        to_m: to,
                        source,
                        method: methods[run],
                    });
                }
            };
            let run_at = |x: f64| -> usize {
                let c = g.chainage(p, x);
                g.runs.iter().position(|r| c >= r.0 - 1e-9 && c <= r.1 + 1e-9).unwrap_or(if c < g.ends[0] {
                    0
                } else {
                    g.runs.len() - 1
                })
            };
            let mut x = p.from_m;
            while x < p.to_m {
                let r = run_at(x);
                match last {
                    Some((lr, from)) if lr != r => {
                        push(lr, from, x, &mut outcomes);
                        last = Some((r, x));
                    }
                    None => last = Some((r, x)),
                    _ => {}
                }
                x += 1.0;
            }
            if let Some((lr, from)) = last {
                push(lr, from, p.to_m, &mut outcomes);
            }
        }
        let (known, methods) = match result {
            Ok(v) => v,
            Err(reason) => {
                if reason != UncorrectedReason::Disabled {
                    for &k in &g.passages {
                        let p = &typo.passages[k];
                        warnings.push(warn(
                            WarningCode::StructureUncorrected,
                            "structure left uncorrected",
                            p.from_m,
                            p.to_m,
                        ));
                    }
                }
                continue;
            }
        };
        let (c_lo, c_hi) = (known[0].0, known[known.len() - 1].0);
        // Application à chaque passage : stations strictement entre les sondes.
        let windows: Vec<(f64, f64, Option<&crate::typology::StructurePassage>)> = match g.mapping {
            Mapping::Route => vec![(c_lo, c_hi, None)],
            Mapping::Chainage => g
                .passages
                .iter()
                .map(|&k| {
                    let p = &typo.passages[k];
                    let margin = (g.ends[0] - c_lo).max(c_hi - g.ends[1]).max(0.0) + 1.0;
                    (p.from_m - margin, p.to_m + margin, Some(p))
                })
                .collect(),
        };
        for (lo, hi, p) in windows {
            let start = route.samples.partition_point(|x| x.d_m < lo);
            for i in start..route.samples.len() {
                if d(i) > hi {
                    break;
                }
                let c = match p {
                    Some(p) => g.chainage(p, d(i)),
                    None => d(i),
                };
                if !(c > c_lo + 1e-9 && c < c_hi - 1e-9) {
                    continue;
                }
                let run = g
                    .runs
                    .iter()
                    .position(|r| c >= r.0 - 1e-9 && c <= r.1 + 1e-9)
                    .unwrap_or(if c < g.ends[0] { 0 } else { g.runs.len() - 1 });
                match methods[run] {
                    OutcomeMethod::KeptGround => {}
                    OutcomeMethod::DsmDeck => {
                        ele[i] = interp(&known, c);
                        method[i] = EleMethod::DsmDeck;
                    }
                    _ => {
                        ele[i] = interp(&known, c);
                        method[i] = EleMethod::InterpolatedStructure;
                    }
                }
            }
        }
    }
    // Ouvrages `heuristic` : corrigés après les ouvrages OSM, sur les stations restées au MNT.
    let verdicts = heuristic::correct_heuristics(
        verdicts,
        route,
        raw,
        dsm,
        rules,
        ele,
        method,
        &mut outcomes,
        &mut warnings,
    );
    outcomes.sort_by(|a, b| a.from_m.total_cmp(&b.from_m).then(a.kind.cmp(&b.kind)));
    Correction { outcomes, verdicts, warnings }
}

/// Issues sans correction (option `structure_correction: false`) : chaque passage, méthode `none`.
pub fn disabled(typo: &TypologyOutput) -> Correction {
    let verdicts = reconcile(&typo.passages, &typo.structures, &[]);
    let outcomes = verdicts
        .iter()
        .map(|v| StructureOutcome {
            kind: v.kind,
            from_m: v.from_m,
            to_m: v.to_m,
            source: v.source,
            method: OutcomeMethod::Uncorrected(UncorrectedReason::Disabled),
        })
        .collect();
    Correction { outcomes, verdicts, warnings: Vec::new() }
}
