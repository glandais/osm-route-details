//! Tests synthétiques du 4c (lot 4, §15.13) : recherche point à point, facteur minimal, couloir, puis figures
//! organiques. Aucune donnée de production.

use std::cell::Cell;

use ord_graph::build::MemSource;
use ord_graph::util::SplitMix64;

use crate::cost::Taste;
use crate::search::{
    astar, astar_p2p, make_anchor, t1_min_factor, tree_backward, tree_forward, Anchor, Ban, CorGrid, Ctx,
    Disc, Forbidden, P2pEnd, P2pTarget, ViewWs, Weighter, Workspace, BAN, NONE, P2P_SLACK, TAIL,
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
        figures: crate::Figures::Classic,
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
            let f_min = t1_min_factor(&cx, t1);
            assert!(f_min > 0.0, "facteur nul");
            let safe = run(&tgt(P2P_SLACK, f_min), true);
            let bare = run(&tgt(0, f_min), true);
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

/// Test 6 : `chain(c) ≥ f_min · len_dm(c)` pour toutes les chaînes que l'arbre aller atteint, trois modes,
/// familiarités {0 ; 0,5 ; 1}, λ {1 ; 3 ; 5}, avec et sans goût d'équipe ; et le facteur ne dépend pas de la fenêtre
/// (celle des tuiles nécessaires rend celui du graphe entier, I4).
#[test]
fn the_minimum_factor_bounds_every_chain_cost() {
    let e = engine_zoomed(&mut scenario_graph(7), 11, scenario_params());
    let team = TeamData::build(&e, &overlay_of(&e, &scenario_rows(&e, 11))).unwrap();
    let mut checked = 0usize;
    for vi in 0..2 {
        let full = e.full_window(vi);
        for taste in [None, Some(Taste::common()), Some(Taste::neutral())] {
            for ride in crate::engine::rides_of_view(vi) {
                let mut r = loop_request(ll(3000.0, 3000.0), 6000.0, Mode::Discovery);
                r.ride_type = *ride;
                let plan = e.plan(&r, 500.0);
                let need = e.build_window(plan.view, &plan.need, &mut || false).unwrap();
                for mode in [Mode::Strict, Mode::TeamFirst, Mode::Discovery] {
                    for fam in [0.0f32, 0.5, 1.0] {
                        for lambda in [256u32, 768, 1280] {
                            let factor = |win: &crate::Window, check: bool| {
                                let wt = WindowTeam::build_with_taste(&e, win, &team, taste.as_ref());
                                let v = &win.v;
                                let w = Weighter::new(
                                    v,
                                    Some(TeamCtx { data: &team, win: &wt }),
                                    *ride,
                                    mode,
                                    fam,
                                    lambda,
                                );
                                let s_snap =
                                    e.snap(win, r.start, &|c| w.chain(c) != u32::MAX, 0).unwrap().unwrap();
                                let sa: Anchor = make_anchor(&e, v, &w, s_snap);
                                let s_e7 = r.start.to_e7();
                                let disc = Disc::new(s_e7, s_e7, r.distance_m, true);
                                let forb = Forbidden::default();
                                let esc = Cell::new(false);
                                let cx =
                                    Ctx { v, w: &w, disc: &disc, s: &sa, e: &sa, forb: &forb, esc: &esc };
                                let mut ws = Workspace::new();
                                let t1 = &mut ws.views[vi].t1;
                                tree_forward(&cx, t1, &mut || false).unwrap();
                                let f_min = t1_min_factor(&cx, t1);
                                if check {
                                    for &f in &t1.settled {
                                        let c = f >> 1;
                                        let (wc, l) = (w.chain(c), v.len_dm[c as usize]);
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
                                f_min
                            };
                            let f_full = factor(&full, true);
                            if mode != Mode::Strict {
                                assert!(f_full > 0.0, "facteur nul");
                            }
                            assert_eq!(
                                f_full.to_bits(),
                                factor(&need, false).to_bits(),
                                "le facteur dépend de la fenêtre"
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(checked > 100);
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

// --- La génération organique (commit 3, §15.13 tests 2, 3, 4, 8 à 11) -----------------------------------------------

use crate::engine::Engine;
use crate::generate::{
    fingerprint, generate_with_diagnostics, generate_with_diagnostics_in, generate_with_figures_in,
};
use crate::tests::{edge_between, request};
use crate::tests_reference::{
    assert_matches_fixture, assert_matches_reference, scenario_requests, REQUESTS, ZOOMS,
};
use crate::tests_window::{all_tiles, run_in};
use crate::{Family, FigureMix, Figures, Organic, OrganicTune, Route, Step};

/// La répartition des tests : tous les premiers parcours préférés triangles, r / D comme celui des vraies équipes.
pub(crate) fn test_mix() -> FigureMix {
    FigureMix { tri_q8: 256, rd_tri_milli: [230, 270, 318, 360, 420] }
}

pub(crate) fn organic() -> Figures {
    Figures::Organic(Organic { mix: test_mix(), tune: OrganicTune::default() })
}

fn with_figures(r: &Request, figures: Figures) -> Request {
    Request { figures, ..r.clone() }
}

fn stop() -> impl FnMut() -> bool {
    || false
}

/// Test 2 (I2) : sans triangle ni couloir, `Organic` rend ce que rend `Classic`, sur les 140 requêtes de la référence.
#[test]
fn an_organic_request_without_triangles_or_corridor_is_the_classic_one() {
    let lens_only = Figures::Organic(Organic {
        mix: FigureMix::LENS_ONLY,
        tune: OrganicTune { corridor_w_milli: 0, ..OrganicTune::default() },
    });
    assert_matches_reference(&|e, team, r, ws| {
        generate_with_diagnostics(e, team, &with_figures(r, lens_only), ws, &mut stop())
    });
}

pub(crate) fn organic_fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/organic_reference.txt")
}

/// Test 3 : la référence organique, bénie au commit 3 (`tri_q8 = 256`, réglages par défaut), rejouée ensuite ;
/// `ORD_LOOPS_BLESS=1` la réécrit quand le réglage change, en le disant au commit.
#[test]
fn the_organic_reference_is_replayed() {
    assert_matches_fixture(
        &|e, team, r, ws| generate_with_diagnostics(e, team, &with_figures(r, organic()), ws, &mut stop()),
        scenario_params,
        &organic_fixture_path(),
        true,
    );
}

/// Test 11 (I4) : toute fenêtre qui contient les tuiles nécessaires rend la sortie organique figée, sans échappement.
#[test]
fn any_window_gives_the_organic_reference() {
    let rng = std::cell::RefCell::new(SplitMix64(7));
    assert_matches_fixture(
        &|e, team, r, ws| {
            run_in(e, team, &with_figures(r, organic()), ws, &|e, p| {
                let all = all_tiles(e, p.view);
                let share = rng.borrow_mut().below(101);
                all.into_iter().filter(|_| rng.borrow_mut().below(100) < share).collect()
            })
        },
        scenario_params,
        &organic_fixture_path(),
        false,
    );
}

/// Test 11 (I4) : même graine, même sortie, compteurs compris.
#[test]
fn the_organic_generation_is_deterministic() {
    let e = engine_zoomed(&mut scenario_graph(7), 11, scenario_params());
    let team = TeamData::build(&e, &overlay_of(&e, &scenario_rows(&e, 11))).unwrap();
    let win = [e.full_window(0), e.full_window(1)];
    let mut triangles = 0;
    for r in scenario_requests(13, REQUESTS).iter().map(|r| with_figures(r, organic())) {
        let vi = r.ride_type.view_index();
        let wt = WindowTeam::build(&e, &win[vi], &team);
        let ctx = || Some(TeamCtx { data: &team, win: &wt });
        let a = generate_with_figures_in(&e, &win[vi], ctx(), &r, &mut Workspace::new(), &mut stop());
        let b = generate_with_figures_in(&e, &win[vi], ctx(), &r, &mut Workspace::new(), &mut stop());
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
        // Au plus `max_pairs` paires distinctes par requête (décision 2 du §9.7), reprises à part.
        assert!(a.2.searched - a.2.retried <= u32::from(OrganicTune::default().max_pairs), "{:?}", a.2);
        if let Ok(g) = &a.0 {
            triangles += g.routes.iter().filter(|x| x.family == Family::Triangle).count();
        }
    }
    assert!(triangles > 0, "aucun triangle sur la référence : le test ne prouve rien");
}

/// Aire signée (lacet) du parcours dans le repère du départ : négative pour un parcours dans le sens horaire.
fn signed_area(e: &Engine<Vec<u8>>, r: &Request, route: &crate::Route) -> f64 {
    let base = e.base();
    let loc = crate::geo::Local::new(r.start.to_e7());
    let pts: Vec<(f64, f64)> = crate::route_geometry(base, route).iter().map(|p| loc.xy(p.to_e7())).collect();
    let mut a = 0.0;
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        a += p.0 * q.1 - q.0 * p.1;
    }
    a / 2.0
}

/// Test 4 : sur la grille de 48 × 48, `tri_q8 = 256` : des triangles sont rendus, dans le sens tiré, à ±10 % de D.
#[test]
fn a_triangle_is_found_on_the_grid_in_the_drawn_sense_and_at_the_asked_length() {
    let e = engine_zoomed(&mut scenario_graph_dense_for_tests(), 11, scenario_params());
    let (mut seen, mut sense_ok) = (0, 0);
    let win = e.full_window(0);
    for k in 0..120u64 {
        let mut r = request(ll(3000.0 + 20.0 * k as f64, 3000.0 + 13.0 * k as f64), 7000.0, Mode::Discovery);
        r.seed = k;
        r.figures = organic();
        let Ok(g) = generate_with_diagnostics_in(&e, &win, None, &r, &mut Workspace::new(), &mut stop()).0
        else {
            continue;
        };
        let Figures::Organic(o) = r.figures else { unreachable!() };
        let draw = crate::figures::draw(r.seed ^ fingerprint(&r) ^ crate::figures::FIGURE_SALT, &o);
        for route in g.routes.iter().filter(|x| x.family == Family::Triangle) {
            seen += 1;
            assert!((route.length_m - r.distance_m).abs() <= 0.10 * r.distance_m + 1.0, "{}", route.length_m);
            sense_ok += usize::from((signed_area(&e, &r, route) < 0.0) == draw.clockwise);
        }
    }
    assert!(seen >= 10, "trop peu de triangles ({seen})");
    assert!(sense_ok * 10 >= seen * 9, "sens tiré : {sense_ok} sur {seen}");
}

fn scenario_graph_dense_for_tests() -> MemSource {
    crate::tests_reference::scenario_graph_dense(5)
}

/// Test 9 : la part de `tri_first` suit `tri_q8 / 256`, ρ* reste entre P10 et P90, et le tirage de la figure ne touche
/// pas la suite de la lentille (test 2 : `FIGURE_SALT` et le générateur à part).
#[test]
fn the_figure_draw_follows_the_mix() {
    let o = Organic {
        mix: FigureMix { tri_q8: 115, rd_tri_milli: [230, 270, 318, 360, 420] },
        tune: OrganicTune::default(),
    };
    let n = 10_000;
    let (mut tri, mut cw) = (0, 0);
    for seed in 0..n {
        let d = crate::figures::draw(SplitMix64(seed).next_u64(), &o);
        tri += usize::from(d.tri_first);
        cw += usize::from(d.clockwise);
        assert!((0.230..=0.420).contains(&d.rho), "{}", d.rho);
    }
    let p = tri as f64 / n as f64;
    assert!((p - 115.0 / 256.0).abs() < 0.02, "part de triangles {p}");
    assert!((cw as f64 / n as f64 - 0.5).abs() < 0.02);
    // Aucun triangle : jamais `tri_first`.
    let none = Organic { mix: FigureMix::LENS_ONLY, tune: OrganicTune::default() };
    assert!((0..1000).all(|s| !crate::figures::draw(s, &none).tri_first));
    // Au-delà du minimum, le multiplicateur plafonne à 256.
    let boosted = Organic {
        mix: FigureMix { tri_q8: 200, ..test_mix() },
        tune: OrganicTune { tri_boost_q8: 512, ..OrganicTune::default() },
    };
    assert_eq!(crate::figures::tri_q8_eff(&boosted), 256);
}

/// Test 8 : un sommet au bout d'un peigne (des carrefours tous les 500 m sur l'aller, une impasse au bout) : le
/// parcours repart par où il est venu, c'est un épi ; P' est le dernier carrefour à 500 m au moins avant le sommet ;
/// sans carrefour, pas de réparation possible.
#[test]
fn a_comb_apex_is_a_spike_and_is_repaired_at_the_last_junction_500_m_back() {
    for (junctions, expect_repair_at) in [(true, Some(1500.0)), (false, None)] {
        let mut s = MemSource::default();
        let mut way = 10i64;
        for k in 0..=4i64 {
            pt(&mut s, 1 + k, 0.0, k as f64 * 500.0);
        }
        for k in 0..4i64 {
            way += 1;
            s.way(way, &[1 + k, 2 + k], &tags("tertiary", &[]));
        }
        if junctions {
            for k in 1..=3i64 {
                pt(&mut s, 100 + k, 40.0, k as f64 * 500.0);
                way += 1;
                s.way(way, &[1 + k, 100 + k], &tags("tertiary", &[]));
            }
        }
        let e = engine_of(&mut s);
        let d = 6000.0;
        let r = request(ll(0.0, 0.0), d, Mode::Discovery);
        let win = e.full_window(0);
        let v = &win.v;
        let w = Weighter::new(v, None, RideType::Road, Mode::Discovery, 0.5, DEFAULT_OFF_TEAM_COST_Q8);
        let s_snap = e.snap(&win, r.start, &|_| true, 0).unwrap().unwrap();
        let sa = make_anchor(&e, v, &w, s_snap);
        let disc = Disc::new(r.start.to_e7(), r.start.to_e7(), d, true);
        let (forb, esc) = (Forbidden::default(), Cell::new(false));
        let cx = Ctx { v, w: &w, disc: &disc, s: &sa, e: &sa, forb: &forb, esc: &esc };
        let mut ws = Workspace::new();
        let ViewWs { t1, t2, .. } = &mut ws.views[0];
        tree_forward(&cx, t1, &mut stop()).unwrap();
        tree_backward(&cx, t2, &mut stop()).unwrap();
        let (t1, t2) = (&*t1, &*t2);
        let apex = *t1
            .reached
            .iter()
            .max_by(|&&a, &&b| disc.xy_of(v.pos[a as usize]).0.total_cmp(&disc.xy_of(v.pos[b as usize]).0))
            .unwrap();
        assert!((disc.xy_of(v.pos[apex as usize]).0 - 2000.0).abs() < 5.0);
        let arr = t1.path_to(t1.node_best(apex).unwrap());
        let mut dep = t2.path_to(t2.node_best(apex).unwrap());
        dep.reverse();
        let o = Organic { mix: test_mix(), tune: OrganicTune::default() };
        let (s_snap2, e_snap) = (s_snap, s_snap);
        let fx = crate::figures::Fixed {
            e: &e,
            win: &win,
            team: None,
            r: &r,
            cx: &cx,
            s_snap: &s_snap2,
            e_snap: &e_snap,
            o: &o,
            d,
            tail_dm: 0,
        };
        assert!(crate::figures::is_spike(&fx, &arr, &dep), "le retour par la même route est un épi");
        let prefix = crate::figures::repair_prefix(&fx, &arr);
        match expect_repair_at {
            Some(x) => {
                let i = prefix.expect("un carrefour à 500 m ou plus");
                let at = disc.xy_of(v.pos[v.tgt(arr[i - 1]) as usize]).0;
                assert!((at - x).abs() < 5.0, "P' à {at} m, attendu {x}");
            }
            None => assert!(prefix.is_none(), "sans carrefour, pas de réparation : la paire est jetée"),
        }
        // Un départ qui s'écarte du sens d'arrivée n'est pas un épi : on continue tout droit après le carrefour.
        let straight = [arr[0]];
        assert!(!crate::figures::is_spike(&fx, &straight, &arr[..]), "tout droit n'est pas un épi");
    }
}

/// Test 10 : le choix. Premier : le meilleur de la figure tirée, sinon de toutes ; les suivants préfèrent une figure
/// absente à note voisine.
#[test]
fn the_choice_prefers_the_drawn_figure_then_an_absent_one_at_a_close_score() {
    let e = engine_of(&mut corridor_graph());
    let scored = |edge: u32, family: Family, score: f64| {
        let route = Route {
            start: crate::Position { edge, offset_m: 0.0 },
            end: crate::Position { edge, offset_m: 0.0 },
            steps: vec![Step { base_edge: edge, reverse: false }],
            length_m: 1.0,
            team_m: 0.0,
            bridged_m: 0.0,
            shared_m: 0.0,
            crossings: 0,
            unpaved_m: 0.0,
            presumed_surface_m: 0.0,
            bearing_deviation_deg: None,
            familiarity: 0.5,
            family,
        };
        crate::generate::Scored { key: route.steps.clone(), route, score }
    };
    let (a, b, c, d) = (
        edge_between(&e, 1000, 1001),
        edge_between(&e, 1001, 1002),
        edge_between(&e, 2000, 2001),
        edge_between(&e, 2001, 2002),
    );
    let pool = || {
        vec![
            scored(a, Family::Turnaround, 0.5),
            scored(b, Family::Turnaround, 0.6),
            scored(c, Family::Triangle, 1.2),
            scored(d, Family::Triangle, 3.0),
        ]
    };
    let fams = |routes: Vec<Route>| routes.iter().map(|r| r.family).collect::<Vec<_>>();
    // Un triangle tiré : il passe en premier même s'il note moins bien ; le second est la lentille, la figure absente.
    let got = fams(crate::figures::choose(&e, pool(), true, 1.0));
    assert_eq!(got[0], Family::Triangle);
    // Une lentille tirée : première, puis la figure absente à note voisine (1,2 ≤ 0,6 + 1,0), puis le reste.
    let got = crate::figures::choose(&e, pool(), false, 1.0);
    assert_eq!(fams(got.clone())[..2], [Family::Turnaround, Family::Triangle]);
    assert_eq!(got[0].steps[0].base_edge, a);
    // Note trop éloignée (écart de 0,1) : la figure absente n'est pas préférée.
    let got = fams(crate::figures::choose(&e, pool(), false, 0.1));
    assert_eq!(got[..2], [Family::Turnaround, Family::Turnaround]);
    // Pas de triangle tiré mais aucun en pool : la lentille.
    let only_lens = vec![scored(a, Family::Turnaround, 0.5)];
    assert_eq!(crate::figures::choose(&e, only_lens, true, 1.0).len(), 1);
    let _ = (Mode::Strict, ZOOMS);
}

/// Les constantes générales (écrites par `shape-common`) : MTB sans triangle, les deux autres types avec une part et une
/// loi croissantes.
#[test]
fn the_general_mix_has_triangles_for_road_and_gravel_and_none_for_mtb() {
    assert_eq!(FigureMix::general(RideType::Mtb), FigureMix::LENS_ONLY);
    // Les constantes sont la répartition des comptes : ROAD 1 098 triangles sur 1 936 boucles à k ≤ 2.
    let counts = crate::shape_common::SHAPE_COUNTS;
    for (ty, ride) in [RideType::Road, RideType::Gravel].into_iter().enumerate() {
        let want =
            (256.0 * f64::from(counts[ty][1]) / f64::from(counts[ty][0] + counts[ty][1])).round() as u16;
        assert_eq!(FigureMix::general(ride).tri_q8, want);
    }
    for ride in [RideType::Road, RideType::Gravel] {
        let m = FigureMix::general(ride);
        assert!(m.tri_q8 > 64 && m.tri_q8 < 192, "{ride:?} : {}", m.tri_q8);
        assert!(m.rd_tri_milli.windows(2).all(|w| w[0] < w[1]), "{ride:?} : {:?}", m.rd_tri_milli);
    }
}
