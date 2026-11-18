//! Tests synthétiques du 4c (lot 4, §15.13) : recherche point à point, facteur minimal, couloir, puis figures
//! organiques. Aucune donnée de production.

use std::cell::Cell;

use ord_graph::build::MemSource;
use ord_graph::util::SplitMix64;

use crate::cost::Taste;
use crate::search::{
    astar, astar_p2p, make_anchor, tree_backward, tree_forward, Anchor, Ban, CorGrid, Ctx, Disc, Forbidden,
    P2pEnd, P2pTarget, ViewWs, Weighter, Workspace, BAN, NONE, P2P_SLACK, TAIL,
};
use crate::team::{TeamCtx, TeamData, WindowTeam};
use crate::tests::{engine_of, engine_zoomed, ll, overlay_of, pt, tags};
use crate::tests_reference::{scenario_graph, scenario_params, scenario_rows};
use crate::{Mode, Request, RideType, DEFAULT_OFF_TEAM_COST_Q8};

fn loop_request(start: crate::LatLon, d: f64, mode: Mode) -> Request {
    Request {
        start,
        end: None,
        bearing: None,
        distance_m: d,
        ride_type: RideType::Road,
        mode,
        familiarity: 0.5,
        off_team_cost_q8: DEFAULT_OFF_TEAM_COST_Q8,
        seed: 1,
    }
}

/// Test 5 : l'heuristique de la recherche point à point (vol d'oiseau × facteur minimal, bornes des deux arbres
/// moins la marge des virages) est admissible : même coût que la recherche à heuristique nulle ; sans la marge (la
/// sonde), un cas plus cher peut apparaître, toléré.
#[test]
fn the_point_to_point_heuristic_is_admissible_with_the_turn_slack() {
    let e = engine_zoomed(&mut scenario_graph(7), 11, scenario_params());
    let team = TeamData::build(&e, &overlay_of(&e, &scenario_rows(&e, 11))).unwrap();
    let mut rng = SplitMix64(99);
    let (mut pairs, mut worse_without_slack) = (0, 0);
    for (mode, start_n, start_e, d) in [
        (Mode::Discovery, 3000.0, 3000.0, 9000.0),
        (Mode::TeamFirst, 2500.0, 2800.0, 10_000.0),
        (Mode::TeamFirst, 3500.0, 2000.0, 8000.0),
        (Mode::Discovery, 1500.0, 4000.0, 7000.0),
    ] {
        let r = loop_request(ll(start_n, start_e), d, mode);
        let win = e.full_window(0);
        let wt = WindowTeam::build(&e, &win, &team);
        let ctx_team = TeamCtx { data: &team, win: &wt };
        let v = &win.v;
        let w = Weighter::new(v, Some(ctx_team), RideType::Road, mode, 0.5, DEFAULT_OFF_TEAM_COST_Q8);
        let s_snap = e.snap(&win, r.start, &|c| w.chain(c) != u32::MAX, 0).unwrap().unwrap();
        let sa: Anchor = make_anchor(&e, v, &w, s_snap);
        let s_e7 = r.start.to_e7();
        let disc = Disc::new(s_e7, s_e7, d, true);
        let forb = Forbidden::default();
        let esc = Cell::new(false);
        let cx = Ctx { v, w: &w, disc: &disc, s: &sa, e: &sa, forb: &forb, esc: &esc };
        let mut ws = Workspace::new();
        let ViewWs { t1, t2, ta, .. } = &mut ws.views[0];
        tree_forward(&cx, t1, &mut || false).unwrap();
        tree_backward(&cx, t2, &mut || false).unwrap();
        let (t1, t2) = (&*t1, &*t2);
        let both: Vec<u32> = t1.reached.iter().copied().filter(|&n| t2.node_best(n).is_some()).collect();
        assert!(both.len() > 50, "trop peu de nœuds atteints");
        let stamp = vec![0u32; v.n_chains];
        let ban = Ban::plain(&stamp, 1);
        for _ in 0..50 {
            let (n1, n2) = (both[rng.below(both.len())], both[rng.below(both.len())]);
            if n1 == n2 {
                continue;
            }
            let b1 = t1.node_best(n1).unwrap();
            let leg1 = t1.path_to(b1);
            let last = *leg1.last().unwrap();
            let last_pred = if leg1.len() >= 2 { leg1[leg1.len() - 2] } else { NONE };
            let tgt = |slack: u32, f_min: f64| P2pTarget {
                node: n2,
                xy: disc.xy_of(v.pos[n2 as usize]),
                t1: t1.node_best(n2).map_or(0, |x| t1.get(x)),
                t2: t2.node_best(n2).map_or(0, |x| t2.get(x)),
                f_min,
                slack,
            };
            let mut run = |tg: &P2pTarget, tri: bool| {
                let mut budget = u64::MAX / 2;
                astar_p2p(
                    &cx,
                    ta,
                    t1,
                    t2,
                    last,
                    last_pred,
                    tg,
                    &ban,
                    tri,
                    u64::MAX / 2,
                    &mut budget,
                    &mut || false,
                )
                .unwrap()
            };
            let zero = run(&tgt(0, 0.0), false);
            let safe = run(&tgt(P2P_SLACK, w.min_factor()), true);
            let bare = run(&tgt(0, w.min_factor()), true);
            assert_eq!(zero.end == P2pEnd::Found, safe.end == P2pEnd::Found, "{n1} -> {n2}");
            if zero.end == P2pEnd::Found {
                assert_eq!(zero.cost, safe.cost, "{n1} -> {n2} : l'heuristique n'est pas admissible");
                if bare.end == P2pEnd::Found && bare.cost > zero.cost {
                    worse_without_slack += 1;
                }
                assert!(safe.settled <= zero.settled + 1, "l'heuristique ne doit pas coûter plus d'états");
                pairs += 1;
            }
        }
    }
    assert!(pairs >= 100, "trop peu de paires comparées ({pairs})");
    eprintln!("paires {pairs}, plus chères sans la marge {worse_without_slack}");
}

