//! Tests du contrôleur : des parcours **faits à la main** sur de petits graphes synthétiques, un test par garantie
//! (un parcours qui la viole, un qui la respecte) et des parcours valides. Aucun appel au moteur.

use ord_graph::build::{build_graph, BuildOptions, MemSource};
use ord_graph::format::{write_graph, GraphFile};
use ord_graph::overlay::{encode, Overlay, OverlayMeta};
use ord_loops::{Family, LatLon, Mode, Position, Request, RideType, Route, Step};

use crate::{Checker, Config, Violation};

const LAT0: f64 = 47.2;
const LON0: f64 = -1.56;
const M_LAT: f64 = 111_194.926_6;

fn m_lon() -> f64 {
    M_LAT * LAT0.to_radians().cos()
}

fn node(s: &mut MemSource, id: i64, north: f64, east: f64) {
    s.node(id, LAT0 + north / M_LAT, LON0 + east / m_lon());
}

fn ll(north: f64, east: f64) -> LatLon {
    LatLon::new(LAT0 + north / M_LAT, LON0 + east / m_lon())
}

const RES: &[(&str, &str)] = &[("highway", "residential")];

struct Fx {
    g: GraphFile<Vec<u8>>,
    _dir: tempfile::TempDir,
}

impl Fx {
    fn new(mut src: MemSource) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let mut data =
            build_graph(&mut src, &BuildOptions { zoom: 11 }, "test", &mut |_| {}).expect("construction");
        let path = dir.path().join("base.ordg");
        write_graph(&path, &mut data, "test", 1_700_000_000).expect("écriture");
        let g = GraphFile::open(std::fs::read(path).unwrap()).expect("ouverture");
        g.verify().expect("verify");
        Fx { g, _dir: dir }
    }

    /// Arête entre deux nœuds OSM, et vrai si elle est écrite de `y` vers `x`.
    fn edge(&self, x: i64, y: i64) -> (u32, bool) {
        for e in 0..self.g.header().edges as u32 {
            let (a, b) = self.g.edge_ends(e);
            let (oa, ob) = (self.g.osm_node_id(a) as i64, self.g.osm_node_id(b) as i64);
            if (oa, ob) == (x, y) {
                return (e, false);
            }
            if (oa, ob) == (y, x) {
                return (e, true);
            }
        }
        panic!("pas d'arête {x}-{y}");
    }

    fn len(&self, e: u32) -> f64 {
        f64::from(self.g.edge(e).len_dm) / 10.0
    }

    /// Un parcours le long de `nodes` : une arête par couple consécutif. `skip` : part du premier morceau déjà
    /// derrière le départ ; `walked` : part du dernier morceau parcourue (de 0 à 1).
    fn route(&self, nodes: &[i64], skip: f64, walked: f64) -> Route {
        let mut steps = Vec::new();
        for w in nodes.windows(2) {
            let (e, rev) = self.edge(w[0], w[1]);
            steps.push(Step { base_edge: e, reverse: rev });
        }
        let offset = |s: &Step, frac: f64| {
            let l = self.len(s.base_edge);
            if s.reverse {
                l - frac * l
            } else {
                frac * l
            }
        };
        let (first, last) = (steps[0], *steps.last().unwrap());
        Route {
            start: Position { edge: first.base_edge, offset_m: offset(&first, skip) },
            end: Position { edge: last.base_edge, offset_m: offset(&last, walked) },
            steps,
            length_m: 0.0,
            team_m: 0.0,
            bridged_m: 0.0,
            shared_m: 0.0,
            crossings: 0,
            unpaved_m: 0.0,
            presumed_surface_m: 0.0,
            bearing_deviation_deg: None,
            familiarity: 0.5,
            family: Family::Turnaround,
        }
    }

    /// Longueur de la suite d'arêtes d'un parcours construit par `route`, départ et arrivée compris.
    fn total(&self, nodes: &[i64], skip: f64, walked: f64) -> f64 {
        let lens: Vec<f64> = nodes.windows(2).map(|w| self.len(self.edge(w[0], w[1]).0)).collect();
        let n = lens.len();
        let mut t: f64 = lens.iter().sum();
        t -= skip * lens[0];
        t -= (1.0 - walked) * lens[n - 1];
        if n == 1 {
            t = (walked - skip) * lens[0];
        }
        t
    }

    fn overlay(&self, ridden: &[(i64, i64)]) -> Overlay {
        let mut es: Vec<u32> = ridden.iter().map(|&(a, b)| self.edge(a, b).0).collect();
        es.sort_unstable();
        let meta = OverlayMeta {
            graph: self.g.graph_identity(),
            team_id: 1,
            export_xxh3: 0,
            export_rows: 0,
            params_hash: 0,
            tool_version: "test".into(),
            build_epoch: 0,
            types: ["ROAD", "GRAVEL", "MTB", "MIXED"].iter().map(|s| (*s).to_string()).collect(),
        };
        let mut counts = Vec::new();
        for _ in &es {
            counts.extend_from_slice(&[1u16, 1, 0, 0, 0, 0, 0, 0]);
        }
        Overlay::decode(&encode(&meta, &es, &counts).unwrap()).unwrap()
    }
}

