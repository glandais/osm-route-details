//! Typologie sur des réseaux synthétiques, sans réseau ni fixture.

use std::sync::Arc;

use ord_domain::budget::Unbounded;
use ord_domain::geo::{Grid, EARTH_RADIUS_M};
use ord_domain::street::{PavedGroup, StreetLine, StreetTile, StructureTag, SurfaceTag, TrackType, WayAttrs};
use ord_domain::LatLon;

use super::*;
use crate::classification::tests::{caps, way};
use crate::classification::{StructureClass, SurfaceClass, SurfaceSource, WayKind};
use crate::prepare::prepare;
use crate::rules::TypologyRules;

const ORIGIN: LatLon = LatLon::new(47.2, -1.5);
const GRID: Grid = Grid::new(14, 4096);

/// Mètres vers l'est et le nord de l'origine.
fn at(east: f64, north: f64) -> LatLon {
    let lat = ORIGIN.lat + (north / EARTH_RADIUS_M).to_degrees();
    let lon = ORIGIN.lon + (east / (EARTH_RADIUS_M * ORIGIN.lat.to_radians().cos())).to_degrees();
    LatLon::new(lat, lon)
}

fn u(east: f64, north: f64) -> [f64; 2] {
    GRID.to_units(at(east, north))
}

struct Net {
    attrs: Vec<WayAttrs>,
    lines: Vec<StreetLine>,
}

impl Net {
    fn new() -> Self {
        Self { attrs: Vec::new(), lines: Vec::new() }
    }

    fn add(mut self, a: WayAttrs, pts: &[(f64, f64)]) -> Self {
        self.attrs.push(a);
        let points = pts.iter().map(|&(e, n)| u(e, n)).collect();
        self.lines.push(StreetLine { attr: (self.attrs.len() - 1) as u32, points, cut: [false, false] });
        self
    }

    /// Tuiles du couloir : la première porte tout le réseau, les autres sont vides ; `fail` met
    /// en panne la tuile qui contient ce point.
    fn tiles(self, route: &[LatLon], fail: Option<LatLon>) -> Vec<(TileKey, TileState)> {
        let keys = corridor_tiles(GRID, route, 100.0);
        let failed = fail.map(|p| GRID.tile_of(GRID.to_units(p)));
        let mut tile = StreetTile::empty(keys[0]);
        tile.attrs = self.attrs;
        tile.lines = self.lines;
        let mut tile = Some(Arc::new(tile));
        keys.into_iter()
            .map(|k| {
                let state = if Some(k) == failed {
                    TileState::Failed
                } else if let Some(t) = tile.take() {
                    TileState::Data(t)
                } else {
                    TileState::Empty
                };
                (k, state)
            })
            .collect()
    }
}

fn with_surface(kind: &str, s: PavedGroup) -> WayAttrs {
    WayAttrs { surface: SurfaceTag::Grouped(s), ..way(kind) }
}

fn bridge(kind: &str) -> WayAttrs {
    WayAttrs { structure: StructureTag::Bridge, ..with_surface(kind, PavedGroup::Paved) }
}

fn run(net: Net, pts: &[(f64, f64)], fail: Option<LatLon>) -> (Vec<TypedSegment>, TypologyOutput) {
    let route_pts: Vec<LatLon> = pts.iter().map(|&(e, n)| at(e, n)).collect();
    let route = prepare(&route_pts, 5.0);
    let rules = TypologyRules::default();
    let tiles = net.tiles(&route_pts, fail);
    let out = compute(&route, GRID, &tiles, &caps(), &rules, &Unbounded).expect("calcul");
    let segs = build_segments(&out.runs, &out.classes, &route, rules.min_segment_m);
    // Partition exacte de [0, L].
    assert_eq!(segs.first().map(|s| s.from_m), Some(0.0));
    assert!((segs.last().unwrap().to_m - route.length_m).abs() < 1e-6);
    assert!(segs.windows(2).all(|w| (w[0].to_m - w[1].from_m).abs() < 1e-9 && !w[0].same_key_pub(&w[1])));
    (segs, out)
}

impl TypedSegment {
    fn same_key_pub(&self, o: &Self) -> bool {
        (self.structure, self.surface_class, self.surface_source, self.way_kind, self.matched, self.element)
            == (o.structure, o.surface_class, o.surface_source, o.way_kind, o.matched, o.element)
    }
}

fn length_where(segs: &[TypedSegment], f: impl Fn(&TypedSegment) -> bool) -> f64 {
    segs.iter().filter(|s| f(s)).map(TypedSegment::len).sum()
}

