//! Référentiel de couverture (T02 §7) : contour IGN LiDAR HD (filtre réseau et zone France),
//! détection GLO-30 par l'index `coverage-index.pmtiles` et les polygones `coverage` de
//! Mapterhorn, coutures, contrôle de l'issue #340.
//!
//! **Provenance et licence du contour.** Les sommets de `METROPOLE` et `CORSICA` sont recopiés de
//! vcyclist, `elevation/src/commonMain/kotlin/io/github/glandais/elevation/IgnLidarHdCoverage.kt`
//! (commit vcyclist `ee2b214` du 2026-10-14), © Gabriel Landais, sous licence Apache 2.0
//! (`LICENSE` de vcyclist). Copie gelée, sans dépendance de fichiers entre les projets (décision
//! Q11). Polygones tracés à la main, généreux : 5 à 20 km au-delà de la frontière et de la côte.
//! Aucune donnée tierce (Natural Earth, Admin Express) n'y entre.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::PI;
use std::sync::{Arc, Mutex, PoisonError};

use ord_domain::coverage::{CoverageRef, CoverageTile, Extent};
use ord_domain::ctx::RequestCtx;
use ord_domain::elevation::DemKind;
use ord_domain::tiles::{SourceId, TileKey, TileOutcome, TileStore};
use ord_domain::LatLon;

use crate::pmtiles::{Pmtiles, PmtilesError};

/// France métropolitaine, paires `lon, lat`, sens horaire depuis la mer du Nord (vcyclist).
const METROPOLE: [[f64; 2]; 47] = [
    [2.40, 51.40],
    [3.00, 51.15],
    [3.40, 50.85],
    [4.30, 50.55],
    [4.70, 50.35],
    [5.00, 50.25],
    [5.20, 49.95],
    [5.85, 49.62],
    [6.40, 49.56],
    [7.40, 49.30],
    [8.35, 49.10],
    [8.00, 48.40],
    [7.75, 47.75],
    [7.70, 47.60],
    [6.95, 47.20],
    [6.55, 46.75],
    [7.00, 46.45],
    [7.15, 46.00],
    [7.05, 45.60],
    [7.35, 45.30],
    [7.15, 44.90],
    [7.15, 44.60],
    [7.85, 44.20],
    [7.70, 43.65],
    [7.70, 43.30],
    [6.50, 42.90],
    [4.50, 43.10],
    [3.40, 42.90],
    [3.40, 42.35],
    [2.30, 42.25],
    [1.30, 42.35],
    [0.40, 42.55],
    [-0.70, 42.65],
    [-1.40, 42.90],
    [-1.90, 43.20],
    [-2.10, 43.50],
    [-1.70, 44.50],
    [-2.60, 46.60],
    [-3.40, 47.20],
    [-5.30, 47.80],
    [-5.35, 48.65],
    [-4.20, 48.95],
    [-2.40, 49.05],
    [-2.10, 49.80],
    [-0.20, 49.85],
    [1.20, 50.25],
    [1.40, 51.05],
];

/// Corse : une boîte (l'île s'étend sur 8,53–9,56° E, 41,37–43,03° N), vcyclist.
const CORSICA: [[f64; 2]; 4] = [[8.40, 43.15], [9.70, 43.15], [9.70, 41.25], [8.40, 41.25]];

/// Contour IGN LiDAR HD : filtre réseau (jamais une affirmation sur la donnée) et zone France.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IgnHull;

/// Pair-impair (lancer de rayon) ; `x` = longitude, `y` = latitude.
fn inside(ring: &[[f64; 2]], x: f64, y: f64) -> bool {
    let mut result = false;
    let mut j = ring.len().wrapping_sub(1);
    for (i, &[xi, yi]) in ring.iter().enumerate() {
        let [xj, yj] = ring[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            result = !result;
        }
        j = i;
    }
    result
}

fn orient(p: [f64; 2], q: [f64; 2], r: [f64; 2]) -> f64 {
    (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
}

fn segments_cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    (orient(c, d, a) > 0.0) != (orient(c, d, b) > 0.0) && (orient(a, b, c) > 0.0) != (orient(a, b, d) > 0.0)
}