fn req_at(start: LatLon, d: f64, mode: Mode) -> Request {
    Request {
        start,
        end: None,
        bearing: None,
        distance_m: d,
        ride_type: RideType::Road,
        mode,
        familiarity: 0.5,
        off_team_cost_q8: ord_loops::DEFAULT_OFF_TEAM_COST_Q8,
        seed: 0,
    }
}

/// Carré de 1 000 m de côté : nœuds 1 à 4, tous de degré 2.
fn ring_source() -> MemSource {
    let mut s = MemSource::default();
    node(&mut s, 1, 0.0, 0.0);
    node(&mut s, 2, 0.0, 1000.0);
    node(&mut s, 3, 1000.0, 1000.0);
    node(&mut s, 4, 1000.0, 0.0);
    for (i, (a, b)) in [(1, 2), (2, 3), (3, 4), (4, 1)].into_iter().enumerate() {
        s.way(100 + i as i64, &[a, b], RES);
    }
    s
}

const RING_LOOP: [i64; 6] = [1, 2, 3, 4, 1, 2];

/// Violations d'un parcours sur le carré, départ au milieu de l'arête 1-2 et arrivée au même point.
fn ring_check(mutate: impl FnOnce(&mut Route), d: f64, mode: Mode) -> crate::Report {
    let fx = Fx::new(ring_source());
    let mut r = fx.route(&RING_LOOP, 0.5, 0.5);
    mutate(&mut r);
    Checker::new(&fx.g, None, Config::default()).check(&req_at(ll(0.0, 500.0), d, mode), &r)
}

#[test]
fn valid_ring_closing_in_the_middle_of_an_edge() {
    let rep = ring_check(|_| {}, 4000.0, Mode::Discovery);
    assert!(rep.ok(), "{:?}", rep.violations);
    assert!((rep.length_m - 4000.0).abs() < 2.0, "{}", rep.length_m);
    assert_eq!(rep.crossings, 0);
    assert!(rep.shared_m < 1.0);
    assert_eq!(rep.pieces, 5);
}

#[test]
fn empty_route() {
    let rep = ring_check(|r| r.steps.clear(), 4000.0, Mode::Discovery);
    assert_eq!(rep.violations, vec![Violation::Empty]);
}

#[test]
fn edge_outside_the_graph() {
    let rep = ring_check(|r| r.steps[1].base_edge = 9_999, 4000.0, Mode::Discovery);
    assert_eq!(rep.violations, vec![Violation::NotOnGraph]);
}

#[test]
fn discontinuous_steps() {
    let fx = Fx::new(ring_source());
    // 1-2 puis 3-4 : le nœud 2 n'est pas le nœud 3
    let mut r = fx.route(&[1, 2, 3], 0.0, 1.0);
    let (e34, rev) = fx.edge(3, 4);
    r.steps[1] = Step { base_edge: e34, reverse: rev };
    r.end = Position { edge: e34, offset_m: 10.0 };
    let rep = Checker::new(&fx.g, None, Config::default())
        .check(&req_at(ll(0.0, 0.0), 2000.0, Mode::Discovery), &r);
    assert!(rep.has(Violation::Discontinuous), "{:?}", rep.violations);
}

