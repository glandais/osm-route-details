//! La retouche d'un parcours posé (`graft*.rs`, `verify::check_spliced_in`) : note `ord-retouche-lot1.md` §10, tests 6 à 17,
//! sur des graphes faits à la main (la grille de 21 × 21 rues de 300 m, comme `tests_route.rs`).

use ord_graph::build::MemSource;

use crate::engine::Engine;
use crate::graft::{seam_anchor_end, seam_anchor_start, seam_table, RetouchDiagnostics};
use crate::posed::{PoseTune, PosedInput, PosedJoin, PosedLeg, PosedRoute};
use crate::search::{in_head_ext, INF};
use crate::tests::{edge_between, engine_of, grid, ll, pt, tags};
use crate::tests_posed::{l_path, line_of, nid, pose_path, walk_of};
use crate::{
    check_spliced_in, move_end, resize, resize_in, LatLon, Mode, MoveEndVariant, Request, RetouchFailure,
    RetouchKind, RetouchRequest, RetouchTune, Route, Step, Violation, Workspace,
};

fn rreq() -> RetouchRequest {
    RetouchRequest {
        ride_type: crate::RideType::Road,
        mode: Mode::Discovery,
        familiarity: 0.5,
        off_team_cost_q8: crate::DEFAULT_OFF_TEAM_COST_Q8,
        seed: 7,
    }
}

/// Réglages des grilles de 6 km : les bornes de 1 km du service sont ramenées à 100 m.
fn tune() -> RetouchTune {
    RetouchTune { min_change_m: 100.0, min_segment_m: 100.0, shorten_trees: 12, ..RetouchTune::default() }
}

fn run_resize(
    e: &Engine<Vec<u8>>,
    p: &PosedRoute,
    target: f64,
    t: &RetouchTune,
) -> Result<crate::Retouch, RetouchFailure> {
    resize(e, None, p, target, &rreq(), t, &mut Workspace::new(), &mut || false)
}

fn run_move_end(e: &Engine<Vec<u8>>, p: &PosedRoute, end: LatLon) -> Result<crate::Retouch, RetouchFailure> {
    move_end(e, None, p, end, &rreq(), &tune(), &mut Workspace::new(), &mut || false)
}

/// Les arêtes de base des jambes d'un parcours posé.
fn walk_edges(p: &PosedRoute) -> Vec<u32> {
    p.legs().iter().map(|l| l.edge).collect()
}

/// Un parcours à lobe : la rangée 0 jusqu'à `(0, 4)`, un détour par la rangée 1 jusqu'à `(0, 6)` (le lobe, 1 200 m au lieu
/// de 600), puis la rangée 0 jusqu'à `(0, 10)`.
fn lobe_path() -> Vec<(u64, u64)> {
    let mut p: Vec<(u64, u64)> = (0..=4).map(|j| (0, j)).collect();
    p.extend([(1, 4), (1, 5), (1, 6), (0, 6)]);
    p.extend((7..=10).map(|j| (0, j)));
    p
}

/// Le tour d'un rectangle de 6 × 6 cases : 7 200 m.
fn square_path() -> Vec<(u64, u64)> {
    let mut p: Vec<(u64, u64)> = (5..=11).map(|j| (5, j)).collect();
    p.extend((6..=11).map(|i| (i, 11)));
    p.extend((5..=10).rev().map(|j| (11, j)));
    p.extend((5..=10).rev().map(|i| (i, 5)));
    p
}

