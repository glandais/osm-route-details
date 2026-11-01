//! Baro des points insérés (bench README §2.1, étape 3 ; `criteria.yaml`
//! `preparation.baro_points_inseres`, décision de l'utilisateur du 2026-10-31).
//!
//! Un point d'origine inséré par le recalage à la place d'un saut (`insere = 1` dans
//! `<id>.baro.csv`) n'a pas été validé par l'appariement. Il ne garde son baro que si sa position
//! sur la géométrie recalée (abscisse `s_recale_m`) est à `inserted_street_max_m` au plus d'une
//! ligne de la couche `streets`, toutes classes, dans la tuile z14 du point et ses 8 voisines.
//! Les tuiles sont celles du cache de la campagne en cours : une tuile réellement absente à la
//! source (`Empty`, `Absent`) compte « sans voie » ; une tuile absente du cache n'est jamais
//! « sans voie » : le cas est sauté avec sa raison. Aucun terme du service.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use ord_adapters::Shortbread;
use ord_domain::budget::{Deadline, RequestId};
use ord_domain::ctx::RequestCtx;
use ord_domain::geo::{dist_units, project_on_segment};
use ord_domain::street::StreetTile;
use ord_domain::tiles::{TileKey, TileOutcome};
use ord_domain::LatLon;

use crate::campaign::{interp_one, PersoCase};
use crate::BenchRules;

/// Tuiles de rues lues : `Some` = lignes de la tuile, `None` = tuile absente à la source (sans
/// voie). Une clé absente de la table est une tuile absente du cache.
pub type StreetTiles = BTreeMap<TileKey, Option<Arc<StreetTile>>>;

/// Tuile z14 d'un point et ses 8 voisines.
pub fn neighbourhood(p: LatLon) -> [TileKey; 9] {
    let k = Shortbread::GRID.tile_of(Shortbread::GRID.to_units(p));
    let mut out = [k; 9];
    let mut i = 0;
    for dx in -1i64..=1 {
        for dy in -1i64..=1 {
            out[i] = TileKey { z: k.z, x: (k.x as i64 + dx) as u32, y: (k.y as i64 + dy) as u32 };
            i += 1;
        }
    }
    out
}

/// Distance (m) de `p` à la ligne la plus proche de la couche `streets` des 9 tuiles de son
/// voisinage, `INFINITY` sans voie ; `Err(tuile)` si l'une d'elles est absente du cache.
pub fn nearest_street_m(tiles: &StreetTiles, p: LatLon) -> Result<f64, TileKey> {
    let grid = Shortbread::GRID;
    let u = grid.to_units(p);
    let mpu = grid.metres_per_unit(p.lat);
    let mut best = f64::INFINITY;
    for key in neighbourhood(p) {
        match tiles.get(&key) {
            None => return Err(key),
            Some(None) => {}
            Some(Some(t)) => {
                for l in &t.lines {
                    for w in l.points.windows(2) {
                        best = best.min(dist_units(u, project_on_segment(u, w[0], w[1]).point) * mpu);
                    }
                }
            }
        }
    }
    Ok(best)
}

/// Position sur la géométrie recalée des points baro insérés d'un cas, restreints aux plages
/// `ranges` (abscisses du cas) si données : `(indice dans baro_s, position)`.
pub fn inserted_positions(case: &PersoCase, ranges: Option<&[Range<f64>]>) -> Vec<(usize, LatLon)> {
    let pts: Vec<LatLon> = case
        .parts
        .iter()
        .enumerate()
        .flat_map(|(k, part)| part.iter().skip(usize::from(k > 0)).map(|q| LatLon::new(q.lat, q.lon)))
        .collect();
    if pts.len() != case.s_ref.len() || pts.is_empty() {
        return Vec::new();
    }
    let lat: Vec<f64> = pts.iter().map(|p| p.lat).collect();
    let lon: Vec<f64> = pts.iter().map(|p| p.lon).collect();
    case.baro_s
        .iter()
        .zip(&case.baro_inserted)
        .enumerate()
        .filter(|(_, (s, ins))| **ins && ranges.is_none_or(|r| r.iter().any(|r| r.contains(*s))))
        .map(|(i, (&s, _))| {
            (i, LatLon::new(interp_one(&case.s_ref, &lat, s), interp_one(&case.s_ref, &lon, s)))
        })
        .collect()
}

/// Clés des tuiles à lire pour des positions (tuile et 8 voisines).
pub fn tile_keys(positions: &[(usize, LatLon)]) -> Vec<TileKey> {
    positions.iter().flat_map(|(_, p)| neighbourhood(*p)).collect::<BTreeSet<_>>().into_iter().collect()
}

/// Bilan de la règle sur un cas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Points insérés portant du baro examinés.
    pub inserted: usize,
    /// Points insérés écartés (plus loin que le seuil de toute voie).
    pub removed: usize,
}