/// Rectangle `(ouest, sud, est, nord)` et polygone : coin dedans, sommet dedans, ou côtés croisés.
fn rect_intersects(ring: &[[f64; 2]], (w, s, e, n): (f64, f64, f64, f64)) -> bool {
    let corners = [[w, s], [w, n], [e, s], [e, n]];
    if corners.iter().any(|c| inside(ring, c[0], c[1])) {
        return true;
    }
    if ring.iter().any(|v| (w..=e).contains(&v[0]) && (s..=n).contains(&v[1])) {
        return true;
    }
    let sides = [([w, s], [e, s]), ([e, s], [e, n]), ([e, n], [w, n]), ([w, n], [w, s])];
    (0..ring.len()).any(|i| {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        sides.iter().any(|&(c, d)| segments_cross(a, b, c, d))
    })
}

impl IgnHull {
    fn rings() -> [&'static [[f64; 2]]; 2] {
        [&METROPOLE, &CORSICA]
    }

    /// Le point est-il dans le contour (zone France, T02 §7.2) ?
    pub fn contains(self, p: LatLon) -> bool {
        // Pré-filtre par boîte englobante.
        if !(41.0..=51.5).contains(&p.lat) || !(-5.5..=10.0).contains(&p.lon) {
            return false;
        }
        Self::rings().iter().any(|r| inside(r, p.lon, p.lat))
    }

    /// La tuile touche-t-elle le contour, bords compris ?
    pub fn intersects(self, key: TileKey) -> bool {
        let b = key.bounds();
        if b.2 < -5.5 || b.0 > 10.0 || b.3 < 41.0 || b.1 > 51.5 {
            return false;
        }
        Self::rings().iter().any(|r| rect_intersects(r, b))
    }
}

/// Source listée par l'index pour une tuile z12, avec son rang de priorité.
#[derive(Clone, Debug, PartialEq)]
pub struct IndexSource {
    pub name: String,
    /// Résolution publiée (m) ; `None` : inconnue (rang du plancher z12).
    pub resolution: Option<f64>,
}

/// Nom de la source GLO-30 chez Mapterhorn.
pub const GLO30: &str = "glo30";
/// Zoom de l'index de couverture (tuiles z12, exploration 05 §1.1).
pub const INDEX_ZOOM: u8 = 12;

/// maxzoom d'une source chez Mapterhorn : plus petit z dont le pixel Mercator (512 px) est plus
/// fin que la résolution, plancher 12 (`aggregation_covering.py`, exploration 05 §1.3).
pub fn source_maxzoom(resolution: Option<f64>) -> u8 {
    let Some(res) = resolution.filter(|r| r.is_finite() && *r > 0.0) else { return 12 };
    let mut z = 12u8;
    while z < 22 && 40_075_016.686 / (512.0 * f64::from(1u32 << z)) >= res {
        z += 1;
    }
    z
}

/// Ordre de fusion de Mapterhorn : maxzoom décroissant, puis nom.
pub fn priority_order(sources: &mut [IndexSource]) {
    sources.sort_by(|a, b| {
        source_maxzoom(b.resolution).cmp(&source_maxzoom(a.resolution)).then_with(|| a.name.cmp(&b.name))
    });
}

/// `coverage-index.pmtiles` en mémoire : sources présentes par tuile z4 à z12.
pub struct CoverageIndex {
    archive: Pmtiles,
    /// Version déclarée (ETag ou date du fichier), pour `data_versions`.
    pub version: Option<String>,
    memo: Mutex<HashMap<TileKey, Option<Arc<Vec<IndexSource>>>>>,
}

impl std::fmt::Debug for CoverageIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoverageIndex")
            .field("archive", &self.archive)
            .field("version", &self.version)
            .finish()
    }
}

impl CoverageIndex {
    pub fn parse(bytes: Vec<u8>, version: Option<String>) -> Result<Self, PmtilesError> {
        Ok(Self { archive: Pmtiles::parse(bytes)?, version, memo: Mutex::new(HashMap::new()) })
    }

