//! Équivalence du moteur fenêtré (note lot 6, §14, points 1 à 6) : toute fenêtre qui contient les tuiles nécessaires
//! rend les sorties figées de la référence ; les données d'équipe de la zone sont celles du graphe entier.

use std::cell::RefCell;
use std::collections::BTreeMap;

use ord_graph::format::DIR_BIT;
use ord_graph::grid::tile_key;
use ord_graph::util::SplitMix64;

use crate::engine::Engine;
use crate::generate::generate_with_diagnostics_in;
use crate::team::{TeamCtx, TeamData, WindowTeam};
use crate::tests::{engine_zoomed, overlay_of};
use crate::tests_reference::{
    assert_matches_fixture, assert_matches_reference, fixture_path_192, scenario_graph, scenario_graph_dense,
    scenario_params, scenario_params_192, scenario_requests, scenario_requests_sized, scenario_rows,
    REQUESTS, ZOOMS,
};
use crate::window::{tiles_union, Window};
use crate::{Diagnostics, Failure, Generation, Request, RideType, Workspace};

fn all_tiles(e: &Engine<Vec<u8>>, vi: usize) -> Vec<u32> {
    let v = &e.views_files[vi];
    let mut t: Vec<u32> = (0..v.n_cells()).map(|c| tile_key(v.cell_xy(c).0, v.cell_xy(c).1)).collect();
    t.sort_unstable();
    t
}

type Pick<'a> = &'a dyn Fn(&Engine<Vec<u8>>, &crate::WindowPlan) -> Vec<u32>;

/// Génère dans la fenêtre de `tiles` (réunie aux tuiles nécessaires), équipe comprise.
fn run_in(
    e: &Engine<Vec<u8>>,
    team: Option<&TeamData>,
    r: &Request,
    ws: &mut Workspace,
    pick: Pick<'_>,
) -> (Result<Generation, Failure>, Diagnostics) {
    let plan = e.plan(r, 500.0);
    let tiles = tiles_union(&pick(e, &plan), &plan.need);
    let win: Window = e.build_window(plan.view, &tiles, &mut || false).unwrap();
    let wt = team.map(|t| WindowTeam::build(e, &win, t));
    let ctx = team.zip(wt.as_ref()).map(|(data, win)| TeamCtx { data, win });
    generate_with_diagnostics_in(e, &win, ctx, r, ws, &mut || false)
}

#[test]
fn the_window_of_all_tiles_is_the_flat_engine() {
    assert_matches_reference(&|e, team, r, ws| {
        let win = e.full_window(r.ride_type.view_index());
        let wt = team.map(|t| WindowTeam::build(e, &win, t));
        let ctx = team.zip(wt.as_ref()).map(|(data, win)| TeamCtx { data, win });
        generate_with_diagnostics_in(e, &win, ctx, r, ws, &mut || false)
    });
}

#[test]
fn a_neutral_taste_keeps_the_frozen_reference_bit_for_bit() {
    // Le goût neutre passe par l'arithmétique du goût (`taste_weight`) et rend la référence figée, sans bénédiction :
    // les trois modes, la découverte avec équipe comprise.
    // Contre la référence de 1.9.2 (poids de base neutre), jamais re-bénie.
    assert_matches_fixture(
        &|e, team, r, ws| {
            let win = e.full_window(r.ride_type.view_index());
            let wt = team.map(|t| WindowTeam::build_with_taste(e, &win, t, Some(&crate::Taste::neutral())));
            let ctx = team.zip(wt.as_ref()).map(|(data, win)| TeamCtx { data, win });
            generate_with_diagnostics_in(e, &win, ctx, r, ws, &mut || false)
        },
        scenario_params_192,
        &fixture_path_192(),
        false,
    );
}