#[test]
fn one_way_against_the_flow() {
    let mut s = ring_source();
    // le côté 2-3 devient sens unique de 3 vers 2 : le parcours le prend de 2 vers 3
    s.ways[1].2.push(("oneway".into(), "-1".into()));
    let fx = Fx::new(s);
    let r = fx.route(&RING_LOOP, 0.5, 0.5);
    let rep = Checker::new(&fx.g, None, Config::default())
        .check(&req_at(ll(0.0, 500.0), 4000.0, Mode::Discovery), &r);
    assert!(rep.has(Violation::OneWay), "{:?}", rep.violations);
    // dans le bon sens, le même côté est permis
    let back = fx.route(&[1, 4, 3, 2, 1, 4], 0.5, 0.5);
    let rep = Checker::new(&fx.g, None, Config::default())
        .check(&req_at(ll(0.0, 500.0), 4000.0, Mode::Discovery), &back);
    assert!(!rep.has(Violation::OneWay), "{:?}", rep.violations);
}

#[test]
fn edge_not_admitted_for_the_type() {
    let mut s = ring_source();
    // un `track` non revêtu n'est pas admis en route
    s.ways[2].2 = vec![("highway".into(), "track".into()), ("surface".into(), "dirt".into())];
    let fx = Fx::new(s);
    let r = fx.route(&RING_LOOP, 0.5, 0.5);
    let ck = Checker::new(&fx.g, None, Config::default());
    let mut rq = req_at(ll(0.0, 500.0), 4000.0, Mode::Discovery);
    assert!(ck.check(&rq, &r).has(Violation::NotAdmitted));
    // le même côté est admis en gravel
    rq.ride_type = RideType::Gravel;
    assert!(!ck.check(&rq, &r).has(Violation::NotAdmitted));
}

#[test]
fn start_too_far_from_the_requested_point() {
    let fx = Fx::new(ring_source());
    let r = fx.route(&RING_LOOP, 0.5, 0.5);
    let ck = Checker::new(&fx.g, None, Config::default());
    assert!(ck.check(&req_at(ll(5_000.0, 500.0), 4000.0, Mode::Discovery), &r).has(Violation::WrongStart));
    assert!(!ck.check(&req_at(ll(1_500.0, 500.0), 4000.0, Mode::Discovery), &r).has(Violation::WrongStart));
}

#[test]
fn loop_that_does_not_close_and_arrival_too_far() {
    // la boucle finit à 30 % au lieu de 50 % de la première arête
    let fx = Fx::new(ring_source());
    let r = fx.route(&RING_LOOP, 0.5, 0.3);
    let ck = Checker::new(&fx.g, None, Config::default());
    assert!(ck.check(&req_at(ll(0.0, 500.0), 4000.0, Mode::Discovery), &r).has(Violation::WrongEnd));
    // un aller simple demandé vers un point à 5 km de l'arrivée
    let r = fx.route(&[1, 2, 3], 0.0, 1.0);
    let mut rq = req_at(ll(0.0, 0.0), 2000.0, Mode::Discovery);
    rq.end = Some(ll(6_000.0, 6_000.0));
    assert!(ck.check(&rq, &r).has(Violation::WrongEnd));
    rq.end = Some(ll(1000.0, 1000.0));
    assert!(!ck.check(&rq, &r).has(Violation::WrongEnd));
}

#[test]
fn offsets_outside_the_edge_are_refused() {
    let rep = ring_check(|r| r.start.offset_m = 5_000.0, 4000.0, Mode::Discovery);
    assert!(rep.has(Violation::BadOffset), "{:?}", rep.violations);
}

