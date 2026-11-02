//! Profil d'élévation (T04) : profil brut lu par le pipeline, fusion, dérivés, points de sortie.
//!
//! `fuse` part du MNT brut (méthode `dem` ou `none`), corrige les ouvrages OSM (`fusion` : tablier
//! MNS, interpolation, tunnels), comble ensuite les trous courts (`gap_filled`), puis calcule les
//! plages de terrain et de manque, le D+ du preset avec adaptation du seuil sur GLO-30, le lissage
//! optionnel, et les avertissements de `fuse` (T06 §4).

pub mod breakpoints;
pub mod derive;
pub mod fusion;
pub mod heuristics;
pub mod output;

use std::fmt;

use ord_domain::elevation::{DemKind, DsmSample, Missing, TerrainOrigin, TerrainSample};
use ord_domain::report::{Warning, WarningCode};
use ord_domain::request::RequestOptions;
use ord_domain::route::PreparedRoute;
use serde::Serialize;

use crate::classification::StructureVerdict;
use crate::rules::Rules;
use crate::typology::TypologyOutput;
use derive::{gain_turns_on_runs, smooth_profile, AdaptedRange, GainParams, GainResult};
use fusion::plan::StructurePlan;
use fusion::{StructureCorrectionReport, StructureOutcome};

pub use breakpoints::{select_breakpoints, BreakpointReason};
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
/// ensuite (T04 §2.2), et les lectures des ouvrages (étape 4c). Défini ici, et non dans
/// `ord-pipeline`, parce que `fuse` le consomme.
#[derive(Clone, PartialEq)]
pub struct RawProfile {
    pub terrain: Vec<TerrainSample>,
    /// Lectures des ouvrages : absentes sans typologie ou avec `structure_correction: false`.
    pub structures: Option<StructureSamples>,
    /// MNS lu sur tout le terrain IGN LiDAR HD (`DsmScope::LidarHull`, heuristiques V1, T04 §2.3) :
    /// une valeur par station, `NoData` hors du terrain IGN mesuré sans repli ; `None` sans
    /// heuristiques (MVP, `structure_correction: false`).
    pub dsm: Option<Vec<DsmSample>>,
}

