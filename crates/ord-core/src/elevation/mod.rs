//! Profil d'élévation (T04) : profil brut lu par le pipeline, fusion, dérivés, points de sortie.
//!
//! Incrément 1 : MNT seul. `fuse` ne corrige aucun ouvrage (pas de typologie) et ne comble aucun
//! trou (incrément 4) : il pose la méthode `dem` ou `none`, les plages de terrain et de manque, le
//! D+ du preset et le lissage optionnel.

pub mod derive;
pub mod output;

use std::fmt;

use ord_domain::elevation::{DemKind, Missing, TerrainOrigin, TerrainSample};
use ord_domain::report::{Warning, WarningCode};
use ord_domain::request::RequestOptions;
use ord_domain::route::PreparedRoute;
use serde::Serialize;

use crate::rules::Rules;
use derive::{gain_on_runs, smooth_profile, GainParams, GainResult};

pub use output::{sample_output, OutputPoint, PointOrigin};

/// Méthode d'altitude d'une station ou d'un point (doc 02 §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EleMethod {
    Dem,
    DsmDeck,
    InterpolatedStructure,
    GapFilled,
    None,
}

/// Profil brut : un échantillon de terrain par station de `PreparedRoute::samples`, jamais modifié
/// ensuite (T04 §2.2). Défini ici, et non dans `ord-pipeline`, parce que `fuse` le consomme.
#[derive(Clone, PartialEq)]
pub struct RawProfile {
    pub terrain: Vec<TerrainSample>,
}

impl fmt::Debug for RawProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawProfile").field("stations", &self.terrain.len()).finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StationTerrain {
    pub origin: TerrainOrigin,
    pub fallback_after_error: bool,
    pub dem_kind: DemKind,
}

/// Plage de terrain homogène (`quality.terrain`).
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainRange {
    pub from_m: f64,
    pub to_m: f64,
    pub origin: TerrainOrigin,
    pub measured: bool,
    pub fallback_after_error: bool,
}

/// Plage de stations sans altitude après fusion.
#[derive(Clone, Debug, PartialEq)]
pub struct MissingRange {
    pub from_m: f64,
    pub to_m: f64,
    pub kind: Missing,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ElevationError {
    #[error("request budget exceeded")]
    Timeout,
    #[error("request cancelled")]
    Cancelled,
    #[error("internal error: {0}")]
    Internal(String),
}

/// Profil corrigé (ici : MNT seul), aligné sur les stations.
#[derive(Clone, PartialEq)]
pub struct ElevationProfile {
    /// Distance de chaque station.
    pub d: Vec<f64>,
    /// Altitude corrigée non lissée ; `NaN` = absente (`none`).
    pub ele: Vec<f64>,
    pub method: Vec<EleMethod>,
    pub terrain: Vec<StationTerrain>,
    pub ele_smoothed: Option<Vec<f64>>,
    pub gain: GainResult,
    pub terrain_ranges: Vec<TerrainRange>,
    pub missing: Vec<MissingRange>,
    pub warnings: Vec<Warning>,
    pub length_m: f64,
}

impl fmt::Debug for ElevationProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElevationProfile").field("stations", &self.d.len()).finish_non_exhaustive()
    }
}

/// Valeur interpolée en un point du profil (T04 §6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointElevation {
    pub ele: Option<f64>,
    pub ele_smoothed: Option<f64>,
    pub method: EleMethod,
    /// Rang de la station qui encadre `d` par en dessous.
    pub lower: usize,
    /// Position entre `lower` et `lower + 1`, dans [0, 1].
    pub t: f64,
}

impl ElevationProfile {
    /// Altitude à la distance `d` : interpolation linéaire entre les deux stations qui l'encadrent ;
    /// méthode de la station la plus proche, `none` si l'une des deux est `none`.
    pub fn at(&self, d: f64) -> PointElevation {
        let n = self.d.len();
        if n == 0 {
            return PointElevation {
                ele: None,
                ele_smoothed: None,
                method: EleMethod::None,
                lower: 0,
                t: 0.0,
            };
        }
        // Dernière station telle que d[i] ≤ d.
        let i = self.d.partition_point(|&x| x <= d).saturating_sub(1).min(n - 1);
        let (lower, t) = if i + 1 < n && d > self.d[i] {
            let span = self.d[i + 1] - self.d[i];
            (i, if span > 0.0 { ((d - self.d[i]) / span).clamp(0.0, 1.0) } else { 0.0 })
        } else {
            (i, 0.0)
        };
        let upper = (lower + 1).min(n - 1);
        let interp = |v: &[f64]| -> Option<f64> {
            let (a, b) = (v[lower], v[upper]);
            if t == 0.0 {
                return (!a.is_nan()).then_some(a);
            }
            (!a.is_nan() && !b.is_nan()).then_some(a + t * (b - a))
        };
        let ele = interp(&self.ele);
        let method = if ele.is_none() {
            EleMethod::None
        } else if t <= 0.5 {
            self.method[lower]
        } else {
            self.method[upper]
        };
        let ele_smoothed = self.ele_smoothed.as_deref().and_then(interp);
        PointElevation { ele, ele_smoothed, method, lower, t }
    }

