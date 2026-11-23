//! Tests du calcul d'itinéraire (`route.rs`) sur des graphes faits à la main.

use ord_graph::build::MemSource;

use crate::cost::RideType;
use crate::engine::Engine;
use crate::route::{route, route_geometry_spans, RouteFailure, RouteRequest, RouteTune, Routed};
use crate::search::{
    make_anchor, retro_ok, tree_forward, turn_cost, Ctx, Disc, Tree, Weighter, Workspace, INF, NONE,
};
use crate::team::TeamData;
use crate::tests::{edge_between, engine_of, grid, ll, m_lon, overlay_of, pt, tags, M_LAT, ROAD1};
use crate::{LatLon, Mode};

fn req(start: LatLon, end: LatLon) -> RouteRequest {
    RouteRequest {
        start,
        end,
        ride_type: RideType::Road,
        mode: Mode::Discovery,
        familiarity: 0.5,
        off_team_cost_q8: crate::DEFAULT_OFF_TEAM_COST_Q8,
    }
}

fn run(e: &Engine<Vec<u8>>, team: Option<&TeamData>, r: &RouteRequest) -> Result<Routed, RouteFailure> {
    route(e, team, r, &RouteTune::default(), &mut Workspace::new(), &mut || false)
}

fn dist_m(a: LatLon, b: LatLon) -> f64 {
    ((a.lat - b.lat) * M_LAT).hypot((a.lon - b.lon) * m_lon())
}

/// Les deux bouts de la géométrie sont aux points demandés (à `tol` mètres) et la géométrie est continue.
fn assert_joins(e: &Engine<Vec<u8>>, r: &RouteRequest, out: &Routed, tol: f64) -> Vec<LatLon> {
    let g = crate::route_geometry(e.base(), &out.route);
    assert!(g.len() >= 2, "géométrie de {} points", g.len());
    assert!(dist_m(g[0], r.start) <= tol, "départ à {} m", dist_m(g[0], r.start));
    assert!(dist_m(g[g.len() - 1], r.end) <= tol, "arrivée à {} m", dist_m(g[g.len() - 1], r.end));
    let len: f64 = g.windows(2).map(|w| dist_m(w[0], w[1])).sum();
    assert!((len - out.route.length_m).abs() <= 0.01 * len + 1.0, "{len} contre {}", out.route.length_m);
    g
}

#[test]
fn the_route_costs_what_a_reference_dijkstra_finds() {
    let e = engine_of(&mut grid(9, 300.0));
    // Départ et arrivée au milieu d'une rue, pas à un carrefour.
    let r = req(ll(300.0, 450.0), ll(1650.0, 1800.0));
    let out = run(&e, None, &r).expect("un itinéraire");
    assert!(!out.sharp_relaxed);
    assert_joins(&e, &r, &out, 2.0);
    // Au plus court sur une grille : 1350 m au nord, 1350 m à l'est.
    assert!((out.route.length_m - 2700.0).abs() < 5.0, "{}", out.route.length_m);

    // Référence : l'arbre aller du générateur sur toute la grille, puis le dernier pas vers le morceau d'arrivée.
    let win = e.full_window(0);
    let v = &win.v;
    let w = Weighter::new(v, None, RideType::Road, Mode::Discovery, 0.5, 768);
    let ok = |c: u32| w.chain(c) != INF;
    let (s, en) =
        (e.snap(&win, r.start, &ok, 0).unwrap().unwrap(), e.snap(&win, r.end, &ok, 0).unwrap().unwrap());
    let (sa, ea) = (make_anchor(&e, v, &w, s), make_anchor(&e, v, &w, en));
    let s7 = r.start.to_e7();
    let disc = Disc::new(s7, s7, 1.0e6, true);
    let forb = Default::default();
    let esc = std::cell::Cell::new(false);
    let cx = Ctx { v, w: &w, disc: &disc, s: &sa, e: &ea, forb: &forb, esc: &esc, org: None };
    let mut t = Tree::default();
    tree_forward(&cx, &mut t, &mut || false).unwrap();
    let mut best = u32::MAX;
    for dir in 0..2u32 {
        let fo = (en.chain << 1) | dir;
        if ea.in_cost[dir as usize] == INF {
            continue;
        }
        for p in 0..2 * v.n_chains as u32 {
            if t.get(p) == INF || v.tgt(p) != v.src(fo) {
                continue;
            }
            let over = (t.pred(p) == NONE).then(|| sa.out_head[(p & 1) as usize]);
            let hout = Some(ea.in_head[dir as usize]);
            let Some(tc) = turn_cost(v, &forb, p, t.pred(p), fo, over, hout) else { continue };
            if !retro_ok(v, t.pred(p), p, fo) {
                continue;
            }
            best = best.min(t.get(p) + tc + ea.in_cost[dir as usize]);
        }
    }
    assert_eq!(out.cost, best);
}

