//! Assembleur (T06), partiel : `summary.length_m`, `gain_*` (`adapted[]` compris), extrema,
//! `terrain_m`, `quality.terrain`, `quality.elevation_coverage`, avertissements d'élévation et
//! `outside_primary_zone`, `status`, versions et attributions. Incrément 3 : `segments[]`,
//! répartitions `by_*_m`, `unknown_m`, `matched_ratio`, `inferred_surface_ratio`,
//! `unmatched_range`, `typology_missing` et `typology_unavailable`. Incrément 4 :
//! `summary.structures[]` et `quality.structure_correction`.
//!
//! Fonction pure ; les arrondis de sortie sont appliqués ici (T01 §9).

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

use ord_domain::catalog::{Attribution, Versions};
use ord_domain::coverage::CoverageRanges;
use ord_domain::elevation::TerrainOrigin;
use ord_domain::report::{Warning, WarningCode};
use ord_domain::route::PreparedRoute;
use ord_domain::street::WayAttrs;

use crate::classification::{StructureClass, SurfaceClass, SurfaceSource, WayKind};
use crate::elevation::derive::{AdaptedRange, GainResult};
use crate::elevation::fusion::{StructureCorrectionReport, StructureOutcome};
use crate::elevation::{EleMethod, ElevationProfile, OutputPoint, TerrainRange};
use crate::round::{cm, coord, pct, ratio};
use crate::typology::{TypedSegment, TypologyOutput};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Complete,
    Partial,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub length_m: f64,
    pub gain: GainResult,
    pub ele_min_m: Option<f64>,
    pub ele_max_m: Option<f64>,
    pub terrain_m: BTreeMap<TerrainOrigin, f64>,
    pub by_structure_m: BTreeMap<StructureClass, f64>,
    pub by_surface_class_m: BTreeMap<SurfaceClass, f64>,
    pub by_surface_source_m: BTreeMap<SurfaceSource, f64>,
    pub by_way_kind_m: BTreeMap<WayKind, f64>,
    /// Longueur non appariée (`matched = false`).
    pub unknown_m: f64,
    /// Ponts et tunnels rencontrés, triés par distance (une entrée par suite d'éléments de même
    /// nature d'un passage).
    pub structures: Vec<StructureOutcome>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quality {
    pub matched_ratio: f64,
    pub inferred_surface_ratio: f64,
    pub elevation_coverage: f64,
    pub structure_correction: StructureCorrectionReport,
    pub terrain: Vec<TerrainRange>,
}

/// Segment du contrat : typologie (T03 §8) et altitude aux bornes (T06 §2).
#[derive(Clone, Debug, PartialEq)]
pub struct ReportSegment {
    pub typed: TypedSegment,
    pub ele_method: EleMethod,
    pub ele_start: Option<f64>,
    pub ele_end: Option<f64>,
    pub grade_pct: Option<f64>,
    /// Attributs de la source, seulement avec `debug`.
    pub osm: Option<WayAttrs>,
}

/// Ce que l'assembleur reçoit de la typologie.
#[derive(Clone, Copy, Debug)]
pub struct TypologyPart<'a> {
    pub output: &'a TypologyOutput,
    pub segments: &'a [TypedSegment],
    /// Panne acceptée (`accept_partial`) : `typology_unavailable` sur les plages concernées.
    pub failed: bool,
    pub debug: bool,
}

#[derive(Clone, PartialEq)]
pub struct RouteReport {
    pub status: Status,
    pub points: Vec<OutputPoint>,
    pub segments: Vec<ReportSegment>,
    pub summary: Summary,
    pub quality: Quality,
    pub warnings: Vec<Warning>,
    pub versions: Versions,
    pub attribution: Vec<Attribution>,
}

impl fmt::Debug for RouteReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteReport")
            .field("status", &self.status)
            .field("points", &self.points.len())
            .field("length_m", &self.summary.length_m)
            .finish_non_exhaustive()
    }
}

/// Codes qui rendent la réponse partielle (T06 §3).
const PARTIAL_CODES: [WarningCode; 4] = [
    WarningCode::ElevationMissing,
    WarningCode::TypologyMissing,
    WarningCode::TypologyUnavailable,
    WarningCode::ElevationUnavailable,
];