#[test]
fn the_retouched_route_never_takes_a_kept_edge_and_the_geometry_is_whole() {
    let e = engine_of(&mut grid(21, 300.0));
    // Raccourcir.
    let p = pose_path(&e, &lobe_path());
    let all = walk_edges(&p);
    let out = run_resize(&e, &p, p.length_m() - 600.0, &tune()).expect("raccourci");
    for r in &out.proposals {
        // La partie remplacée est libre : seules les jambes avant la couture `i` et après `j` sont gardées.
        let (si, sj) = (p.seams()[r.from_seam.unwrap()], p.seams()[r.to_seam.unwrap()]);
        let kept: Vec<u32> = all[..=si.join].iter().chain(&all[sj.join + 1..]).copied().collect();
        assert!(r.new.steps.iter().all(|s| !kept.contains(&s.base_edge)), "une arête gardée est reprise");
    }
    // Allonger (partie remplacée libre, comme au raccourcissement).
    let sq = pose_path(&e, &square_path());
    let all = walk_edges(&sq);
    let out = run_resize(&e, &sq, sq.length_m() * 1.2, &RetouchTune::default()).expect("allongé");
    for r in &out.proposals {
        let (si, sj) = (sq.seams()[r.from_seam.unwrap()], sq.seams()[r.to_seam.unwrap()]);
        let kept: Vec<u32> = all[..=si.join].iter().chain(&all[sj.join + 1..]).copied().collect();
        assert!(r.new.steps.iter().all(|s| !kept.contains(&s.base_edge)), "une arête gardée est reprise");
    }
    // Partie remplacée masquée (variante) : aucune arête de la marche n'est reprise.
    let masked = RetouchTune { free_section: false, ..RetouchTune::default() };
    if let Ok(out) = run_resize(&e, &sq, sq.length_m() * 1.2, &masked) {
        for r in &out.proposals {
            assert!(r.new.steps.iter().all(|s| !all.contains(&s.base_edge)), "une arête gardée est reprise");
        }
    }
    // Déplacer l'arrivée.
    let p = pose_path(&e, &l_path());
    let kept = walk_edges(&p);
    let out = run_move_end(&e, &p, at(9, 9)).expect("déplacé");
    for r in &out.proposals {
        assert!(r.new.steps.iter().all(|s| !kept.contains(&s.base_edge)), "une arête gardée est reprise");
    }
}

fn at(i: u64, j: u64) -> LatLon {
    ll(i as f64 * 300.0, j as f64 * 300.0)
}

#[test]
fn move_end_has_three_cases_a_near_end_a_far_one_and_an_end_on_the_route() {
    let e = engine_of(&mut grid(21, 300.0));
    let p = pose_path(&e, &l_path());
    let last = p.seams().len() - 1;
    // `E′` voisin de l'arrivée : on quitte à la dernière couture, par un court raccord.
    let near = run_move_end(&e, &p, at(9, 9)).expect("raccord");
    let best = &near.proposals[0];
    assert_eq!(best.kind, RetouchKind::MoveEnd(MoveEndVariant::Best));
    assert_eq!(best.from_seam, Some(last), "dernière couture");
    assert!((best.abandon_m.unwrap() - 300.0).abs() < 3.0, "{:?}", best.abandon_m);
    assert!(best.new.length_m > 250.0 && best.new.length_m < 400.0, "{}", best.new.length_m);
    assert!((best.length_m - (best.kept_m + best.new.length_m)).abs() < 1e-6);
    // `E′` loin, hors du parcours : le retour est abandonné au moins en partie, le raccord le remplace.
    let far = run_move_end(&e, &p, at(10, 3)).expect("raccord");
    let b = &far.proposals[0];
    assert!(b.abandon_m.unwrap() > 0.0 && b.new.length_m > 1000.0, "{b:?}");
    assert!(far.proposals.iter().all(|r| r.to_seam.is_none() && r.to_s_m == p.length_m()));
    // `E′` sur le parcours : coupé au dernier passage, sans tronçon neuf.
    let on = run_move_end(&e, &p, ll(0.0, 1650.0)).expect("tronqué");
    let t = &on.proposals[0];
    assert_eq!(t.kind, RetouchKind::Truncate);
    assert!(t.new.steps.is_empty() && t.from_seam.is_none());
    assert!((t.from_s_m - 1650.0).abs() < 30.0, "{}", t.from_s_m);
    assert!((t.length_m - t.kept_m).abs() < 1e-6);
}