#[test]
fn two_points_on_the_same_street_are_joined_directly_in_both_orders() {
    let e = engine_of(&mut grid(5, 300.0));
    for (a, b) in [(50.0, 250.0), (250.0, 50.0)] {
        let r = req(ll(300.0, a), ll(300.0, b));
        let out = run(&e, None, &r).expect("un itinéraire");
        assert_joins(&e, &r, &out, 2.0);
        assert!((out.route.length_m - 200.0).abs() < 3.0, "{}", out.route.length_m);
        assert_eq!(out.settled, 0, "morceau direct : aucune recherche");
    }
}

/// Carré de 600 m à sens unique (1 → 2 → 3 → 4 → 1), une rue à double sens à chaque coin pour garder les carrefours.
fn one_way_square() -> MemSource {
    let mut s = MemSource::default();
    let corners = [(1, 0.0, 0.0), (2, 0.0, 600.0), (3, 600.0, 600.0), (4, 600.0, 0.0)];
    for (id, n, east) in corners {
        pt(&mut s, id, n, east);
    }
    pt(&mut s, 11, -300.0, -300.0);
    pt(&mut s, 12, -300.0, 900.0);
    pt(&mut s, 13, 900.0, 900.0);
    pt(&mut s, 14, 900.0, -300.0);
    for (way, a, b) in [(101, 1, 2), (102, 2, 3), (103, 3, 4), (104, 4, 1)] {
        s.way(way, &[a, b], &tags("residential", &[("oneway", "yes")]));
    }
    for (way, a, b) in [(111, 1, 11), (112, 2, 12), (113, 3, 13), (114, 4, 14)] {
        s.way(way, &[a, b], &tags("residential", &[]));
    }
    s
}

#[test]
fn an_arrival_behind_the_start_on_a_one_way_street_goes_around_the_block() {
    let e = engine_of(&mut one_way_square());
    // Sur le côté 1 → 2 : départ à 400 m du coin 1, arrivée à 100 m. Le sens unique interdit le retour direct.
    let r = req(ll(0.0, 400.0), ll(0.0, 100.0));
    let out = run(&e, None, &r).expect("un itinéraire");
    assert!(!out.sharp_relaxed);
    assert_joins(&e, &r, &out, 2.0);
    assert!((out.route.length_m - 2100.0).abs() < 5.0, "le tour moins 300 m : {}", out.route.length_m);
    // Dans le bon ordre, le morceau direct.
    let r = req(ll(0.0, 100.0), ll(0.0, 400.0));
    let out = run(&e, None, &r).expect("un itinéraire");
    assert!((out.route.length_m - 300.0).abs() < 3.0, "{}", out.route.length_m);
}

#[test]
fn a_sharp_turn_is_taken_only_when_there_is_no_other_way() {
    // A — J vers l'est, J — D qui continue (impasse), J — C qui repart à 160° du sens d'arrivée.
    let mut s = MemSource::default();
    pt(&mut s, 1, 0.0, 0.0);
    pt(&mut s, 2, 0.0, 1000.0);
    pt(&mut s, 3, 0.0, 1300.0);
    pt(&mut s, 4, -171.0, 530.0);
    s.way(101, &[1, 2], &tags("residential", &[]));
    s.way(102, &[2, 3], &tags("residential", &[]));
    s.way(103, &[2, 4], &tags("residential", &[]));
    let e = engine_of(&mut s);
    let r = req(ll(0.0, 500.0), ll(-120.0, 670.0));
    let out = run(&e, None, &r).expect("un itinéraire, par le virage serré");
    assert!(out.sharp_relaxed);
    assert_joins(&e, &r, &out, 5.0);
    assert!(out.route.length_m > 800.0 && out.route.length_m < 900.0, "{}", out.route.length_m);
}

