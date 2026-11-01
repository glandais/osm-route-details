//! M1 hors réseau (bench README §4 bis.6, `criteria.yaml` `criteres.M1.hors_reseau` ; décision de
//! l'utilisateur du 2026-11-01).
//!
//! Sur une trace recalée, une plage dont aucun point n'a de voie de la couche `streets` à moins de
//! `distance_voie_m_max` (30 m), dans les tuiles de la campagne (tuile z14 du point et ses 8
//! voisines, comme pour les points insérés ; tuile absente à la source = sans voie), sort de M1 :
//! ni dans la longueur non appariée, ni dans la longueur totale, ni dans la part `unknown`. Elle
//! est rapportée à part (`M1.hors_reseau_m`, longueur par trace).
//!
//! Points testés : stations au pas de la grille de la campagne (`grid_step_m`, 5 m, celui du mode
//! `distance`) sur la géométrie recalée, en abscisse `s_recale_m` ; une plage court de la première
//! à la dernière station hors réseau d'une suite, élargie d'un demi-pas de chaque côté (chaque
//! station vaut pour son pas), bornée à la trace ; pas de longueur minimale. Le test ne lit que
//! les tuiles et la trace, jamais la réponse du service. Une tuile absente du cache n'est jamais
//! « sans voie » : le cas est sauté avec sa raison.

use std::ops::Range;
use std::path::Path;

use ord_adapters::Shortbread;
use ord_domain::geo::{dist_units, project_on_segment};
use ord_domain::tiles::TileKey;
use ord_domain::LatLon;

use crate::campaign::{interp_one, PersoCase};
use crate::inserted::{load_tiles, neighbourhood, StreetTiles};

/// Distance maximale à une voie de `streets` (`criteria.yaml`, `M1.hors_reseau.distance_voie_m_max`).
pub const STREET_MAX_M: f64 = 30.0;

/// Stations de la géométrie recalée au pas `step` : `(abscisse s_recale_m, position)`.
pub fn stations(case: &PersoCase, step: f64) -> Vec<(f64, LatLon)> {
    let pts: Vec<LatLon> = case
        .parts
        .iter()
        .enumerate()
        .flat_map(|(k, part)| part.iter().skip(usize::from(k > 0)).map(|q| LatLon::new(q.lat, q.lon)))
        .collect();
    if pts.len() != case.s_ref.len() || pts.len() < 2 || step <= 0.0 {
        return Vec::new();
    }
    let lat: Vec<f64> = pts.iter().map(|p| p.lat).collect();
    let lon: Vec<f64> = pts.iter().map(|p| p.lon).collect();
    let (s0, s1) = (case.s_ref[0], case.s_ref[case.s_ref.len() - 1]);
    let n = ((s1 - s0) / step).floor() as usize;
    (0..=n)
        .map(|k| s0 + k as f64 * step)
        .map(|s| (s, LatLon::new(interp_one(&case.s_ref, &lat, s), interp_one(&case.s_ref, &lon, s))))
        .collect()
}

/// Une voie de `streets` est-elle à `max_m` au plus de `p` (tuile et 8 voisines) ? `Err(tuile)`
/// si l'une d'elles est absente du cache.
pub fn street_within(tiles: &StreetTiles, p: LatLon, max_m: f64) -> Result<bool, TileKey> {
    let grid = Shortbread::GRID;
    let u = grid.to_units(p);
    let mpu = grid.metres_per_unit(p.lat);
    let mut found = false;
    for key in neighbourhood(p) {
        match tiles.get(&key) {
            None => return Err(key),
            Some(None) => {}
            Some(Some(t)) if !found => {
                found = t.lines.iter().any(|l| {
                    l.points
                        .windows(2)
                        .any(|w| dist_units(u, project_on_segment(u, w[0], w[1]).point) * mpu <= max_m)
                });
            }
            Some(Some(_)) => {}
        }
    }
    Ok(found)
}

/// Plages hors réseau (abscisse `s_recale_m`) à partir des stations et des tuiles.
pub fn ranges_with(
    tiles: &StreetTiles,
    stations: &[(f64, LatLon)],
    step: f64,
    max_m: f64,
) -> Result<Vec<Range<f64>>, String> {
    let Some((&(lo, _), &(hi, _))) = stations.first().zip(stations.last()) else {
        return Ok(Vec::new());
    };
    let mut out: Vec<Range<f64>> = Vec::new();
    let mut run: Option<(f64, f64)> = None;
    let half = 0.5 * step;
    for &(s, p) in stations {
        let near = street_within(tiles, p, max_m)
            .map_err(|_| "tuile de rues absente du cache le long de la trace (M1 hors réseau)".to_owned())?;
        match (near, run) {
            (false, None) => run = Some((s, s)),
            (false, Some((a, _))) => run = Some((a, s)),
            (true, Some((a, b))) => {
                out.push((a - half).max(lo)..(b + half).min(hi));
                run = None;
            }
            (true, None) => {}
        }
    }
    if let Some((a, b)) = run {
        out.push((a - half).max(lo)..(b + half).min(hi));
    }
    Ok(out)
}

/// Clés des tuiles à lire pour des stations (tuile et 8 voisines).
fn keys(stations: &[(f64, LatLon)]) -> Vec<TileKey> {
    let set: std::collections::BTreeSet<TileKey> =
        stations.iter().flat_map(|(_, p)| neighbourhood(*p)).collect();
    set.into_iter().collect()
}

/// Plages hors réseau d'une trace recalée, tuiles du cache de la campagne (hors ligne) ;
/// `Err(raison)` (sans coordonnée) si une tuile manque au cache.
pub async fn ranges(
    case: &PersoCase,
    cache: &Path,
    step: f64,
) -> anyhow::Result<Result<Vec<Range<f64>>, String>> {
    let st = stations(case, step);
    let tiles = load_tiles(cache, &keys(&st), true).await?;
    Ok(ranges_with(&tiles, &st, step, STREET_MAX_M))
}