    /// Pente moyenne entre deux distances, en %, sur `ele` corrigé non lissé ; `None` si une borne
    /// n'a pas d'altitude ou si `d1 ≤ d0` (T04 §5.4, « Pentes »).
    pub fn grade_pct(&self, d0: f64, d1: f64) -> Option<f64> {
        if d1.partial_cmp(&d0) != Some(std::cmp::Ordering::Greater) {
            return None;
        }
        let (a, b) = (self.at(d0).ele?, self.at(d1).ele?);
        Some(100.0 * (b - a) / (d1 - d0))
    }
}

/// Bornes de la plage couverte par les stations `[i, j]` : chaque station vaut la moitié de ses
/// deux intervalles, si bien que les plages d'une partition des stations partitionnent `[0, L]`.
fn station_span(d: &[f64], length: f64, i: usize, j: usize) -> (f64, f64) {
    let from = if i == 0 { 0.0 } else { 0.5 * (d[i - 1] + d[i]) };
    let to = if j + 1 >= d.len() { length } else { 0.5 * (d[j] + d[j + 1]) };
    (from, to)
}

/// Suites de stations de même clé, en plages de distance.
fn runs<K: PartialEq + Copy>(d: &[f64], length: f64, keys: &[K]) -> Vec<(f64, f64, K)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < keys.len() {
        let mut j = i;
        while j + 1 < keys.len() && keys[j + 1] == keys[i] {
            j += 1;
        }
        let (from, to) = station_span(d, length, i, j);
        out.push((from, to, keys[i]));
        i = j + 1;
    }
    out
}

fn measured(origin: TerrainOrigin) -> bool {
    matches!(origin, TerrainOrigin::IgnLidarHd | TerrainOrigin::MapterhornMeasured)
}

