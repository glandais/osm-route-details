//! La consigne de part de non-revêtu au banc (lot 5, étape 7, §13) : la variante `target:<t>` joue la requête par
//! `generate_steered_in`, et chaque jeu garde ce que la recherche a fait (pas, μ, `reached`) et, quand la première
//! proposition manque la cible, la part de non-revêtu disponible dans le disque D / 2 (critère O5, §9).

use ord_graph::grid;
use ord_graph::mmap::MappedGraph;
use ord_loops::{OffroadTarget, Request, RideType, Steer};
use serde::Serialize;

/// Tolérance de la consigne au banc : celle du service par défaut (§4.3), cinq points.
pub const TOLERANCE_MILLI: u16 = 50;
/// Pas au plus (§4.3, décision 3 du propriétaire).
pub const MAX_STEPS: u8 = 4;
/// Aucun pas lancé au-delà de 10 s de calcul ; le banc n'a pas d'échéance de service.
pub const STEPS_UNTIL_MS: u32 = 10_000;

/// La consigne d'un jeu : `None` pour ROAD (la cible y est refusée par le moteur : le banc joue ROAD sans consigne, les
/// critères O et E ne portent que sur le gravel et le VTT) et pour une variante sans `target`.
pub fn steer_of(target_milli: Option<u16>, ride: RideType) -> Option<Steer> {
    let t = target_milli.filter(|_| ride != RideType::Road)?;
    Some(Steer {
        offroad: Some(OffroadTarget {
            target_milli: t,
            tolerance_milli: TOLERANCE_MILLI,
            max_steps: MAX_STEPS,
            steps_until_ms: STEPS_UNTIL_MS,
        }),
    })
}

/// Ce que la recherche a fait pour un jeu, et ce que le disque D / 2 offrait quand la cible n'est pas tenue.
#[derive(Clone, Debug, Serialize)]
pub struct SteerRec {
    pub target: f64,
    /// Générations jouées (pas 0 compris).
    pub steps: u8,
    /// μ du pas qui a donné la première proposition.
    pub mu: f64,
    /// `reached` du moteur (vérificateur interne).
    pub reached_engine: bool,
    /// Part de non-revêtu du premier parcours **gardé** par le contrôleur (mesure du banc) ; `None` sans parcours gardé.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept_share: Option<f64>,
    /// Part de non-revêtu de la longueur des arêtes dont les deux extrémités sont dans le disque D / 2 autour du départ,
    /// rapportée quand la cible n'est pas tenue (O5) : `(part, km)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disc: Option<(f64, f64)>,
}

/// La cible est-elle tenue à la tolérance de la consigne ?
pub fn within(share: f64, target: f64) -> bool {
    (share - target).abs() <= f64::from(TOLERANCE_MILLI) / 1000.0 + 1e-9
}