/// Test 3 de la spécification du 3b (§14.11) : la table commune comme poids de base et la même table passée au crochet
/// donnent les mêmes agrégats, les mêmes `w_osm` et les mêmes parcours, sur le scénario de référence.
#[test]
fn the_common_table_as_base_weight_equals_the_common_table_through_the_hook() {
    for zoom in ZOOMS {
        let e_common = engine_zoomed(&mut scenario_graph(7), zoom, scenario_params());
        let e_neutral = engine_zoomed(&mut scenario_graph(7), zoom, scenario_params_192());
        let common = crate::Taste::common();
        let (t_c, t_n) = (
            TeamData::build(&e_common, &overlay_of(&e_common, &scenario_rows(&e_common, 11))).unwrap(),
            TeamData::build(&e_neutral, &overlay_of(&e_neutral, &scenario_rows(&e_neutral, 11))).unwrap(),
        );
        let (mut ws_a, mut ws_b, mut ws_c) = (Workspace::new(), Workspace::new(), Workspace::new());
        for vi in 0..2 {
            let (win_c, win_n) = (e_common.full_window(vi), e_neutral.full_window(vi));
            let plain = WindowTeam::build(&e_common, &win_c, &t_c);
            let through_hook = WindowTeam::build_with_taste(&e_neutral, &win_n, &t_n, Some(&common));
            let hook_on_common = WindowTeam::build_with_taste(&e_common, &win_c, &t_c, Some(&common));
            for ride in RideType::ALL {
                if !crate::engine::rides_of_view(vi).contains(&ride) {
                    continue;
                }
                // (a) mêmes agrégats ; `w_osm` du moteur C = poids au goût de la table C du moteur neutre.
                let (a, b) = (plain.chains_of(ride), through_hook.chains_of(ride));
                assert_eq!(a.len(), b.len());
                for (x, y) in a.iter().zip(b) {
                    assert_eq!((x.a, x.b, x.z, x.strict), (y.a, y.b, y.z, y.strict), "{ride:?}");
                }
                let (w, _) = through_hook.taste_of(ride).unwrap();
                assert_eq!(&win_c.v.w_osm[ride.index()][..], w, "z{zoom} {ride:?} : w_osm = w_taste");
                // (b) moteur C : `build(None)` et le crochet à C sont la même chose.
                let (w2, _) = hook_on_common.taste_of(ride).unwrap();
                assert_eq!(w, w2);
                for (x, y) in a.iter().zip(hook_on_common.chains_of(ride)) {
                    assert_eq!((x.a, x.b, x.z, x.strict), (y.a, y.b, y.z, y.strict));
                }
            }
            // Les mêmes sorties, requête par requête.
            for r in scenario_requests(13, 24).iter().filter(|r| r.ride_type.view_index() == vi) {
                let run =
                    |e: &Engine<Vec<u8>>, win: &Window, wt: &WindowTeam, t: &TeamData, ws: &mut Workspace| {
                        let ctx = Some(TeamCtx { data: t, win: wt });
                        format!(
                            "{:?}",
                            generate_with_diagnostics_in(e, win, ctx, r, ws, &mut || false)
                                .0
                                .map(|g| g.routes)
                        )
                    };
                let x = run(&e_common, &win_c, &plain, &t_c, &mut ws_a);
                let y = run(&e_neutral, &win_n, &through_hook, &t_n, &mut ws_b);
                let z = run(&e_common, &win_c, &hook_on_common, &t_c, &mut ws_c);
                assert_eq!(x, y, "(a) z{zoom}");
                assert_eq!(x, z, "(b) z{zoom}");
            }
        }
    }
}

/// Test 3 (c) : sans équipe, `w_osm` d'une fenêtre du moteur C est la somme des poids au goût de C de ses liens.
#[test]
fn without_a_team_the_window_weights_are_the_common_table() {
    let e = engine_zoomed(&mut scenario_graph(7), 11, scenario_params());
    let common = crate::Taste::common();
    for vi in 0..2 {
        let win = e.full_window(vi);
        for ride in RideType::ALL {
            if !crate::engine::rides_of_view(vi).contains(&ride) {
                continue;
            }
            for c in 0..win.n_chains() {
                let sum: u64 = win
                    .v
                    .links(c as u32)
                    .iter()
                    .map(|l| {
                        let (a, len) = e.attrs_len(l & !DIR_BIT);
                        u64::from(crate::cost::taste_weight(ride, a, len, &common.q8[ride.index()]))
                    })
                    .sum();
                assert_eq!(u64::from(win.v.w_osm[ride.index()][c]), sum.min(u64::from(u32::MAX / 8)));
            }
        }
    }
}

/// Test 4 : `base_weight` à table neutre est `edge_weight`, sur les attributs de la grille de référence.
#[test]
fn the_base_weight_with_a_neutral_table_is_edge_weight() {
    let e = engine_zoomed(&mut scenario_graph(7), 11, scenario_params_192());
    for edge in 0..e.base().header().edges as u32 {
        let (a, len) = e.attrs_len(edge);
        for ride in RideType::ALL {
            for l in [len, 1, 7, 999] {
                assert_eq!(e.base_weight(ride, a, l), crate::cost::edge_weight(ride, a, l));
            }
        }
    }
}

