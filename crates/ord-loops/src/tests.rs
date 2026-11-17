//! Tests synthétiques du moteur (note lot 2, §9, points 1 à 3) : graphes faits à la main (`MemSource`), surcouches
//! fabriquées. Le contrôleur indépendant (point 4) est écrit à part.

use ord_graph::build::{build_graph, BuildOptions, MemSource};
use ord_graph::format::{write_graph, GraphFile};
use ord_graph::overlay::{encode, Overlay, OverlayMeta};
use ord_graph::view::{build_view, check_view_structure, ViewSpec};

use crate::cost::{self, RideType, Taste};
use crate::engine::{Engine, Params};
use crate::generate::generate;
use crate::search::{in_head_ext, turn_cost, Weighter, Workspace, INF, NONE};
use crate::team::{TeamCtx, TeamData, WindowTeam};
use crate::verify::{check_route, Violation};
use crate::{Failure, LatLon, Mode, Request, Route};

pub(crate) const LAT0: f64 = 47.2;
pub(crate) const LON0: f64 = -1.56;
pub(crate) const M_LAT: f64 = 111_194.926_6;

pub(crate) fn m_lon() -> f64 {
    M_LAT * LAT0.to_radians().cos()
}

/// Nœud à `north` mètres au nord et `east` mètres à l'est du point de référence.
pub(crate) fn pt(s: &mut MemSource, id: i64, north: f64, east: f64) {
    s.node(id, LAT0 + north / M_LAT, LON0 + east / m_lon());
}

pub(crate) fn ll(north: f64, east: f64) -> LatLon {
    LatLon::new(LAT0 + north / M_LAT, LON0 + east / m_lon())
}

pub(crate) fn tags<'a>(highway: &'a str, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut t = vec![("highway", highway)];
    t.extend_from_slice(extra);
    t
}

pub(crate) fn roundtrip(
    mut data: ord_graph::build::GraphData,
    dir: &std::path::Path,
    name: &str,
) -> GraphFile<Vec<u8>> {
    let path = dir.join(name);
    write_graph(&path, &mut data, "test", 1_700_000_000).expect("écriture");
    let g = GraphFile::open(std::fs::read(path).unwrap()).expect("ouverture");
    g.verify().expect("verify");
    g
}

pub(crate) fn engine_with(src: &mut MemSource, params: Params) -> Engine<Vec<u8>> {
    engine_zoomed(src, 11, params)
}

/// Comme `engine_with`, à un zoom de cellules choisi (16 : tuiles de ~400 m, une vraie fenêtre partielle).
pub(crate) fn engine_zoomed(src: &mut MemSource, zoom: u8, params: Params) -> Engine<Vec<u8>> {
    let dir = tempfile::tempdir().unwrap();
    let data = build_graph(src, &BuildOptions { zoom }, "test", &mut |_| {}).expect("construction");
    let base = roundtrip(data, dir.path(), "base.ordg");
    let view = |name: &str| {
        let v = roundtrip(build_view(&base, &ViewSpec::parse(name).unwrap()).unwrap(), dir.path(), name);
        check_view_structure(&base, &v).unwrap();
        v
    };
    let (road, gravel) = (view("road"), view("gravel"));
    Engine::new(base, road, gravel, params).expect("moteur")
}

/// Les paramètres par défaut avec le poids de base de 1.9.2 : les tests d'arithmétique et de non-régression ne doivent
/// pas dépendre de la table commune (3b).
pub(crate) fn neutral_params() -> Params {
    Params { base_taste: Taste::neutral(), ..Params::default() }
}

pub(crate) fn engine_of(src: &mut MemSource) -> Engine<Vec<u8>> {
    engine_with(src, neutral_params())
}

/// Chaîne de la vue (fenêtre de toutes les tuiles : ids locaux = ids globaux) qui porte l'arête de base.
pub(crate) fn chain_of_edge(v: &crate::engine::ViewData, edge: u32) -> u32 {
    (0..v.n_chains as u32)
        .find(|&c| v.links(c).iter().any(|l| l & !ord_graph::format::DIR_BIT == edge))
        .expect("arête vivante")
}

/// Arête de base entre deux nœuds OSM.
pub(crate) fn edge_between(e: &Engine<Vec<u8>>, a: u64, b: u64) -> u32 {
    let g = e.base();
    (0..g.header().edges as u32)
        .find(|&x| {
            let (na, nb) = g.edge_ends(x);
            let (oa, ob) = (g.osm_node_id(na), g.osm_node_id(nb));
            (oa, ob) == (a, b) || (oa, ob) == (b, a)
        })
        .unwrap_or_else(|| panic!("pas d'arête {a}-{b}"))
}

/// Comptes d'une arête : `[ROAD a→b, ROAD b→a, GRAVEL.., MTB.., MIXED..]`.
pub(crate) type Counts = [u16; 8];
pub(crate) const ROAD1: Counts = [1, 1, 0, 0, 0, 0, 0, 0];
pub(crate) const GRAVEL1: Counts = [0, 0, 1, 1, 0, 0, 0, 0];
pub(crate) const MIXED1: Counts = [0, 0, 0, 0, 0, 0, 1, 1];

pub(crate) fn overlay_of(e: &Engine<Vec<u8>>, rows: &[(u32, Counts)]) -> Overlay {
    let mut rows = rows.to_vec();
    rows.sort_by_key(|r| r.0);
    let meta = OverlayMeta {
        graph: e.base().graph_identity(),
        team_id: 1,
        export_xxh3: 0,
        export_rows: 0,
        params_hash: 0,
        tool_version: "test".into(),
        build_epoch: 0,
        types: vec!["ROAD".into(), "GRAVEL".into(), "MTB".into(), "MIXED".into()],
    };
    let edges: Vec<u32> = rows.iter().map(|r| r.0).collect();
    let counts: Vec<u16> = rows.iter().flat_map(|r| r.1).collect();
    Overlay::decode(&encode(&meta, &edges, &counts).unwrap()).unwrap()
}

pub(crate) fn request(start: LatLon, d: f64, mode: Mode) -> Request {
    Request {
        start,
        end: None,
        bearing: None,
        distance_m: d,
        ride_type: RideType::Road,
        mode,
        familiarity: 0.5,
        off_team_cost_q8: crate::DEFAULT_OFF_TEAM_COST_Q8,
        seed: 1,
    }
}