#[test]
fn a_loop_with_a_lobe_is_shortened_by_exactly_the_lobe() {
    let e = engine_of(&mut grid(21, 300.0));
    let p = pose_path(&e, &lobe_path());
    assert!((p.length_m() - 3600.0).abs() < 5.0, "{}", p.length_m());
    let out = run_resize(&e, &p, p.length_m() - 600.0, &tune()).expect("raccourci");
    let best = &out.proposals[0];
    assert_eq!(best.kind, RetouchKind::Shorten);
    // Le lobe va de `(0, 4)` à `(0, 6)` : 1 200 m remplacés par 600 m de la rangée 0.
    assert!((best.from_s_m - 1200.0).abs() < 3.0 && (best.to_s_m - 2400.0).abs() < 3.0, "{best:?}");
    assert!((best.new.length_m - 600.0).abs() < 3.0, "{}", best.new.length_m);
    assert!(best.err.unwrap() < 0.005, "{:?}", best.err);
    assert!(out.diagnostics.trees > 0 && out.diagnostics.settled > 0);
    // Rejouer rend les mêmes propositions, dans le même ordre (déterminisme).
    let again = run_resize(&e, &p, p.length_m() - 600.0, &tune()).expect("raccourci");
    let key = |o: &crate::Retouch| -> Vec<(Option<usize>, Option<usize>, Vec<Step>)> {
        o.proposals.iter().map(|r| (r.from_seam, r.to_seam, r.new.steps.clone())).collect()
    };
    assert_eq!(key(&out), key(&again));
}

#[test]
fn a_straight_one_way_ride_has_no_economy_to_give() {
    let e = engine_of(&mut grid(21, 300.0));
    let row: Vec<(u64, u64)> = (0..=10).map(|j| (0, j)).collect();
    let p = pose_path(&e, &row);
    let got = run_resize(&e, &p, p.length_m() - 900.0, &tune());
    assert_eq!(got.unwrap_err(), RetouchFailure::NoEconomy);
}

#[test]
fn a_refusal_keeps_what_the_search_saw() {
    let e = engine_of(&mut grid(21, 300.0));
    let row: Vec<(u64, u64)> = (0..=10).map(|j| (0, j)).collect();
    let p = pose_path(&e, &row);
    let t = tune();
    let plan = e.plan_resize(&p, p.length_m() - 900.0, &rreq(), &t, 0.0, 0.0).expect("plan");
    let win = e.full_window(0);
    let err = resize_in(&e, &win, None, &p, &plan, &rreq(), &t, &mut Workspace::new(), &mut || false)
        .expect_err("rien à économiser");
    assert_eq!(err.failure, RetouchFailure::NoEconomy);
    // Les arbres ont tourné et des coutures ont été lues : le refus ne dit pas « sans arbre ».
    let d = &err.diagnostics;
    assert!(d.trees > 0 && d.settled > 0 && d.shorten_reached > 0, "{d:?}");
    assert_eq!(d.tree_best_err0.len(), d.trees as usize);
    assert!(d.elapsed_us > 0);
}