#[test]
fn the_class_window_gives_the_same_routes_as_the_needed_tiles() {
    assert_matches_reference(&|e, team, r, ws| run_in(e, team, r, ws, &|_, p| p.class.clone()));
}

#[test]
fn any_superset_of_the_needed_tiles_gives_the_same_routes_and_no_escape() {
    let rng = RefCell::new(SplitMix64(99));
    assert_matches_reference(&|e, team, r, ws| {
        run_in(e, team, r, ws, &|e, p| {
            // Une part tirée de toutes les tuiles de la vue, de 0 à 100 %.
            let all = all_tiles(e, p.view);
            let share = rng.borrow_mut().below(101);
            let mut extra: Vec<u32> =
                all.into_iter().filter(|_| rng.borrow_mut().below(100) < share).collect();
            extra.sort_unstable();
            extra
        })
    });
}

/// Moteur z16 sur la grille dense : des fenêtres de quelques dizaines de tuiles sur 272.
fn dense() -> Engine<Vec<u8>> {
    engine_zoomed(&mut scenario_graph_dense(5), 16, scenario_params())
}

fn dense_requests() -> Vec<Request> {
    scenario_requests_sized(21, 60, 800.0, 2200)
}

#[test]
fn on_a_dense_z16_grid_any_window_gives_the_answer_of_the_whole_graph() {
    let e = dense();
    let team = TeamData::build(&e, &overlay_of(&e, &scenario_rows(&e, 4))).unwrap();
    let totals = [e.full_window(0).tiles().len(), e.full_window(1).tiles().len()];
    let mut rng = SplitMix64(5);
    let (mut partial, mut ok, mut compared) = (0, 0, 0);
    let mut ws = Workspace::new();
    for r in dense_requests() {
        let full = e.full_window(r.ride_type.view_index());
        let wt = WindowTeam::build(&e, &full, &team);
        let ctx = Some(TeamCtx { data: &team, win: &wt });
        let want = format!("{:?}", generate_with_diagnostics_in(&e, &full, ctx, &r, &mut ws, &mut || false));
        let plan = e.plan(&r, 500.0);
        let inside = plan.need.iter().filter(|k| full.tiles().binary_search(k).is_ok()).count();
        partial += usize::from(inside * 2 < totals[plan.view]);
        let extra: Vec<u32> = all_tiles(&e, plan.view).into_iter().filter(|_| rng.below(100) < 25).collect();
        for tiles in [plan.need.clone(), plan.class.clone(), tiles_union(&plan.need, &extra)] {
            let win = e.build_window(plan.view, &tiles, &mut || false).unwrap();
            let wt = WindowTeam::build(&e, &win, &team);
            let ctx = Some(TeamCtx { data: &team, win: &wt });
            let got = generate_with_diagnostics_in(&e, &win, ctx, &r, &mut ws, &mut || false);
            assert!(!matches!(got.0, Err(Failure::WindowEscape)), "échappement");
            assert_eq!(format!("{got:?}"), want, "D {} mode {:?}", r.distance_m, r.mode);
            compared += 1;
        }
        ok += usize::from(want.contains("routes: [Route"));
    }
    assert!(partial * 3 >= 60, "{partial}/60 requêtes à fenêtre partielle");
    assert!(ok >= 5 && compared == 180, "trop peu de succès ({ok})");
}

#[test]
fn a_window_that_is_too_small_raises_the_guard_instead_of_answering() {
    let e = dense();
    let mut seen = 0;
    for r in dense_requests().iter().filter(|r| r.end.is_none() && r.distance_m > 1500.0) {
        let r = discovery(r);
        let plan = e.plan(&r, 0.0);
        let run = |tiles: &[u32]| {
            let win = e.build_window(plan.view, tiles, &mut || false).unwrap();
            generate_with_diagnostics_in(&e, &win, None, &r, &mut Workspace::new(), &mut || false).0
        };
        let honest = run(&plan.need);
        // La moitié des tuiles nécessaires seulement.
        let half: Vec<u32> = plan.need.iter().copied().step_by(2).collect();
        match (honest, run(&half)) {
            (Ok(h), Err(Failure::WindowEscape)) => {
                assert!(!h.routes.is_empty());
                seen += 1;
            }
            (Ok(h), Ok(g)) => assert_eq!(
                format!("{:?}", g.routes),
                format!("{:?}", h.routes),
                "une fenêtre incomplète ne doit jamais changer la réponse sans lever le garde-fou"
            ),
            _ => {}
        }
    }
    assert!(seen > 0, "le garde-fou n'a jamais été levé : le test ne prouve rien");
}