pub(crate) fn run(e: &Engine<Vec<u8>>, team: Option<&TeamData>, r: &Request) -> Result<Vec<Route>, Failure> {
    generate(e, team, r, &mut Workspace::new(), &mut || false).map(|g| g.routes)
}

fn assert_valid(e: &Engine<Vec<u8>>, team: Option<&TeamData>, r: &Request, routes: &[Route]) {
    assert!(!routes.is_empty());
    for route in routes {
        let c = check_route(e, team, r, route);
        assert!(c.violations.is_empty(), "violations {:?}", c.violations);
        assert!(
            (route.length_m - r.distance_m).abs() <= 0.1 * r.distance_m + 1.0,
            "longueur {}",
            route.length_m
        );
    }
}

// --- Graphes ----------------------------------------------------------------------------------------------------

/// Grille de `n × n` carrefours, rues de 300 m, toutes résidentielles. Nœud `(i, j)` : `1 + i n + j`.
pub(crate) fn grid(n: usize, cell: f64) -> MemSource {
    let mut s = MemSource::default();
    let id = |i: usize, j: usize| (1 + i * n + j) as i64;
    for i in 0..n {
        for j in 0..n {
            pt(&mut s, id(i, j), i as f64 * cell, j as f64 * cell);
        }
    }
    for i in 0..n {
        let row: Vec<i64> = (0..n).map(|j| id(i, j)).collect();
        s.way(1000 + i as i64, &row, &tags("residential", &[]));
        let col: Vec<i64> = (0..n).map(|j| id(j, i)).collect();
        s.way(2000 + i as i64, &col, &tags("residential", &[]));
    }
    s
}

/// Anneau de 1520 × 1560 m. Avec `hole`, le côté sud est coupé en `P0-H` (longueur `hole`) et `H-P1`, `H` porte une
/// impasse de 150 m. Avec `dead_end`, une impasse de 400 m part de `P0` vers l'ouest.
fn ring(hole: Option<f64>, dead_end: bool) -> MemSource {
    let mut s = MemSource::default();
    pt(&mut s, 1, 0.0, 0.0);
    pt(&mut s, 2, 0.0, 1520.0);
    pt(&mut s, 3, 1560.0, 1520.0);
    pt(&mut s, 4, 1560.0, 0.0);
    match hole {
        Some(h) => {
            pt(&mut s, 10, 0.0, h);
            pt(&mut s, 11, 150.0, h);
            s.way(101, &[1, 10], &tags("residential", &[]));
            s.way(102, &[10, 2], &tags("residential", &[]));
            s.way(110, &[10, 11], &tags("residential", &[]));
        }
        None => {
            s.way(101, &[1, 2], &tags("residential", &[]));
        }
    }
    s.way(103, &[2, 3], &tags("residential", &[]));
    s.way(104, &[3, 4], &tags("residential", &[]));
    s.way(105, &[4, 1], &tags("residential", &[]));
    if dead_end {
        pt(&mut s, 12, 0.0, -400.0);
        s.way(120, &[1, 12], &tags("residential", &[]));
    }
    s
}

fn ring_rows(e: &Engine<Vec<u8>>, hole: bool) -> Vec<(u32, Counts)> {
    let mut rows =
        vec![(edge_between(e, 2, 3), ROAD1), (edge_between(e, 3, 4), ROAD1), (edge_between(e, 4, 1), ROAD1)];
    if hole {
        rows.push((edge_between(e, 10, 2), ROAD1));
    } else {
        rows.push((edge_between(e, 1, 2), ROAD1));
    }
    rows
}

// --- Coût ----------------------------------------------------------------------------------------------------------

#[test]
fn weights_are_monotone_in_lambda_and_strict_is_infinite_off_team() {
    let mut last = 0;
    let e = engine_with(&mut grid(5, 300.0), neutral_params());
    for lambda_q8 in [256u32, 512, 768, 1280] {
        let id = |i: u64, j: u64| 1 + i * 5 + j;
        // La rue `i = 2` est roulée en entier.
        let rows: Vec<_> = (0..4).map(|j| (edge_between(&e, id(2, j), id(2, j + 1)), ROAD1)).collect();
        let team = TeamData::build(&e, &overlay_of(&e, &rows)).unwrap();
        let win = e.full_window(0);
        let v = &win.v;
        let c_team = chain_of_edge(v, edge_between(&e, id(2, 1), id(2, 2)));
        let c_off = chain_of_edge(v, edge_between(&e, id(1, 1), id(1, 2)));
        let wt = WindowTeam::build(&e, &win, &team);
        let tc = TeamCtx { data: &team, win: &wt };
        let w = Weighter::new(v, Some(tc), RideType::Road, Mode::TeamFirst, 0.5, lambda_q8);
        let off = w.chain(c_off);
        assert!(off > last, "poids hors équipe croissant en λ : {off} après {last}");
        last = off;
        assert_eq!(w.chain(c_team), v.w_osm[0][c_team as usize], "f = 0,5 : l'équipe au prix OSM");
        let strict = Weighter::new(v, Some(tc), RideType::Road, Mode::Strict, 0.5, lambda_q8);
        assert_eq!(strict.chain(c_off), INF, "strict : infini hors équipe");
        assert_ne!(strict.chain(c_team), INF);
        // Familiarité : plus elle monte, moins la route d'équipe coûte ; la route hors équipe, jamais.
        let low = Weighter::new(v, Some(tc), RideType::Road, Mode::TeamFirst, 0.0, lambda_q8);
        let high = Weighter::new(v, Some(tc), RideType::Road, Mode::TeamFirst, 1.0, lambda_q8);
        assert!(high.chain(c_team) < low.chain(c_team));
        assert_eq!(high.chain(c_off), low.chain(c_off));
    }
}

// --- Goût (sonde du lot 4, §12) -----------------------------------------------------------------------------------