/// Applique la règle à `case` avec des tuiles déjà lues ; `Err` (raison sans coordonnée) si une
/// tuile du voisinage d'un point inséré est absente du cache : rien n'est retiré.
pub fn apply_with(
    case: &mut PersoCase,
    tiles: &StreetTiles,
    positions: &[(usize, LatLon)],
    rules: &BenchRules,
) -> Result<Outcome, String> {
    let mut drop = BTreeSet::new();
    for &(i, p) in positions {
        let d = nearest_street_m(tiles, p).map_err(|_| {
            "tuile de rues absente du cache autour d'un point inséré (baro_points_inseres)".to_owned()
        })?;
        if d > rules.inserted_street_max_m {
            drop.insert(i);
        }
    }
    let keep = |i: &usize| !drop.contains(i);
    let filter =
        |v: &[f64]| v.iter().enumerate().filter(|(i, _)| keep(i)).map(|(_, x)| *x).collect::<Vec<_>>();
    case.baro_s = filter(&case.baro_s);
    case.baro_z = filter(&case.baro_z);
    case.baro_inserted =
        case.baro_inserted.iter().enumerate().filter(|(i, _)| keep(i)).map(|(_, x)| *x).collect();
    Ok(Outcome { inserted: positions.len(), removed: drop.len() })
}

/// Tuiles de rues des clés données, depuis le cache de banc (`offline`) ou en ligne
/// (`fill-cache`). Échec de lecture (hors ligne : tuile absente du cache) = clé absente.
pub async fn load_tiles(cache: &Path, keys: &[TileKey], offline: bool) -> anyhow::Result<StreetTiles> {
    if keys.is_empty() {
        return Ok(StreetTiles::new());
    }
    let config = crate::service::bench_config(cache, offline, crate::service::Variant::DEFAULT);
    let chain = ord_server::wiring::typology_chain(&config)?;
    let ctx =
        RequestCtx::new(RequestId("bench".into()), Deadline::after(std::time::Duration::from_secs(600)));
    let outcomes = chain.store().get_many(keys, &ctx).await;
    Ok(keys
        .iter()
        .zip(outcomes)
        .filter_map(|(k, o)| match o {
            TileOutcome::Data { tile, .. } => Some((*k, Some(tile))),
            TileOutcome::Empty(_) | TileOutcome::Absent => Some((*k, None)),
            TileOutcome::Failed(_) => None,
        })
        .collect())
}

/// Règle appliquée au chargement du baro d'un cas, tuiles du cache de la campagne (hors ligne) ;
/// `ranges` : plages évaluées d'une trace tenue à l'écart (jamais la trace entière).
pub async fn apply(
    case: &mut PersoCase,
    cache: &Path,
    ranges: Option<&[Range<f64>]>,
    rules: &BenchRules,
) -> anyhow::Result<Result<Outcome, String>> {
    let positions = inserted_positions(case, ranges);
    let tiles = load_tiles(cache, &tile_keys(&positions), true).await?;
    Ok(apply_with(case, &tiles, &positions, rules))
}