/// Méthode d'altitude qui couvre la plus grande longueur de `[from, to]` ; à égalité,
/// `dsm_deck` > `interpolated_structure` > `gap_filled` > `dem` > `none` (T06 §2).
fn dominant_method(profile: &ElevationProfile, from: f64, to: f64) -> EleMethod {
    let d = &profile.d;
    let n = d.len();
    if n == 0 {
        return EleMethod::None;
    }
    let mut acc: BTreeMap<EleMethod, f64> = BTreeMap::new();
    let start = d.partition_point(|&x| x < from).saturating_sub(1);
    for i in start..n {
        let lo = if i == 0 { 0.0 } else { 0.5 * (d[i - 1] + d[i]) };
        let hi = if i + 1 == n { profile.length_m } else { 0.5 * (d[i] + d[i + 1]) };
        if lo >= to {
            break;
        }
        let overlap = hi.min(to) - lo.max(from);
        if overlap > 0.0 {
            *acc.entry(profile.method[i]).or_default() += overlap;
        }
    }
    // L'ordre de déclaration de `EleMethod` est l'ordre de priorité à égalité.
    acc.into_iter()
        .min_by(|a, b| b.1.total_cmp(&a.1).then(priority(a.0).cmp(&priority(b.0))))
        .map_or(EleMethod::None, |(m, _)| m)
}

fn priority(m: EleMethod) -> u8 {
    match m {
        EleMethod::DsmDeck => 0,
        EleMethod::InterpolatedStructure => 1,
        EleMethod::GapFilled => 2,
        EleMethod::Dem => 3,
        EleMethod::None => 4,
    }
}

/// `a` privée des plages de `minus` (toutes deux triées ou non).
fn subtract(a: Range<f64>, minus: &[Range<f64>]) -> Vec<Range<f64>> {
    let mut parts = vec![a];
    for m in minus {
        parts = parts
            .into_iter()
            .flat_map(|p| {
                let mut out = Vec::new();
                if m.end <= p.start || m.start >= p.end {
                    out.push(p);
                } else {
                    if m.start > p.start {
                        out.push(p.start..m.start);
                    }
                    if m.end < p.end {
                        out.push(m.end..p.end);
                    }
                }
                out
            })
            .collect();
    }
    parts.into_iter().filter(|r| r.end - r.start > 1e-6).collect()
}

fn warning(code: WarningCode, message: &str, range: Range<f64>) -> Warning {
    Warning { code, message: message.into(), range: Some(range) }
}