/// `fill-cache` : tuiles du voisinage de toute la trace recalée lues en ligne et mises en cache.
pub async fn fill(case: &PersoCase, cache: &Path, step: f64) -> anyhow::Result<()> {
    load_tiles(cache, &keys(&stations(case, step)), false).await?;
    Ok(())
}

/// Longueur totale de plages disjointes.
pub fn length(ranges: &[Range<f64>]) -> f64 {
    ranges.iter().map(|r| r.end - r.start).sum::<f64>() + 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Serie;
    use crate::service::Pt;
    use ord_domain::geo::haversine_m;
    use ord_domain::street::{StreetLine, StreetTile};
    use std::sync::Arc;

    /// Trace est-ouest de 2 km à 47° N, un point tous les 10 m.
    fn case() -> PersoCase {
        let m_per_deg = haversine_m(LatLon::new(47.0, 0.0), LatLon::new(47.0, 1.0));
        let pts: Vec<Pt> =
            (0..=200).map(|k| Pt { lat: 47.0, lon: 0.5 + k as f64 * 10.0 / m_per_deg }).collect();
        PersoCase {
            id: "perso-99".into(),
            groupe: "plaine".into(),
            serie: Serie::Calibration,
            parts: vec![pts],
            s_ref: (0..=200).map(|k| k as f64 * 10.0).collect(),
            baro_s: Vec::new(),
            baro_z: Vec::new(),
            baro_inserted: Vec::new(),
            masks: Vec::new(),
            ring: Vec::new(),
        }
    }

    /// Voie est-ouest à `offset_m` au nord de la trace, de `lon0` à `lon1`.
    fn line(lon0: f64, lon1: f64, offset_m: f64) -> StreetLine {
        let grid = Shortbread::GRID;
        let m_per_deg_lat = haversine_m(LatLon::new(47.0, 0.0), LatLon::new(48.0, 0.0));
        let lat = 47.0 + offset_m / m_per_deg_lat;
        StreetLine {
            attr: 0,
            points: vec![grid.to_units(LatLon::new(lat, lon0)), grid.to_units(LatLon::new(lat, lon1))],
            cut: [false; 2],
        }
    }

    /// Toutes les tuiles du voisinage, chacune portant `lines` (clippées à rien : la distance se
    /// calcule en unités globales).
    fn tiles(st: &[(f64, LatLon)], lines: &[StreetLine]) -> StreetTiles {
        keys(st)
            .into_iter()
            .map(|k| {
                let mut t = StreetTile::empty(k);
                t.lines.extend(lines.iter().cloned());
                (k, Some(Arc::new(t)))
            })
            .collect()
    }

    #[test]
    fn stations_follow_the_recaled_abscissa_at_the_grid_step() {
        let st = stations(&case(), 5.0);
        assert_eq!(st.len(), 401);
        assert_eq!((st[0].0, st[400].0), (0.0, 2_000.0));
    }

    #[test]
    fn a_trace_along_a_street_has_no_off_network_range() {
        let st = stations(&case(), 5.0);
        let t = tiles(&st, &[line(0.4, 0.6, 20.0)]);
        assert_eq!(ranges_with(&t, &st, 5.0, STREET_MAX_M), Ok(Vec::new()));
    }

    #[test]
    fn a_stretch_without_any_street_within_30_m_is_off_network() {
        let m_per_deg = haversine_m(LatLon::new(47.0, 0.0), LatLon::new(47.0, 1.0));
        let lon = |s: f64| 0.5 + s / m_per_deg;
        // Voie à 10 m jusqu'à s = 500 m et à partir de s = 1 500 m ; une voie à 45 m entre les deux
        // ne compte pas.
        let st = stations(&case(), 5.0);
        let t =
            tiles(&st, &[line(0.4, lon(500.0), 10.0), line(lon(1_500.0), 0.6, 10.0), line(0.4, 0.6, 45.0)]);
        let r = ranges_with(&t, &st, 5.0, STREET_MAX_M).unwrap();
        assert_eq!(r.len(), 1, "{r:?}");
        // Station hors réseau à plus de 30 m du bout de voie (à 10 m de côté : écart le long de la
        // trace > 28,3 m) : de 530 à 1 470 m, élargie d'un demi-pas de chaque côté.
        assert!((r[0].start - 527.5).abs() < 0.5 && (r[0].end - 1_472.5).abs() < 0.5, "{r:?}");
        assert!((length(&r) - 945.0).abs() < 1.0);
    }

    #[test]
    fn tile_absent_at_source_is_no_street_and_a_missing_tile_is_an_error() {
        let st = stations(&case(), 5.0);
        let mut t: StreetTiles = keys(&st).into_iter().map(|k| (k, None)).collect();
        let r = ranges_with(&t, &st, 5.0, STREET_MAX_M).unwrap();
        assert_eq!(r, vec![0.0..2_000.0], "toute la trace hors réseau, bornée à la trace");
        let k = *t.keys().next().unwrap();
        t.remove(&k);
        assert!(ranges_with(&t, &st, 5.0, STREET_MAX_M).is_err());
    }

    #[test]
    fn threshold_matches_criteria_yaml() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../bench/criteria.yaml"))
            .unwrap();
        let root: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).unwrap();
        let m1 =
            root["criteres"].as_sequence().unwrap().iter().find(|c| c["id"].as_str() == Some("M1")).unwrap();
        assert_eq!(m1["hors_reseau"]["distance_voie_m_max"].as_f64(), Some(STREET_MAX_M));
    }
}