/// Route revêtue sur y = 0 de 0 à 600 m, chemin non revêtu parallèle à 20 m au nord de 100 à
/// 500 m, relié à la route aux deux bouts, autoroute à 10 m au sud.
fn parallel() -> Net {
    let road: Vec<(f64, f64)> = (0..=12).map(|k| (k as f64 * 50.0, 0.0)).collect();
    Net::new()
        .add(with_surface("residential", PavedGroup::Paved), &road)
        .add(
            WayAttrs { tracktype: Some(TrackType::Grade2), ..way("track") },
            &[(100.0, 0.0), (100.0, 20.0), (300.0, 20.0), (500.0, 20.0), (500.0, 0.0)],
        )
        .add(with_surface("motorway", PavedGroup::Paved), &[(0.0, -10.0), (600.0, -10.0)])
}

#[test]
fn follows_the_parallel_track_it_is_on() {
    let trace =
        [(0.0, 1.0), (100.0, 1.0), (100.0, 19.0), (300.0, 18.0), (500.0, 19.0), (500.0, 1.0), (600.0, 0.0)];
    let (segs, out) = run(parallel(), &trace, None);
    assert!(segs.iter().all(|s| s.matched), "{segs:?}");
    assert_eq!(segs.first().unwrap().surface_class, SurfaceClass::Paved);
    assert_eq!(segs.last().unwrap().surface_class, SurfaceClass::Paved);
    let track = length_where(&segs, |s| s.way_kind == WayKind::Track);
    assert!(track > 380.0 && track < 460.0, "{track} m de chemin : {segs:?}");
    assert!(segs
        .iter()
        .filter(|s| s.way_kind == WayKind::Track)
        .all(|s| s.surface_source == SurfaceSource::InferredTracktype));
    assert!(out.classes.iter().all(|c| &*c.attrs.highway != "motorway"), "autoroute hors graphe");
    assert!(out.coverage.missing.is_empty() && out.coverage.unavailable.is_empty());
}

#[test]
fn far_points_are_unmatched() {
    let trace = [(0.0, 0.0), (200.0, 0.0), (300.0, 300.0), (400.0, 0.0), (600.0, 0.0)];
    let (segs, _) = run(parallel(), &trace, None);
    let unmatched = length_where(&segs, |s| !s.matched);
    assert!(unmatched > 400.0, "{unmatched}");
    assert!(segs.iter().filter(|s| !s.matched).all(|s| s.structure == StructureClass::Unknown
        && s.surface_class == SurfaceClass::Unknown
        && s.surface_source == SurfaceSource::None
        && s.way_kind == WayKind::Unknown
        && s.structure_source.is_none()));
    assert!(segs.first().unwrap().matched && segs.last().unwrap().matched);
}

/// Route au sol nord-sud sous un pont est-ouest ; le pont rejoint par ses culées une route au sol
/// à chaque bout.
fn overpass() -> Net {
    Net::new()
        .add(with_surface("residential", PavedGroup::Paved), &[(0.0, -400.0), (0.0, 400.0)])
        .add(with_surface("tertiary", PavedGroup::Paved), &[(-400.0, 0.0), (-60.0, 0.0)])
        .add(bridge("tertiary"), &[(-60.0, 0.0), (0.0, 0.0), (60.0, 0.0)])
        .add(with_surface("tertiary", PavedGroup::Paved), &[(60.0, 0.0), (400.0, 0.0)])
}

#[test]
fn passing_under_a_bridge_stays_on_the_ground() {
    let (segs, _) = run(overpass(), &[(0.0, -300.0), (0.0, 300.0)], None);
    assert!(segs.iter().all(|s| s.structure == StructureClass::Ground && s.matched), "{segs:?}");
    assert_eq!(segs.len(), 1);
}

#[test]
fn crossing_on_the_bridge_is_a_bridge_segment() {
    let (segs, _) = run(overpass(), &[(-300.0, 0.0), (300.0, 0.0)], None);
    let b: Vec<&TypedSegment> = segs.iter().filter(|s| s.structure == StructureClass::Bridge).collect();
    assert_eq!(b.len(), 1, "{segs:?}");
    assert!((b[0].len() - 120.0).abs() < 12.0, "{}", b[0].len());
    assert_eq!(b[0].structure_source, Some(crate::classification::StructureSource::Osm));
    assert!(b[0].element.is_some());
}

#[test]
fn under_then_over_the_same_bridge() {
    // Sous le pont vers le nord, demi-tour, retour par l'ouest et passage sur le pont vers l'est.
    let trace = [(0.0, -300.0), (0.0, 300.0), (0.0, 100.0), (-300.0, 0.0), (300.0, 0.0)];
    let (segs, _) = run(overpass(), &trace, None);
    let bridges: Vec<&TypedSegment> = segs.iter().filter(|s| s.structure == StructureClass::Bridge).collect();
    assert_eq!(bridges.len(), 1, "seul le passage dessus est un pont : {segs:?}");
    assert!(bridges[0].from_m > 900.0);
}