/// Fusion du profil (T04 §5.1), réduite au MNT seul : départ `ele = MNT brut`, méthode `dem` ou
/// `none` ; plages de terrain et de manque ; D+ du preset ; lissage optionnel `ele_smoothed`.
pub fn fuse(
    route: &PreparedRoute,
    raw: &RawProfile,
    opts: &RequestOptions,
    rules: &Rules,
) -> Result<ElevationProfile, ElevationError> {
    let n = route.samples.len();
    if raw.terrain.len() != n {
        return Err(ElevationError::Internal(format!(
            "raw profile has {} stations for {n} route stations",
            raw.terrain.len()
        )));
    }
    let d: Vec<f64> = route.samples.iter().map(|s| s.d_m).collect();
    let length = route.length_m;
    let ele: Vec<f64> = raw.terrain.iter().map(|t| t.ele.map_or(f64::NAN, f64::from)).collect();
    let method: Vec<EleMethod> =
        ele.iter().map(|e| if e.is_nan() { EleMethod::None } else { EleMethod::Dem }).collect();
    let terrain: Vec<StationTerrain> = raw
        .terrain
        .iter()
        .map(|t| StationTerrain {
            origin: t.origin,
            fallback_after_error: t.fallback_after_error,
            dem_kind: t.dem_kind,
        })
        .collect();

    let keys: Vec<(TerrainOrigin, bool)> =
        terrain.iter().map(|t| (t.origin, t.fallback_after_error)).collect();
    let terrain_ranges = runs(&d, length, &keys)
        .into_iter()
        .map(|(from_m, to_m, (origin, fallback_after_error))| TerrainRange {
            from_m,
            to_m,
            origin,
            measured: measured(origin),
            fallback_after_error,
        })
        .collect();

    let missing_keys: Vec<Option<Missing>> = raw
        .terrain
        .iter()
        .map(|t| if t.ele.is_some() { None } else { Some(t.missing.unwrap_or(Missing::NoData)) })
        .collect();
    let missing: Vec<MissingRange> = runs(&d, length, &missing_keys)
        .into_iter()
        .filter_map(|(from_m, to_m, kind)| kind.map(|kind| MissingRange { from_m, to_m, kind }))
        .collect();
    let warnings = missing
        .iter()
        .map(|m| match m.kind {
            Missing::NoData => Warning {
                code: WarningCode::ElevationMissing,
                message: "no elevation data on this range".into(),
                range: Some(m.from_m..m.to_m),
            },
            Missing::Failed => Warning {
                code: WarningCode::ElevationUnavailable,
                message: "elevation sources failed on this range".into(),
                range: Some(m.from_m..m.to_m),
            },
        })
        .collect();

    let preset = rules.elevation.gain.preset(opts.gain_preset);
    let (gain_m, loss_m, partial) = gain_on_runs(&d, &ele, preset);
    let gain = GainResult {
        gain_m,
        loss_m,
        params: GainParams {
            preset: opts.gain_preset,
            threshold_m: preset.threshold_m,
            smoothing_m: preset.smoothing_m,
            adapted: Vec::new(),
            partial,
        },
    };

    // Lissage client : jamais à travers une plage `none`, sans toucher `ele` (T04 §5.3).
    let ele_smoothed = (opts.smoothing_m > 0.0).then(|| {
        let mut out = vec![f64::NAN; n];
        let mut i = 0;
        while i < n {
            if ele[i].is_nan() {
                i += 1;
                continue;
            }
            let start = i;
            while i < n && !ele[i].is_nan() {
                i += 1;
            }
            let s = smooth_profile(&d[start..i], &ele[start..i], opts.smoothing_m);
            out[start..i].copy_from_slice(&s);
        }
        out
    });

    Ok(ElevationProfile {
        d,
        ele,
        method,
        terrain,
        ele_smoothed,
        gain,
        terrain_ranges,
        missing,
        warnings,
        length_m: length,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::prepare::prepare;
    use ord_domain::geo::EARTH_RADIUS_M;
    use ord_domain::LatLon;

    pub(crate) fn sample(ele: Option<f32>, missing: Option<Missing>) -> TerrainSample {
        TerrainSample {
            ele,
            origin: if ele.is_some() { TerrainOrigin::Mapterhorn } else { TerrainOrigin::None },
            fallback_after_error: false,
            dem_kind: DemKind::Unknown,
            missing,
        }
    }

    pub(crate) fn line(length_m: f64) -> PreparedRoute {
        let o = LatLon::new(45.0, 6.0);
        let b = LatLon::new(45.0 + (length_m / EARTH_RADIUS_M).to_degrees(), 6.0);
        prepare(&[o, b], 5.0)
    }

    #[test]
    fn ranges_partition_the_route_and_gaps_warn() {
        let route = line(50.0);
        let n = route.samples.len();
        assert_eq!(n, 11);
        let terrain: Vec<TerrainSample> = (0..n)
            .map(|i| match i {
                4 | 5 => sample(None, Some(Missing::NoData)),
                _ => sample(Some(100.0 + i as f32), None),
            })
            .collect();
        let p = fuse(&route, &RawProfile { terrain }, &RequestOptions::default(), &Rules::default()).unwrap();
        let total: f64 = p.terrain_ranges.iter().map(|r| r.to_m - r.from_m).sum();
        assert!((total - route.length_m).abs() < 1e-6);
        assert_eq!(p.terrain_ranges.len(), 3);
        assert_eq!(p.missing.len(), 1);
        assert!((p.missing[0].from_m - 17.5).abs() < 1e-6 && (p.missing[0].to_m - 27.5).abs() < 1e-6);
        assert_eq!(p.warnings[0].code, WarningCode::ElevationMissing);
        assert!(p.gain.params.partial);
        assert_eq!(p.method[4], EleMethod::None);
    }

    #[test]
    fn at_interpolates_and_propagates_none() {
        let route = line(20.0);
        let terrain: Vec<TerrainSample> = [Some(10.0), Some(20.0), None, Some(40.0), Some(50.0)]
            .iter()
            .map(|e| sample(*e, e.is_none().then_some(Missing::NoData)))
            .collect();
        let p = fuse(&route, &RawProfile { terrain }, &RequestOptions::default(), &Rules::default()).unwrap();
        assert_eq!(p.at(2.5).ele, Some(15.0));
        assert_eq!(p.at(5.0).ele, Some(20.0));
        assert_eq!(p.at(7.0).ele, None);
        assert_eq!(p.at(7.0).method, EleMethod::None);
        assert_eq!(p.at(20.0).ele, Some(50.0));
        assert_eq!(p.grade_pct(15.0, 20.0), Some(200.0));
        assert_eq!(p.grade_pct(5.0, 10.0), None);
    }

    #[test]
    fn smoothing_never_crosses_a_gap() {
        let route = line(100.0);
        let n = route.samples.len();
        let terrain: Vec<TerrainSample> = (0..n)
            .map(|i| {
                if i == 10 {
                    sample(None, Some(Missing::NoData))
                } else {
                    sample(Some((i * i) as f32), None)
                }
            })
            .collect();
        let opts = RequestOptions { smoothing_m: 20.0, ..RequestOptions::default() };
        let p = fuse(&route, &RawProfile { terrain }, &opts, &Rules::default()).unwrap();
        let s = p.ele_smoothed.as_ref().unwrap();
        assert!(s[10].is_nan());
        assert!(s.iter().enumerate().all(|(i, v)| i == 10 || v.is_finite()));
        assert_eq!(p.ele[3], 9.0, "ele n'est pas lissé");
    }
}