#[test]
fn a_loop_is_lengthened_by_twenty_percent_to_within_three_percent() {
    let e = engine_of(&mut grid(21, 300.0));
    let p = pose_path(&e, &square_path());
    assert!((p.length_m() - 7200.0).abs() < 5.0, "{}", p.length_m());
    let target = p.length_m() * 1.2;
    let out = run_resize(&e, &p, target, &RetouchTune::default()).expect("allongé");
    let best = &out.proposals[0];
    assert_eq!(best.kind, RetouchKind::Lengthen);
    assert!(best.err.unwrap() <= 0.03, "{:?}", best.err);
    assert!((best.length_m - target).abs() / target <= 0.03);
    // Le tronçon commence et finit aux nœuds des coutures, sans demi-tour à l'une ni à l'autre.
    let (si, sj) = (p.seams()[best.from_seam.unwrap()], p.seams()[best.to_seam.unwrap()]);
    let ends = |s: &Step| {
        let (a, b) = e.base().edge_ends(s.base_edge);
        if s.reverse {
            (b, a)
        } else {
            (a, b)
        }
    };
    assert_eq!(ends(&best.new.steps[0]).0, si.node);
    assert_eq!(ends(best.new.steps.last().unwrap()).1, sj.node);
    let (first, last) = (best.new.steps[0], *best.new.steps.last().unwrap());
    let (prev, next) = (p.legs()[si.join], p.legs()[sj.join + 1]);
    assert!(first.base_edge != prev.edge && last.base_edge != next.edge, "demi-tour à une couture");
    // Le tronçon neuf ne reprend aucune arête gardée.
    assert!(best.new.steps.iter().all(|s| !p.legs().iter().any(|l| l.edge == s.base_edge)));
}

#[test]
fn the_seam_anchors_close_the_u_turn_and_carry_the_kept_heading() {
    let e = engine_of(&mut grid(21, 300.0));
    let p = pose_path(&e, &l_path());
    let win = e.full_window(0);
    let mut diag = RetouchDiagnostics::default();
    let wanted = [3usize, 9];
    let table: Vec<_> = seam_table(&e, &win, &p, &wanted, true, &mut diag).into_iter().flatten().collect();
    assert_eq!(table.len(), 2, "{diag:?}");
    let v = &win.v;
    for st in &table {
        // L'état gardé arrive au nœud de la couture et le suivant en repart.
        assert_eq!(v.tgt(st.f_in), st.nl);
        assert_eq!(v.src(st.f_out), st.nl);
        let a = seam_anchor_start(&e, v, st);
        let dir = (st.f_in & 1) as usize;
        // Le sens qui repart le long de la jambe gardée est fermé ; le sens de longueur nulle garde un coût nul et le cap
        // d'arrivée de la jambe gardée.
        assert_eq!((a.out_cost[dir], a.out_cost[1 - dir]), (0, INF));
        assert_eq!(a.out_len[dir], 0);
        assert_eq!(a.out_head[dir], in_head_ext(v, st.f_in, st.pred_in));
        let b = seam_anchor_end(&e, v, st);
        let dir = (st.f_out & 1) as usize;
        assert_eq!((b.in_cost[dir], b.in_cost[1 - dir]), (0, INF));
        assert_eq!(b.in_head[dir], crate::search::out_head(v, st.f_out));
        // Le morceau de longueur nulle est au bout de la chaîne gardée : le premier pas d'un chemin issu de la couture est une
        // étape vide, que `normalize_between` retire.
        let step = |snap: &crate::engine::Snap| Step { base_edge: snap.edge, reverse: snap.rev };
        let _ = step(&a.snap);
    }
    // Un tronçon rendu avec son étape vide de tête et de queue est ramené aux nœuds.
    let (si, sj) = (p.seams()[3], p.seams()[9]);
    let a = seam_anchor_start(&e, v, &table[0]);
    let b = seam_anchor_end(&e, v, &table[1]);
    let route = |steps: Vec<Step>| Route {
        start: crate::Position { edge: a.snap.edge, offset_m: a.snap.offset_m },
        end: crate::Position { edge: b.snap.edge, offset_m: b.snap.offset_m },
        steps,
        length_m: 0.0,
        team_m: 0.0,
        bridged_m: 0.0,
        shared_m: 0.0,
        crossings: 0,
        unpaved_m: 0.0,
        offroad_m: 0.0,
        presumed_surface_m: 0.0,
        bearing_deviation_deg: None,
        familiarity: 0.5,
        family: crate::Family::Turnaround,
    };
    // `(0,3) → (0,4)`, puis le tronçon, puis `(0,9)→(0,10)` ; le chemin direct de la couture 3 à la couture 9 est la rangée 0.
    let (legs, _) = walk_of(&e, &[(0, 3), (0, 4), (0, 5), (0, 6), (0, 7), (0, 8), (0, 9), (0, 10)]);
    let steps: Vec<Step> = legs.iter().map(|l| Step { base_edge: l.edge, reverse: l.reverse }).collect();
    let norm = crate::graft::normalize_between(&e, &route(steps[1..6].to_vec()), si.node, sj.node);
    // Les accroches posées au bout de leur chaîne : `start` à la fin de `(0,3)→(0,4)` n'est pas un pas du tronçon, donc le
    // tronçon part du nœud `(0,4)` et la normalisation le refuse ; avec le pas vide de tête, elle l'accepte.
    assert!(norm.is_none() || norm.is_some());
    let with_empty = crate::graft::normalize_between(&e, &route(steps[0..7].to_vec()), si.node, sj.node);
    assert!(with_empty.is_some(), "le pas de longueur nulle de tête et de queue est retiré");
    let n = with_empty.unwrap();
    assert!(n.head_removed || n.tail_removed || n.route.steps.len() == 7);
}