    /// Sources de la tuile z12 (ordre de priorité de Mapterhorn) ; `None` : tuile absente.
    pub fn sources(&self, tile: TileKey) -> Option<Arc<Vec<IndexSource>>> {
        if let Some(hit) = self.memo.lock().unwrap_or_else(PoisonError::into_inner).get(&tile) {
            return hit.clone();
        }
        let parsed = self.archive.tile(tile).and_then(|bytes| {
            let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
            let mut list: Vec<IndexSource> = v
                .as_object()?
                .iter()
                .map(|(name, meta)| IndexSource {
                    name: name.clone(),
                    resolution: meta["resolution"].as_f64(),
                })
                .collect();
            priority_order(&mut list);
            Some(Arc::new(list))
        });
        self.memo.lock().unwrap_or_else(PoisonError::into_inner).insert(tile, parsed.clone());
        parsed
    }
}

/// Tuile de l'index (z12) qui contient un point.
fn tile_at(p: LatLon, z: u8) -> (TileKey, f64, f64) {
    let n = f64::from(1u32 << z);
    let gx = (p.lon + 180.0) / 360.0 * n;
    let lat = p.lat.to_radians();
    let gy = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / PI) / 2.0 * n;
    let clamp = |v: f64| (v.floor().max(0.0) as u32).min((1u32 << z) - 1);
    (TileKey { z, x: clamp(gx), y: clamp(gy) }, gx, gy)
}

/// Point décalé de `d` mètres dans la direction `bearing` (radians, depuis le nord).
fn offset(p: LatLon, d: f64, bearing: f64) -> LatLon {
    let dlat = d * bearing.cos() / 111_195.0;
    let dlon = d * bearing.sin() / (111_195.0 * p.lat.to_radians().cos().max(1e-6));
    LatLon::new(p.lat + dlat, p.lon + dlon)
}

/// Première source, dans l'ordre de priorité, dont un polygone contient le point ; `Err` si la
/// tuile de polygones n'est pas lisible.
fn first_source<'a>(
    p: LatLon,
    zoom: u8,
    order: &'a [IndexSource],
    tiles: &BTreeMap<TileKey, TileOutcome<CoverageTile>>,
) -> Result<Option<&'a str>, ()> {
    let (key, gx, gy) = tile_at(p, zoom);
    let tile = match tiles.get(&key) {
        Some(TileOutcome::Data { tile, .. } | TileOutcome::Empty(tile)) => tile,
        _ => return Err(()),
    };
    let e = f64::from(tile.extent);
    let (lx, ly) = ((gx - f64::from(key.x)) * e, (gy - f64::from(key.y)) * e);
    Ok(order.iter().map(|s| s.name.as_str()).find(|name| {
        tile.polygons
            .iter()
            .filter(|poly| poly.source == *name)
            .any(|poly| poly.rings.iter().filter(|r| inside(r, lx, ly)).count() % 2 == 1)
    }))
}

/// Référentiel de couverture du service : contour, zone France, index et polygones Mapterhorn.
pub struct MapterhornCoverage {
    hull: IgnHull,
    index: Option<Arc<CoverageIndex>>,
    polygons: Option<Arc<dyn TileStore<CoverageTile>>>,
    polygon_zoom: u8,
    seam_m: f64,
    world: Extent,
}

impl std::fmt::Debug for MapterhornCoverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MapterhornCoverage")
            .field("index", &self.index.is_some())
            .field("polygons", &self.polygons.is_some())
            .field("polygon_zoom", &self.polygon_zoom)
            .field("seam_m", &self.seam_m)
            .finish()
    }
}

impl MapterhornCoverage {
    pub fn new(
        index: Option<Arc<CoverageIndex>>,
        polygons: Option<Arc<dyn TileStore<CoverageTile>>>,
        polygon_zoom: u8,
        seam_m: f64,
    ) -> Self {
        Self { hull: IgnHull, index, polygons, polygon_zoom, seam_m, world: Extent::World }
    }

    /// Sans fichier de couverture : contour et zone seulement, `DemKind::Unknown` partout.
    pub fn hull_only() -> Self {
        Self::new(None, None, INDEX_ZOOM, 150.0)
    }

    pub fn index_version(&self) -> Option<String> {
        self.index.as_ref().and_then(|i| i.version.clone())
    }

