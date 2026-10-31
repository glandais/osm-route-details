//! Chaîne élévation, partie asynchrone (T04 §2.2) : lecture du MNT aux stations.

use ord_core::elevation::{ElevationError, RawProfile};
use ord_domain::ctx::RequestCtx;
use ord_domain::elevation::TerrainReader;
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
    Ok(RawProfile { terrain })
}