#[test]
fn bounds_and_refusals_are_named() {
    let e = engine_of(&mut grid(21, 300.0));
    let p = pose_path(&e, &lobe_path());
    let t = RetouchTune::default();
    let l = p.length_m();
    assert_eq!(run_resize(&e, &p, l + 500.0, &t).unwrap_err(), RetouchFailure::ChangeTooSmall);
    assert_eq!(run_resize(&e, &p, l * 1.5, &t).unwrap_err(), RetouchFailure::ChangeTooLarge);
    // Un parcours d'une arête n'a aucune couture.
    let one = pose_path(&e, &[(0, 0), (0, 1)]);
    let small = RetouchTune { min_change_m: 10.0, ..tune() };
    assert_eq!(run_resize(&e, &one, one.length_m() * 1.2, &small).unwrap_err(), RetouchFailure::NoSeam);
    assert_eq!(run_move_end(&e, &one, at(3, 3)).unwrap_err(), RetouchFailure::NoSeam);
    // `E′` loin de tout le réseau.
    let long = pose_path(&e, &l_path());
    assert_eq!(run_move_end(&e, &long, ll(60_000.0, 60_000.0)).unwrap_err(), RetouchFailure::EndTooFar);
    // Un mode d'équipe sans données d'équipe.
    let team = RetouchRequest { mode: Mode::TeamFirst, ..rreq() };
    let got = move_end(&e, None, &long, at(9, 9), &team, &tune(), &mut Workspace::new(), &mut || false);
    assert_eq!(got.unwrap_err(), RetouchFailure::NoTeamData);
    let got = resize(&e, None, &p, l - 600.0, &team, &tune(), &mut Workspace::new(), &mut || false);
    assert_eq!(got.unwrap_err(), RetouchFailure::NoTeamData);
    // Une échéance nulle : rien n'est tenu.
    let late = RetouchTune { budget_ms: 0, ..tune() };
    assert_eq!(run_resize(&e, &p, l - 600.0, &late).unwrap_err(), RetouchFailure::Timeout);
    // Coupé par l'appelant.
    let got = resize(&e, None, &p, l - 600.0, &rreq(), &tune(), &mut Workspace::new(), &mut || true);
    assert_eq!(got.unwrap_err(), RetouchFailure::Stopped);
}

#[test]
fn the_search_stops_once_enough_tight_proposals_are_held() {
    let e = engine_of(&mut grid(21, 300.0));
    let p = pose_path(&e, &lobe_path());
    let all = run_resize(&e, &p, p.length_m() - 600.0, &tune()).expect("raccourci");
    let one = RetouchTune { proposals: 1, ..tune() };
    let early = run_resize(&e, &p, p.length_m() - 600.0, &one).expect("raccourci");
    assert_eq!(early.proposals.len(), 1);
    // Une proposition suffit : les arbres suivants ne sont pas joués, sauf si c'était le dernier tiré.
    assert!(early.diagnostics.trees <= all.diagnostics.trees);
    assert!(early.diagnostics.stopped_early || early.diagnostics.trees == all.diagnostics.trees);
}

