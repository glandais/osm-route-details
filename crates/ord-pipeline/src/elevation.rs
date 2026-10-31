//! Chaîne élévation, partie asynchrone (T04 §2.2–§2.3) : lecture du MNT aux stations (étape 4b),
//! puis, après la typologie, lectures des ouvrages (étape 4c) : MNS aux stations des ponts
//! appariés, MNT et MNS hors du parcours (extensions de l'axe, sondes au-delà d'une culée).

use ord_core::elevation::fusion::plan::plan;
use ord_core::elevation::{ElevationError, RawProfile, StructureSamples};
use ord_core::typology::TypologyOutput;
use ord_core::Rules;
use ord_domain::ctx::RequestCtx;
use ord_domain::elevation::{DsmSample, TerrainReader};
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
    Ok(RawProfile { terrain, structures: None })
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