/// Anneau de trois côtés longs et d'un côté court entre les nœuds 4 et 1, `gap` mètres.
fn gap_source(gap: f64, spurs: bool) -> MemSource {
    let mut s = MemSource::default();
    node(&mut s, 1, 0.0, 0.0);
    node(&mut s, 2, 0.0, 1000.0);
    node(&mut s, 3, 1000.0, 1000.0);
    node(&mut s, 4, gap, 0.0);
    for (i, (a, b)) in [(1, 2), (2, 3), (3, 4), (4, 1)].into_iter().enumerate() {
        s.way(100 + i as i64, &[a, b], RES);
    }
    if spurs {
        node(&mut s, 5, gap, -100.0);
        node(&mut s, 6, 0.0, -100.0);
        s.way(110, &[4, 5], RES);
        s.way(111, &[1, 6], RES);
    }
    s
}

fn strict_gap(gap: f64, spurs: bool) -> (crate::Report, f64) {
    let fx = Fx::new(gap_source(gap, spurs));
    let mut ridden = vec![(1, 2), (2, 3), (3, 4)];
    if spurs {
        ridden.extend([(4, 5), (1, 6)]);
    }
    let ov = fx.overlay(&ridden);
    let nodes = [1, 2, 3, 4, 1, 2];
    let d = fx.total(&nodes, 0.5, 0.5);
    let r = fx.route(&nodes, 0.5, 0.5);
    let rep =
        Checker::new(&fx.g, Some(&ov), Config::default()).check(&req_at(ll(0.0, 500.0), d, Mode::Strict), &r);
    (rep, d)
}

#[test]
fn strict_bridges_a_short_hole_from_a_fictive_dead_end() {
    let (rep, _) = strict_gap(60.0, false);
    assert!(rep.ok(), "{:?}", rep.violations);
    assert!((rep.bridged_m - 60.0).abs() < 1.0, "{}", rep.bridged_m);
    assert!(rep.team_m > 2900.0);
}

#[test]
fn strict_refuses_a_hole_longer_than_100_m() {
    let (rep, _) = strict_gap(150.0, false);
    assert!(rep.has(Violation::OffTeam), "{:?}", rep.violations);
}

#[test]
fn strict_refuses_a_short_hole_between_ordinary_nodes() {
    // des deux côtés du trou, l'équipe a aussi une impasse : degré d'équipe 2, pas un cul-de-sac fictif
    let (rep, _) = strict_gap(60.0, true);
    assert!(rep.has(Violation::OffTeam), "{:?}", rep.violations);
}

#[test]
fn strict_bridges_a_connector_of_at_most_30_m_between_two_team_nodes() {
    // la jonction courte (§2.3) : 20 m entre deux nœuds d'équipe ordinaires est comblée, 40 m ne l'est pas
    let (rep, _) = strict_gap(20.0, true);
    assert!(rep.ok(), "{:?}", rep.violations);
    assert!((rep.bridged_m - 20.0).abs() < 1.0, "{}", rep.bridged_m);
    let (rep, _) = strict_gap(40.0, true);
    assert!(rep.has(Violation::OffTeam), "{:?}", rep.violations);
}

#[test]
fn off_team_is_not_a_violation_outside_strict() {
    let fx = Fx::new(gap_source(400.0, false));
    let ov = fx.overlay(&[(1, 2)]);
    let nodes = [1, 2, 3, 4, 1, 2];
    let d = fx.total(&nodes, 0.5, 0.5);
    let r = fx.route(&nodes, 0.5, 0.5);
    let ck = Checker::new(&fx.g, Some(&ov), Config::default());
    let rep = ck.check(&req_at(ll(0.0, 500.0), d, Mode::TeamFirst), &r);
    assert!(!rep.has(Violation::OffTeam), "{:?}", rep.violations);
    assert!(rep.off_team_m > 2000.0);
    assert!(ck.check(&req_at(ll(0.0, 500.0), d, Mode::Strict), &r).has(Violation::OffTeam));
}