fn team_of_street_two(e: &Engine<Vec<u8>>) -> TeamData {
    let id = |i: u64, j: u64| 1 + i * 5 + j;
    let rows: Vec<_> = (0..4).map(|j| (edge_between(e, id(2, j), id(2, j + 1)), ROAD1)).collect();
    TeamData::build(e, &overlay_of(e, &rows)).unwrap()
}

#[test]
fn a_neutral_taste_gives_the_same_window_team_chain_by_chain() {
    let e = engine_with(&mut grid(5, 300.0), neutral_params());
    let team = team_of_street_two(&e);
    for vi in 0..2 {
        let win = e.full_window(vi);
        let plain = WindowTeam::build(&e, &win, &team);
        let neutral = WindowTeam::build_with_taste(&e, &win, &team, Some(&Taste::neutral()));
        assert!(plain.taste_of(RideType::Road).is_none() && plain.taste_of(RideType::Gravel).is_none());
        for ride in RideType::ALL {
            let served = crate::engine::rides_of_view(vi).contains(&ride);
            assert_eq!(neutral.taste_of(ride).is_some(), served, "{ride:?}");
            if !served {
                continue;
            }
            let (a, b) = (plain.chains_of(ride), neutral.chains_of(ride));
            assert_eq!(a.len(), b.len());
            for (x, y) in a.iter().zip(b) {
                assert_eq!((x.a, x.b, x.z, x.strict), (y.a, y.b, y.z, y.strict));
            }
            let (w, q) = neutral.taste_of(ride).unwrap();
            assert_eq!(w, &win.v.w_osm[ride.index()][..], "{ride:?}");
            assert!(q.iter().all(|&x| x == 256));
        }
        assert!(neutral.bytes() > plain.bytes());
    }
}

#[test]
fn a_taste_scales_every_edge_of_the_view_team_edges_included_in_every_mode() {
    // Une grille de rues résidentielles et une voie tertiaire hors équipe, accrochée au coin (0, 0).
    let mut s = grid(5, 300.0);
    pt(&mut s, 100, -300.0, 0.0);
    s.way(3000, &[1, 100], &tags("tertiary", &[]));
    let e = engine_with(&mut s, neutral_params());
    let team = team_of_street_two(&e);
    let win = e.full_window(0);
    let v = &win.v;
    let id = |i: u64, j: u64| 1 + i * 5 + j;
    let (c_team, c_off, c_other) = (
        chain_of_edge(v, edge_between(&e, id(2, 1), id(2, 2))),
        chain_of_edge(v, edge_between(&e, id(1, 1), id(1, 2))),
        chain_of_edge(v, edge_between(&e, 1, 100)),
    );
    // La classe de la rue résidentielle roulée vaut 512 : les rues, roulées ou non, doublent ; la tertiaire non.
    let street = e.attrs_len(edge_between(&e, id(2, 1), id(2, 2))).0;
    let tertiary = e.attrs_len(edge_between(&e, 1, 100)).0;
    assert_ne!(cost::taste_class(street), cost::taste_class(tertiary));
    let mut taste = Taste::neutral();
    taste.q8[RideType::Road.index()][cost::taste_class(street)] = 512;
    let plain = WindowTeam::build(&e, &win, &team);
    let tasted = WindowTeam::build_with_taste(&e, &win, &team, Some(&taste));
    let (ctx_plain, ctx_taste) =
        (TeamCtx { data: &team, win: &plain }, TeamCtx { data: &team, win: &tasted });
    let double = |w: u32, len_dm: u32| {
        // Valeur exacte : ⌈2 × len × K / 64⌉ pour une arête (K = 64 en rue résidentielle).
        assert_eq!(w, 2 * len_dm, "attendu le double exact");
        w
    };
    let len_dm = e.attrs_len(edge_between(&e, id(2, 1), id(2, 2))).1;
    for (mode, fam) in [(Mode::Discovery, 0.5), (Mode::TeamFirst, 0.5), (Mode::TeamFirst, 0.0)] {
        let base = Weighter::new(v, Some(ctx_plain), RideType::Road, mode, fam, 768);
        let got = Weighter::new(v, Some(ctx_taste), RideType::Road, mode, fam, 768);
        assert_eq!(base.chain(c_other), got.chain(c_other), "{mode:?} : la tertiaire ne bouge pas");
        if mode == Mode::Discovery {
            // Sans goût, la découverte rend toujours `w_osm` ; avec, les rues doublent, équipe ou non.
            assert_eq!(base.chain(c_team), v.w_osm[0][c_team as usize]);
            assert_eq!(base.chain(c_off), v.w_osm[0][c_off as usize]);
            double(got.chain(c_team), v.w_osm[0][c_team as usize]);
            double(got.chain(c_off), v.w_osm[0][c_off as usize]);
        } else if fam == 0.5 {
            // λ = 3 hors équipe, 1 en équipe à f = 0,5 : le goût double les deux.
            assert_eq!(got.chain(c_team), 2 * base.chain(c_team));
            assert_eq!(got.chain(c_off), 2 * base.chain(c_off));
        } else {
            assert!(got.chain(c_team) > base.chain(c_team));
            assert_eq!(got.chain(c_off), 2 * base.chain(c_off));
        }
    }
    // Les morceaux partiels du départ et de l'arrivée (`edge`) lisent le même goût.
    let base = Weighter::new(v, Some(ctx_plain), RideType::Road, Mode::Discovery, 0.5, 768);
    let got = Weighter::new(v, Some(ctx_taste), RideType::Road, Mode::Discovery, 0.5, 768);
    assert_eq!(got.edge(&e, edge_between(&e, id(2, 1), id(2, 2))), 2 * len_dm);
    assert_eq!(base.edge(&e, edge_between(&e, id(2, 1), id(2, 2))), len_dm);
    assert_eq!(got.edge(&e, edge_between(&e, 1, 100)), base.edge(&e, edge_between(&e, 1, 100)));
}

// --- Attributs d'équipe -------------------------------------------------------------------------------------------