/// `fill-cache` : tuiles du voisinage des points insérés lues en ligne et mises en cache.
pub async fn fill(case: &PersoCase, cache: &Path, ranges: Option<&[Range<f64>]>) -> anyhow::Result<()> {
    let positions = inserted_positions(case, ranges);
    load_tiles(cache, &tile_keys(&positions), false).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Serie;
    use crate::service::Pt;
    use ord_domain::geo::haversine_m;
    use ord_domain::street::StreetLine;

    /// Trace est-ouest de 2 km à 47° N, un point tous les 10 m.
    fn case(inserted: &[usize]) -> PersoCase {
        let m_per_deg = haversine_m(LatLon::new(47.0, 0.0), LatLon::new(47.0, 1.0));
        let pts: Vec<Pt> =
            (0..=200).map(|k| Pt { lat: 47.0, lon: 0.5 + k as f64 * 10.0 / m_per_deg }).collect();
        let s_ref: Vec<f64> = (0..=200).map(|k| k as f64 * 10.0).collect();
        let baro_s: Vec<f64> = (0..=40).map(|k| k as f64 * 50.0).collect();
        let baro_inserted = (0..baro_s.len()).map(|i| inserted.contains(&i)).collect();
        PersoCase {
            id: "perso-99".into(),
            groupe: "dplus".into(),
            serie: Serie::Calibration,
            parts: vec![pts],
            s_ref,
            baro_z: baro_s.iter().map(|s| 100.0 + s / 100.0).collect(),
            baro_s,
            baro_inserted,
            masks: Vec::new(),
            ring: Vec::new(),
            raw: None,
        }
    }

    /// Tuile contenant une ligne est-ouest décalée de `offset_m` vers le nord de la trace.
    fn street_tile(key: TileKey, offset_m: f64) -> Arc<StreetTile> {
        let grid = Shortbread::GRID;
        let m_per_deg_lat = haversine_m(LatLon::new(47.0, 0.0), LatLon::new(48.0, 0.0));
        let lat = 47.0 + offset_m / m_per_deg_lat;
        let a = grid.to_units(LatLon::new(lat, 0.4));
        let b = grid.to_units(LatLon::new(lat, 0.6));
        let mut t = StreetTile::empty(key);
        // Attributs sans effet : toutes les classes comptent.
        t.lines.push(StreetLine { attr: 0, points: vec![a, b], cut: [false; 2] });
        Arc::new(t)
    }

    /// Voisinage de toutes les positions : la tuile `street` porte la voie, les autres sont
    /// `None` (absentes à la source) ; sauf `missing`, retirée de la table.
    fn tiles_for(positions: &[(usize, LatLon)], offset_m: Option<f64>, missing: bool) -> StreetTiles {
        let keys = tile_keys(positions);
        let home = Shortbread::GRID.tile_of(Shortbread::GRID.to_units(positions[0].1));
        let mut t: StreetTiles =
            keys.iter().map(|k| (*k, offset_m.filter(|_| *k == home).map(|o| street_tile(*k, o)))).collect();
        if missing {
            let far = *keys.iter().find(|k| **k != home).unwrap();
            t.remove(&far);
        }
        t
    }

    #[test]
    fn inserted_point_near_a_street_keeps_its_baro() {
        let mut c = case(&[10, 11]);
        let pos = inserted_positions(&c, None);
        assert_eq!(pos.len(), 2);
        let tiles = tiles_for(&pos, Some(20.0), false);
        let out = apply_with(&mut c, &tiles, &pos, &BenchRules::default()).unwrap();
        assert_eq!(out, Outcome { inserted: 2, removed: 0 });
        assert_eq!(c.baro_s.len(), 41);
    }

    #[test]
    fn inserted_point_far_from_any_street_loses_its_baro() {
        let mut c = case(&[10, 11]);
        let pos = inserted_positions(&c, None);
        let tiles = tiles_for(&pos, Some(45.0), false);
        let out = apply_with(&mut c, &tiles, &pos, &BenchRules::default()).unwrap();
        assert_eq!(out, Outcome { inserted: 2, removed: 2 });
        assert_eq!(c.baro_s.len(), 39);
        assert!(!c.baro_s.contains(&500.0) && !c.baro_s.contains(&550.0));
        assert_eq!(c.baro_s.len(), c.baro_z.len());
        assert_eq!(c.baro_s.len(), c.baro_inserted.len());
        // Altitudes restées sur leurs abscisses.
        assert!(c.baro_s.iter().zip(&c.baro_z).all(|(s, z)| (z - (100.0 + s / 100.0)).abs() < 1e-9));
    }

    #[test]
    fn matched_points_are_never_tested() {
        let mut c = case(&[]);
        let pos = inserted_positions(&c, None);
        assert!(pos.is_empty());
        let out = apply_with(&mut c, &StreetTiles::new(), &pos, &BenchRules::default()).unwrap();
        assert_eq!(out, Outcome::default());
        assert_eq!(c.baro_s.len(), 41);
    }

    #[test]
    fn tile_absent_at_source_counts_as_no_street() {
        let mut c = case(&[10]);
        let pos = inserted_positions(&c, None);
        let tiles = tiles_for(&pos, None, false);
        assert!(tiles.values().all(Option::is_none));
        assert_eq!(nearest_street_m(&tiles, pos[0].1), Ok(f64::INFINITY));
        let out = apply_with(&mut c, &tiles, &pos, &BenchRules::default()).unwrap();
        assert_eq!(out.removed, 1);
    }

    #[test]
    fn tile_missing_from_cache_is_an_error_never_no_street() {
        let mut c = case(&[10]);
        let pos = inserted_positions(&c, None);
        let tiles = tiles_for(&pos, Some(5.0), true);
        assert!(nearest_street_m(&tiles, pos[0].1).is_err());
        let before = c.baro_s.clone();
        assert!(apply_with(&mut c, &tiles, &pos, &BenchRules::default()).is_err());
        assert_eq!(c.baro_s, before, "rien n'est retiré sur une erreur");
    }

    #[test]
    fn street_in_a_neighbour_tile_counts() {
        let c = case(&[10]);
        let pos = inserted_positions(&c, None);
        let home = Shortbread::GRID.tile_of(Shortbread::GRID.to_units(pos[0].1));
        let north = TileKey { y: home.y - 1, ..home };
        let mut tiles = tiles_for(&pos, None, false);
        // Ligne posée dans la tuile voisine nord, à 10 m du point (distance en unités globales).
        tiles.insert(north, Some(street_tile(north, 10.0)));
        let d = nearest_street_m(&tiles, pos[0].1).unwrap();
        assert!((d - 10.0).abs() < 0.5, "{d}");
    }

    #[test]
    fn positions_follow_the_recaled_abscissa_and_ranges() {
        let c = case(&[10, 30]);
        let all = inserted_positions(&c, None);
        let m_per_deg = haversine_m(LatLon::new(47.0, 0.0), LatLon::new(47.0, 1.0));
        assert!((all[0].1.lon - (0.5 + 500.0 / m_per_deg)).abs() < 1e-9);
        assert!((all[1].1.lon - (0.5 + 1_500.0 / m_per_deg)).abs() < 1e-9);
        let only = inserted_positions(&c, Some(&[1_000.0..2_000.0]));
        assert_eq!(only.iter().map(|p| p.0).collect::<Vec<_>>(), vec![30]);
    }
}
