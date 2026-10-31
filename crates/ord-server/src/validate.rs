//! Validation de l'entrée (T01 §4.2, étapes 4 à 7) : DTO → `ValidatedRequest`.

use ord_domain::geo::haversine_m;
use ord_domain::request::{RequestOptions, Sampling, SectionSet, ValidatedRequest, MERCATOR_MAX_LAT};
use ord_domain::LatLon;

use crate::config::LimitsConfig;
use crate::dto::{OptionsV1, RouteDetailsRequestV1, SamplingModeV1, SectionV1};
use crate::problem::{FieldError, LimitKind, Problem};

/// Au-delà, la liste d'erreurs est tronquée (le corps d'erreur reste borné).
const MAX_REPORTED_ERRORS: usize = 100;

/// Deux points consécutifs plus proches que ce seuil (en degrés) sont des doublons (T01 §5.1.1).
const DUPLICATE_EPS_DEG: f64 = 1e-7;

pub fn validate(dto: RouteDetailsRequestV1, limits: &LimitsConfig) -> Result<ValidatedRequest, Problem> {
    let (points, origin) = route_points(&dto)?;
    let mut errors = coordinate_errors(&points, origin);
    let options = resolve_options(dto.options.unwrap_or_default(), limits, &mut errors);
    if !errors.is_empty() {
        errors.truncate(MAX_REPORTED_ERRORS);
        return Err(Problem::invalid_input("invalid request", errors));
    }
    check_bounds(&points, origin, limits)?;
    Ok(ValidatedRequest { input_len: points.len(), points, options })
}

/// Origine des points, pour les pointeurs d'erreur.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Points,
    Polyline,
}

impl Origin {
    fn pointer(self) -> &'static str {
        match self {
            Self::Points => "/points",
            Self::Polyline => "/polyline",
        }
    }
}

fn route_points(dto: &RouteDetailsRequestV1) -> Result<(Vec<LatLon>, Origin), Problem> {
    let single = |pointer: &str, message: &str| {
        Problem::invalid_input("invalid route", vec![FieldError::new(pointer, message)])
    };
    match (&dto.points, &dto.polyline) {
        (Some(_), Some(_)) | (None, None) => Err(single("", "exactly one of points or polyline is required")),
        (Some(points), None) => {
            if dto.polyline_precision.is_some() {
                return Err(single("/polyline_precision", "only allowed with polyline"));
            }
            Ok((points.iter().map(|p| LatLon::new(p.lat, p.lon)).collect(), Origin::Points))
        }
        (None, Some(encoded)) => {
            let precision = match dto.polyline_precision {
                Some(p @ (5 | 6)) => p,
                Some(_) => return Err(single("/polyline_precision", "must be 5 or 6")),
                None => return Err(single("/polyline_precision", "required with polyline")),
            };
            let points = decode_polyline(encoded, precision)
                .map_err(|msg| single("/polyline", &format!("invalid polyline: {msg}")))?;
            Ok((points, Origin::Polyline))
        }
    }
}

/// Coordonnées finies, dans la plage Web Mercator ; jamais la valeur dans le message.
fn coordinate_errors(points: &[LatLon], origin: Origin) -> Vec<FieldError> {
    let mut errors = Vec::new();
    for (i, p) in points.iter().enumerate() {
        if errors.len() >= MAX_REPORTED_ERRORS {
            break;
        }
        let lat_ok = p.lat.is_finite() && p.lat.abs() <= MERCATOR_MAX_LAT;
        let lon_ok = p.lon.is_finite() && p.lon.abs() <= 180.0;
        let mut push = |field: &str, message: &str| match origin {
            Origin::Points => errors.push(FieldError::new(format!("/points/{i}/{field}"), message)),
            Origin::Polyline => {
                errors.push(FieldError::new("/polyline", format!("decoded point {i}: {message}")))
            }
        };
        if !lat_ok {
            push("lat", "latitude must lie within the Web Mercator range [-85.05112878, 85.05112878]");
        }
        if !lon_ok {
            push("lon", "longitude must lie within [-180, 180]");
        }
    }
    errors
}

fn resolve_options(dto: OptionsV1, limits: &LimitsConfig, errors: &mut Vec<FieldError>) -> RequestOptions {
    let bounds = &limits.options;
    let mut options = RequestOptions::default();
    if let Some(sections) = dto.sections {
        let mut set = SectionSet::NONE;
        for s in sections {
            match s {
                SectionV1::Elevation => set.elevation = true,
                SectionV1::Typology => set.typology = true,
                SectionV1::Summary => set.summary = true,
            }
        }
        options.sections = set;
    }
    let sampling = dto.sampling.unwrap_or_default();
    // Le pas est borné même hors du mode `distance`, où il est ignoré.
    let step_m = match sampling.step_m {
        Some(v) if !(bounds.step_m_min..=bounds.step_m_max).contains(&v) => {
            errors.push(FieldError::new(
                "/options/sampling/step_m",
                format!("must lie within [{}, {}]", bounds.step_m_min, bounds.step_m_max),
            ));
            bounds.step_m_default
        }
        Some(v) => v,
        None => bounds.step_m_default,
    };
    options.sampling = match sampling.mode.unwrap_or(SamplingModeV1::InputWithBreakpoints) {
        SamplingModeV1::InputWithBreakpoints => Sampling::InputWithBreakpoints,
        SamplingModeV1::Input => Sampling::Input,
        SamplingModeV1::Distance => Sampling::Distance { step_m },
    };
    let elevation = dto.elevation.unwrap_or_default();
    options.smoothing_m = match elevation.smoothing_m {
        Some(v) if v != 0.0 && !(bounds.smoothing_m_min..=bounds.smoothing_m_max).contains(&v) => {
            errors.push(FieldError::new(
                "/options/elevation/smoothing_m",
                format!("must be 0 or lie within [{}, {}]", bounds.smoothing_m_min, bounds.smoothing_m_max),
            ));
            bounds.smoothing_m_default
        }
        Some(v) => v,
        None => bounds.smoothing_m_default,
    };
    if let Some(preset) = elevation.gain_preset {
        options.gain_preset = preset;
    }
    if let Some(v) = dto.structure_correction {
        options.structure_correction = v;
    }
    if let Some(v) = dto.accept_partial {
        options.accept_partial = v;
    }
    if dto.debug == Some(true) && !limits.debug_allowed {
        errors.push(FieldError::new("/options/debug", "debug is not allowed on this deployment"));
    }
    options.debug = dto.debug.unwrap_or(false);
    options
}