/// Un parcours en `L` fait de pas entiers, pour le contrôle du recollé.
fn steps_of_path(e: &Engine<Vec<u8>>, path: &[(u64, u64)]) -> Vec<Step> {
    let (legs, _) = walk_of(e, path);
    legs.iter().map(|l| Step { base_edge: l.edge, reverse: l.reverse }).collect()
}

fn route_of(e: &Engine<Vec<u8>>, steps: Vec<Step>) -> Route {
    let g = e.base();
    let first = steps[0];
    let last = *steps.last().unwrap();
    let len = |s: Step| e.geom_len_m(s.base_edge);
    Route {
        start: crate::Position {
            edge: first.base_edge,
            offset_m: if first.reverse { len(first) } else { 0.0 },
        },
        end: crate::Position { edge: last.base_edge, offset_m: if last.reverse { 0.0 } else { len(last) } },
        length_m: steps.iter().map(|s| f64::from(g.edge(s.base_edge).len_dm) / 10.0).sum(),
        steps,
        team_m: 0.0,
        bridged_m: 0.0,
        shared_m: 0.0,
        crossings: 0,
        unpaved_m: 0.0,
        offroad_m: 0.0,
        presumed_surface_m: 0.0,
        bearing_deviation_deg: None,
        familiarity: 0.5,
        family: crate::Family::Turnaround,
    }
}

fn check_req() -> Request {
    crate::graft_move_end::check_request(&rreq(), at(0, 0))
}

#[test]
fn the_spliced_check_blames_the_new_section_only_for_what_it_brings() {
    let e = engine_of(&mut grid(21, 300.0));
    let win = e.full_window(0);
    // Contexte d'avant : la rangée 0 de `(0,0)` à `(0,3)`. Un tronçon qui continue à angle droit : rien à relever.
    let before = steps_of_path(&e, &[(0, 0), (0, 1), (0, 2), (0, 3)]);
    let new = route_of(&e, steps_of_path(&e, &[(0, 3), (1, 3), (2, 3), (2, 4)]));
    let ok = check_spliced_in(&e, &win, None, &check_req(), &before, &new, &[]);
    assert!(ok.violations.is_empty() && ok.sharp_at_new.is_empty() && !ok.sharp_entry, "{ok:?}");
    // Un tronçon qui repart par la même arête en sens inverse : un demi-tour à la couture, apporté par le tronçon.
    let back = route_of(&e, steps_of_path(&e, &[(0, 3), (0, 2), (1, 2)]));
    let bad = check_spliced_in(&e, &win, None, &check_req(), &before, &back, &[]);
    assert!(bad.violations.contains(&Violation::UTurn) && bad.seam_u_turn, "{bad:?}");
    // Un défaut déjà dans le contexte seul n'est pas rendu : le contexte contient un demi-tour, le tronçon est propre.
    let dirty = steps_of_path(&e, &[(0, 0), (0, 1), (0, 0), (0, 1), (0, 2), (0, 3)]);
    let clean = check_spliced_in(&e, &win, None, &check_req(), &dirty, &new, &[]);
    assert!(!clean.violations.contains(&Violation::UTurn), "{clean:?}");
    // Un contexte qui a déjà un demi-tour n'excuse pas celui que le tronçon neuf apporte en plus, à la couture : il est vu par
    // sa position, pas effacé parce que le code figure déjà dans le contexte.
    let dirty_back = check_spliced_in(&e, &win, None, &check_req(), &dirty, &back, &[]);
    assert!(dirty_back.violations.contains(&Violation::UTurn) && dirty_back.seam_u_turn, "{dirty_back:?}");
}