    /// Sondes de couture : 8 directions à `seam_m` et à `seam_m / 2`.
    fn probes(&self, p: LatLon) -> Vec<LatLon> {
        let mut out = Vec::with_capacity(16);
        for k in 0..8 {
            let b = f64::from(k) * PI / 4.0;
            out.push(offset(p, self.seam_m, b));
            out.push(offset(p, self.seam_m / 2.0, b));
        }
        out
    }
}

/// Cas d'un point vis-à-vis de l'index.
enum IndexCase {
    Known(DemKind),
    Mixed(Arc<Vec<IndexSource>>),
}

#[async_trait::async_trait]
impl CoverageRef for MapterhornCoverage {
    fn ign_may_have_data(&self, tile: TileKey) -> bool {
        self.hull.intersects(tile)
    }

    fn in_primary_zone(&self, p: LatLon) -> bool {
        self.hull.contains(p)
    }

    /// Règle de T02 §7.3 : (1) index « glo30 seul » → `Glo30`, sans `glo30` → `Dtm` ; (2) tuile
    /// mixte → polygones z12, première source qui contient le point, couture à `seam_m` ; (3) sans
    /// fichiers de couverture → `Unknown`.
    async fn dem_kinds(&self, pts: &[LatLon], ctx: &RequestCtx) -> Vec<DemKind> {
        let Some(index) = &self.index else { return vec![DemKind::Unknown; pts.len()] };
        let cases: Vec<IndexCase> = pts
            .iter()
            .map(|&p| match index.sources(tile_at(p, INDEX_ZOOM).0) {
                None => IndexCase::Known(DemKind::Unknown),
                Some(list) if list.iter().all(|s| s.name == GLO30) => IndexCase::Known(DemKind::Glo30),
                Some(list) if !list.iter().any(|s| s.name == GLO30) => IndexCase::Known(DemKind::Dtm),
                Some(list) => IndexCase::Mixed(list),
            })
            .collect();
        let Some(store) = &self.polygons else {
            // Tuile mixte sans polygones : la provenance n'est pas établie.
            return cases
                .iter()
                .map(|c| match c {
                    IndexCase::Known(k) => *k,
                    IndexCase::Mixed(_) => DemKind::Unknown,
                })
                .collect();
        };
        // Tuiles de polygones des points mixtes et de leurs sondes de couture.
        let mut keys = BTreeSet::new();
        for (p, c) in pts.iter().zip(&cases) {
            if let IndexCase::Mixed(_) = c {
                keys.insert(tile_at(*p, self.polygon_zoom).0);
                for q in self.probes(*p) {
                    keys.insert(tile_at(q, self.polygon_zoom).0);
                }
            }
        }
        let keys: Vec<TileKey> = keys.into_iter().collect();
        let outcomes = store.get_many(&keys, ctx).await;
        let tiles: BTreeMap<TileKey, TileOutcome<CoverageTile>> = keys.into_iter().zip(outcomes).collect();
        // Contrôle de l'issue #340 par tuile : une source listée par l'index sans aucun polygone.
        let mut doubtful: BTreeMap<TileKey, bool> = BTreeMap::new();
        let mut is_doubtful = |key: TileKey, order: &[IndexSource]| -> bool {
            *doubtful.entry(key).or_insert_with(|| match tiles.get(&key) {
                Some(TileOutcome::Data { tile, .. } | TileOutcome::Empty(tile)) => {
                    let missing: Vec<&str> = order
                        .iter()
                        .filter(|s| !tile.polygons.iter().any(|p| p.source == s.name))
                        .map(|s| s.name.as_str())
                        .collect();
                    for source in &missing {
                        tracing::warn!(source, "coverage polygons doubtful (Mapterhorn issue #340)");
                        metrics::counter!("coverage_anomalies_total", "source" => (*source).to_owned())
                            .increment(1);
                    }
                    !missing.is_empty()
                }
                _ => false,
            })
        };
        pts.iter()
            .zip(&cases)
            .map(|(&p, c)| {
                let order = match c {
                    IndexCase::Known(k) => return *k,
                    IndexCase::Mixed(order) => order,
                };
                let key = tile_at(p, self.polygon_zoom).0;
                if is_doubtful(key, order) {
                    // L'index fait foi pour la présence : il reste une source nationale.
                    return DemKind::Dtm;
                }
                match first_source(p, self.polygon_zoom, order, &tiles) {
                    Err(()) | Ok(None) => DemKind::Unknown,
                    Ok(Some(GLO30)) => DemKind::Glo30,
                    Ok(Some(_)) => {
                        let seam = self.probes(p).into_iter().any(|q| {
                            matches!(first_source(q, self.polygon_zoom, order, &tiles), Ok(Some(GLO30)))
                        });
                        if seam {
                            DemKind::Seam
                        } else {
                            DemKind::Dtm
                        }
                    }
                }
            })
            .collect()
    }