/// Bornes d'entrée, après dédoublonnage des points consécutifs (T01 §4.2, étape 7).
fn check_bounds(points: &[LatLon], origin: Origin, limits: &LimitsConfig) -> Result<(), Problem> {
    let distinct = 1 + points
        .windows(2)
        .filter(|w| {
            (w[1].lat - w[0].lat).abs() > DUPLICATE_EPS_DEG || (w[1].lon - w[0].lon).abs() > DUPLICATE_EPS_DEG
        })
        .count();
    if distinct > limits.max_points {
        return Err(Problem::too_large(
            LimitKind::Points,
            format!("route has more than {} distinct points", limits.max_points),
        ));
    }
    let length_m: f64 = points.windows(2).map(|w| haversine_m(w[0], w[1])).sum();
    if length_m > limits.max_length_m {
        return Err(Problem::too_large(
            LimitKind::LengthM,
            format!("route is longer than {} m", limits.max_length_m),
        ));
    }
    if points.is_empty() || distinct < 2 {
        return Err(Problem::invalid_input(
            "invalid route",
            vec![FieldError::new(origin.pointer(), "at least 2 distinct points are required")],
        ));
    }
    Ok(())
}

/// Décode une polyline encodée (algorithme Google) à la précision donnée.
pub fn decode_polyline(encoded: &str, precision: u8) -> Result<Vec<LatLon>, &'static str> {
    fn next_value(bytes: &[u8], i: &mut usize) -> Result<i64, &'static str> {
        let (mut result, mut shift) = (0i64, 0u32);
        loop {
            let b = *bytes.get(*i).ok_or("truncated value")?;
            *i += 1;
            if !(63..=126).contains(&b) {
                return Err("invalid character");
            }
            let chunk = i64::from(b - 63);
            result |= (chunk & 0x1f) << shift;
            if chunk & 0x20 == 0 {
                break;
            }
            shift += 5;
            if shift > 55 {
                return Err("value too large");
            }
        }
        Ok(if result & 1 == 1 { !(result >> 1) } else { result >> 1 })
    }

    let factor = 10f64.powi(i32::from(precision));
    let bytes = encoded.as_bytes();
    let (mut i, mut lat, mut lon) = (0usize, 0i64, 0i64);
    let mut out = Vec::new();
    while i < bytes.len() {
        let dlat = next_value(bytes, &mut i)?;
        let dlon = next_value(bytes, &mut i)?;
        lat = lat.checked_add(dlat).ok_or("value too large")?;
        lon = lon.checked_add(dlon).ok_or("value too large")?;
        out.push(LatLon::new(lat as f64 / factor, lon as f64 / factor));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_reference_polyline() {
        let pts = decode_polyline("_p~iF~ps|U_ulLnnqC_mqNvxq`@", 5).expect("polyline valide");
        let expected = [(38.5, -120.2), (40.7, -120.95), (43.252, -126.453)];
        assert_eq!(pts.len(), 3);
        for (p, (lat, lon)) in pts.iter().zip(expected) {
            assert!((p.lat - lat).abs() < 1e-9 && (p.lon - lon).abs() < 1e-9);
        }
    }

    #[test]
    fn precision_six_scales_by_ten() {
        let p5 = decode_polyline("_p~iF~ps|U", 5).expect("valide");
        let p6 = decode_polyline("_p~iF~ps|U", 6).expect("valide");
        assert!((p5[0].lat - 10.0 * p6[0].lat).abs() < 1e-9);
    }

    #[test]
    fn rejects_broken_polylines() {
        assert!(decode_polyline("_p~iF", 5).is_err());
        assert!(decode_polyline("_p~iF~ps|", 5).is_err());
        assert!(decode_polyline("_p~iF~ps|U ", 5).is_err());
        assert!(decode_polyline("", 5).expect("vide").is_empty());
    }

    #[test]
    fn consecutive_duplicates_do_not_count() {
        let limits = LimitsConfig { max_points: 2, ..LimitsConfig::default() };
        let a = LatLon::new(47.0, -1.0);
        let b = LatLon::new(47.001, -1.0);
        assert!(check_bounds(&[a, a, a, b, b], Origin::Points, &limits).is_ok());
        assert!(check_bounds(&[a, b, a], Origin::Points, &limits).is_err());
        assert!(check_bounds(&[a, a], Origin::Points, &limits).is_err());
    }
}