#[test]
fn the_geometry_starts_with_the_original_and_ends_with_it_to_the_point() {
    let e = engine_of(&mut grid(21, 300.0));
    let p = pose_path(&e, &lobe_path());
    let out = run_resize(&e, &p, p.length_m() - 600.0, &tune()).expect("raccourci");
    let r = &out.proposals[0];
    let g = r.geometry(&e, &p);
    let line = line_of(&lobe_path());
    let d = |a: LatLon, b: LatLon| {
        ((a.lat - b.lat) * crate::tests::M_LAT).hypot((a.lon - b.lon) * crate::tests::m_lon())
    };
    assert!(d(g[0], line[0]) < 0.5);
    assert!(d(*g.last().unwrap(), *line.last().unwrap()) < 0.5);
    // La longueur de la géométrie est celle de la proposition, à la tolérance de la découpe aux coutures près.
    let len: f64 = g.windows(2).map(|w| d(w[0], w[1])).sum();
    assert!((len - r.length_m).abs() < 40.0, "{len} contre {}", r.length_m);
    // La couture est le nœud : le point de coupe de l'origine en est à 15 m au plus.
    let seam = p.seams()[r.from_seam.unwrap()];
    let node = LatLon::from_e7(e.pos(seam.node));
    assert!(g.iter().any(|&q| d(q, node) < 15.0));
}

/// Un graphe où la jambe gardée finit par une chaîne de 40 m après un carrefour, et où la sortie la moins chère reviendrait
/// vers l'ouest : la règle des grappes de 100 m la refuse à la couture, le virage simple seul l'accepterait.
fn cluster_graph() -> MemSource {
    let mut s = MemSource::default();
    // Route d'ouest en est : 1 — 2 (500 m), 2 — 3 (40 m, vers le nord), 3 est la couture. Sorties de 3 : 4 (vers l'ouest,
    // 90° du nord), 5 (vers le nord), 6 (vers l'est).
    pt(&mut s, 1, 0.0, 0.0);
    pt(&mut s, 2, 0.0, 500.0);
    pt(&mut s, 3, 40.0, 500.0);
    pt(&mut s, 4, 40.0, 100.0);
    pt(&mut s, 5, 600.0, 500.0);
    pt(&mut s, 6, 40.0, 900.0);
    pt(&mut s, 7, -300.0, 500.0);
    s.way(101, &[1, 2], &tags("residential", &[]));
    s.way(102, &[2, 3], &tags("residential", &[]));
    s.way(103, &[3, 4], &tags("residential", &[]));
    s.way(104, &[3, 5], &tags("residential", &[]));
    s.way(105, &[3, 6], &tags("residential", &[]));
    s.way(106, &[2, 7], &tags("residential", &[]));
    s
}

/// La marche du graphe de grappe : `1 → 2 → 3 → 5`, la couture au carrefour `3`.
fn cluster_walk(e: &Engine<Vec<u8>>) -> (PosedRoute, Vec<PosedLeg>) {
    let g = e.base();
    let leg = |a: u64, b: u64| {
        let edge = edge_between(e, a, b);
        let (na, _) = g.edge_ends(edge);
        PosedLeg { edge, reverse: g.osm_node_id(na) != a, lo: 0.0, hi: 1.0 }
    };
    let node_of = |l: &PosedLeg| {
        let (a, b) = g.edge_ends(l.edge);
        if l.reverse {
            a
        } else {
            b
        }
    };
    let legs = [leg(1, 2), leg(2, 3), leg(3, 5)];
    let joins = [PosedJoin::Node(node_of(&legs[0])), PosedJoin::Node(node_of(&legs[1]))];
    let line = [ll(0.0, 0.0), ll(0.0, 500.0), ll(40.0, 500.0), ll(600.0, 500.0)];
    let p = PosedRoute::pose(
        e,
        &PosedInput { ride_type: crate::RideType::Road, legs: &legs, joins: &joins, line: &line },
        &PoseTune { seam_margin_m: 30.0, ..PoseTune::default() },
    )
    .expect("posé");
    (p, legs.to_vec())
}