#[allow(clippy::too_many_arguments)]
pub fn assemble(
    route: &PreparedRoute,
    cov: &CoverageRanges,
    typology: TypologyPart<'_>,
    profile: ElevationProfile,
    points: Vec<OutputPoint>,
    versions: Versions,
    attribution: Vec<Attribution>,
) -> RouteReport {
    let length = route.length_m;
    let present = profile.ele.iter().copied().filter(|e| !e.is_nan());
    let (ele_min_m, ele_max_m) = present.fold((None, None), |(lo, hi): (Option<f64>, Option<f64>), e| {
        (Some(lo.map_or(e, |v| v.min(e))), Some(hi.map_or(e, |v| v.max(e))))
    });

    let mut terrain_m: BTreeMap<TerrainOrigin, f64> = BTreeMap::new();
    for r in &profile.terrain_ranges {
        *terrain_m.entry(r.origin).or_default() += r.to_m - r.from_m;
    }
    let missing_m: f64 = profile.missing.iter().map(|m| m.to_m - m.from_m).sum();
    let elevation_coverage = if length > 0.0 { ((length - missing_m) / length).clamp(0.0, 1.0) } else { 0.0 };

    // Typologie : segments, répartitions, avertissements (T06 §2, §4).
    let segs = typology.segments;
    let mut by_structure_m: BTreeMap<StructureClass, f64> = BTreeMap::new();
    let mut by_surface_class_m: BTreeMap<SurfaceClass, f64> = BTreeMap::new();
    let mut by_surface_source_m: BTreeMap<SurfaceSource, f64> = BTreeMap::new();
    let mut by_way_kind_m: BTreeMap<WayKind, f64> = BTreeMap::new();
    let (mut unknown_m, mut inferred_m) = (0.0, 0.0);
    for s in segs {
        let l = s.len();
        *by_structure_m.entry(s.structure).or_default() += l;
        *by_surface_class_m.entry(s.surface_class).or_default() += l;
        *by_surface_source_m.entry(s.surface_source).or_default() += l;
        *by_way_kind_m.entry(s.way_kind).or_default() += l;
        if !s.matched {
            unknown_m += l;
        }
        if matches!(s.surface_source, SurfaceSource::InferredTracktype | SurfaceSource::InferredHighway) {
            inferred_m += l;
        }
    }
    let share = |m: f64| if length > 0.0 { (m / length).clamp(0.0, 1.0) } else { 0.0 };
    let tcov = &typology.output.coverage;
    let mut no_typology: Vec<Range<f64>> = tcov.missing.clone();
    no_typology.extend(cov.typology_absent.iter().cloned());
    no_typology.extend(tcov.unavailable.iter().cloned());
    let segments: Vec<ReportSegment> = segs
        .iter()
        .map(|s| {
            let at = |d: f64| profile.at(d).ele.map(cm);
            let mut typed = s.clone();
            typed.from_m = cm(s.from_m);
            typed.to_m = cm(s.to_m);
            ReportSegment {
                ele_method: dominant_method(&profile, s.from_m, s.to_m),
                ele_start: at(s.from_m),
                ele_end: at(s.to_m),
                grade_pct: profile.grade_pct(s.from_m, s.to_m).map(pct),
                osm: if typology.debug {
                    s.class.and_then(|c| typology.output.classes.get(c as usize)).map(|c| c.attrs.clone())
                } else {
                    None
                },
                typed,
            }
        })
        .collect();

    let mut warnings = profile.warnings.clone();
    // Producteurs uniques de `unmatched_range`, `typology_missing`, `typology_unavailable`.
    for s in segs.iter().filter(|s| !s.matched) {
        for r in subtract(s.from_m..s.to_m, &no_typology) {
            warnings.push(warning(WarningCode::UnmatchedRange, "route not matched on the street network", r));
        }
    }
    for r in tcov.missing.iter().chain(&cov.typology_absent) {
        warnings.push(warning(WarningCode::TypologyMissing, "no street data on this range", r.clone()));
    }
    if typology.failed {
        for r in &tcov.unavailable {
            warnings.push(warning(WarningCode::TypologyUnavailable, "street tiles failed", r.clone()));
        }
    }
    // Producteur unique d'`outside_primary_zone` (T06 §4).
    warnings.extend(cov.outside_primary_zone.iter().map(|r| Warning {
        code: WarningCode::OutsidePrimaryZone,
        message: "outside France, Mapterhorn only".into(),
        range: Some(r.clone()),
    }));
    let warnings = merge_warnings(warnings);
    let status = if warnings.iter().any(|w| PARTIAL_CODES.contains(&w.code)) {
        Status::Partial
    } else {
        Status::Complete
    };

    let mut gain = profile.gain.clone();
    gain.gain_m = cm(gain.gain_m);
    gain.loss_m = cm(gain.loss_m);
    gain.params.adapted = gain
        .params
        .adapted
        .iter()
        .map(|a| AdaptedRange { from_m: cm(a.from_m), to_m: cm(a.to_m), threshold_m: a.threshold_m })
        .collect();

    RouteReport {
        status,
        points: points.into_iter().map(round_point).collect(),
        segments,
        summary: Summary {
            length_m: cm(length),
            gain,
            ele_min_m: ele_min_m.map(cm),
            ele_max_m: ele_max_m.map(cm),
            terrain_m: terrain_m.into_iter().map(|(k, v)| (k, cm(v))).collect(),
            by_structure_m: rounded(by_structure_m),
            by_surface_class_m: rounded(by_surface_class_m),
            by_surface_source_m: rounded(by_surface_source_m),
            by_way_kind_m: rounded(by_way_kind_m),
            unknown_m: cm(unknown_m),
            structures: profile
                .structures
                .iter()
                .map(|o| StructureOutcome { from_m: cm(o.from_m), to_m: cm(o.to_m), ..*o })
                .collect(),
        },
        quality: Quality {
            matched_ratio: ratio(share(length - unknown_m)),
            inferred_surface_ratio: ratio(share(inferred_m)),
            elevation_coverage: ratio(elevation_coverage),
            structure_correction: profile.correction.clone(),
            terrain: profile
                .terrain_ranges
                .iter()
                .map(|r| TerrainRange { from_m: cm(r.from_m), to_m: cm(r.to_m), ..r.clone() })
                .collect(),
        },
        warnings,
        versions,
        attribution,
    }
}

