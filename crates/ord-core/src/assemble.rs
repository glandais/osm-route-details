//! Assembleur (T06), partiel : `summary.length_m`, `gain_*` (`adapted[]` compris), extrema,
//! `terrain_m`, `quality.terrain`, `quality.elevation_coverage`, avertissements d'élévation et
//! `outside_primary_zone`, `status`, versions et attributions. Segments, typologie et ouvrages arrivent aux incréments 3–4.
//!
//! Fonction pure ; les arrondis de sortie sont appliqués ici (T01 §9).

use std::collections::BTreeMap;
use std::fmt;

use ord_domain::catalog::{Attribution, Versions};
use ord_domain::coverage::CoverageRanges;
use ord_domain::elevation::TerrainOrigin;
use ord_domain::report::{Warning, WarningCode};
use ord_domain::route::PreparedRoute;

use crate::elevation::derive::{AdaptedRange, GainResult};
use crate::elevation::{ElevationProfile, OutputPoint, TerrainRange};
use crate::round::{cm, coord, ratio};

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
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quality {
    pub elevation_coverage: f64,
    pub terrain: Vec<TerrainRange>,
}

#[derive(Clone, PartialEq)]
pub struct RouteReport {
    pub status: Status,
    pub points: Vec<OutputPoint>,
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

pub fn assemble(
    route: &PreparedRoute,
    cov: &CoverageRanges,
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

    let mut warnings = profile.warnings.clone();
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
        summary: Summary {
            length_m: cm(length),
            gain,
            ele_min_m: ele_min_m.map(cm),
            ele_max_m: ele_max_m.map(cm),
            terrain_m: terrain_m.into_iter().map(|(k, v)| (k, cm(v))).collect(),
        },
        quality: Quality {
            elevation_coverage: ratio(elevation_coverage),
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
    use crate::elevation::tests::{line, sample};
    use crate::elevation::{fuse, sample_output, RawProfile};
    use crate::rules::Rules;
    use ord_domain::catalog::DataVersions;
    use ord_domain::elevation::Missing;
    use ord_domain::request::{RequestOptions, Sampling};

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
        let p = fuse(&route, &RawProfile { terrain }, &opts, &Rules::default()).unwrap();
        let pts = sample_output(&route, &p, &opts.sampling);
        assemble(&route, &CoverageRanges::default(), p, pts, versions(), Vec::new())
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
        let p = fuse(&route, &RawProfile { terrain }, &opts, &Rules::default()).unwrap();
        let cov = CoverageRanges { outside_primary_zone: vec![40.0..100.0], ..CoverageRanges::default() };
        let r = assemble(&route, &cov, p, Vec::new(), versions(), Vec::new());
        assert_eq!(r.status, Status::Complete);
        assert_eq!(r.warnings.len(), 1);
        assert_eq!(r.warnings[0].code, WarningCode::OutsidePrimaryZone);
        assert_eq!(r.warnings[0].range, Some(40.0..100.0));
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