#[test]
fn greenways_without_surface_count_only_for_the_matching_ride_type() {
    let mut s = MemSource::default();
    pt(&mut s, 1, 0.0, 0.0);
    pt(&mut s, 2, 0.0, 400.0);
    pt(&mut s, 3, 0.0, 800.0);
    pt(&mut s, 4, 300.0, 400.0);
    s.way(10, &[1, 2], &tags("residential", &[]));
    s.way(11, &[2, 3], &tags("path", &[("bicycle", "designated")]));
    s.way(12, &[2, 4], &tags("path", &[("bicycle", "designated")]));
    let e = engine_of(&mut s);
    let (road_only, gravel_only, mixed) =
        (edge_between(&e, 2, 3), edge_between(&e, 2, 4), edge_between(&e, 1, 2));
    let ov = overlay_of(&e, &[(road_only, ROAD1), (gravel_only, GRAVEL1), (mixed, MIXED1)]);
    let t = TeamData::build(&e, &ov).unwrap();
    // Voie verte sans surface : roulée en route seulement si par ROAD ou MIXED ; en gravel, par GRAVEL ou MIXED.
    assert!(t.is_team_edge(RideType::Road, road_only));
    assert!(!t.is_team_edge(RideType::Road, gravel_only));
    assert!(t.is_team_edge(RideType::Gravel, gravel_only));
    assert!(!t.is_team_edge(RideType::Gravel, road_only));
    // MIXED compte pour les deux.
    assert!(t.is_team_edge(RideType::Road, mixed) && t.is_team_edge(RideType::Gravel, mixed));
}

#[test]
fn a_gap_of_40_m_from_a_false_dead_end_is_bridged_but_not_120_m_nor_a_shortcut() {
    for (hole, bridged) in [(40.0, true), (120.0, false)] {
        let mut s = ring(Some(hole), false);
        let e = engine_of(&mut s);
        let t = TeamData::build(&e, &overlay_of(&e, &ring_rows(&e, true))).unwrap();
        let gap = edge_between(&e, 1, 10);
        assert_eq!(t.is_bridged_edge(RideType::Road, gap), bridged, "trou de {hole} m");
        assert!(!t.is_team_edge(RideType::Road, gap));
        // L'impasse de 150 m n'est jamais comblée : son bout n'est pas un nœud d'équipe.
        assert!(!t.is_bridged_edge(RideType::Road, edge_between(&e, 10, 11)));
    }
    // Raccourci de 60 m entre deux routes roulées : leurs nœuds ont deux arêtes d'équipe, pas de cul-de-sac fictif,
    // et 60 m dépasse la jonction courte (30 m).
    assert!(!connector_bridged(60.0), "un raccourci de 60 m n'est pas un trou");
}

/// Deux routes roulées parallèles reliées par une arête non roulée de `len` m ; vrai si le moteur la comble.
fn connector_bridged(len: f64) -> bool {
    let mut s = MemSource::default();
    for (i, x) in [0.0, 300.0, 600.0].into_iter().enumerate() {
        pt(&mut s, 1 + i as i64, 0.0, x);
        pt(&mut s, 11 + i as i64, len, x);
    }
    s.way(10, &[1, 2, 3], &tags("residential", &[]));
    s.way(11, &[11, 12, 13], &tags("residential", &[]));
    s.way(12, &[2, 12], &tags("residential", &[]));
    let e = engine_of(&mut s);
    let rows: Vec<_> =
        [(1, 2), (2, 3), (11, 12), (12, 13)].iter().map(|&(a, b)| (edge_between(&e, a, b), ROAD1)).collect();
    let t = TeamData::build(&e, &overlay_of(&e, &rows)).unwrap();
    t.is_bridged_edge(RideType::Road, edge_between(&e, 2, 12))
}

#[test]
fn a_short_connector_between_two_team_roads_is_bridged_up_to_30_m() {
    // Seconde chaussée d'une 2 x 2 : sans ce lien, une chaussée à sens unique roulée dans un sens ne se rejoint pas.
    assert!(connector_bridged(20.0));
    assert!(connector_bridged(29.0));
    assert!(!connector_bridged(35.0));
}