/// Longueurs au cm, valeurs nulles omises.
fn rounded<K: Ord>(m: BTreeMap<K, f64>) -> BTreeMap<K, f64> {
    m.into_iter().map(|(k, v)| (k, cm(v))).filter(|(_, v)| *v > 0.0).collect()
}

fn round_point(mut p: OutputPoint) -> OutputPoint {
    p.d_m = cm(p.d_m);
    p.ele = p.ele.map(cm);
    p.ele_smoothed = p.ele_smoothed.map(cm);
    p.latlon = p.latlon.map(|l| ord_domain::LatLon::new(coord(l.lat), coord(l.lon)));
    p
}

/// Plages contiguës d'un même code fusionnées, tri par `(from_m, code)`, distances au cm (T06 §2).
fn merge_warnings(mut warnings: Vec<Warning>) -> Vec<Warning> {
    warnings.sort_by(|a, b| {
        let fa = a.range.as_ref().map_or(f64::NEG_INFINITY, |r| r.start);
        let fb = b.range.as_ref().map_or(f64::NEG_INFINITY, |r| r.start);
        a.code.cmp(&b.code).then(fa.total_cmp(&fb))
    });
    let mut out: Vec<Warning> = Vec::with_capacity(warnings.len());
    for w in warnings {
        if let (Some(last), Some(r)) = (out.last_mut(), w.range.as_ref()) {
            if last.code == w.code {
                if let Some(lr) = last.range.as_mut() {
                    if (lr.end - r.start).abs() < 1e-6 {
                        lr.end = r.end;
                        continue;
                    }
                }
            }
        }
        out.push(w);
    }
    for w in &mut out {
        w.range = w.range.as_ref().map(|r| cm(r.start)..cm(r.end));
    }
    out.sort_by(|a, b| {
        let fa = a.range.as_ref().map_or(f64::NEG_INFINITY, |r| r.start);
        let fb = b.range.as_ref().map_or(f64::NEG_INFINITY, |r| r.start);
        fa.total_cmp(&fb).then(a.code.cmp(&b.code))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elevation::tests::{line, no_fill, sample};
    use crate::elevation::{fuse, sample_output, RawProfile};
    use crate::rules::Rules;
    use crate::typology::{build_segments, EdgeClass, RunMatch, TypedRun, TypologyCoverage};
    use ord_domain::catalog::DataVersions;
    use ord_domain::elevation::Missing;
    use ord_domain::request::{RequestOptions, Sampling};
    use std::sync::Arc;

    /// Typologie d'un parcours : une route revêtue partout, sauf les plages données.
    fn typology(route: &PreparedRoute, gaps: &[(f64, f64, RunMatch)]) -> (TypologyOutput, Vec<TypedSegment>) {
        let attrs = crate::classification::tests::way("residential");
        let classified = crate::classification::classify(&attrs, &crate::classification::tests::caps());
        let classes: Arc<[EdgeClass]> = vec![EdgeClass { attrs, classified }].into();
        let road = RunMatch::Matched { class: 0, element: None };
        let mut runs = Vec::new();
        let mut at = 0.0;
        for &(a, b, m) in gaps {
            runs.push(TypedRun { from_m: at, to_m: a, matched: road });
            runs.push(TypedRun { from_m: a, to_m: b, matched: m });
            at = b;
        }
        runs.push(TypedRun { from_m: at, to_m: route.length_m, matched: road });
        let rng = |want: RunMatch| -> Vec<std::ops::Range<f64>> {
            gaps.iter().filter(|g| g.2 == want).map(|g| g.0..g.1).collect()
        };
        let coverage = TypologyCoverage {
            missing: rng(RunMatch::NoData { failed: false }),
            unavailable: rng(RunMatch::NoData { failed: true }),
        };
        let segs = build_segments(&runs, &classes, &[], route, 20.0);
        (
            TypologyOutput {
                runs,
                coverage,
                classes,
                stats: Default::default(),
                structures: Vec::new(),
                passages: Vec::new(),
            },
            segs,
        )
    }

    fn part<'a>(out: &'a TypologyOutput, segs: &'a [TypedSegment]) -> TypologyPart<'a> {
        TypologyPart { output: out, segments: segs, failed: true, debug: false }
    }

    fn versions() -> Versions {
        Versions { api_version: "1.0", rules_version: "test", data_versions: DataVersions::default() }
    }

    fn report(eles: impl Fn(usize) -> Option<f32>) -> RouteReport {
        let route = line(100.0);
        let terrain = (0..route.samples.len())
            .map(|i| {
                let e = eles(i);
                sample(e, e.is_none().then_some(Missing::NoData))
            })
            .collect();
        let opts =
            RequestOptions { sampling: Sampling::Distance { step_m: 10.0 }, ..RequestOptions::default() };
        let p = fuse(&route, &RawProfile { terrain, structures: None }, None, &opts, &no_fill()).unwrap();
        let pts = sample_output(&route, &p, &opts.sampling, &[], &Default::default());
        let (out, segs) = typology(&route, &[]);
        assemble(&route, &CoverageRanges::default(), part(&out, &segs), p, pts, versions(), Vec::new())
    }

    #[test]
    fn complete_report() {
        let r = report(|i| Some(100.0 + i as f32 * 0.5));
        assert_eq!(r.status, Status::Complete);
        assert_eq!(r.summary.length_m, 100.0);
        assert_eq!(r.summary.ele_min_m, Some(100.0));
        assert_eq!(r.summary.ele_max_m, Some(110.0));
        // Lissage renormalisé aux bouts : une droite perd 1 m à chaque extrémité.
        assert!((r.summary.gain.gain_m - 8.0).abs() < 0.011, "{}", r.summary.gain.gain_m);
        assert_eq!(r.quality.elevation_coverage, 1.0);
        assert_eq!(r.summary.terrain_m.get(&TerrainOrigin::Mapterhorn), Some(&100.0));
        assert_eq!(r.quality.terrain.len(), 1);
        assert!(r.warnings.is_empty());
        assert_eq!(r.points.len(), 11);
    }

    #[test]
    fn missing_elevation_makes_it_partial() {
        let r = report(|i| (!(3..=6).contains(&i)).then_some(10.0));
        assert_eq!(r.status, Status::Partial);
        assert_eq!(r.warnings.len(), 1);
        assert_eq!(r.warnings[0].code, WarningCode::ElevationMissing);
        assert_eq!(r.warnings[0].range, Some(12.5..32.5));
        assert!(r.quality.elevation_coverage > 0.0 && r.quality.elevation_coverage < 1.0);
        assert!(r.summary.gain.params.partial);
        let none: f64 = r.summary.terrain_m.get(&TerrainOrigin::None).copied().unwrap_or(0.0);
        assert_eq!(none, 20.0);
    }

    #[test]
    fn outside_primary_zone_warns_without_making_it_partial() {
        let route = line(100.0);
        let terrain = (0..route.samples.len()).map(|_| sample(Some(5.0), None)).collect();
        let opts = RequestOptions::default();
        let p =
            fuse(&route, &RawProfile { terrain, structures: None }, None, &opts, &Rules::default()).unwrap();
        let cov = CoverageRanges { outside_primary_zone: vec![40.0..100.0], ..CoverageRanges::default() };
        let (out, segs) = typology(&route, &[]);
        let r = assemble(&route, &cov, part(&out, &segs), p, Vec::new(), versions(), Vec::new());
        assert_eq!(r.status, Status::Complete);
        assert_eq!(r.warnings.len(), 1);
        assert_eq!(r.warnings[0].code, WarningCode::OutsidePrimaryZone);
        assert_eq!(r.warnings[0].range, Some(40.0..100.0));
    }

    #[test]
    fn typology_fields_and_warnings() {
        let route = line(100.0);
        let terrain = (0..route.samples.len()).map(|i| sample(Some(10.0 + i as f32), None)).collect();
        let opts = RequestOptions::default();
        let p =
            fuse(&route, &RawProfile { terrain, structures: None }, None, &opts, &Rules::default()).unwrap();
        let gaps = [(20.0, 30.0, RunMatch::Unmatched), (60.0, 70.0, RunMatch::NoData { failed: false })];
        let (out, segs) = typology(&route, &gaps);
        let r = assemble(
            &route,
            &CoverageRanges::default(),
            part(&out, &segs),
            p,
            Vec::new(),
            versions(),
            Vec::new(),
        );
        assert_eq!(r.segments.len(), 5);
        assert_eq!(r.segments.first().unwrap().typed.from_m, 0.0);
        assert_eq!(r.segments.last().unwrap().typed.to_m, 100.0);
        assert!(r.segments.windows(2).all(|w| w[0].typed.to_m == w[1].typed.from_m));
        let s0 = &r.segments[0];
        assert_eq!((s0.ele_method, s0.ele_start), (EleMethod::Dem, Some(10.0)));
        assert!(s0.grade_pct.is_some_and(|g| (g - 20.0).abs() < 0.01), "{:?}", s0.grade_pct);
        assert_eq!(r.summary.unknown_m, 20.0);
        assert_eq!(r.summary.by_structure_m.get(&StructureClass::Ground), Some(&80.0));
        assert_eq!(r.summary.by_structure_m.get(&StructureClass::Unknown), Some(&20.0));
        assert_eq!(r.summary.by_surface_source_m.get(&SurfaceSource::InferredHighway), Some(&80.0));
        assert_eq!(r.quality.matched_ratio, 0.8);
        assert_eq!(r.quality.inferred_surface_ratio, 0.8);
        let codes: Vec<(WarningCode, Option<std::ops::Range<f64>>)> =
            r.warnings.iter().map(|w| (w.code, w.range.clone())).collect();
        assert_eq!(
            codes,
            vec![
                (WarningCode::UnmatchedRange, Some(20.0..30.0)),
                (WarningCode::TypologyMissing, Some(60.0..70.0))
            ],
            "la plage sans tuile n'est pas aussi `unmatched_range`"
        );
        assert_eq!(r.status, Status::Partial, "typology_missing rend la réponse partielle");
    }

    #[test]
    fn unmatched_alone_stays_complete_and_failures_need_accept_partial() {
        let route = line(100.0);
        let mk = |gaps: &[(f64, f64, RunMatch)], failed: bool| {
            let terrain = (0..route.samples.len()).map(|_| sample(Some(1.0), None)).collect();
            let p = fuse(
                &route,
                &RawProfile { terrain, structures: None },
                None,
                &RequestOptions::default(),
                &Rules::default(),
            )
            .unwrap();
            let (out, segs) = typology(&route, gaps);
            let t = TypologyPart { output: &out, segments: &segs, failed, debug: false };
            assemble(&route, &CoverageRanges::default(), t, p, Vec::new(), versions(), Vec::new())
        };
        let r = mk(&[(10.0, 50.0, RunMatch::Unmatched)], false);
        assert_eq!(r.status, Status::Complete);
        let r = mk(&[(10.0, 50.0, RunMatch::NoData { failed: true })], true);
        assert_eq!(r.status, Status::Partial);
        assert_eq!(r.warnings[0].code, WarningCode::TypologyUnavailable);
        assert!((0.0..=1.0).contains(&r.quality.matched_ratio));
    }

    #[test]
    fn dominant_method_ties_follow_priority() {
        assert!(priority(EleMethod::DsmDeck) < priority(EleMethod::Dem));
        assert_eq!(subtract(0.0..10.0, &[2.0..3.0, 9.0..12.0]), vec![0.0..2.0, 3.0..9.0]);
    }

    #[test]
    fn contiguous_warnings_of_a_code_are_merged() {
        let w = |a: f64, b: f64| Warning {
            code: WarningCode::ElevationMissing,
            message: String::new(),
            range: Some(a..b),
        };
        let merged = merge_warnings(vec![w(10.0, 20.0), w(0.0, 10.0), w(30.0, 40.0)]);
        assert_eq!(
            merged.iter().map(|w| w.range.clone().unwrap()).collect::<Vec<_>>(),
            [0.0..20.0, 30.0..40.0]
        );
    }
}