fn discovery(r: &Request) -> Request {
    Request { mode: crate::Mode::Discovery, ..r.clone() }
}

#[test]
fn the_team_data_of_the_zone_is_the_team_data_of_the_whole_graph() {
    let mut zones_partial = 0;
    for zoom in ZOOMS {
        for seed in [1u64, 2, 3, 11] {
            let e = engine_zoomed(&mut scenario_graph(7), zoom, scenario_params());
            let ov = overlay_of(&e, &scenario_rows(&e, seed));
            let (zone, whole) =
                (TeamData::build(&e, &ov).unwrap(), TeamData::build_on(&e, &ov, true).unwrap());
            for ride in crate::RideType::ALL {
                let (a, b) = (zone.of(ride), whole.of(ride));
                assert_eq!(a.edges, b.edges, "arêtes z{zoom} seed {seed} {ride:?}");
                assert_eq!(a.strict_len, b.strict_len, "composantes strictes z{zoom} seed {seed} {ride:?}");
                assert_eq!(
                    (
                        a.team_edges,
                        a.team_m.to_bits(),
                        a.bridged_m.to_bits(),
                        a.outside_m.0.to_bits(),
                        a.outside_m.1.to_bits()
                    ),
                    (
                        b.team_edges,
                        b.team_m.to_bits(),
                        b.bridged_m.to_bits(),
                        b.outside_m.0.to_bits(),
                        b.outside_m.1.to_bits()
                    ),
                    "statistiques"
                );
                assert!(!a.edges.is_empty());
            }
            zones_partial += usize::from(zoom == 16);
        }
    }
    assert!(zones_partial > 0);
}

#[test]
fn the_window_team_of_a_partial_window_is_the_flat_one_chain_by_chain() {
    let e = engine_zoomed(&mut scenario_graph(7), 16, scenario_params());
    let team = TeamData::build(&e, &overlay_of(&e, &scenario_rows(&e, 11))).unwrap();
    let full = e.full_window(0);
    let full_wt = WindowTeam::build(&e, &full, &team);
    let r = &scenario_requests(13, REQUESTS)[3];
    let plan = e.plan(r, 0.0);
    let tiles = tiles_union(&plan.need, &all_tiles(&e, 0)[..30]);
    let win = e.build_window(0, &tiles, &mut || false).unwrap();
    let wt = WindowTeam::build(&e, &win, &team);
    assert!(win.n_chains() < full.n_chains());
    let mut compared = 0;
    for (lc, &g) in win.v.gid.iter().enumerate() {
        // Dans la fenêtre de toutes les tuiles, l'id local est l'id global.
        let (a, b) =
            (wt.chains_of(crate::RideType::Road)[lc], full_wt.chains_of(crate::RideType::Road)[g as usize]);
        assert_eq!((a.a, a.b, a.z, a.strict), (b.a, b.b, b.z, b.strict));
        compared += usize::from(a.a > 0);
    }
    assert!(compared > 0);
}

#[test]
fn an_admitted_degree_is_the_view_degree_or_two_and_no_base_edge_is_a_loop() {
    for zoom in ZOOMS {
        let e = engine_zoomed(&mut scenario_graph(7), zoom, scenario_params());
        let g = e.base();
        for e_i in 0..g.header().edges as u32 {
            let (a, b) = g.edge_ends(e_i);
            assert_ne!(a, b, "arête de base bouclée");
        }
        for vi in 0..2 {
            let win = e.full_window(vi);
            // Force brute : arêtes de base vivantes autour de chaque nœud de base.
            let mut brute: BTreeMap<u32, usize> = BTreeMap::new();
            for c in 0..win.v.n_chains as u32 {
                for l in win.v.links(c) {
                    let (a, b) = g.edge_ends(l & !DIR_BIT);
                    *brute.entry(a).or_default() += 1;
                    *brute.entry(b).or_default() += 1;
                }
            }
            assert!(!brute.is_empty());
            for (node, want) in brute {
                let got = match e.views_files[vi].view_node_of_base(node) {
                    None => 2,
                    Some(vn) => usize::from(win.v.deg[win.v.ngid.binary_search(&vn).unwrap()]),
                };
                assert_eq!(got, want, "nœud de base {node}, vue {vi}, z{zoom}");
            }
        }
    }
}