/// Étoile : centre J (nœud 10) et trois bras de 200 m ; le bras B part au cap `theta`.
fn star(theta: f64) -> (Fx, Route) {
    let mut s = MemSource::default();
    node(&mut s, 10, 0.0, 0.0);
    node(&mut s, 1, 200.0, 0.0);
    let t = theta.to_radians();
    node(&mut s, 2, 200.0 * t.cos(), 200.0 * t.sin());
    node(&mut s, 3, 0.0, -200.0);
    s.way(1, &[1, 10], RES);
    s.way(2, &[10, 2], RES);
    s.way(3, &[10, 3], RES);
    let fx = Fx::new(s);
    let r = fx.route(&[1, 10, 2], 0.0, 1.0);
    (fx, r)
}

fn star_check(theta: f64) -> crate::Report {
    let (fx, r) = star(theta);
    Checker::new(&fx.g, None, Config::default()).check(&req_at(ll(200.0, 0.0), 400.0, Mode::Discovery), &r)
}

#[test]
fn turn_of_149_degrees_is_allowed_and_151_refused() {
    // arrivée par le nord (cap 180) : sortie au cap theta, virage 180 - theta
    assert!(!star_check(31.0).has(Violation::SharpTurn));
    assert!(star_check(29.0).has(Violation::SharpTurn));
    assert!(!star_check(90.0).has(Violation::SharpTurn));
}

#[test]
fn u_turn_on_the_same_edge() {
    let (fx, _) = star(90.0);
    let mut r = fx.route(&[1, 10, 1], 0.0, 1.0);
    r.end = Position { edge: r.steps[1].base_edge, offset_m: r.start.offset_m };
    let rep = Checker::new(&fx.g, None, Config::default())
        .check(&req_at(ll(200.0, 0.0), 400.0, Mode::Discovery), &r);
    assert!(rep.has(Violation::UTurn), "{:?}", rep.violations);
    let (fx, r) = star(90.0);
    let rep = Checker::new(&fx.g, None, Config::default())
        .check(&req_at(ll(200.0, 0.0), 400.0, Mode::Discovery), &r);
    assert!(!rep.has(Violation::UTurn));
}

#[test]
fn dead_end_inside_the_route_but_not_at_its_arrival() {
    let (fx, _) = star(90.0);
    // 1 -> 10 -> 2 (impasse) -> 10 : le nœud 2 est intérieur
    let r = fx.route(&[1, 10, 2, 10, 3], 0.0, 1.0);
    let ck = Checker::new(&fx.g, None, Config::default());
    let rep = ck.check(&req_at(ll(200.0, 0.0), 800.0, Mode::Discovery), &r);
    assert!(rep.has(Violation::DeadEnd), "{:?}", rep.violations);
    // arrivée au milieu de l'arête qui mène à l'impasse : permis
    let r = fx.route(&[1, 10, 2], 0.0, 0.5);
    let rep = ck.check(&req_at(ll(200.0, 0.0), 300.0, Mode::Discovery), &r);
    assert!(!rep.has(Violation::DeadEnd), "{:?}", rep.violations);
    // départ dans l'impasse, par son arête : permis aussi
    let r = fx.route(&[2, 10, 1], 0.5, 1.0);
    let rep = ck.check(&req_at(ll(0.0, 200.0), 300.0, Mode::Discovery), &r);
    assert!(!rep.has(Violation::DeadEnd), "{:?}", rep.violations);
}

#[test]
fn edge_used_three_times() {
    let (fx, _) = star(90.0);
    let r = fx.route(&[10, 2, 10, 2], 0.0, 1.0);
    let rep =
        Checker::new(&fx.g, None, Config::default()).check(&req_at(ll(0.0, 0.0), 600.0, Mode::Discovery), &r);
    assert!(rep.has(Violation::EdgeUsedMoreThanTwice), "{:?}", rep.violations);
    // deux fois seulement : pas ce code
    let r = fx.route(&[10, 2, 10], 0.0, 1.0);
    let rep =
        Checker::new(&fx.g, None, Config::default()).check(&req_at(ll(0.0, 0.0), 400.0, Mode::Discovery), &r);
    assert!(!rep.has(Violation::EdgeUsedMoreThanTwice));
}

/// Carré de 1 000 m avec une impasse de `spur` mètres au nord du nœud 3 (nœud 5).
fn spur_source(spur: f64) -> MemSource {
    let mut s = ring_source();
    node(&mut s, 5, 1000.0 + spur, 1000.0);
    s.way(120, &[3, 5], RES);
    s
}