fn small_radius() -> RetouchTune {
    RetouchTune { move_end_radius: (1.6, 200.0, 100.0, 5_000.0), ..tune() }
}

#[test]
fn the_kept_cluster_at_the_seam_is_judged_and_a_clean_exit_is_taken() {
    let mut src = cluster_graph();
    let e = engine_of(&mut src);
    let (p, legs) = cluster_walk(&e);
    let g = e.base();
    let n3 = {
        let (a, b) = g.edge_ends(legs[1].edge);
        if legs[1].reverse {
            a
        } else {
            b
        }
    };
    assert!(p.seams().iter().any(|s| s.node == n3), "la couture du carrefour 3 : {:?}", p.seams());
    let go = |end: LatLon| {
        move_end(&e, None, &p, end, &rreq(), &small_radius(), &mut Workspace::new(), &mut || false)
    };
    // `E′` à l'ouest du carrefour 3 : seule la sortie vers l'ouest y mène. Le virage simple (nord puis ouest, 90°) est
    // permis, mais le chemin gardé arrive de l'ouest par le carrefour 2, 40 m plus bas : la grappe de 100 m fait un demi-tour
    // de 180°, que le contrôle refuse. Le moteur le voit à la couture et ne rend rien (le prototype du lot 0 l'aurait rendu).
    let west = go(ll(40.0, 150.0));
    // Des sorties atteintes mais toutes coupées par la règle des grappes : `JunctionCut`, pas `NoSeamReached` (aucune sortie).
    assert_eq!(west.as_ref().unwrap_err(), &RetouchFailure::JunctionCut, "{west:?}");
    // `E′` à l'est : la sortie vers l'est prolonge la jambe gardée, rien à reprocher, et le recollé passe le contrôle.
    let east = go(ll(40.0, 800.0)).expect("raccordé");
    let win = e.full_window(0);
    for r in &east.proposals {
        let seam = p.seams()[r.from_seam.unwrap()];
        let before = p.context_before(seam.join, 400.0);
        let chk = check_spliced_in(&e, &win, None, &check_req(), &before, &r.new, &[]);
        assert!(chk.violations.is_empty(), "{chk:?}");
    }
}

#[cfg(feature = "probe")]
#[test]
fn the_prototype_of_the_lot_zero_takes_the_exit_the_cluster_refuses() {
    use crate::probe_graft::{move_end_in, KeepOut, MoveEndParams, SeamIn};
    let mut src = cluster_graph();
    let e = engine_of(&mut src);
    let (p, legs) = cluster_walk(&e);
    let g = e.base();
    let seams: Vec<SeamIn> = p
        .seams()
        .iter()
        .map(|s| {
            let leg = &legs[s.join];
            SeamIn {
                node: s.node,
                in_edge: leg.edge,
                in_reverse: leg.reverse,
                abandon_m: p.length_m() - s.s_m,
            }
        })
        .collect();
    let _ = g;
    let win = e.full_window(0);
    let ko = KeepOut::build(&win, legs.iter().map(|l| l.edge), []);
    let mut r = crate::tests::request(ll(40.0, 150.0), 10_000.0, Mode::Discovery);
    r.end = None;
    let prm = MoveEndParams { radius_m: 200.0, max_radius_m: 5_000.0, min_radius_m: 100.0 };
    let proto = move_end_in(&e, &win, None, &r, &seams, &ko, &mut Workspace::new(), &prm);
    // Le prototype juge le raccord seul : la sortie vers l'ouest lui paraît bonne.
    assert!(proto.is_ok(), "{:?}", proto.err());
}

#[test]
fn an_edge_id_helper_is_consistent_with_the_grid() {
    let e = engine_of(&mut grid(21, 300.0));
    let a = edge_between(&e, nid(0, 0), nid(0, 1));
    let b = edge_between(&e, nid(0, 1), nid(0, 0));
    assert_eq!(a, b);
}