#[test]
fn failed_tile_gives_no_data_not_unmatched() {
    let trace = [(0.0, -300.0), (0.0, 300.0)];
    let fail = Some(at(0.0, 0.0));
    let (segs, out) = run(overpass(), &trace, fail);
    assert!(!out.coverage.unavailable.is_empty(), "{:?}", out.coverage);
    assert!(out.coverage.missing.is_empty());
    assert!(segs.iter().all(|s| !s.matched));
}

#[test]
fn short_ground_pieces_are_absorbed() {
    // Une piste cyclable de 10 m au milieu d'une route : absorbée.
    let net = Net::new()
        .add(with_surface("residential", PavedGroup::Paved), &[(0.0, 0.0), (200.0, 0.0)])
        .add(with_surface("cycleway", PavedGroup::Paved), &[(200.0, 0.0), (210.0, 0.0)])
        .add(with_surface("residential", PavedGroup::Paved), &[(210.0, 0.0), (400.0, 0.0)]);
    let (segs, _) = run(net, &[(0.0, 0.0), (400.0, 0.0)], None);
    assert_eq!(segs.len(), 1, "{segs:?}");
    assert_eq!(segs[0].way_kind, WayKind::MinorRoad);
    assert_eq!((segs[0].from_index, segs[0].to_index), (0, 1));
}

#[test]
fn tile_border_cut_is_rejoined_on_its_level_only() {
    let mk = |a: WayAttrs, pts: Vec<[f64; 2]>, cut: [bool; 2]| (a, StreetLine { attr: 0, points: pts, cut });
    let base = u(0.0, 0.0);
    let p = |dx: f64, dy: f64| [base[0] + dx, base[1] + dy];
    let road = with_surface("residential", PavedGroup::Paved);
    let pieces = vec![
        mk(road.clone(), vec![p(0.0, 0.0), p(100.0, 0.0)], [false, true]),
        mk(road.clone(), vec![p(100.0, 0.3), p(200.0, 0.0)], [true, false]),
        // Un pont dont un sommet intérieur tombe à 0,5 unité de la route : jamais relié.
        mk(bridge("residential"), vec![p(150.0, -50.0), p(150.0, 0.5), p(150.0, 50.0)], [false, false]),
    ];
    let tiles: Vec<Arc<StreetTile>> = pieces
        .into_iter()
        .map(|(a, l)| {
            let mut t = StreetTile::empty(GRID.tile_of(base));
            t.attrs.push(a);
            t.lines.push(l);
            Arc::new(t)
        })
        .collect();
    let pts: Vec<([f64; 2], f64)> = (0..=20).map(|k| (p(k as f64 * 10.0, 0.0), ORIGIN.lat)).collect();
    let g = StreetGraph::build(GRID, &tiles, &Corridor::new(GRID, &pts, 200.0), &caps());
    let ground: Vec<_> = g.segs.iter().filter(|s| !s.level.is_structure()).collect();
    let deck: Vec<_> = g.segs.iter().filter(|s| s.level.is_structure()).collect();
    assert_eq!(ground.len(), 2, "{:?}", g.segs);
    let shared = [ground[0].a, ground[0].b].iter().filter(|n| [ground[1].a, ground[1].b].contains(n)).count();
    assert_eq!(shared, 1, "recollés au bord de tuile");
    assert_eq!(deck.len(), 2);
    let ground_nodes: Vec<u32> = ground.iter().flat_map(|s| [s.a, s.b]).collect();
    assert!(
        deck.iter().all(|s| !ground_nodes.contains(&s.a) && !ground_nodes.contains(&s.b)),
        "pont déconnecté"
    );
}

#[test]
fn contiguous_bridge_lines_are_two_elements() {
    let net = Net::new()
        .add(with_surface("residential", PavedGroup::Paved), &[(0.0, 0.0), (100.0, 0.0)])
        .add(bridge("residential"), &[(100.0, 0.0), (200.0, 0.0)])
        .add(
            WayAttrs { tracktype: Some(TrackType::Grade1), ..bridge("residential") },
            &[(200.0, 0.0), (300.0, 0.0)],
        )
        .add(with_surface("residential", PavedGroup::Paved), &[(300.0, 0.0), (400.0, 0.0)]);
    let (segs, _) = run(net, &[(0.0, 0.0), (400.0, 0.0)], None);
    let b: Vec<&TypedSegment> = segs.iter().filter(|s| s.structure == StructureClass::Bridge).collect();
    assert_eq!(b.len(), 2, "{segs:?}");
    assert_ne!(b[0].element, b[1].element);
}