#[test]
fn reuse_in_the_middle_of_the_route_is_outside_any_tail() {
    let fx = Fx::new(spur_source(500.0));
    // aller-retour dans l'impasse à mi-parcours
    let nodes = [1, 2, 3, 5, 3, 4, 1, 2];
    let d = fx.total(&nodes, 0.5, 0.5);
    let r = fx.route(&nodes, 0.5, 0.5);
    let rep =
        Checker::new(&fx.g, None, Config::default()).check(&req_at(ll(0.0, 500.0), d, Mode::Discovery), &r);
    assert!(rep.has(Violation::ReuseOutsideTail), "{:?}", rep.violations);
}

#[test]
fn a_short_tail_at_the_start_and_the_end_is_allowed() {
    let fx = Fx::new(spur_source(500.0));
    // le départ est à 100 m du carré, dans l'impasse ; l'arrivée au même point
    let nodes = [5, 3, 4, 1, 2, 3, 5];
    let d = fx.total(&nodes, 0.8, 0.2);
    let r = fx.route(&nodes, 0.8, 0.2);
    let rep = Checker::new(&fx.g, None, Config::default())
        .check(&req_at(ll(1400.0, 1000.0), d, Mode::Discovery), &r);
    assert!(rep.ok(), "{:?}", rep.violations);
    assert!((rep.shared_m - 100.0).abs() < 2.0, "{}", rep.shared_m);
    assert!((rep.start_zone_m - 100.0).abs() < 2.0 && (rep.end_zone_m - 100.0).abs() < 2.0);
}

#[test]
fn a_long_tail_is_refused_twice() {
    let fx = Fx::new(spur_source(1500.0));
    let nodes = [5, 3, 4, 1, 2, 3, 5];
    let d = fx.total(&nodes, 0.0, 1.0);
    let r = fx.route(&nodes, 0.0, 1.0);
    let rep = Checker::new(&fx.g, None, Config::default())
        .check(&req_at(ll(2500.0, 1000.0), d, Mode::Discovery), &r);
    assert!(rep.has(Violation::ReuseOutsideTail), "{:?}", rep.violations);
    assert!(rep.has(Violation::TailTooLong), "{:?}", rep.violations);
}

/// Fleur : centre 100 et `petals` pétales triangulaires de 700 m, répartis tous les 90 degrés.
fn flower(petals: usize) -> (Fx, Vec<i64>) {
    let mut s = MemSource::default();
    node(&mut s, 100, 0.0, 0.0);
    let mut nodes = vec![100];
    let mut way = 1;
    for p in 0..petals {
        let th = (p as f64 * 90.0).to_radians();
        let tb = th + 0.45;
        let (a, b) = (200 + 2 * p as i64, 201 + 2 * p as i64);
        node(&mut s, a, 700.0 * th.cos(), 700.0 * th.sin());
        node(&mut s, b, 700.0 * tb.cos(), 700.0 * tb.sin());
        for (x, y) in [(100, a), (a, b), (b, 100)] {
            s.way(way, &[x, y], RES);
            way += 1;
        }
        nodes.extend([a, b, 100]);
    }
    (Fx::new(s), nodes)
}

fn flower_check(petals: usize) -> crate::Report {
    let (fx, mut nodes) = flower(petals);
    // le départ est au milieu de la première arête (100 -> premier pétale) et l'arrivée au même point
    nodes.push(nodes[1]);
    let d = fx.total(&nodes, 0.5, 0.5);
    let r = fx.route(&nodes, 0.5, 0.5);
    Checker::new(&fx.g, None, Config::default()).check(&req_at(ll(350.0, 0.0), d, Mode::Discovery), &r)
}

#[test]
fn two_crossings_are_allowed_and_three_refused() {
    let three = flower_check(3);
    assert_eq!(three.crossings, 2, "{:?}", three.violations);
    assert!(!three.has(Violation::TooManyCrossings), "{:?}", three.violations);
    let four = flower_check(4);
    assert_eq!(four.crossings, 3);
    assert!(four.has(Violation::TooManyCrossings), "{:?}", four.violations);
}