#[test]
fn team_first_follows_the_team_street_and_discovery_the_shortest() {
    let e = engine_of(&mut grid(5, 300.0));
    let id = |i: u64, j: u64| 1 + i * 5 + j;
    let rows: Vec<_> = (0..4).map(|j| (edge_between(&e, id(2, j), id(2, j + 1)), ROAD1)).collect();
    let team = TeamData::build(&e, &overlay_of(&e, &rows)).unwrap();
    // Sur les deux rues nord-sud des bords, 50 m au-dessus de la rue 1 et 250 m sous la rue 2 (celle de l'équipe).
    let mut r = req(ll(350.0, 0.0), ll(350.0, 1200.0));
    let plain = run(&e, None, &r).expect("découverte");
    assert!((plain.route.length_m - 1300.0).abs() < 5.0, "{}", plain.route.length_m);
    assert_eq!(plain.route.team_m, 0.0);
    r.mode = Mode::TeamFirst;
    r.familiarity = 1.0;
    let with = run(&e, Some(&team), &r).expect("équipe d'abord");
    assert_joins(&e, &r, &with, 2.0);
    assert!((with.route.length_m - 1700.0).abs() < 5.0, "{}", with.route.length_m);
    assert!((with.route.team_m - 1200.0).abs() < 1.0, "{}", with.route.team_m);
    // Sans surcouche, un mode d'équipe est refusé.
    assert_eq!(run(&e, None, &r).unwrap_err(), RouteFailure::NoTeamData);
}

#[test]
fn far_points_and_long_legs_are_refused() {
    let e = engine_of(&mut grid(5, 300.0));
    let inside = ll(300.0, 450.0);
    assert_eq!(run(&e, None, &req(ll(-800.0, 600.0), inside)).unwrap_err(), RouteFailure::StartTooFar);
    assert_eq!(run(&e, None, &req(inside, ll(-800.0, 600.0))).unwrap_err(), RouteFailure::EndTooFar);
    let tune = RouteTune { crow_max_m: 500.0, ..RouteTune::default() };
    let r = req(inside, ll(1200.0, 1050.0));
    let got = route(&e, None, &r, &tune, &mut Workspace::new(), &mut || false);
    assert_eq!(got.unwrap_err(), RouteFailure::TooLong);
    let stopped = route(&e, None, &r, &RouteTune::default(), &mut Workspace::new(), &mut || true);
    assert_eq!(stopped.unwrap_err(), RouteFailure::Stopped);
}

#[test]
fn two_networks_that_do_not_touch_have_no_route() {
    let mut s = MemSource::default();
    for (id, east) in [(1, 0.0), (2, 600.0), (3, 900.0), (4, 1500.0)] {
        pt(&mut s, id, 0.0, east);
    }
    s.way(101, &[1, 2], &tags("residential", &[]));
    s.way(102, &[3, 4], &tags("residential", &[]));
    let e = engine_of(&mut s);
    let tune = RouteTune { min_component_share: 0.0, ..RouteTune::default() };
    let r = req(ll(0.0, 300.0), ll(0.0, 1200.0));
    let got = route(&e, None, &r, &tune, &mut Workspace::new(), &mut || false);
    assert_eq!(got.unwrap_err(), RouteFailure::NoRoute);
}

#[test]
fn the_unpaved_spans_cover_exactly_the_track() {
    // Rue, piste en gravier avec un point intermédiaire, rue.
    let mut s = MemSource::default();
    for (id, east) in [(1, 0.0), (2, 400.0), (3, 600.0), (4, 800.0), (5, 1200.0)] {
        pt(&mut s, id, 0.0, east);
    }
    s.way(101, &[1, 2], &tags("residential", &[]));
    s.way(102, &[2, 3, 4], &tags("track", &[("surface", "gravel")]));
    s.way(103, &[4, 5], &tags("residential", &[]));
    let e = engine_of(&mut s);
    let mut r = req(ll(0.0, 100.0), ll(0.0, 1100.0));
    r.ride_type = RideType::Gravel;
    let out = run(&e, None, &r).expect("un itinéraire");
    assert!((out.route.length_m - 1000.0).abs() < 3.0 && (out.route.offroad_m - 400.0).abs() < 1.0);
    let (g, spans) = route_geometry_spans(e.base(), &out.route);
    assert_eq!(g.len(), crate::route_geometry(e.base(), &out.route).len());
    assert_eq!(spans.len(), 1, "{spans:?}");
    let [i, j] = spans[0];
    assert!(dist_m(g[i as usize], ll(0.0, 400.0)) < 1.0 && dist_m(g[j as usize], ll(0.0, 800.0)) < 1.0);
    // À rebours : la même plage, comptée depuis l'autre bout.
    let back = run(&e, None, &RouteRequest { start: r.end, end: r.start, ..r }).expect("un itinéraire");
    let (gb, sb) = route_geometry_spans(e.base(), &back.route);
    assert_eq!(sb, vec![[gb.len() as u32 - 1 - j, gb.len() as u32 - 1 - i]]);
    // Sur la vue route, la piste n'existe pas : pas d'itinéraire entre les deux rues.
    r.ride_type = RideType::Road;
    assert!(run(&e, None, &r).is_err());
}
