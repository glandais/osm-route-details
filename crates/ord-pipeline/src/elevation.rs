//! Chaîne élévation, partie asynchrone (T04 §2.2–§2.3) : lecture du MNT aux stations (étape 4b),
//! puis, après la typologie, lectures des ouvrages (étape 4c) : MNS aux stations des ponts
//! appariés, MNT et MNS hors du parcours (extensions de l'axe, sondes au-delà d'une culée).

use ord_core::elevation::fusion::heuristic::{tunnel_cross_points, TunnelCross};
use ord_core::elevation::fusion::plan::plan;
use ord_core::elevation::{ElevationError, RawProfile, StructureSamples};
use ord_core::typology::TypologyOutput;
use ord_core::Rules;
use ord_domain::ctx::RequestCtx;
use ord_domain::elevation::{DsmSample, TerrainOrigin, TerrainReader};
use ord_domain::route::PreparedRoute;
use ord_domain::LatLon;

/// Lit le terrain à toutes les stations (étape 4b). Une panne de tuile n'est jamais une erreur :
/// elle est portée par `TerrainSample::missing`.
pub async fn sample_raw(
    route: &PreparedRoute,
    reader: &dyn TerrainReader,
    ctx: &RequestCtx,
) -> Result<RawProfile, ElevationError> {
    let pts: Vec<LatLon> = route.samples.iter().map(|s| s.at).collect();
    let terrain = reader.sample_terrain(&pts, ctx).await;
    if terrain.len() != pts.len() {
        return Err(ElevationError::Internal(format!(
            "terrain reader returned {} samples for {} stations",
            terrain.len(),
            pts.len()
        )));
    }
    Ok(RawProfile { terrain, structures: None, dsm: None, tunnel_cross: None })
}

/// Portée de la lecture du MNS (T04 §7.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DsmScope {
    /// MVP : stations des ponts appariés et extensions seulement (`sample_structures`).
    Structures,
    /// V1, heuristiques actives : toutes les stations sur terrain IGN LiDAR HD mesuré, sans
    /// repli (le MNS n'existe qu'à l'IGN, invariant 10), en parallèle de la typologie.
    LidarHull,
}

/// MNS sur l'enveloppe LiDAR (`DsmScope::LidarHull`, T04 §2.3) : une valeur par station du
/// parcours, `NoData` hors du terrain IGN mesuré (ou après un repli d'erreur). Le MNS lu ici sert
/// aux détecteurs ; il n'est jamais écrit comme altitude hors d'un ouvrage (inv. 9). Une tuile en
/// échec donne `Unavailable` aux stations concernées, jamais une erreur.
pub async fn sample_dsm_hull(
    raw: &mut RawProfile,
    route: &PreparedRoute,
    reader: &dyn TerrainReader,
    ctx: &RequestCtx,
) -> Result<(), ElevationError> {
    let idx: Vec<usize> = (0..raw.terrain.len())
        .filter(|&i| {
            let t = &raw.terrain[i];
            t.origin == TerrainOrigin::IgnLidarHd && !t.fallback_after_error && t.ele.is_some()
        })
        .collect();
    let mut dsm = vec![DsmSample::NoData; raw.terrain.len()];
    if !idx.is_empty() {
        let pts: Vec<LatLon> = idx.iter().map(|&i| route.samples[i].at).collect();
        let values = reader.sample_dsm(&pts, ctx).await;
        if values.len() != pts.len() {
            return Err(ElevationError::Internal(
                "terrain reader returned a wrong number of DSM samples".into(),
            ));
        }
        for (i, v) in idx.into_iter().zip(values) {
            dsm[i] = v;
        }
    }
    raw.dsm = Some(dsm);
    Ok(())
}

/// MNT en travers de l'axe des tunnels candidats du profil brut (heuristiques V1, T04 §4.2,
/// règles 2026.11.11) : quelques points de part et d'autre de chaque candidat, lus avec repli
/// comme les stations. Une tuile en échec donne un échantillon sans altitude, jamais une erreur ;
/// la fusion écarte alors le candidat si trop de lectures manquent.
pub async fn sample_tunnel_cross(
    raw: &mut RawProfile,
    route: &PreparedRoute,
    reader: &dyn TerrainReader,
    ctx: &RequestCtx,
    rules: &Rules,
) -> Result<(), ElevationError> {
    let points = tunnel_cross_points(route, &raw.terrain, rules);
    let pts: Vec<LatLon> = points.iter().map(|p| p.at).collect();
    let terrain = if pts.is_empty() { Vec::new() } else { reader.sample_terrain(&pts, ctx).await };
    if terrain.len() != pts.len() {
        return Err(ElevationError::Internal(
            "terrain reader returned a wrong number of cross-section samples".into(),
        ));
    }
    raw.tunnel_cross = Some(TunnelCross { points, terrain });
    Ok(())
}

/// Lectures des ouvrages (étape 4c), seconde phase courte limitée aux ponts appariés (MVP, sans
/// heuristiques) : le MNS n'est jamais lu hors d'un ouvrage (inv. 9). Un MNS en échec n'est pas
/// une erreur : le pont passe en interpolation (`dsm_unavailable`).
pub async fn sample_structures(
    raw: &mut RawProfile,
    route: &PreparedRoute,
    typo: &TypologyOutput,
    reader: &dyn TerrainReader,
    ctx: &RequestCtx,
    rules: &Rules,
) -> Result<(), ElevationError> {
    if typo.passages.is_empty() {
        raw.structures = Some(StructureSamples::default());
        return Ok(());
    }
    let pixel = |p: LatLon, o| reader.pixel_size_m(p, o);
    let plan = plan(route, &raw.terrain, typo, rules, &pixel);
    let route_pts: Vec<LatLon> = plan.route_dsm.iter().map(|&i| route.samples[i].at).collect();
    let off_pts: Vec<LatLon> = plan.off_route.iter().map(|o| o.at).collect();
    let off_dsm_idx: Vec<usize> = (0..plan.off_route.len()).filter(|&k| plan.off_route[k].dsm).collect();
    let off_dsm_pts: Vec<LatLon> = off_dsm_idx.iter().map(|&k| plan.off_route[k].at).collect();
    let empty = |pts: &[LatLon]| pts.is_empty();
    let (route_dsm, off_terrain, off_dsm) = tokio::join!(
        async {
            if empty(&route_pts) {
                Vec::new()
            } else {
                reader.sample_dsm(&route_pts, ctx).await
            }
        },
        async {
            if empty(&off_pts) {
                Vec::new()
            } else {
                reader.sample_terrain(&off_pts, ctx).await
            }
        },
        async {
            if empty(&off_dsm_pts) {
                Vec::new()
            } else {
                reader.sample_dsm(&off_dsm_pts, ctx).await
            }
        }
    );
    if route_dsm.len() != route_pts.len()
        || off_terrain.len() != off_pts.len()
        || off_dsm.len() != off_dsm_pts.len()
    {
        return Err(ElevationError::Internal("terrain reader returned a wrong number of samples".into()));
    }
    let mut off: Vec<Option<DsmSample>> = vec![None; plan.off_route.len()];
    for (k, v) in off_dsm_idx.into_iter().zip(off_dsm) {
        off[k] = Some(v);
    }
    raw.structures = Some(StructureSamples { plan, route_dsm, off_terrain, off_dsm: off });
    Ok(())
}