#[test]
fn a_workspace_reused_after_a_bigger_window_gives_the_same_answer() {
    let e = engine_zoomed(&mut scenario_graph(7), 16, scenario_params());
    let reqs: Vec<Request> = scenario_requests(13, REQUESTS)
        .into_iter()
        .filter(|r| r.ride_type == crate::RideType::Road)
        .collect();
    let mut shared = Workspace::new();
    for r in reqs.iter().take(12) {
        let r = discovery(r);
        let big = e.full_window(0);
        let _ = generate_with_diagnostics_in(&e, &big, None, &r, &mut shared, &mut || false);
        let plan = e.plan(&r, 0.0);
        let small = e.build_window(0, &plan.need, &mut || false).unwrap();
        let reused = generate_with_diagnostics_in(&e, &small, None, &r, &mut shared, &mut || false);
        let fresh = generate_with_diagnostics_in(&e, &small, None, &r, &mut Workspace::new(), &mut || false);
        assert_eq!(format!("{reused:?}"), format!("{fresh:?}"));
    }
}

/// Grille de 14 × 14 carrefours de 140 m et, au coin sud-ouest, une route de campagne de 4 km (une chaîne, ses nœuds
/// intermédiaires sont contractés) qui s'éloigne vers l'est puis ne mène nulle part.
fn grid_with_a_long_chain() -> ord_graph::build::MemSource {
    use crate::tests::{pt, tags};
    let n = 14usize;
    let mut s = ord_graph::build::MemSource::default();
    let id = |i: usize, j: usize| (1 + i * n + j) as i64;
    for i in 0..n {
        for j in 0..n {
            pt(&mut s, id(i, j), i as f64 * 140.0, j as f64 * 140.0);
        }
    }
    for i in 0..n {
        let row: Vec<i64> = (0..n).map(|j| id(i, j)).collect();
        let col: Vec<i64> = (0..n).map(|j| id(j, i)).collect();
        s.way(1000 + i as i64, &row, &tags("residential", &[]));
        s.way(2000 + i as i64, &col, &tags("residential", &[]));
    }
    // 40 nœuds tous les 100 m, vers l'est depuis le coin sud-est de la grille.
    let mut ids = vec![id(0, n - 1)];
    for k in 1..=40 {
        let nid = 10_000 + k as i64;
        pt(&mut s, nid, 0.0, (n - 1) as f64 * 140.0 + k as f64 * 100.0);
        ids.push(nid);
    }
    s.way(5000, &ids, &tags("tertiary", &[]));
    s
}

#[test]
fn a_chain_longer_than_the_margin_that_leaves_the_disc_is_loaded_whole_and_the_answer_is_the_same() {
    let e = engine_zoomed(&mut grid_with_a_long_chain(), 16, scenario_params());
    let lmax = e.longest_chain_m(0);
    assert!(lmax > 3_500.0, "la route de campagne est une seule chaîne ({lmax} m)");
    let full = e.full_window(0);
    let mut compared = 0;
    // Des départs sur la grille, près de la route de campagne, et sur elle ; D court, le disque la quitte.
    for (north, east) in
        [(0.0, 1700.0), (140.0, 1500.0), (10.0, 1900.0), (700.0, 900.0), (0.0, 2500.0), (60.0, 3500.0)]
    {
        for d in [1200.0, 2000.0] {
            let r = Request {
                start: crate::tests::ll(north, east),
                end: None,
                bearing: None,
                distance_m: d,
                ride_type: crate::RideType::Road,
                mode: crate::Mode::Discovery,
                familiarity: 0.5,
                off_team_cost_q8: 768,
                seed: 3,
            };
            let want =
                generate_with_diagnostics_in(&e, &full, None, &r, &mut Workspace::new(), &mut || false);
            let plan = e.plan(&r, 0.0);
            let win = e.build_window(0, &plan.need, &mut || false).unwrap();
            assert!(win.n_chains() <= full.n_chains());
            let got = generate_with_diagnostics_in(&e, &win, None, &r, &mut Workspace::new(), &mut || false);
            assert!(!matches!(got.0, Err(Failure::WindowEscape)));
            assert_eq!(format!("{got:?}"), format!("{want:?}"), "départ ({north}, {east}), D {d}");
            compared += 1;
        }
    }
    assert_eq!(compared, 12);
}
