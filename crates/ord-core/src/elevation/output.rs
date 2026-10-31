//! Points de sortie (T04 §6) : modes `input`, `distance` et `input_with_breakpoints` (points
//! d'entrée et points de rupture insérés, `breakpoints`).

use std::fmt;

use ord_domain::request::Sampling;
use ord_domain::route::PreparedRoute;
use ord_domain::LatLon;
use serde::Serialize;

use super::breakpoints::{select_breakpoints, BreakpointReason};
use super::{EleMethod, ElevationProfile};
use crate::rules::BreakpointRules;
use crate::typology::TypedSegment;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PointOrigin {
    Input,
    Inserted,
}

/// Point de `points[]` (doc 02 §4.2).
#[derive(Clone, PartialEq)]
pub struct OutputPoint {
    pub d_m: f64,
    /// Indice client du point d'entrée, ou du dernier point d'entrée qui précède.
    pub index: u32,
    /// `None` en mode `distance`.
    pub origin: Option<PointOrigin>,
    /// Raisons d'insertion (mode `input_with_breakpoints`) : obligatoires sur un point inséré,
    /// présentes sur un point d'entrée si une rupture lui est rattachée.
    pub reasons: Vec<BreakpointReason>,
    /// Position interpolée : mode `distance` et points insérés seulement.
    pub latlon: Option<LatLon>,
    pub ele: Option<f64>,
    pub ele_smoothed: Option<f64>,
    pub ele_method: EleMethod,
}

impl fmt::Debug for OutputPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutputPoint")
            .field("d_m", &self.d_m)
            .field("index", &self.index)
            .field("ele", &self.ele)
            .field("ele_method", &self.ele_method)
            .field("reasons", &self.reasons)
            .finish_non_exhaustive()
    }
}

/// Tolérance sur le dernier pas du mode `distance` : un pas qui tombe à moins de 1 mm de `L`
/// est `L` lui-même.
const LAST_STEP_EPS_M: f64 = 1e-3;

/// Points de `points[]` selon le mode (T04 §6).
pub fn sample_output(
    route: &PreparedRoute,
    profile: &ElevationProfile,
    mode: &Sampling,
    segs: &[TypedSegment],
    rules: &BreakpointRules,
) -> Vec<OutputPoint> {
    match *mode {
        Sampling::Input => input_points(route, profile),
        Sampling::InputWithBreakpoints => select_breakpoints(route, profile, segs, rules),
        Sampling::Distance { step_m } => {
            let length = route.length_m;
            let mut out = Vec::with_capacity((length / step_m) as usize + 2);
            let mut k: u64 = 0;
            loop {
                let d = k as f64 * step_m;
                if d > length + LAST_STEP_EPS_M {
                    break;
                }
                out.push(point_at(route, profile, d.min(length)));
                k += 1;
            }
            if out.last().is_some_and(|p| length - p.d_m > LAST_STEP_EPS_M) {
                out.push(point_at(route, profile, length));
            }
            out
        }
    }
}

/// Un point par point d'entrée dédoublonné, lu à sa station (jamais déplacé), sans position.
pub(super) fn input_points(route: &PreparedRoute, profile: &ElevationProfile) -> Vec<OutputPoint> {
    route
        .input_pos
        .iter()
        .map(|&pos| {
            let i = pos as usize;
            let s = &route.samples[i];
            let ele = profile.ele[i];
            OutputPoint {
                d_m: s.d_m,
                index: s.input_index.unwrap_or(s.prev_input),
                origin: Some(PointOrigin::Input),
                reasons: Vec::new(),
                latlon: None,
                ele: (!ele.is_nan()).then_some(ele),
                ele_smoothed: profile.ele_smoothed.as_ref().map(|v| v[i]).filter(|v| !v.is_nan()),
                ele_method: profile.method[i],
            }
        })
        .collect()
}

/// Point à la distance `d`, position interpolée sur la trace d'origine (mode `distance`, point
/// inséré).
pub(super) fn point_at(route: &PreparedRoute, profile: &ElevationProfile, d: f64) -> OutputPoint {
    let pe = profile.at(d);
    let a = &route.samples[pe.lower];
    let b = &route.samples[(pe.lower + 1).min(route.samples.len() - 1)];
    // Entre deux stations consécutives, la trace d'origine est un segment droit.
    let latlon =
        LatLon::new(a.at.lat + pe.t * (b.at.lat - a.at.lat), a.at.lon + pe.t * (b.at.lon - a.at.lon));
    OutputPoint {
        d_m: d,
        index: a.prev_input,
        origin: None,
        reasons: Vec::new(),
        latlon: Some(latlon),
        ele: pe.ele,
        ele_smoothed: pe.ele_smoothed,
        ele_method: pe.method,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elevation::tests::{line, sample};
    use crate::elevation::{fuse, RawProfile};
    use crate::prepare::prepare;
    use crate::rules::Rules;
    use ord_domain::request::RequestOptions;

    #[test]
    fn distance_mode_adds_the_last_point() {
        let route = line(52.0);
        let terrain = route.samples.iter().map(|s| sample(Some(s.d_m as f32), None)).collect();
        let p = fuse(
            &route,
            &RawProfile { terrain, structures: None },
            None,
            &RequestOptions::default(),
            &Rules::default(),
        )
        .unwrap();
        let pts = sample_output(&route, &p, &Sampling::Distance { step_m: 25.0 }, &[], &Default::default());
        let d: Vec<f64> = pts.iter().map(|p| p.d_m).collect();
        assert_eq!(d.len(), 4);
        assert_eq!(&d[..3], [0.0, 25.0, 50.0]);
        assert!((d[3] - 52.0).abs() < 1e-6);
        assert!(pts.iter().all(|p| p.origin.is_none() && p.latlon.is_some()));
        assert!((pts[1].ele.unwrap() - 25.0).abs() < 1e-4);
        assert!(pts.windows(2).all(|w| w[1].d_m > w[0].d_m));
    }

    #[test]
    fn input_mode_keeps_client_indices() {
        let route = line(30.0);
        let (a, b) = (route.samples[0].at, route.samples.last().unwrap().at);
        let route = prepare(&[a, a, b], 5.0);
        let terrain = route.samples.iter().map(|_| sample(Some(1.0), None)).collect();
        let p = fuse(
            &route,
            &RawProfile { terrain, structures: None },
            None,
            &RequestOptions::default(),
            &Rules::default(),
        )
        .unwrap();
        let pts = sample_output(&route, &p, &Sampling::Input, &[], &Default::default());
        assert_eq!(pts.iter().map(|p| p.index).collect::<Vec<_>>(), [0, 2]);
        assert!(pts.iter().all(|p| p.origin == Some(PointOrigin::Input) && p.latlon.is_none()));
    }
}