#[test]
fn the_two_carriageways_of_a_dual_road_are_a_doubled_corridor() {
    let mut s = MemSource::default();
    node(&mut s, 1, 0.0, 0.0);
    node(&mut s, 2, 0.0, 3000.0);
    node(&mut s, 3, 15.0, 3000.0);
    node(&mut s, 4, 15.0, 0.0);
    for (i, (a, b)) in [(1, 2), (2, 3), (3, 4), (4, 1)].into_iter().enumerate() {
        s.way(100 + i as i64, &[a, b], RES);
    }
    let fx = Fx::new(s);
    let nodes = [1, 2, 3, 4, 1, 2];
    let d = fx.total(&nodes, 0.5, 0.5);
    let r = fx.route(&nodes, 0.5, 0.5);
    let rep =
        Checker::new(&fx.g, None, Config::default()).check(&req_at(ll(0.0, 1500.0), d, Mode::Discovery), &r);
    assert!(rep.has(Violation::CorridorDoubled), "{:?}", rep.violations);
    assert!(rep.corridor_share > 0.3, "{}", rep.corridor_share);
    // le carré de 1 000 m n'a pas de couloir doublé
    assert!(ring_check(|_| {}, 4000.0, Mode::Discovery).corridor_share < 0.05);
}

#[test]
fn distance_within_ten_percent() {
    assert!(!ring_check(|_| {}, 4300.0, Mode::Discovery).has(Violation::DistanceOutOfTolerance));
    assert!(!ring_check(|_| {}, 3700.0, Mode::Discovery).has(Violation::DistanceOutOfTolerance));
    assert!(ring_check(|_| {}, 5000.0, Mode::Discovery).has(Violation::DistanceOutOfTolerance));
    assert!(ring_check(|_| {}, 3000.0, Mode::Discovery).has(Violation::DistanceOutOfTolerance));
}

/// Deux routes parallèles reliées par un connecteur de `conn` mètres ; en un seul `way` si `one_way_id`.
/// Le parcours monte la première, traverse le connecteur et descend la seconde (un demi-tour fait de trois
/// morceaux). Chaque extrémité du connecteur a une impasse de plus, donc un degré de 3.
fn hairpin(conn: f64, same_way: bool) -> (Fx, Route) {
    let mut s = MemSource::default();
    node(&mut s, 1, -300.0, 0.0);
    node(&mut s, 11, 0.0, 0.0);
    node(&mut s, 12, 0.0, conn);
    node(&mut s, 2, -300.0, conn);
    node(&mut s, 21, 0.0, -100.0);
    node(&mut s, 22, 0.0, conn + 100.0);
    if same_way {
        s.way(1, &[1, 11, 12, 2], RES);
    } else {
        s.way(1, &[1, 11], RES);
        s.way(2, &[11, 12], RES);
        s.way(3, &[12, 2], RES);
    }
    s.way(4, &[11, 21], RES);
    s.way(5, &[12, 22], RES);
    let fx = Fx::new(s);
    let r = fx.route(&[1, 11, 12, 2], 0.0, 1.0);
    (fx, r)
}

fn hairpin_check(conn: f64, same_way: bool, cfg: Config) -> crate::Report {
    let (fx, r) = hairpin(conn, same_way);
    let d = 600.0 + conn;
    Checker::new(&fx.g, None, cfg).check(&req_at(ll(-300.0, 0.0), d, Mode::Discovery), &r)
}

#[test]
fn a_u_turn_made_of_a_connector_between_two_junctions_is_refused() {
    // chaque nœud tourne de 90 degrés seulement, mais l'ensemble, à moins de 100 m, fait demi-tour
    let rep = hairpin_check(70.0, false, Config::default());
    assert!(rep.has(Violation::SharpTurn), "{:?}", rep.violations);
}