#[test]
fn a_rough_surface_is_presumed_only_for_a_ride_of_the_right_type_not_a_compacted_one() {
    // Gravel : `compacted` ou `gravel` ne demandent aucun parcours gravel (règle Java : seuls path, footway et
    // les surfaces rugueuses sont présumés) ; `dirt`, `grass`, `wood` ou un `tracktype` grade4 en demandent un.
    let mut s = MemSource::default();
    pt(&mut s, 1, 0.0, 0.0);
    pt(&mut s, 2, 0.0, 400.0);
    for (i, x) in [800.0, 1200.0, 1600.0, 2000.0, 2400.0].into_iter().enumerate() {
        pt(&mut s, 3 + i as i64, 0.0, x);
    }
    s.way(10, &[1, 2], &tags("track", &[("surface", "compacted"), ("tracktype", "grade2")]));
    s.way(11, &[2, 3], &tags("track", &[("surface", "gravel"), ("tracktype", "grade2")]));
    s.way(12, &[3, 4], &tags("track", &[("surface", "dirt")]));
    s.way(13, &[4, 5], &tags("track", &[("surface", "wood")]));
    s.way(14, &[5, 6], &tags("track", &[("tracktype", "grade4")]));
    s.way(15, &[6, 7], &tags("track", &[("surface", "gravel")]));
    let e = engine_of(&mut s);
    let ed = |a, b| edge_between(&e, a, b);
    // Tout roulé par un parcours ROAD seulement : ne vaut que pour ce qui n'est pas présumé.
    let rows: Vec<_> =
        [(1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (6, 7)].iter().map(|&(a, b)| (ed(a, b), ROAD1)).collect();
    let t = TeamData::build(&e, &overlay_of(&e, &rows)).unwrap();
    assert!(t.is_team_edge(RideType::Gravel, ed(1, 2)), "compacted");
    assert!(t.is_team_edge(RideType::Gravel, ed(2, 3)), "gravel");
    assert!(!t.is_team_edge(RideType::Gravel, ed(3, 4)), "dirt");
    assert!(!t.is_team_edge(RideType::Gravel, ed(4, 5)), "wood");
    assert!(!t.is_team_edge(RideType::Gravel, ed(5, 6)), "grade4");
    assert!(t.is_team_edge(RideType::Gravel, ed(6, 7)), "gravel");
    // Roulé par un parcours GRAVEL : tout compte.
    let rows: Vec<_> = [(3, 4), (4, 5), (5, 6)].iter().map(|&(a, b)| (ed(a, b), GRAVEL1)).collect();
    let t = TeamData::build(&e, &overlay_of(&e, &rows)).unwrap();
    assert!(t.is_team_edge(RideType::Gravel, ed(3, 4)) && t.is_team_edge(RideType::Gravel, ed(4, 5)));
    assert!(t.is_team_edge(RideType::Gravel, ed(5, 6)));
}

// --- Règle de virage ------------------------------------------------------------------------------------------------

/// Jonction en Y : un bras vers l'ouest (A), un vers l'est (B), un bras C dont le virage depuis A vaut `turn` degrés.
fn y_junction(turn: f64, short_c: Option<f64>) -> MemSource {
    let mut s = MemSource::default();
    pt(&mut s, 1, 0.0, -300.0);
    pt(&mut s, 2, 0.0, 0.0);
    pt(&mut s, 3, 0.0, 300.0);
    let h = (90.0 + turn).to_radians();
    let l = short_c.unwrap_or(300.0);
    pt(&mut s, 4, l * h.cos(), l * h.sin());
    s.way(10, &[1, 2], &tags("residential", &[]));
    s.way(11, &[2, 3], &tags("residential", &[]));
    s.way(12, &[2, 4], &tags("residential", &[]));
    s
}

/// État orienté de l'arête de vue qui joint les nœuds OSM `from` → `to` (par positions).
fn field(e: &Engine<Vec<u8>>, from: u64, to: u64) -> u32 {
    let g = e.base();
    let win = e.full_window(0);
    let v = &win.v;
    let node = |osm: u64| (0..g.header().nodes as u32).find(|&n| g.osm_node_id(n) == osm).unwrap();
    let (nf, nt) = (node(from), node(to));
    let vn = |n: u32| v.pos.iter().position(|&p| p == g.node_pos(n)).unwrap() as u32;
    let (vf, vt) = (vn(nf), vn(nt));
    for c in 0..v.n_chains as u32 {
        let (a, b) = v.ends[c as usize];
        if (a, b) == (vf, vt) {
            return c << 1;
        }
        if (a, b) == (vt, vf) {
            return (c << 1) | 1;
        }
    }
    panic!("pas d'arête de vue {from}→{to}");
}

#[test]
fn a_turn_of_149_degrees_is_allowed_and_150_is_refused_at_a_junction() {
    for (turn, allowed) in [(120.0, true), (149.0, true), (151.0, false), (170.0, false)] {
        let e = engine_of(&mut y_junction(turn, None));
        let win = e.full_window(0);
        let v = &win.v;
        let (fa, fc) = (field(&e, 1, 2), field(&e, 2, 4));
        let forb = Default::default();
        assert_eq!(turn_cost(v, &forb, fa, NONE, fc, None, None).is_some(), allowed, "virage de {turn}°");
        // Le demi-tour sur la même arête est toujours refusé.
        assert!(turn_cost(v, &forb, fa, NONE, fa ^ 1, None, None).is_none());
    }
    // Tout droit : gratuit.
    let e = engine_of(&mut y_junction(150.0, None));
    let win = e.full_window(0);
    let v = &win.v;
    assert_eq!(
        turn_cost(v, &Default::default(), field(&e, 1, 2), NONE, field(&e, 2, 3), None, None),
        Some(0)
    );
}

#[test]
fn the_heading_of_a_20_m_chain_is_extended_through_its_predecessor() {
    // A (300 m vers l'est) puis un bout de 20 m qui part à 90° : l'arrivée de ce bout prolonge A.
    let mut s = MemSource::default();
    pt(&mut s, 1, 0.0, 0.0);
    pt(&mut s, 2, 0.0, 300.0);
    pt(&mut s, 3, 20.0, 300.0);
    pt(&mut s, 4, 20.0, 600.0);
    pt(&mut s, 5, -300.0, 300.0);
    pt(&mut s, 6, 320.0, 300.0);
    s.way(10, &[1, 2], &tags("residential", &[]));
    s.way(11, &[2, 3], &tags("residential", &[]));
    s.way(12, &[3, 4], &tags("residential", &[]));
    s.way(13, &[2, 5], &tags("residential", &[]));
    s.way(14, &[3, 6], &tags("residential", &[]));
    let e = engine_of(&mut s);
    let win = e.full_window(0);
    let v = &win.v;
    let (fa, fshort) = (field(&e, 1, 2), field(&e, 2, 3));
    let alone = in_head_ext(v, fshort, NONE);
    let ext = in_head_ext(v, fshort, fa);
    let deg = |h: u16| f64::from(h) * 360.0 / 65_536.0;
    assert!((deg(alone) - 0.0).abs() < 2.0, "seul : plein nord ({})", deg(alone));
    assert!(deg(ext) > 20.0 && deg(ext) < 90.0, "prolongé par un prédécesseur vers l'est ({})", deg(ext));
}

// --- Génération -----------------------------------------------------------------------------------------------------

#[test]
fn a_loop_on_a_grid_has_up_to_three_valid_routes_that_overlap_less_than_half() {
    let e = engine_of(&mut grid(21, 300.0));
    let r = request(ll(3000.0, 3000.0), 6000.0, Mode::Discovery);
    let routes = run(&e, None, &r).expect("une boucle");
    assert_valid(&e, None, &r, &routes);
    assert!(routes.len() <= 3);
    for (i, a) in routes.iter().enumerate() {
        for b in &routes[i + 1..] {
            let ea: std::collections::BTreeSet<u32> = a.steps.iter().map(|s| s.base_edge).collect();
            let eb: std::collections::BTreeSet<u32> = b.steps.iter().map(|s| s.base_edge).collect();
            let len = |s: &std::collections::BTreeSet<u32>| {
                s.iter().map(|&x| u64::from(e.base().edge(x).len_dm)).sum::<u64>()
            };
            let common: u64 = ea.intersection(&eb).map(|&x| u64::from(e.base().edge(x).len_dm)).sum();
            assert!((common as f64) < 0.5 * len(&ea).min(len(&eb)) as f64);
        }
    }
    // Déterminisme : même graine, même sortie ; la géométrie part près du départ.
    let again = run(&e, None, &r).unwrap();
    assert_eq!(
        routes.iter().map(|x| x.steps.clone()).collect::<Vec<_>>(),
        again.iter().map(|x| x.steps.clone()).collect::<Vec<_>>()
    );
    let geom = crate::route_geometry(e.base(), &routes[0]);
    assert!(geom.len() > 10);
}

#[test]
fn a_one_way_ride_between_two_points_arrives_at_the_second_one() {
    let e = engine_of(&mut grid(21, 300.0));
    let mut r = request(ll(3000.0, 1500.0), 4200.0, Mode::Discovery);
    r.end = Some(ll(3000.0, 4500.0));
    let routes = run(&e, None, &r).expect("un aller simple");
    assert_valid(&e, None, &r, &routes);
    let g = crate::route_geometry(e.base(), &routes[0]);
    let (first, last) = (g[0], g[g.len() - 1]);
    let near = |a: LatLon, b: LatLon| ((a.lat - b.lat) * M_LAT).hypot((a.lon - b.lon) * m_lon()) < 160.0;
    assert!(
        near(first, r.start) && near(last, r.end.unwrap()),
        "départ et arrivée accrochés près des points"
    );
}

#[test]
fn a_bearing_keeps_the_barycentre_on_its_side() {
    let e = engine_of(&mut grid(21, 300.0));
    let mut r = request(ll(3000.0, 3000.0), 6000.0, Mode::Discovery);
    r.bearing = Some(crate::Bearing8::E);
    let routes = run(&e, None, &r).expect("une boucle vers l'est");
    assert_valid(&e, None, &r, &routes);
    assert!(routes[0].bearing_deviation_deg.unwrap() < 60.0, "{:?}", routes[0].bearing_deviation_deg);
}

#[test]
fn strict_rides_the_team_ring_and_nothing_else_and_team_first_bridges_a_gap() {
    let mut s = ring(None, false);
    let e = engine_of(&mut s);
    let rows = ring_rows(&e, false);
    let team = TeamData::build(&e, &overlay_of(&e, &rows)).unwrap();
    let start = ll(1560.0, 700.0); // milieu du côté nord
    let r = request(start, 6160.0, Mode::Strict);
    let routes = run(&e, Some(&team), &r).expect("le tour de l'anneau");
    assert_valid(&e, Some(&team), &r, &routes);
    let ridden: Vec<u32> = rows.iter().map(|x| x.0).collect();
    for route in &routes {
        assert!(route.steps.iter().all(|s| ridden.contains(&s.base_edge)), "strict : que des arêtes roulées");
        assert!((route.team_m - route.length_m).abs() < 1.0);
    }
    // Sans le côté sud, le strict n'a plus de boucle ; l'équipe d'abord raccorde par la rue voisine… qui n'existe pas
    // ici : l'anneau coupé n'a pas de raccord, donc rien. Sur une grille, il en trouve un.
    let cut = overlay_of(&e, &rows[..3]);
    let team_cut = TeamData::build(&e, &cut).unwrap();
    assert!(run(&e, Some(&team_cut), &request(start, 6160.0, Mode::Strict)).is_err());

    let mut g = grid(21, 300.0);
    let e = engine_of(&mut g);
    // Anneau de 5 × 5 blocs (6 km) dans la grille, privé d'une arête.
    let id = |i: i64, j: i64| 1 + i * 21 + j;
    let mut rows = Vec::new();
    let mut add = |a: i64, b: i64| rows.push((edge_between(&e, a as u64, b as u64), ROAD1));
    for k in 5..10 {
        add(id(5, k), id(5, k + 1));
        add(id(10, k), id(10, k + 1));
        add(id(k, 5), id(k + 1, 5));
        add(id(k, 10), id(k + 1, 10));
    }
    let full = TeamData::build(&e, &overlay_of(&e, &rows)).unwrap();
    let start = ll(5.0 * 300.0, 7.5 * 300.0);
    let r = request(start, 6000.0, Mode::Strict);
    let ok = run(&e, Some(&full), &r).expect("strict sur l'anneau entier");
    assert_valid(&e, Some(&full), &r, &ok);
    rows.remove(0);
    let broken = TeamData::build(&e, &overlay_of(&e, &rows)).unwrap();
    assert!(run(&e, Some(&broken), &r).is_err(), "strict : l'anneau est coupé");
    let r2 = request(start, 6000.0, Mode::TeamFirst);
    let linked = run(&e, Some(&broken), &r2).expect("équipe d'abord : raccord par une arête OSM");
    assert_valid(&e, Some(&broken), &r2, &linked);
    assert!(
        linked[0].team_m / linked[0].length_m > 0.6,
        "la route de l'équipe domine : {}",
        linked[0].team_m / linked[0].length_m
    );
}

#[test]
fn strict_bridges_a_40_m_hole_and_refuses_a_120_m_one() {
    for (hole, ok) in [(40.0, true), (120.0, false)] {
        let mut s = ring(Some(hole), false);
        let e = engine_of(&mut s);
        let team = TeamData::build(&e, &overlay_of(&e, &ring_rows(&e, true))).unwrap();
        let r = request(ll(1560.0, 700.0), 6160.0 + 2.0 * 0.0, Mode::Strict);
        let res = run(&e, Some(&team), &r);
        assert_eq!(res.is_ok(), ok, "trou de {hole} m : {res:?}");
        if let Ok(routes) = res {
            assert_valid(&e, Some(&team), &r, &routes);
            assert!((routes[0].bridged_m - hole).abs() < 1.0, "comblé : {}", routes[0].bridged_m);
        }
    }
}

#[test]
fn a_sector_does_not_retry_turnarounds_on_a_loop_it_has_just_rejected() {
    // Réseau strict = un anneau de 5 × 5 blocs (6 000 m, un carrefour tous les 300 m) dans une grille, D = 5 000 m :
    // chaque demi-tour de l'anneau redonne le même cycle, à 1,2 × D. Les nœuds d'un candidat écarté sortent du
    // secteur : au plus un candidat par secteur, et non un par essai (campagne du lot 2 : 5 à 18 candidats
    // identiques avant la règle).
    let mut g = grid(21, 300.0);
    let e = engine_with(&mut g, Params { tries_per_sector: 6, ..neutral_params() });
    let id = |i: i64, j: i64| 1 + i * 21 + j;
    let mut rows = Vec::new();
    let mut add = |a: i64, b: i64| rows.push((edge_between(&e, a as u64, b as u64), ROAD1));
    for k in 5..10 {
        add(id(5, k), id(5, k + 1));
        add(id(10, k), id(10, k + 1));
        add(id(k, 5), id(k + 1, 5));
        add(id(k, 10), id(k + 1, 10));
    }
    let team = TeamData::build(&e, &overlay_of(&e, &rows)).unwrap();
    let r = request(ll(5.0 * 300.0, 7.5 * 300.0), 5000.0, Mode::Strict);
    let (res, diag) =
        crate::generate_with_diagnostics(&e, Some(&team), &r, &mut Workspace::new(), &mut || false);
    assert_eq!(res.unwrap_err(), Failure::NoRoute(crate::NoRouteDetail::NoCandidate));
    assert!(!diag.off_tolerance.is_empty(), "{diag:?}");
    assert!(diag.off_tolerance.iter().all(|&x| (x - 1.2).abs() < 0.02), "{:?}", diag.off_tolerance);
    // Mesuré : 4 candidats (un par secteur qui en rend un) ; sans la règle, 8 (le même cycle deux fois par secteur).
    assert!(diag.candidates <= 4, "un candidat par secteur au plus : {}", diag.candidates);
}

#[test]
fn a_dead_end_start_returns_through_a_short_tail() {
    let mut s = ring(None, true);
    let e = engine_of(&mut s);
    let r = request(ll(0.0, -398.0), 6960.0, Mode::Discovery);
    let routes = run(&e, None, &r).expect("la boucle de l'impasse");
    assert_valid(&e, None, &r, &routes);
    let shared = routes[0].shared_m;
    assert!(shared > 300.0 && shared <= 0.15 * 6960.0, "queue de {shared} m");
}

#[test]
fn infeasible_requests_fail_with_the_java_codes() {
    let e = engine_of(&mut ring(None, false));
    // Départ à plus de 2 km du réseau.
    assert_eq!(
        run(&e, None, &request(ll(6000.0, 6000.0), 6000.0, Mode::Discovery)).unwrap_err(),
        Failure::StartTooFar
    );
    // Aucune arête assez grande : la composante de l'anneau (6 km) est sous 0,9 × 60 km.
    assert_eq!(
        run(&e, None, &request(ll(0.0, 700.0), 60_000.0, Mode::Discovery)).unwrap_err(),
        Failure::StartTooFar
    );
    // Réseau trop petit : une rue de 12 km, départ à son bout, D = 12 km (le disque de 6 km n'en atteint que la moitié).
    let mut s = MemSource::default();
    for k in 0..25i64 {
        pt(&mut s, 1 + k, 0.0, k as f64 * 500.0);
        pt(&mut s, 100 + k, 50.0, k as f64 * 500.0);
        s.way(200 + k, &[1 + k, 100 + k], &tags("residential", &[]));
    }
    s.way(1, &(1..=25).collect::<Vec<i64>>(), &tags("residential", &[]));
    let e2 = engine_of(&mut s);
    assert_eq!(
        run(&e2, None, &request(ll(0.0, 1.0), 12_000.0, Mode::Discovery)).unwrap_err(),
        Failure::NetworkTooSmall
    );
    // Un échec rend aussi ses diagnostics : le réseau atteint, sans quoi il ne se classe pas.
    let (res, diag) = crate::generate_with_diagnostics(
        &e2,
        None,
        &request(ll(0.0, 1.0), 12_000.0, Mode::Discovery),
        &mut Workspace::new(),
        &mut || false,
    );
    assert_eq!(res.unwrap_err(), Failure::NetworkTooSmall);
    assert!(diag.t1_settled > 0 && diag.reached_m > 5_000.0 && diag.reached_m < 0.9 * 12_000.0, "{diag:?}");
    // Aller simple plus court que 1,05 fois le plus court chemin.
    let mut r = request(ll(0.0, 700.0), 700.0 * 2.0 + 10.0, Mode::Discovery);
    r.end = Some(ll(1560.0, 700.0));
    assert_eq!(run(&e, None, &r).unwrap_err(), Failure::DistanceTooShort);
    // Arrivée trop loin de tout.
    let mut r = request(ll(0.0, 700.0), 6000.0, Mode::Discovery);
    r.end = Some(ll(9000.0, 9000.0));
    assert_eq!(run(&e, None, &r).unwrap_err(), Failure::EndTooFar);
    // Strict sans surcouche.
    assert_eq!(
        run(&e, None, &request(ll(0.0, 700.0), 6000.0, Mode::Strict)).unwrap_err(),
        Failure::NoTeamData
    );
}

#[test]
fn a_start_in_a_one_way_pocket_snaps_onto_the_strongly_connected_network() {
    // Une rue à sens unique de 1 km mène du départ à une grille dont on ne revient pas par elle. Sens ignorés, la
    // rue est dans la composante de la grille et le départ s'y accrochait : `NO_WAY_BACK`. Composante fortement
    // connexe : la rue n'en est pas, le départ s'accroche à la grille, à 1 km (moins des 2 km permis).
    let mut s = grid(11, 300.0);
    pt(&mut s, 5000, -1000.0, 0.0);
    pt(&mut s, 5001, -500.0, 0.0);
    s.way(300, &[5000, 5001, 1], &tags("residential", &[("oneway", "yes")]));
    let e = engine_of(&mut s);
    let r = request(ll(-1000.0, 0.0), 4000.0, Mode::Discovery);
    let (res, diag) = crate::generate_with_diagnostics(&e, None, &r, &mut Workspace::new(), &mut || false);
    let routes = res.expect("une boucle sur la grille").routes;
    assert_valid(&e, None, &r, &routes);
    assert!((diag.start_snap_m - 1000.0).abs() < 5.0, "accroche à {} m", diag.start_snap_m);
    let pocket = edge_between(&e, 5000, 1);
    assert!(routes.iter().all(|x| x.steps.iter().all(|st| st.base_edge != pocket)));
}

#[test]
fn strict_does_not_snap_onto_a_one_way_team_street_that_leads_off_the_team_network() {
    // Campagne du lot 2, équipe A : le départ est au bout d'une rue à sens unique roulée une fois, qui sort du réseau
    // de l'équipe par des rues non roulées ; sens ignorés elle tient au grand réseau, l'arbre aller n'atteignait que
    // 200 m (`NETWORK_TOO_SMALL`). L'accroche prend maintenant l'anneau roulé, à 200 m.
    let mut g = grid(21, 300.0);
    let id = |i: i64, j: i64| 1 + i * 21 + j;
    pt(&mut g, 6000, 1300.0, 2150.0);
    pt(&mut g, 6001, 1300.0, 2450.0);
    g.way(400, &[id(5, 7), 6000], &tags("residential", &[("oneway", "yes")]));
    g.way(401, &[6000, id(4, 7)], &tags("residential", &[]));
    g.way(402, &[6000, 6001], &tags("residential", &[]));
    let e = engine_of(&mut g);
    let mut rows = Vec::new();
    let mut add = |a: i64, b: i64| rows.push((edge_between(&e, a as u64, b as u64), ROAD1));
    for k in 5..10 {
        add(id(5, k), id(5, k + 1));
        add(id(10, k), id(10, k + 1));
        add(id(k, 5), id(k + 1, 5));
        add(id(k, 10), id(k + 1, 10));
    }
    rows.push((edge_between(&e, id(5, 7) as u64, 6000), [1, 0, 0, 0, 0, 0, 0, 0]));
    let team = TeamData::build(&e, &overlay_of(&e, &rows)).unwrap();
    let r = request(ll(1300.0, 2150.0), 6000.0, Mode::Strict);
    let (res, diag) =
        crate::generate_with_diagnostics(&e, Some(&team), &r, &mut Workspace::new(), &mut || false);
    let routes = res.expect("strict sur l'anneau").routes;
    assert_valid(&e, Some(&team), &r, &routes);
    assert!(diag.start_snap_m > 150.0 && diag.start_snap_m < 250.0, "accroche à {} m", diag.start_snap_m);
}

#[test]
fn the_caller_can_stop_a_generation() {
    let e = engine_of(&mut grid(21, 300.0));
    let r = request(ll(3000.0, 3000.0), 6000.0, Mode::Discovery);
    let res = generate(&e, None, &r, &mut Workspace::new(), &mut || true);
    assert_eq!(res.unwrap_err(), Failure::Stopped);
}

#[test]
fn the_tree_equals_a_reference_dijkstra_with_turn_costs() {
    use crate::search::{make_anchor, tree_forward, Anchor, Ctx, Disc, Tree};
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let e = engine_of(&mut grid(9, 300.0));
    let win = e.full_window(0);
    let v = &win.v;
    let w = Weighter::new(v, None, RideType::Road, Mode::Discovery, 0.5, 768);
    let ok = |c: u32| w.chain(c) != INF;
    let snap = e.snap(&win, ll(1200.0, 1200.0), &ok, 0).unwrap().unwrap();
    let a: Anchor = make_anchor(&e, v, &w, snap);
    let s7 = ll(1200.0, 1200.0).to_e7();
    let disc = Disc::new(s7, s7, 1.0e6, true);
    let forb = Default::default();
    let esc = std::cell::Cell::new(false);
    let cx = Ctx { v, w: &w, disc: &disc, s: &a, e: &a, forb: &forb, esc: &esc };
    let mut t = Tree::default();
    tree_forward(&cx, &mut t, &mut || false).unwrap();
    // Référence : même graphe de lignes, mêmes coûts, Dijkstra par tas.
    let mut dist = vec![u32::MAX; 2 * v.n_chains];
    let mut pred = vec![NONE; 2 * v.n_chains];
    let mut heap = BinaryHeap::new();
    for dir in 0..2u32 {
        if a.out_cost[dir as usize] != INF {
            let f = (snap.chain << 1) | dir;
            dist[f as usize] = a.out_cost[dir as usize];
            heap.push(Reverse((dist[f as usize], f)));
        }
    }
    while let Some(Reverse((d, f))) = heap.pop() {
        if d > dist[f as usize] {
            continue;
        }
        for &fo in v.out(v.tgt(f)) {
            let over = (pred[f as usize] == NONE).then(|| a.out_head[(f & 1) as usize]);
            let Some(tc) = turn_cost(v, &forb, f, pred[f as usize], fo, over, None) else { continue };
            if !crate::search::retro_ok(v, pred[f as usize], f, fo) {
                continue;
            }
            let nd = d + w.chain(fo >> 1) + tc;
            if nd < dist[fo as usize] {
                dist[fo as usize] = nd;
                pred[fo as usize] = f;
                heap.push(Reverse((nd, fo)));
            }
        }
    }
    let mut compared = 0;
    for f in 0..2 * v.n_chains as u32 {
        assert_eq!(t.get(f), dist[f as usize], "état {f}");
        compared += 1;
    }
    assert!(compared > 100 && t.settled.len() > 100);
}

#[test]
fn a_violating_route_is_refused_by_the_internal_check() {
    // Un parcours qui fait demi-tour sur la même arête, et un autre qui ne part pas du bon endroit.
    let e = engine_of(&mut ring(None, false));
    let r = request(ll(1560.0, 700.0), 6160.0, Mode::Discovery);
    let routes = run(&e, None, &r).unwrap();
    let mut bad = routes[0].clone();
    bad.steps.insert(1, crate::Step { base_edge: bad.steps[0].base_edge, reverse: !bad.steps[0].reverse });
    let v = check_route(&e, None, &r, &bad).violations;
    assert!(v.contains(&Violation::UTurn) || v.contains(&Violation::Discontinuous), "{v:?}");
    let mut empty = routes[0].clone();
    empty.steps.clear();
    assert_eq!(check_route(&e, None, &r, &empty).violations, vec![Violation::Empty]);
    let mut far = request(ll(1560.0, 9000.0), 6160.0, Mode::Discovery);
    far.distance_m = routes[0].length_m;
    assert!(check_route(&e, None, &far, &routes[0]).violations.contains(&Violation::WrongStart));
}