/// Lectures de l'étape 4c, dans l'ordre du plan : MNS aux stations des ponts, MNT et MNS hors du
/// parcours (extensions de l'axe, sondes au-delà d'une culée).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StructureSamples {
    pub plan: StructurePlan,
    /// MNS de chaque station de `plan.route_dsm`.
    pub route_dsm: Vec<DsmSample>,
    /// Terrain de chaque point de `plan.off_route`.
    pub off_terrain: Vec<TerrainSample>,
    /// MNS de chaque point de `plan.off_route` (`None` : non demandé).
    pub off_dsm: Vec<Option<DsmSample>>,
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
    /// Entrées de `summary.structures[]`, triées par distance.
    pub structures: Vec<StructureOutcome>,
    pub verdicts: Vec<StructureVerdict>,
    pub correction: StructureCorrectionReport,
    /// Distances des croisements des ponts qui enjambent la trace au sol (T04 §3.4), triées :
    /// points de rupture `overpass_crossing` (T04 §6.1, règles 2026.11.8).
    pub overpass_crossings: Vec<f64>,
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
pub(crate) fn runs<K: PartialEq + Copy>(d: &[f64], length: f64, keys: &[K]) -> Vec<(f64, f64, K)> {
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

/// Plages GLO-30 retenues pour l'adaptation du seuil (T04 §5.4) : stations `Glo30` ou `Seam`
/// d'origine Mapterhorn, trous de moins de `min_m` fermés, plages de moins de `min_m` écartées.
/// Renvoie des plages de stations `[i, j]` (bornes incluses).
fn glo30_station_ranges(d: &[f64], terrain: &[StationTerrain], min_m: f64) -> Vec<(usize, usize)> {
    let is_glo = |t: &StationTerrain| {
        matches!(t.dem_kind, DemKind::Glo30 | DemKind::Seam)
            && matches!(t.origin, TerrainOrigin::Mapterhorn | TerrainOrigin::MapterhornMeasured)
    };
    let mut raw: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < terrain.len() {
        if !is_glo(&terrain[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i + 1 < terrain.len() && is_glo(&terrain[i + 1]) {
            i += 1;
        }
        raw.push((start, i));
        i += 1;
    }
    // Trous courts fermés : l'écart entre deux plages, de station à station, est < min_m.
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for r in raw {
        match merged.last_mut() {
            Some(last) if d[r.0] - d[last.1] < min_m => last.1 = r.1,
            _ => merged.push(r),
        }
    }
    let span = |r: &(usize, usize)| {
        let from = if r.0 == 0 { d[0] } else { 0.5 * (d[r.0 - 1] + d[r.0]) };
        let to = if r.1 + 1 >= d.len() { d[d.len() - 1] } else { 0.5 * (d[r.1] + d[r.1 + 1]) };
        to - from
    };
    merged.into_iter().filter(|r| span(r) >= min_m).collect()
}

fn measured(origin: TerrainOrigin) -> bool {
    matches!(origin, TerrainOrigin::IgnLidarHd | TerrainOrigin::MapterhornMeasured)
}

/// Suites de `none` d'au plus `max_m`, encadrées de valeurs, interpolées linéairement
/// (`gap_filled`, T04 §5.1 étape 3) ; renvoie les plages comblées.
fn fill_gaps(
    d: &[f64],
    length: f64,
    ele: &mut [f64],
    method: &mut [EleMethod],
    max_m: f64,
) -> Vec<(f64, f64)> {
    let n = ele.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if !ele[i].is_nan() {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && ele[i].is_nan() {
            i += 1;
        }
        // Stations `start..i` sans altitude ; bornes `start − 1` et `i`.
        if start == 0 || i >= n {
            continue;
        }
        let (a, b) = (start - 1, i);
        let (from, to) = station_span(d, length, start, i - 1);
        if to - from > max_m {
            continue;
        }
        for k in start..i {
            let t = (d[k] - d[a]) / (d[b] - d[a]);
            ele[k] = ele[a] + (ele[b] - ele[a]) * t;
            method[k] = EleMethod::GapFilled;
        }
        out.push((from, to));
    }
    out
}

/// Fusion du profil (T04 §5.1) : départ `ele = MNT brut`, méthode `dem` ou `none` ; correction
/// des ouvrages OSM ; comblement des trous courts ; plages de terrain et de manque ; D+ du
/// preset ; lissage optionnel `ele_smoothed`. `typo = None` : typologie absente sur tout le
/// parcours (aucun ouvrage connu).
pub fn fuse(
    route: &PreparedRoute,
    raw: &RawProfile,
    typo: Option<&TypologyOutput>,
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
    let mut ele: Vec<f64> = raw.terrain.iter().map(|t| t.ele.map_or(f64::NAN, f64::from)).collect();
    let mut method: Vec<EleMethod> =
        ele.iter().map(|e| if e.is_nan() { EleMethod::None } else { EleMethod::Dem }).collect();
    // Ouvrages d'abord, trous ensuite (doc 01 §3, T04 §8.10).
    let correction = match typo {
        Some(t) if opts.structure_correction => fusion::correct(
            route,
            &raw.terrain,
            raw.structures.as_ref(),
            raw.dsm.as_deref(),
            t,
            rules,
            &mut ele,
            &mut method,
        ),
        Some(t) => fusion::disabled(t),
        None => fusion::Correction::default(),
    };
    let filled = fill_gaps(&d, length, &mut ele, &mut method, rules.elevation.gap_fill_max_m);
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
    let terrain_ranges: Vec<TerrainRange> = runs(&d, length, &keys)
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
        .zip(&ele)
        .map(|(t, e)| if !e.is_nan() { None } else { Some(t.missing.unwrap_or(Missing::NoData)) })
        .collect();
    let missing: Vec<MissingRange> = runs(&d, length, &missing_keys)
        .into_iter()
        .filter_map(|(from_m, to_m, kind)| kind.map(|kind| MissingRange { from_m, to_m, kind }))
        .collect();
    let mut warnings: Vec<Warning> = missing
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
    warnings.extend(filled.iter().map(|&(from, to)| Warning {
        code: WarningCode::ElevationGapFilled,
        message: "short elevation gap filled linearly".into(),
        range: Some(from..to),
    }));
    warnings.extend(correction.warnings.iter().cloned());
    // Repli subi : tuiles IGN en échec relues dans Mapterhorn (T06 §4).
    warnings.extend(terrain_ranges.iter().filter(|r: &&TerrainRange| r.fallback_after_error).map(|r| {
        Warning {
            code: WarningCode::TerrainFallback,
            message: "IGN tiles failed, read from Mapterhorn".into(),
            range: Some(r.from_m..r.to_m),
        }
    }));

    // D+ du preset ; seuil adapté sur les plages GLO-30 (T04 §5.4). Pas d'adaptation sous `raw`,
    // et un seuil de preset plus haut n'est pas abaissé.
    let gain_rules = &rules.elevation.gain;
    let preset = gain_rules.preset(opts.gain_preset);
    let adapt = preset.threshold_m > 0.0 && gain_rules.glo30_threshold_m > preset.threshold_m;
    let glo30 =
        if adapt { glo30_station_ranges(&d, &terrain, gain_rules.glo30_min_range_m) } else { Vec::new() };
    let tau: Option<Vec<f64>> = (!glo30.is_empty()).then(|| {
        let mut tau = vec![preset.threshold_m; n];
        for &(i, j) in &glo30 {
            tau[i..=j].fill(gain_rules.glo30_threshold_m);
        }
        tau
    });
    let (gain_m, loss_m, partial, turning_points) = gain_turns_on_runs(&d, &ele, preset, tau.as_deref());
    let adapted: Vec<AdaptedRange> = glo30
        .iter()
        .map(|&(i, j)| {
            let (from_m, to_m) = station_span(&d, length, i, j);
            AdaptedRange { from_m, to_m, threshold_m: gain_rules.glo30_threshold_m }
        })
        .collect();
    warnings.extend(adapted.iter().map(|a| Warning {
        code: WarningCode::DsmSource,
        message: "range read on GLO-30, a surface model: gain threshold adapted".into(),
        range: Some(a.from_m..a.to_m),
    }));
    let gain = GainResult {
        gain_m,
        loss_m,
        params: GainParams {
            preset: opts.gain_preset,
            threshold_m: preset.threshold_m,
            smoothing_m: preset.smoothing_m,
            adapted,
            partial,
        },
        turning_points,
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

    let unavailable = typo.is_none_or(|t| {
        let lost: f64 =
            t.coverage.missing.iter().chain(&t.coverage.unavailable).map(|r| r.end - r.start).sum();
        lost >= length - 1e-6
    });
    let report =
        StructureCorrectionReport::new(&correction.outcomes, !opts.structure_correction, unavailable);
    // Lus avec ou sans correction des ouvrages : le point de rupture tient à la géométrie.
    let overpass_crossings: Vec<f64> = typo
        .map(|t| fusion::overpass::crossings_below(t, rules).map(|o| o.d_m).collect())
        .unwrap_or_default();
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
        structures: correction.outcomes,
        verdicts: correction.verdicts,
        correction: report,
        overpass_crossings,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::prepare::prepare;
    use ord_domain::geo::EARTH_RADIUS_M;
    use ord_domain::request::GainPreset;
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

    /// Règles sans comblement des trous : les tests de manque gardent leurs trous courts.
    pub(crate) fn no_fill() -> Rules {
        let mut r = Rules::default();
        r.elevation.gap_fill_max_m = 0.0;
        r
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
        let p = fuse(
            &route,
            &RawProfile { terrain, structures: None, dsm: None },
            None,
            &RequestOptions::default(),
            &no_fill(),
        )
        .unwrap();
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
    fn short_gaps_are_filled_long_gaps_kept() {
        let route = line(400.0);
        let n = route.samples.len();
        // Trou de 20 m (stations 10 à 13) comblé ; trou de 150 m (stations 40 à 69) gardé.
        let terrain: Vec<TerrainSample> = (0..n)
            .map(|i| {
                if (10..14).contains(&i) || (40..70).contains(&i) {
                    sample(None, Some(Missing::NoData))
                } else {
                    sample(Some(i as f32), None)
                }
            })
            .collect();
        let p = fuse(
            &route,
            &RawProfile { terrain, structures: None, dsm: None },
            None,
            &RequestOptions::default(),
            &Rules::default(),
        )
        .unwrap();
        assert_eq!(p.method[11], EleMethod::GapFilled);
        assert!((p.ele[11] - 11.0).abs() < 1e-9, "interpolation linéaire");
        assert_eq!(p.method[50], EleMethod::None);
        let codes: Vec<WarningCode> = p.warnings.iter().map(|w| w.code).collect();
        assert!(
            codes.contains(&WarningCode::ElevationGapFilled)
                && codes.contains(&WarningCode::ElevationMissing)
        );
        assert_eq!(p.missing.len(), 1, "seul le trou long reste");
        // Un trou au bout du parcours n'est jamais comblé (pas encadré).
        let terrain: Vec<TerrainSample> = (0..n)
            .map(|i| if i < 3 { sample(None, Some(Missing::NoData)) } else { sample(Some(1.0), None) })
            .collect();
        let q = fuse(
            &route,
            &RawProfile { terrain, structures: None, dsm: None },
            None,
            &RequestOptions::default(),
            &Rules::default(),
        )
        .unwrap();
        assert_eq!(q.method[0], EleMethod::None);
    }

    #[test]
    fn at_interpolates_and_propagates_none() {
        let route = line(20.0);
        let terrain: Vec<TerrainSample> = [Some(10.0), Some(20.0), None, Some(40.0), Some(50.0)]
            .iter()
            .map(|e| sample(*e, e.is_none().then_some(Missing::NoData)))
            .collect();
        let p = fuse(
            &route,
            &RawProfile { terrain, structures: None, dsm: None },
            None,
            &RequestOptions::default(),
            &no_fill(),
        )
        .unwrap();
        assert_eq!(p.at(2.5).ele, Some(15.0));
        assert_eq!(p.at(5.0).ele, Some(20.0));
        assert_eq!(p.at(7.0).ele, None);
        assert_eq!(p.at(7.0).method, EleMethod::None);
        assert_eq!(p.at(20.0).ele, Some(50.0));
        assert_eq!(p.grade_pct(15.0, 20.0), Some(200.0));
        assert_eq!(p.grade_pct(5.0, 10.0), None);
    }

    fn glo(ele: f32, kind: DemKind, fallback_after_error: bool) -> TerrainSample {
        TerrainSample {
            ele: Some(ele),
            origin: TerrainOrigin::Mapterhorn,
            fallback_after_error,
            dem_kind: kind,
            missing: None,
        }
    }

    #[test]
    fn glo30_ranges_adapt_the_threshold_and_warn() {
        // 2 km : 1 km de MNT, puis 1 km de GLO-30 avec des bosses de 6 m tous les 100 m.
        let route = line(2_000.0);
        let n = route.samples.len();
        let terrain: Vec<TerrainSample> = (0..n)
            .map(|i| {
                let d = route.samples[i].d_m;
                let bump = if ((d / 50.0) as i64) % 2 == 0 { 0.0 } else { 6.0 };
                if d < 1_000.0 {
                    glo(100.0 + bump, DemKind::Dtm, false)
                } else {
                    glo(100.0 + bump, if d < 1_100.0 { DemKind::Seam } else { DemKind::Glo30 }, false)
                }
            })
            .collect();
        let raw = RawProfile { terrain, structures: None, dsm: None };
        let opts = RequestOptions { gain_preset: GainPreset::Barometric, ..RequestOptions::default() };
        let p = fuse(&route, &raw, None, &opts, &Rules::default()).unwrap();
        assert_eq!(p.gain.params.adapted.len(), 1);
        let a = &p.gain.params.adapted[0];
        assert!((a.from_m - 997.5).abs() < 1e-6 && (a.to_m - 2_000.0).abs() < 1e-6, "{a:?}");
        assert_eq!(a.threshold_m, 10.0);
        assert!(p.warnings.iter().any(|w| w.code == WarningCode::DsmSource));
        // Sans adaptation, les bosses de la seconde moitié compteraient.
        let mut rules = Rules::default();
        rules.elevation.gain.glo30_threshold_m = 0.0;
        let q = fuse(&route, &raw, None, &opts, &rules).unwrap();
        assert!(q.gain.params.adapted.is_empty());
        assert!(q.gain.gain_m > p.gain.gain_m + 20.0, "{} vs {}", q.gain.gain_m, p.gain.gain_m);
        // Pas d'adaptation sous `raw`, ni quand le preset est déjà au-dessus.
        for preset in [GainPreset::Raw, GainPreset::Gps] {
            let o = RequestOptions { gain_preset: preset, ..RequestOptions::default() };
            assert!(fuse(&route, &raw, None, &o, &Rules::default()).unwrap().gain.params.adapted.is_empty());
        }
    }

    #[test]
    fn short_glo30_ranges_are_ignored_and_short_gaps_closed() {
        let d: Vec<f64> = (0..200).map(|i| i as f64 * 5.0).collect();
        let t = |kind| StationTerrain {
            origin: TerrainOrigin::Mapterhorn,
            fallback_after_error: false,
            dem_kind: kind,
        };
        // 0–100 m GLO-30 (trop court, à 205 m de la suivante), 300–500 GLO-30, trou de 100 m, 600–800.
        let terrain: Vec<StationTerrain> = d
            .iter()
            .map(|&x| {
                if x < 100.0 || (300.0..500.0).contains(&x) || (600.0..800.0).contains(&x) {
                    t(DemKind::Glo30)
                } else {
                    t(DemKind::Dtm)
                }
            })
            .collect();
        assert_eq!(glo30_station_ranges(&d, &terrain, 200.0), [(60, 159)]);
    }

    #[test]
    fn fallback_after_error_warns_terrain_fallback() {
        let route = line(50.0);
        let n = route.samples.len();
        let terrain = (0..n).map(|i| glo(10.0, DemKind::Dtm, (3..=5).contains(&i))).collect();
        let p = fuse(
            &route,
            &RawProfile { terrain, structures: None, dsm: None },
            None,
            &RequestOptions::default(),
            &Rules::default(),
        )
        .unwrap();
        let w: Vec<_> = p.warnings.iter().filter(|w| w.code == WarningCode::TerrainFallback).collect();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].range, Some(12.5..27.5));
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
        let p = fuse(&route, &RawProfile { terrain, structures: None, dsm: None }, None, &opts, &no_fill())
            .unwrap();
        let s = p.ele_smoothed.as_ref().unwrap();
        assert!(s[10].is_nan());
        assert!(s.iter().enumerate().all(|(i, v)| i == 10 || v.is_finite()));
        assert_eq!(p.ele[3], 9.0, "ele n'est pas lissé");
    }
}