/// Le même demi-tour en trois morceaux que [`hairpin`], avec des branches de `leg` mètres : le parcours part
/// à `leg` mètres du premier carrefour et s'arrête à `leg` mètres du second.
fn block_turn_check(leg: f64, cfg: Config) -> crate::Report {
    let conn = 70.0;
    let mut s = MemSource::default();
    node(&mut s, 1, -leg, 0.0);
    node(&mut s, 11, 0.0, 0.0);
    node(&mut s, 12, 0.0, conn);
    node(&mut s, 2, -leg, conn);
    node(&mut s, 21, 0.0, -100.0);
    node(&mut s, 22, 0.0, conn + 100.0);
    s.way(1, &[1, 11], RES);
    s.way(2, &[11, 12], RES);
    s.way(3, &[12, 2], RES);
    s.way(4, &[11, 21], RES);
    s.way(5, &[12, 22], RES);
    let fx = Fx::new(s);
    let r = fx.route(&[1, 11, 12, 2], 0.0, 1.0);
    Checker::new(&fx.g, None, cfg).check(&req_at(ll(-leg, 0.0), 2.0 * leg + conn, Mode::Discovery), &r)
}

#[test]
fn a_turn_round_the_block_within_200_m_of_an_end_is_not_a_u_turn() {
    // Java `CLUSTER_FREE_ENDS_M` : en quittant son départ, faire le tour d'un pâté de maisons n'est pas un retour.
    // Branches de 100 m : le second carrefour est à 170 m du départ, la grappe est à un bout.
    let near = block_turn_check(100.0, Config::default());
    assert!(!near.has(Violation::SharpTurn), "{:?}", near.violations);
    // Branches de 300 m : la grappe est au milieu du parcours, elle reste refusée.
    let far = block_turn_check(300.0, Config::default());
    assert!(far.has(Violation::SharpTurn), "{:?}", far.violations);
    // Sans la levée, la grappe proche du départ est refusée comme avant.
    let strict = block_turn_check(100.0, Config { cluster_free_ends_m: 0.0, ..Config::default() });
    assert!(strict.has(Violation::SharpTurn), "{:?}", strict.violations);
}

#[test]
fn a_hairpin_of_one_way_is_allowed_and_a_connector_of_another_way_is_not() {
    // La note (§6) n'exempte pas l'apex d'un nœud seul : par défaut il est refusé.
    let strict = hairpin_check(12.0, true, Config::default());
    assert!(strict.has(Violation::SharpTurn), "{:?}", strict.violations);
    // L'exemption d'un nœud seul (Java : un nœud que l'équipe traverse) est une option, désactivée par défaut.
    let opt_in = Config { hairpin_single_node: true, ..Config::default() };
    let ok = hairpin_check(12.0, true, opt_in);
    assert!(!ok.has(Violation::SharpTurn), "{:?}", ok.violations);
    // la même géométrie, mais le connecteur est une autre way : refusé même avec l'option
    let other = hairpin_check(12.0, false, opt_in);
    assert!(other.has(Violation::SharpTurn), "{:?}", other.violations);
    // un connecteur droit de 25 m dans la même way : refusé (portion droite en travers)
    let wide = hairpin_check(25.0, true, opt_in);
    assert!(wide.has(Violation::SharpTurn), "{:?}", wide.violations);
}

#[test]
fn overlapping_start_and_end_on_one_edge() {
    // une arête seule, parcourue de 20 % à 80 % : une boucle qui ne se ferme pas, mais le parcours est sain
    let fx = Fx::new(ring_source());
    let mut r = fx.route(&[1, 2], 0.2, 0.8);
    let ck = Checker::new(&fx.g, None, Config::default());
    let mut rq = req_at(ll(0.0, 200.0), 600.0, Mode::Discovery);
    rq.end = Some(ll(0.0, 800.0));
    let rep = ck.check(&rq, &r);
    assert!(rep.ok(), "{:?}", rep.violations);
    assert!((rep.length_m - 600.0).abs() < 1.0);
    // une arrivée avant le départ sur le sens de marche : position incohérente
    r.end.offset_m = 100.0;
    assert!(ck.check(&rq, &r).has(Violation::BadOffset));
}