    fn extent(&self, _source: SourceId) -> &Extent {
        &self.world
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ord_domain::budget::{Deadline, RequestId};
    use ord_domain::coverage::CoveragePolygon;
    use std::time::Duration;

    #[test]
    fn hull_contains_france_and_not_its_neighbours() {
        let h = IgnHull;
        for (lat, lon) in [(47.2184, -1.5536), (45.19, 5.72), (48.85, 2.35), (42.15, 9.10), (43.30, 5.37)] {
            assert!(h.contains(LatLon::new(lat, lon)), "{lat} {lon}");
        }
        for (lat, lon) in [(45.25, 14.57), (50.63, 5.57), (47.07, 12.8), (57.0, -4.5), (31.5, 35.5)] {
            assert!(!h.contains(LatLon::new(lat, lon)), "{lat} {lon}");
        }
    }

    #[test]
    fn hull_tile_test_includes_edge_tiles() {
        let h = IgnHull;
        let nantes = tile_at(LatLon::new(47.2184, -1.5536), 15).0;
        assert!(h.intersects(nantes));
        let krk = tile_at(LatLon::new(45.246, 14.569), 15).0;
        assert!(!h.intersects(krk));
        // Une très grosse tuile qui contient tout le contour (aucun coin dedans) le touche.
        assert!(h.intersects(TileKey { z: 3, x: 3, y: 2 }));
    }

    #[test]
    fn maxzoom_and_priority() {
        assert_eq!(source_maxzoom(Some(30.0)), 12);
        assert_eq!(source_maxzoom(None), 12);
        assert_eq!(source_maxzoom(Some(1.0)), 17);
        assert_eq!(source_maxzoom(Some(0.5)), 18);
        let mut s = vec![
            IndexSource { name: "glo30".into(), resolution: Some(30.0) },
            IndexSource { name: "frrgealti5metro".into(), resolution: Some(5.0) },
            IndexSource { name: "frrgealti1metro".into(), resolution: Some(1.0) },
            IndexSource { name: "aaa30".into(), resolution: Some(30.0) },
        ];
        priority_order(&mut s);
        let names: Vec<&str> = s.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["frrgealti1metro", "frrgealti5metro", "aaa30", "glo30"]);
    }

    struct Store(BTreeMap<TileKey, CoverageTile>);

    #[async_trait::async_trait]
    impl TileStore<CoverageTile> for Store {
        async fn get_many(&self, keys: &[TileKey], _: &RequestCtx) -> Vec<TileOutcome<CoverageTile>> {
            keys.iter()
                .map(|k| match self.0.get(k) {
                    Some(t) => TileOutcome::Data { tile: Arc::new(t.clone()), stale: false },
                    None => TileOutcome::Absent,
                })
                .collect()
        }
        fn data_version(&self) -> ord_domain::catalog::DataVersion {
            unreachable!()
        }
    }

    fn ctx() -> RequestCtx {
        RequestCtx::new(RequestId("t".into()), Deadline::after(Duration::from_secs(5)))
    }

    fn index_json(names: &[(&str, f64)]) -> Vec<u8> {
        let m: serde_json::Map<String, serde_json::Value> =
            names.iter().map(|(n, r)| ((*n).to_owned(), serde_json::json!({ "resolution": r }))).collect();
        serde_json::to_vec(&m).unwrap()
    }

