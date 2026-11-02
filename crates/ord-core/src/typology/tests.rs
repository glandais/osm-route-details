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

pub(crate) const ORIGIN: LatLon = LatLon::new(47.2, -1.5);
pub(crate) const GRID: Grid = Grid::new(14, 4096);

/// Mètres vers l'est et le nord de l'origine.
pub(crate) fn at(east: f64, north: f64) -> LatLon {
    let lat = ORIGIN.lat + (north / EARTH_RADIUS_M).to_degrees();
    let lon = ORIGIN.lon + (east / (EARTH_RADIUS_M * ORIGIN.lat.to_radians().cos())).to_degrees();
    LatLon::new(lat, lon)
}

fn u(east: f64, north: f64) -> [f64; 2] {
    GRID.to_units(at(east, north))
}

pub(crate) struct Net {
    attrs: Vec<WayAttrs>,
    lines: Vec<StreetLine>,
}

impl Net {
    pub(crate) fn new() -> Self {
        Self { attrs: Vec::new(), lines: Vec::new() }
    }

    pub(crate) fn add(mut self, a: WayAttrs, pts: &[(f64, f64)]) -> Self {
        self.attrs.push(a);
        let points = pts.iter().map(|&(e, n)| u(e, n)).collect();
        self.lines.push(StreetLine { attr: (self.attrs.len() - 1) as u32, points, cut: [false, false] });
        self
    }

