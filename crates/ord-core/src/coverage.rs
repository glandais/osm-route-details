//! Zonage (T01 §5.1, étape 3) : plages en distance hors de la zone France, à partir du test de
//! zone de chaque station (`CoverageRef::in_primary_zone`, lu par le pipeline). Fonction pure.

use ord_domain::coverage::CoverageRanges;
use ord_domain::route::PreparedRoute;

use crate::elevation::runs;

/// Plages hors zone France ; chaque station vaut la moitié de ses deux intervalles, comme les
/// plages de terrain. Les plages d'emprise (`typology_absent`, `elevation_absent`) restent vides :
/// toutes les sources configurées couvrent le monde (`Extent::World`).
pub fn zone_ranges(route: &PreparedRoute, in_zone: &[bool]) -> CoverageRanges {
    let d: Vec<f64> = route.samples.iter().map(|s| s.d_m).collect();
    let outside_primary_zone = runs(&d, route.length_m, in_zone)
        .into_iter()
        .filter(|(_, _, inside)| !inside)
        .map(|(from, to, _)| from..to)
        .collect();
    CoverageRanges { outside_primary_zone, ..CoverageRanges::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elevation::tests::line;

    #[test]
    fn outside_ranges_follow_station_halves() {
        let route = line(50.0);
        let flags: Vec<bool> = (0..route.samples.len()).map(|i| i < 4).collect();
        let r = zone_ranges(&route, &flags);
        assert_eq!(r.outside_primary_zone.len(), 1);
        let range = &r.outside_primary_zone[0];
        assert!((range.start - 17.5).abs() < 1e-6 && (range.end - route.length_m).abs() < 1e-9);
        assert!(zone_ranges(&route, &vec![true; route.samples.len()]).outside_primary_zone.is_empty());
    }
}