/// Test 6 : `chain(c) ≥ f_min · len_dm(c)` pour toutes les chaînes d'une fenêtre, trois modes, familiarités {0 ; 0,5 ;
/// 1}, λ {1 ; 3 ; 5}, avec et sans goût d'équipe.
#[test]
fn the_minimum_factor_bounds_every_chain_cost() {
    let e = engine_zoomed(&mut scenario_graph(7), 11, scenario_params());
    let team = TeamData::build(&e, &overlay_of(&e, &scenario_rows(&e, 11))).unwrap();
    for vi in 0..2 {
        let win = e.full_window(vi);
        for taste in [None, Some(Taste::common()), Some(Taste::neutral())] {
            let wt = WindowTeam::build_with_taste(&e, &win, &team, taste.as_ref());
            for ride in crate::engine::rides_of_view(vi) {
                for mode in [Mode::Strict, Mode::TeamFirst, Mode::Discovery] {
                    for fam in [0.0f32, 0.5, 1.0] {
                        for lambda in [256u32, 768, 1280] {
                            let w = Weighter::new(
                                &win.v,
                                Some(TeamCtx { data: &team, win: &wt }),
                                *ride,
                                mode,
                                fam,
                                lambda,
                            );
                            let f_min = w.min_factor();
                            assert!(f_min > 0.0, "facteur nul");
                            for c in 0..win.v.n_chains as u32 {
                                let wc = w.chain(c);
                                let l = win.v.len_dm[c as usize];
                                if wc == u32::MAX || l == 0 {
                                    continue;
                                }
                                assert!(
                                    f64::from(wc) >= f_min * f64::from(l),
                                    "{ride:?} {mode:?} f={fam} λ={lambda} taste={}: chain {c} = {wc} < {f_min} * {l}",
                                    taste.is_some()
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Aller de 20 km vers le nord (x = 0), retour par une parallèle à 200 m ou par une route à 3 km, 1,28 fois plus longue
/// que la parallèle.
fn corridor_graph() -> MemSource {
    let mut s = MemSource::default();
    let mut way = 100i64;
    let mut line = |s: &mut MemSource, base: i64, east: f64| {
        for k in 0..=20i64 {
            pt(s, base + k, k as f64 * 1000.0, east);
        }
        for k in 0..20i64 {
            way += 1;
            s.way(way, &[base + k, base + k + 1], &tags("tertiary", &[]));
        }
        // Une impasse de 50 m à chaque nœud : autant de carrefours, donc des chaînes de 1 km (un réseau réel).
        for k in 1..20i64 {
            pt(s, base + 100 + k, k as f64 * 1000.0, east + 50.0);
            way += 1;
            s.way(way, &[base + k, base + 100 + k], &tags("tertiary", &[]));
        }
    };
    line(&mut s, 1000, 0.0);
    line(&mut s, 2000, 200.0);
    line(&mut s, 3000, 3000.0);
    // Le départ est au bout d'une impasse de 300 m : il n'est pas sur la chaîne de l'aller.
    pt(&mut s, 500, -300.0, 0.0);
    for (a, b) in [(500, 1000), (1000, 2000), (1020, 2020), (1000, 3000), (1020, 3020)] {
        way += 1;
        s.way(way, &[a, b], &tags("tertiary", &[]));
    }
    s
}

#[test]
fn a_corridor_sends_the_return_off_the_parallel_road_and_leaves_the_tail_and_the_vertex_alone() {
    let e = engine_of(&mut corridor_graph());
    let d = 56_000.0;
    let r = loop_request(ll(-300.0, 0.0), d, Mode::Discovery);
    let win = e.full_window(0);
    let v = &win.v;
    let w = Weighter::new(v, None, RideType::Road, Mode::Discovery, 0.5, DEFAULT_OFF_TEAM_COST_Q8);
    let s_snap = e.snap(&win, r.start, &|_| true, 0).unwrap().unwrap();
    let sa = make_anchor(&e, v, &w, s_snap);
    let s_e7 = r.start.to_e7();
    let disc = Disc::new(s_e7, s_e7, d, true);
    let forb = Forbidden::default();
    let esc = Cell::new(false);
    let cx = Ctx { v, w: &w, disc: &disc, s: &sa, e: &sa, forb: &forb, esc: &esc };
    let mut ws = Workspace::new();
    let ViewWs { t1, t2, ta, ban, ban_gen, cor } = &mut ws.views[0];
    tree_forward(&cx, t1, &mut || false).unwrap();
    tree_backward(&cx, t2, &mut || false).unwrap();
    let (t1, t2) = (&*t1, &*t2);
    // Le demi-tour : le nœud le plus au nord de l'aller (x = 0, repère du départ).
    let north = |n: u32| disc.xy_of(v.pos[n as usize]);
    let tnode = *t1
        .reached
        .iter()
        .filter(|&&n| north(n).0.abs() < 1.0)
        .max_by(|&&a, &&b| north(a).1.total_cmp(&north(b).1))
        .unwrap();
    assert!((north(tnode).1 - 20_300.0).abs() < 5.0);
    let out = t1.path_to(t1.node_best(tnode).unwrap());
    let last = *out.last().unwrap();
    let last_pred = if out.len() >= 2 { out[out.len() - 2] } else { NONE };
    *ban = vec![0; v.n_chains];
    *ban_gen = 1;
    let tail_dm = (0.15 * d * 10.0) as u32;
    for &f in &out {
        let c = (f >> 1) as usize;
        ban[c] = (*ban_gen << 3) | if t1.meters(f) <= tail_dm { TAIL } else { BAN };
    }
    let used_east = |fields: &[u32]| fields.iter().map(|&f| north(v.tgt(f)).0).fold(0.0f64, f64::max);
    let run = |b: &Ban<'_>, ta: &mut crate::search::Tree| {
        let (mut budget, mut exhausted) = (u64::MAX / 2, false);
        astar(&cx, ta, t2, last, last_pred, b, &mut budget, &mut exhausted, &mut || false).unwrap().unwrap()
    };
    // Sans couloir : la parallèle à 200 m.
    let plain = Ban::plain(ban, *ban_gen);
    let found = run(&plain, ta);
    let east = used_east(&found.fields);
    assert!(east < 1000.0, "sans couloir, le retour prend la parallèle (x max = {east})");
    // Avec un couloir de 0,03 D autour de l'aller : la route à 3 km.
    let w_m = 0.03 * d;
    cor.begin(d, w_m);
    cor.mark_leg(v, &disc, &out, 0.15 * d, 2.0 * w_m);
    assert!(cor.hit((200.0, 12_300.0)), "le milieu de l'aller, dilaté, couvre la parallèle");
    assert!(!cor.hit((200.0, 2_300.0)), "la queue du départ n'est pas marquée");
    assert!(!cor.hit((0.0, 20_100.0)), "la zone du sommet n'est pas marquée");
    assert!(!cor.hit((3_000.0, 12_300.0)), "la route à 3 km est hors du couloir");
    let with = Ban { stamp: ban, gen: *ban_gen, cor: Some(&*cor), kappa_q8: 512 };
    let found = run(&with, ta);
    let east = used_east(&found.fields);
    assert!(east > 2500.0, "avec le couloir, le retour prend la route à 3 km (x max = {east})");
    // κ = 1 : le couloir ne change plus rien au coût, le plafond redevient `2 h0 + 10 000`.
    let one = Ban { stamp: ban, gen: *ban_gen, cor: Some(&*cor), kappa_q8: 256 };
    assert_eq!(one.cap(1000), Ban::plain(ban, *ban_gen).cap(1000));
    assert_eq!(run(&one, ta).fields, run(&plain, ta).fields);
    let _ = CorGrid::default().bytes();
}