    /// Tuiles du couloir : la première porte tout le réseau, les autres sont vides ; `fail` met
    /// en panne la tuile qui contient ce point.
    pub(crate) fn tiles(self, route: &[LatLon], fail: Option<LatLon>) -> Vec<(TileKey, TileState)> {
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

pub(crate) fn with_surface(kind: &str, s: PavedGroup) -> WayAttrs {
    WayAttrs { surface: SurfaceTag::Grouped(s), ..way(kind) }
}

pub(crate) fn bridge(kind: &str) -> WayAttrs {
    WayAttrs { structure: StructureTag::Bridge, ..with_surface(kind, PavedGroup::Paved) }
}

fn run(net: Net, pts: &[(f64, f64)], fail: Option<LatLon>) -> (Vec<TypedSegment>, TypologyOutput) {
    let route_pts: Vec<LatLon> = pts.iter().map(|&(e, n)| at(e, n)).collect();
    let route = prepare(&route_pts, 5.0);
    let rules = TypologyRules::default();
    let tiles = net.tiles(&route_pts, fail);
    let out = compute(&route, GRID, &tiles, &caps(), &rules, &Unbounded).expect("calcul");
    let segs = build_segments(&out.runs, &out.classes, &[], &route, rules.min_segment_m);
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
    // L'autoroute est connue (état hors graphe) mais jamais suivie ici.
    assert!(
        segs.iter()
            .all(|s| !s.off_graph
                && s.class.is_none_or(|c| &*out.classes[c as usize].attrs.highway != "motorway")),
        "autoroute hors graphe"
    );
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
pub(crate) fn overpass() -> Net {
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

pub(crate) fn tunnel(kind: &str) -> WayAttrs {
    WayAttrs { structure: StructureTag::Tunnel, ..with_surface(kind, PavedGroup::Paved) }
}

/// Typologie d'un parcours (en mètres est, nord) sur un réseau.
pub(crate) fn typology_of(net: Net, pts: &[(f64, f64)]) -> (PreparedRoute, TypologyOutput) {
    let route_pts: Vec<LatLon> = pts.iter().map(|&(e, n)| at(e, n)).collect();
    let route = prepare(&route_pts, 5.0);
    let tiles = net.tiles(&route_pts, None);
    let out = compute(&route, GRID, &tiles, &caps(), &TypologyRules::default(), &Unbounded).expect("calcul");
    (route, out)
}

#[test]
fn a_crossed_bridge_has_its_abutments_and_one_passage() {
    let (route, out) = typology_of(overpass(), &[(-300.0, 0.0), (300.0, 0.0)]);
    assert_eq!(out.structures.len(), 1, "{:?}", out.structures);
    let s = &out.structures[0];
    assert!((s.length_m - 120.0).abs() < 1.0, "{}", s.length_m);
    assert!(s.ends.iter().all(|e| e.kind == EndKind::Abutment), "{:?}", s.ends);
    assert!(s.ends.iter().all(|e| !e.approach.is_empty()));
    assert_eq!(out.passages.len(), 1);
    let p = &out.passages[0];
    assert!(p.doubts.is_empty(), "{:?}", p.doubts);
    assert!((p.from_m - 240.0).abs() < 12.0 && (p.to_m - 360.0).abs() < 12.0, "{p:?}");
    // Chaînage : de A (ouest, plus petite coordonnée) vers B, un mètre de parcours par mètre.
    let mid = p.chainage_at(300.0);
    assert!((mid - 60.0).abs() < 2.0, "{mid}");
    assert!(route.length_m > 590.0);
}

#[test]
fn a_route_starting_on_the_deck_reaches_both_abutments() {
    // Départ au milieu du pont : l'ouvrage garde ses deux culées (inv. 2).
    let (_, out) = typology_of(overpass(), &[(0.0, 0.0), (300.0, 0.0)]);
    assert_eq!(out.structures.len(), 1);
    let s = &out.structures[0];
    assert!((s.length_m - 120.0).abs() < 1.0, "{}", s.length_m);
    assert!(s.ends.iter().all(|e| e.kind == EndKind::Abutment));
    let (lo, hi) = out.passages[0].chain_extent();
    assert!(lo > 50.0 && hi > 115.0, "{lo} {hi}");
}

#[test]
fn a_u_turn_on_the_deck_is_one_passage_over_the_whole_structure() {
    let (_, out) = typology_of(overpass(), &[(-300.0, 0.0), (10.0, 0.0), (-300.0, 0.0)]);
    assert_eq!(out.structures.len(), 1);
    assert!((out.structures[0].length_m - 120.0).abs() < 1.0, "prolongé jusqu'à la culée opposée");
    assert_eq!(out.passages.len(), 1, "{:?}", out.passages);
    let (lo, hi) = out.passages[0].chain_extent();
    assert!(lo < 5.0 && (hi - 70.0).abs() < 6.0, "{lo} {hi}");
}

#[test]
fn a_bridge_crossed_twice_is_one_structure_two_passages() {
    let (_, out) = typology_of(overpass(), &[(-300.0, 0.0), (300.0, 0.0), (-300.0, 0.0)]);
    assert_eq!(out.structures.len(), 1);
    assert_eq!(out.passages.len(), 2);
    assert_eq!(out.passages[0].structure, out.passages[1].structure);
}

#[test]
fn bridge_then_tunnel_is_one_structure_with_two_elements() {
    let net = Net::new()
        .add(with_surface("tertiary", PavedGroup::Paved), &[(-400.0, 0.0), (-60.0, 0.0)])
        .add(bridge("tertiary"), &[(-60.0, 0.0), (0.0, 0.0)])
        .add(tunnel("tertiary"), &[(0.0, 0.0), (200.0, 0.0)])
        .add(with_surface("tertiary", PavedGroup::Paved), &[(200.0, 0.0), (500.0, 0.0)]);
    let (_, out) = typology_of(net, &[(-300.0, 0.0), (400.0, 0.0)]);
    assert_eq!(out.structures.len(), 1, "{:?}", out.structures);
    let kinds: Vec<StructureKind> = out.structures[0].elements.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [StructureKind::Bridge, StructureKind::Tunnel]);
}

#[test]
fn passing_under_a_bridge_makes_no_passage() {
    let (_, out) = typology_of(overpass(), &[(0.0, -300.0), (0.0, 300.0)]);
    assert!(out.passages.is_empty() && out.structures.is_empty());
}

#[test]
fn an_end_in_an_unread_tile_needs_it() {
    // Pont coupé au bord d'une tuile non lue : l'étape rend la tuile voisine.
    let base = u(0.0, 0.0);
    let tile = GRID.tile_of(base);
    let origin = GRID.tile_origin(tile);
    let edge = origin[0] + 4096.0;
    let y = base[1];
    let mut t = StreetTile::empty(tile);
    t.attrs.push(with_surface("tertiary", PavedGroup::Paved));
    t.attrs.push(bridge("tertiary"));
    t.lines.push(StreetLine {
        attr: 0,
        points: vec![[edge - 600.0, y], [edge - 200.0, y]],
        cut: [false, false],
    });
    t.lines.push(StreetLine { attr: 1, points: vec![[edge - 200.0, y], [edge, y]], cut: [false, true] });
    let route_pts = vec![GRID.to_latlon([edge - 550.0, y]), GRID.to_latlon([edge - 50.0, y])];
    let route = prepare(&route_pts, 5.0);
    let mut tiles: Vec<(TileKey, TileState)> = corridor_tiles(GRID, &route_pts, 100.0)
        .into_iter()
        .filter(|k| k.x <= tile.x)
        .map(|k| (k, if k == tile { TileState::Data(Arc::new(t.clone())) } else { TileState::Empty }))
        .collect();
    tiles.sort_by_key(|a| a.0);
    let rules = TypologyRules::default();
    let m = match_route(&route, GRID, &tiles, &caps(), &rules, &Unbounded).expect("appariement");
    let need = finish(&m, &tiles, &rules, true).expect_err("tuile voisine demandée");
    assert_eq!(need, vec![TileKey { z: tile.z, x: tile.x + 1, y: tile.y }]);
    // Sans extension permise : extrémité non résolue, passage douteux.
    let out = finish(&m, &tiles, &rules, false).expect("calcul");
    assert!(out.structures[0].ends.iter().any(|e| matches!(e.kind, EndKind::Unresolved(_))));
    assert!(out.passages[0].doubts.contains(&PassageDoubt::EndUnresolved));
}

#[test]
fn a_crossing_overpass_is_never_followed() {
    // La voie du parcours (est-ouest sur y = 0) est absente du graphe vélo ; seul un pont
    // nord-sud l'enjambe à x = 0. Croisé à angle droit, il n'est jamais apparié : non apparié.
    let net = || {
        Net::new()
            .add(with_surface("tertiary", PavedGroup::Paved), &[(0.0, -400.0), (0.0, -60.0)])
            .add(bridge("tertiary"), &[(0.0, -60.0), (0.0, 60.0)])
            .add(with_surface("tertiary", PavedGroup::Paved), &[(0.0, 60.0), (0.0, 400.0)])
    };
    let (_, out) = typology_of(net(), &[(-300.0, 0.0), (300.0, 0.0)]);
    assert!(out.passages.is_empty(), "{:?}", out.passages);
    let (segs, _) = run(net(), &[(-300.0, 0.0), (300.0, 0.0)], None);
    assert!(segs.iter().all(|s| s.structure != StructureClass::Bridge && !s.matched), "{segs:?}");
}

/// Pont fermé au vélo (trunk `bicycle=no`) d'ouest en est sur y = 0, prolongé au sol par ses
/// rampes, également fermées.
fn closed_bridge() -> Net {
    let closed = |a: WayAttrs| WayAttrs { bicycle: Some(Arc::from("no")), ..a };
    Net::new()
        .add(closed(with_surface("trunk", PavedGroup::Paved)), &[(-600.0, 0.0), (-400.0, 0.0)])
        .add(closed(bridge("trunk")), &[(-400.0, 0.0), (400.0, 0.0)])
        .add(closed(with_surface("trunk", PavedGroup::Paved)), &[(400.0, 0.0), (600.0, 0.0)])
}

#[test]
fn a_closed_bridge_does_not_snap_to_the_streets_below() {
    // Sous le tablier, une rue parallèle à 25 m et une rue qui le croise, reliées : le parcours
    // qui suit le pont fermé reste hors graphe, il n'accroche pas les rues du dessous.
    let net = || {
        closed_bridge()
            .add(
                with_surface("residential", PavedGroup::Paved),
                &[(-300.0, 25.0), (0.0, 25.0), (300.0, 25.0)],
            )
            .add(with_surface("residential", PavedGroup::Paved), &[(0.0, -200.0), (0.0, 25.0)])
    };
    let (segs, out) = run(net(), &[(-550.0, 0.0), (550.0, 0.0)], None);
    assert!(segs.iter().all(|s| !s.matched), "{segs:?}");
    assert!(out.passages.is_empty(), "{:?}", out.passages);
    // Typologie de la route fermée suivie (api 1.1) : pont, route principale, revêtue ; jamais
    // appariée au graphe vélo, jamais un passage d'ouvrage corrigé.
    let off = length_where(&segs, |s| s.off_graph);
    assert!(off > 1_000.0, "{off} m hors graphe : {segs:?}");
    let deck = length_where(&segs, |s| s.off_graph && s.structure == StructureClass::Bridge);
    assert!((780.0..=820.0).contains(&deck), "{deck} m de pont : {segs:?}");
    assert!(segs
        .iter()
        .filter(|s| s.off_graph)
        .all(|s| s.way_kind == WayKind::MajorRoad && s.surface_class == SurfaceClass::Paved));
    assert!(segs
        .iter()
        .filter(|s| s.structure == StructureClass::Bridge)
        .all(|s| s.structure_source == Some(crate::classification::StructureSource::Osm)));
}

#[test]
fn a_closed_road_does_not_follow_a_parallel_overpass() {
    // Autoroute en tranchée sur y = 0 ; une piste cyclable la longe à 15 m, sur un pont de 200 m
    // entre deux rampes au sol : le parcours de l'autoroute ne crée ni pont ni appariement.
    let net = Net::new()
        .add(with_surface("motorway", PavedGroup::Paved), &[(-600.0, 0.0), (600.0, 0.0)])
        .add(with_surface("cycleway", PavedGroup::Paved), &[(-400.0, 15.0), (-100.0, 15.0)])
        .add(bridge("cycleway"), &[(-100.0, 15.0), (100.0, 15.0)])
        .add(with_surface("cycleway", PavedGroup::Paved), &[(100.0, 15.0), (400.0, 15.0)]);
    let (segs, out) = run(net, &[(-550.0, 0.0), (550.0, 0.0)], None);
    assert!(out.passages.is_empty(), "{:?}", out.passages);
    assert!(segs.iter().all(|s| s.structure != StructureClass::Bridge && !s.matched), "{segs:?}");
}

#[test]
fn a_cycleway_beside_a_motorway_stays_matched() {
    // Piste sur y = 0, autoroute à 12 m au sud ; la trace bruitée (± 4 m, centrée 3 m au sud de
    // la piste) reste sur la piste : l'état hors graphe coûte un passage dans chaque sens.
    let net = Net::new()
        .add(with_surface("cycleway", PavedGroup::Paved), &[(-500.0, 0.0), (500.0, 0.0)])
        .add(with_surface("motorway", PavedGroup::Paved), &[(-500.0, -12.0), (500.0, -12.0)]);
    let trace: Vec<(f64, f64)> =
        (0..=40).map(|k| (-400.0 + 20.0 * k as f64, if k % 2 == 0 { 1.0 } else { -7.0 })).collect();
    let (segs, _) = run(net, &trace, None);
    assert!(segs.iter().all(|s| s.matched && s.way_kind == WayKind::Cycleway), "{segs:?}");
}

#[test]
fn passing_under_a_closed_bridge_stays_matched() {
    // La rue au sol passe sous le pont fermé : appariée de bout en bout, sans ouvrage.
    let net =
        closed_bridge().add(with_surface("residential", PavedGroup::Paved), &[(0.0, -400.0), (0.0, 400.0)]);
    let (segs, out) = run(net, &[(0.0, -300.0), (0.0, 300.0)], None);
    assert!(segs.iter().all(|s| s.matched && s.structure == StructureClass::Ground), "{segs:?}");
    assert!(out.passages.is_empty());
}

/// Ponts au-dessus du parcours (§6.4) : le pont routier est-ouest croise à angle droit la route
/// au sol qui passe dessous, loin de ses culées ; un pont rail (hors graphe vélo) aussi ; la trace
/// qui suit le pont ne le croise pas.
#[test]
fn bridges_crossing_the_trace_are_overpasses() {
    let (_, out) = typology_of(overpass(), &[(0.0, -300.0), (0.0, 300.0)]);
    assert_eq!(out.overpasses.len(), 1, "{:?}", out.overpasses);
    let o = out.overpasses[0];
    assert!((o.d_m - 300.0).abs() < 1.0, "{o:?}");
    assert!((o.angle_deg - 90.0).abs() < 1.0, "{o:?}");
    assert!((o.end_m - 60.0).abs() < 1.0, "{o:?}");
    let rail = WayAttrs { rail: true, structure: StructureTag::Bridge, ..way("rail") };
    let net = Net::new()
        .add(with_surface("residential", PavedGroup::Paved), &[(0.0, -400.0), (0.0, 400.0)])
        .add(rail, &[(-30.0, -30.0), (30.0, 30.0)]);
    let (_, out) = typology_of(net, &[(0.0, -300.0), (0.0, 300.0)]);
    assert_eq!(out.overpasses.len(), 1, "{:?}", out.overpasses);
    assert!((out.overpasses[0].angle_deg - 45.0).abs() < 1.0, "{:?}", out.overpasses);
    let (_, out) = typology_of(overpass(), &[(-300.0, 0.0), (300.0, 0.0)]);
    assert!(out.overpasses.iter().all(|o| o.angle_deg < 20.0), "{:?}", out.overpasses);
}

/// Un pont découpé en deux ways juste au-dessus de la route du dessous (§6.4) : la jonction des
/// deux ways n'est pas une culée.
#[test]
fn a_bridge_split_above_the_trace_has_no_abutment_there() {
    let net = Net::new()
        .add(with_surface("residential", PavedGroup::Paved), &[(0.0, -400.0), (0.0, 400.0)])
        .add(bridge("tertiary"), &[(-60.0, 0.0), (1.0, 0.0)])
        .add(bridge("tertiary"), &[(1.0, 0.0), (60.0, 0.0)]);
    let (_, out) = typology_of(net, &[(0.0, -300.0), (0.0, 300.0)]);
    assert!(!out.overpasses.is_empty(), "{:?}", out.overpasses);
    assert!(out.overpasses.iter().all(|o| o.end_m > 50.0), "{:?}", out.overpasses);
}

/// Règles 2026.11.10 (décision de l'utilisateur, 2026-11-02) : `bicycle=no` (shortbread@1.1, souvent
/// un `access=private`) ne ferme plus une voie non routière. Une piste `bicycle=no` seule sous la
/// trace reste au graphe : appariée et typée.
#[test]
fn a_track_tagged_bicycle_no_stays_matched_and_typed() {
    let track = WayAttrs { bicycle: Some(Arc::from("no")), ..with_surface("track", PavedGroup::Unpaved) };
    let net = Net::new().add(track, &[(-600.0, 0.0), (600.0, 0.0)]);
    let (segs, _) = run(net, &[(-500.0, 0.0), (500.0, 0.0)], None);
    assert!(
        segs.iter().all(|s| s.matched
            && !s.off_graph
            && s.way_kind == WayKind::Track
            && s.surface_class == SurfaceClass::Unpaved),
        "{segs:?}"
    );
}

/// Une route motorisée `bicycle=no` (secondary) garde l'état « hors graphe » : typologie de la
/// route rendue, non appariée (`bicycle_forbidden`).
#[test]
fn a_secondary_tagged_bicycle_no_stays_off_graph() {
    let road = WayAttrs { bicycle: Some(Arc::from("no")), ..with_surface("secondary", PavedGroup::Paved) };
    // Une rue du graphe la croise à x = 0 (comme sous le pont fermé de `closed_bridge`).
    let net = Net::new()
        .add(road, &[(-600.0, 0.0), (600.0, 0.0)])
        .add(with_surface("residential", PavedGroup::Paved), &[(0.0, -400.0), (0.0, 400.0)]);
    let (segs, _) = run(net, &[(-500.0, 0.0), (500.0, 0.0)], None);
    assert!(segs.iter().all(|s| !s.matched), "{segs:?}");
    let off = length_where(&segs, |s| s.off_graph && s.way_kind == WayKind::MajorRoad);
    assert!(off > 900.0, "{off} m hors graphe : {segs:?}");
}