/// Part de non-revêtu (route-details) et longueur (km) des arêtes de `gf` dont les deux extrémités sont à moins de
/// `radius_m` de `req.start` (distance équirectangulaire). Parcourt les tuiles qui touchent le disque.
pub fn disc_offroad(gf: &MappedGraph, req: &Request, radius_m: f64) -> Option<(f64, f64)> {
    const R: f64 = 6_371_008.8;
    let (lat0, lon0) = (req.start.lat, req.start.lon);
    let cos0 = lat0.to_radians().cos().max(0.01);
    let inside = |lat_e7: i32, lon_e7: i32| {
        let dy = (f64::from(lat_e7) * 1e-7 - lat0).to_radians() * R;
        let dx = (f64::from(lon_e7) * 1e-7 - lon0).to_radians() * R * cos0;
        dx * dx + dy * dy <= radius_m * radius_m
    };
    let dlat = (radius_m / R).to_degrees() * 1.01 + 1e-6;
    let dlon = dlat / cos0;
    let zoom = gf.header().zoom;
    let (x0, y0) = grid::tile_of_deg(lat0 + dlat, lon0 - dlon, zoom);
    let (x1, y1) = grid::tile_of_deg(lat0 - dlat, lon0 + dlon, zoom);
    let (mut total_dm, mut off_dm) = (0.0f64, 0.0f64);
    for x in x0..=x1 {
        for y in y0..=y1 {
            let Some(ci) = gf.find_cell(x, y) else { continue };
            let cv = gf.cell(ci);
            let info = cv.info();
            for i in 0..info.n_edges {
                let (a, b) = gf.edge_ends(info.first_edge + i);
                let (pa, pb) = (gf.node_pos(a), gf.node_pos(b));
                if !(inside(pa.0, pa.1) && inside(pb.0, pb.1)) {
                    continue;
                }
                let o = cv.owned(i as usize);
                let l = f64::from(o.len_dm);
                total_dm += l;
                if o.attrs.offroad() {
                    off_dm += l;
                }
            }
        }
    }
    (total_dm > 0.0).then(|| (off_dm / total_dm, total_dm / 10_000.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn road_is_never_steered_and_the_others_carry_the_defaults() {
        assert!(steer_of(None, RideType::Gravel).is_none());
        assert!(steer_of(Some(470), RideType::Road).is_none());
        let s = steer_of(Some(470), RideType::Gravel).unwrap().offroad.unwrap();
        assert_eq!((s.target_milli, s.tolerance_milli, s.max_steps, s.steps_until_ms), (470, 50, 4, 10_000));
        assert!(steer_of(Some(250), RideType::Mtb).is_some());
    }

    /// Une ligne de quatre nœuds espacés de 300 m : `residential`, `track` en gravier, `residential`, puis un nœud à 20 km
    /// (hors de tout disque de 1 km).
    fn line(dir: &std::path::Path) -> MappedGraph {
        use ord_graph::build::{build_graph, BuildOptions, MemSource};
        use ord_graph::format::write_graph;
        let (lat0, lon0, m_lat) = (47.0f64, -1.80f64, 111_194.926_6f64);
        let m_lon = m_lat * lat0.to_radians().cos();
        let mut src = MemSource::default();
        for k in 0..4i64 {
            src.node(k + 1, lat0, lon0 + k as f64 * 300.0 / m_lon);
        }
        src.node(9, lat0, lon0 + 20_000.0 / m_lon);
        src.way(1, &[1, 2], &[("highway", "residential")]);
        src.way(2, &[2, 3], &[("highway", "track"), ("surface", "gravel")]);
        src.way(3, &[3, 4], &[("highway", "residential")]);
        src.way(4, &[4, 9], &[("highway", "track"), ("surface", "gravel")]);
        let mut data = build_graph(&mut src, &BuildOptions { zoom: 11 }, "test", &mut |_| {}).unwrap();
        let path = dir.join("g.ordg");
        write_graph(&path, &mut data, "test", 1_700_000_000).unwrap();
        MappedGraph::open_mmap(&path).unwrap()
    }

    #[test]
    fn the_disc_share_counts_only_edges_inside_the_disc() {
        let dir = tempfile::tempdir().unwrap();
        let gf = line(dir.path());
        let req = Request {
            start: ord_loops::LatLon { lat: 47.0, lon: -1.80 },
            end: None,
            bearing: None,
            distance_m: 2_000.0,
            ride_type: RideType::Gravel,
            mode: ord_loops::Mode::Discovery,
            familiarity: 0.0,
            off_team_cost_q8: 768,
            seed: 1,
            figures: ord_loops::Figures::Classic,
        };
        // Rayon 1 000 m : les trois premières arêtes (900 m), dont une sur trois en gravier ; l'arête de 19 km sort.
        let (share, km) = disc_offroad(&gf, &req, 1_000.0).unwrap();
        assert!((km - 0.9).abs() < 0.02, "{km}");
        assert!((share - 1.0 / 3.0).abs() < 0.01, "{share}");
        // Rayon 400 m : la première arête seule (le nœud 3 est à 600 m), revêtue.
        let (share, km) = disc_offroad(&gf, &req, 400.0).unwrap();
        assert!((km - 0.3).abs() < 0.02 && share == 0.0, "{share} {km}");
        // Loin de tout : aucun chemin.
        let far = Request { start: ord_loops::LatLon { lat: 47.5, lon: -1.0 }, ..req };
        assert!(disc_offroad(&gf, &far, 1_000.0).is_none());
    }

    #[test]
    fn the_tolerance_is_five_points_inclusive() {
        assert!(within(0.52, 0.47) && within(0.42, 0.47));
        assert!(!within(0.5201, 0.47) && !within(0.4199, 0.47));
    }
}