    /// Une tuile z12 « glo30 seul », une sans glo30, une mixte : moitié ouest RGE ALTI, glo30
    /// partout. Points : RGE ALTI, à 50 m du bord RGE ALTI, glo30 du polygone, glo30 de l'index,
    /// tuile sans glo30.
    fn fixture(rge_has_polygon: bool) -> (MapterhornCoverage, [LatLon; 5]) {
        let (mixed, ..) = tile_at(LatLon::new(46.0, -1.0), 12);
        let glo_only = TileKey { x: mixed.x + 2, ..mixed };
        let dtm_only = TileKey { x: mixed.x + 4, ..mixed };
        let index = crate::pmtiles::tests::build(&[
            (mixed, index_json(&[("glo30", 30.0), ("frrgealti1metro", 1.0)])),
            (glo_only, index_json(&[("glo30", 30.0)])),
            (dtm_only, index_json(&[("frrgealti1metro", 1.0)])),
        ]);
        let mut polygons = vec![CoveragePolygon {
            source: "glo30".into(),
            rings: vec![vec![[-64.0, -64.0], [4160.0, -64.0], [4160.0, 4160.0], [-64.0, 4160.0]]],
        }];
        if rge_has_polygon {
            polygons.push(CoveragePolygon {
                source: "frrgealti1metro".into(),
                rings: vec![vec![[-64.0, -64.0], [2048.0, -64.0], [2048.0, 4160.0], [-64.0, 4160.0]]],
            });
        }
        let store = Store(BTreeMap::from([(mixed, CoverageTile { extent: 4096, polygons })]));
        let cov = MapterhornCoverage::new(
            Some(Arc::new(CoverageIndex::parse(index, None).unwrap())),
            Some(Arc::new(store)),
            12,
            150.0,
        );
        let (w, s, e, n) = mixed.bounds();
        let at = |fx: f64| LatLon::new(0.5 * (s + n), w + fx * (e - w));
        let width_m = (e - w) * 111_195.0 * (0.5 * (s + n)).to_radians().cos();
        let centre = |k: TileKey| {
            let (w, s, e, n) = k.bounds();
            LatLon::new(0.5 * (s + n), 0.5 * (w + e))
        };
        (cov, [at(0.25), at(0.5 - 50.0 / width_m), at(0.75), centre(glo_only), centre(dtm_only)])
    }

    #[tokio::test]
    async fn index_polygons_and_seam() {
        let (cov, pts) = fixture(true);
        let kinds = cov.dem_kinds(&pts, &ctx()).await;
        assert_eq!(
            kinds,
            [DemKind::Dtm, DemKind::Seam, DemKind::Glo30, DemKind::Glo30, DemKind::Dtm],
            "RGE ALTI, couture à 50 m du bord, glo30 du polygone, glo30 de l'index, sans glo30"
        );
    }

    #[tokio::test]
    async fn issue_340_falls_back_on_the_index() {
        let (cov, pts) = fixture(false);
        let kinds = cov.dem_kinds(&[pts[0], pts[2]], &ctx()).await;
        assert_eq!(kinds, [DemKind::Dtm, DemKind::Dtm], "source listée sans polygone : l'index fait foi");
    }

    /// Index réel de Mapterhorn, s'il est fourni (`ORD_COVERAGE_INDEX=<fichier>`) : jamais en CI.
    #[test]
    fn real_index_when_provided() {
        let Some(path) = std::env::var_os("ORD_COVERAGE_INDEX") else { return };
        let index = CoverageIndex::parse(std::fs::read(path).unwrap(), None).unwrap();
        let names = |p: LatLon| -> Vec<String> {
            index
                .sources(tile_at(p, INDEX_ZOOM).0)
                .map(|l| l.iter().map(|s| s.name.clone()).collect())
                .unwrap_or_default()
        };
        assert_eq!(names(LatLon::new(45.2464, 14.5692)), ["glo30"], "Krk : GLO-30 seul");
        let paris = names(LatLon::new(48.85, 2.35));
        assert_eq!(paris.last().map(String::as_str), Some("glo30"), "{paris:?}");
        assert!(paris.first().is_some_and(|s| s.starts_with("frhd")), "{paris:?}");
    }

    #[tokio::test]
    async fn without_coverage_files_everything_is_unknown() {
        let cov = MapterhornCoverage::hull_only();
        assert_eq!(cov.dem_kinds(&[LatLon::new(45.2, 14.5)], &ctx()).await, [DemKind::Unknown]);
        assert!(cov.in_primary_zone(LatLon::new(47.0, -1.5)));
    }
}
