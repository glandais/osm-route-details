//! Tests synthétiques du 4d (lot 4, §17.13, tests 9 à 16) : les attracteurs OSM des points lointains organiques. Aucune
//! donnée de production : fichiers fabriqués sur le graphe de référence.

use std::cell::Cell;

use ord_graph::attract::{build_attr, encode_attr, AttrData, AttrFile, AttrWrite, Object};
use ord_graph::util::SplitMix64;
use rustc_hash::FxHashSet;

use crate::attract::attr_w;
use crate::engine::Engine;
use crate::figures::{candidates, pair_order, Cand, Fixed, Pair};
use crate::generate::{
    generate_in, generate_with_diagnostics, generate_with_figures_in, lens_pick, LensAttr,
};
use crate::search::{
    make_anchor, tree_backward, tree_forward, Ctx, Disc, Forbidden, ViewWs, Weighter, Workspace,
};
use crate::tests::{engine_of, engine_zoomed, ll, request};
use crate::tests_organic::{organic, organic_fixture_path};
use crate::tests_reference::{
    assert_matches_fixture, fixture_path, fixture_path_192, reference_lines_with, scenario_graph,
    scenario_graph_dense, scenario_params, scenario_params_192, scenario_requests, REQUESTS,
};
use crate::tests_team_sparse::{fixture_lines, fixture_path as team_fixture_path};
use crate::{AttrWeights, Figures, Mode, Organic, OrganicTune, Request, RideType, DEFAULT_OFF_TEAM_COST_Q8};

thread_local! {
    /// Les moteurs construits par `engine_zoomed` reçoivent un fichier d'attracteurs synthétique (les tests de non-régression
    /// y rejouent leurs références, §17.13 test 9).
    pub(crate) static ATTACH: Cell<bool> = const { Cell::new(false) };
}

/// Pose sur chaque vue un fichier tiré (objets au hasard sur 7 km, catégories au hasard) : zones d'attracteurs partout.
pub(crate) fn attach_synthetic(e: &mut Engine<Vec<u8>>) {
    for vi in 0..2 {
        let mut rng = SplitMix64(0x4a77 + vi as u64);
        let mut objs: Vec<Object> = (0..260)
            .map(|i| {
                let p = ll(-300.0 + rng.below(7600) as f64, -300.0 + rng.below(7600) as f64).to_e7();
                Object { cat: rng.below(7) as u8, lat: p.0, lon: p.1, kind: (i % 2) as u8, id: i }
            })
            .collect();
        objs.sort_unstable();
        let data = build_attr(&e.views_files[vi], &objs, 300, 300);
        set_zone(e, vi, &data);
    }
}

/// Pose un fichier fait de `data` (ancrages et zone choisis à la main).
pub(crate) fn set_zone(e: &mut Engine<Vec<u8>>, vi: usize, data: &AttrData) {
    let w = AttrWrite { snap_m: 300, reach_m: 300, meta: b"{}", built_epoch: 0 };
    let bytes = encode_attr(e.base.header(), &e.views_files[vi], data, &w).expect("encodage");
    e.set_attractors(vi, AttrFile::open(bytes).expect("ouverture")).expect("identité");
}

fn with_attached<R>(f: impl FnOnce() -> R) -> R {
    ATTACH.with(|a| a.set(true));
    let r = f();
    ATTACH.with(|a| a.set(false));
    r
}

fn stop() -> impl FnMut() -> bool {
    || false
}

fn with_figures(r: &Request, figures: Figures) -> Request {
    Request { figures, ..r.clone() }
}

/// Organique aux réglages du service avec les poids généraux du type de la requête.
fn organic_attr(r: &Request) -> Figures {
    let Figures::Organic(o) = organic() else { unreachable!() };
    Figures::Organic(Organic { attr: AttrWeights::general(r.ride_type), ..o })
}

// --- 9. I1 : Classic, fichier chargé ------------------------------------------------------------------------------

#[test]
fn a_synthetic_file_is_attached_and_reaches_the_windows() {
    with_attached(|| {
        let e = engine_zoomed(&mut scenario_graph(7), 11, scenario_params());
        assert!(e.has_attractors(0) && e.has_attractors(1));
        let win = e.full_window(0);
        assert_eq!(win.v.attr.len(), win.v.n_nodes);
        let some = win.v.attr.iter().filter(|&&m| m != 0).count();
        assert!(some > 100 && some < win.v.n_nodes, "{some} nœuds de zone sur {}", win.v.n_nodes);
    });
}

/// Les références sont relues, jamais écrites : sous `ORD_LOOPS_BLESS`, une re-bénédiction faite moteur **avec** fichier
/// masquerait un écart de I1 (et ferait la course avec les tests sans fichier qui écrivent les mêmes références).
#[test]
fn classic_rends_the_frozen_references_with_a_loaded_file() {
    with_attached(|| {
        assert_matches_fixture(
            &|e, team, r, ws| generate_with_diagnostics(e, team, r, ws, &mut stop()),
            scenario_params,
            &fixture_path(),
            false,
        );
        assert_matches_fixture(
            &|e, team, r, ws| generate_with_diagnostics(e, team, r, ws, &mut stop()),
            scenario_params_192,
            &fixture_path_192(),
            false,
        );
        let got = fixture_lines(&|e, ov| crate::TeamData::build(e, ov).unwrap());
        assert_eq!(got, fixture_lines_of(&team_fixture_path()), "team_reference.txt, fichier chargé");
    });
}

// --- 10. I5 : l'organique sans attracteurs actifs --------------------------------------------------------------------

fn fixture_lines_of(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path).expect("fixture").lines().map(str::to_string).collect()
}

#[test]
fn organic_without_active_attractors_is_the_frozen_organic_reference() {
    let want = fixture_lines_of(&organic_fixture_path());
    let run = |runner: &dyn Fn(&Request) -> Request| {
        reference_lines_with(
            &|e, team, r, ws| generate_with_diagnostics(e, team, &runner(r), ws, &mut stop()),
            scenario_params,
        )
    };
    // (b) un fichier chargé et des poids nuls ; réglages de la lentille et du rang coupés, poids nuls.
    let none_tuned = |r: &Request| {
        let Figures::Organic(o) = organic() else { unreachable!() };
        let tune = OrganicTune { attr_lens_tol_milli: 0, attr_rank: false, ..o.tune };
        with_figures(r, Figures::Organic(Organic { tune, ..o }))
    };
    with_attached(|| {
        assert_eq!(run(&|r| with_figures(r, organic())), want, "fichier chargé, poids nuls");
        assert_eq!(run(&none_tuned), want, "fichier chargé, poids nuls, lentille et rang de 1.12");
    });
    // (c) des poids généraux : les requêtes que les attracteurs ne touchent pas (MTB, aller simple, strict) gardent la
    // sortie de 1.12 ; les autres changent quelque part.
    let lines = with_attached(|| run(&|r| with_figures(r, organic_attr(r))));
    assert_eq!(lines.len(), want.len());
    let reqs = scenario_requests(13, REQUESTS);
    let (mut untouched, mut changed) = (0, 0);
    for (k, (g, w)) in lines.iter().zip(&want).enumerate() {
        let r = &reqs[k % REQUESTS];
        let inert = r.ride_type == RideType::Mtb || r.end.is_some() || r.mode == Mode::Strict;
        if inert {
            assert_eq!(g, w, "requête {k} (MTB, aller simple ou strict) : la sortie de 1.12");
            untouched += 1;
        } else {
            changed += usize::from(g != w);
        }
    }
    assert!(untouched > 20, "{untouched} requêtes inertes");
    assert!(changed > 5, "les attracteurs ne changent rien sur {changed} requêtes : le test ne prouve rien");
}

// --- 11. Masque de fenêtre -------------------------------------------------------------------------------------------

#[test]
fn a_window_node_has_the_mask_of_its_cell_in_the_file_whatever_the_tiles() {
    let mut e = engine_zoomed(&mut scenario_graph_dense(5), 16, scenario_params());
    attach_synthetic(&mut e);
    let all = crate::tests_window::all_tiles(&e, 0);
    assert!(all.len() > 30, "{} tuiles", all.len());
    let file = e.attr[0].as_ref().unwrap();
    let view = &e.views_files[0];
    let mut rng = SplitMix64(5);
    let (mut checked, mut foreign_with_mask, mut nonzero) = (0usize, 0usize, 0usize);
    for _ in 0..50 {
        let share = 5 + rng.below(60);
        let tiles: Vec<u32> = all.iter().copied().filter(|_| rng.below(100) < share).collect();
        if tiles.is_empty() {
            continue;
        }
        let win = e.build_window(0, &tiles, &mut stop()).unwrap();
        let v = &win.v;
        for (k, &g) in v.ngid.iter().enumerate() {
            let want = file.zone_mask(view.node_cell(g), g);
            assert_eq!(v.attr[k], want, "nœud {g}");
            checked += 1;
            nonzero += usize::from(want != 0);
            foreign_with_mask += usize::from(!v.complete[k] && want != 0);
        }
    }
    assert!(checked > 5000 && nonzero > 500, "{checked} nœuds, {nonzero} dans une zone");
    assert!(
        foreign_with_mask > 0,
        "aucun nœud hors des tuiles de la fenêtre n'a de masque : cas non éprouvé"
    );
}

// --- 12. I4 : déterminisme et fenêtre, attracteurs actifs --------------------------------------------------------------

#[test]
fn active_attractors_are_deterministic_and_window_independent() {
    let mut e = engine_zoomed(&mut scenario_graph(7), 11, scenario_params());
    attach_synthetic(&mut e);
    let team = crate::TeamData::build(
        &e,
        &crate::tests::overlay_of(&e, &crate::tests_reference::scenario_rows(&e, 11)),
    )
    .unwrap();
    let run = |pick: crate::tests_window::Pick<'_>| -> Vec<String> {
        let mut ws = Workspace::new();
        scenario_requests(13, REQUESTS)
            .iter()
            .map(|r| {
                let r = with_figures(r, organic_attr(r));
                let res = crate::tests_window::run_in(&e, Some(&team), &r, &mut ws, pick);
                crate::tests_reference::snapshot(0, &res)
            })
            .collect()
    };
    let minimal = run(&|_, _| Vec::new());
    assert_eq!(run(&|_, _| Vec::new()), minimal, "même graine, même sortie");
    let all = run(&|e, p| crate::tests_window::all_tiles(e, p.view));
    assert_eq!(all, minimal, "fenêtre des tuiles nécessaires ou du graphe entier : même sortie");
    // Les attracteurs ont joué quelque part (le test ne prouve rien sinon).
    let (mut cands, mut lens, mut pairs) = (0, 0, 0);
    for r in scenario_requests(13, REQUESTS).iter().map(|r| with_figures(r, organic_attr(r))) {
        let vi = r.ride_type.view_index();
        let win = e.full_window(vi);
        let (_, _, fig) = generate_with_figures_in(&e, &win, None, &r, &mut Workspace::new(), &mut stop());
        cands += fig.attr_candidates;
        lens += fig.attr_lens_picks;
        pairs += fig.attr_pairs_searched;
    }
    assert!(cands > 0 && lens > 0 && pairs > 0, "candidats {cands}, demi-tours {lens}, paires {pairs}");
}

// --- 13. Candidats -------------------------------------------------------------------------------------------------

/// Les candidats d'une requête de boucle sur la grille dense, avec `o`, dans la fenêtre du graphe entier.
fn candidates_of(e: &Engine<Vec<u8>>, o: &Organic) -> (Vec<Cand>, u32, Vec<u32>) {
    let d = 7000.0;
    let r = request(ll(3300.0, 3300.0), d, Mode::Discovery);
    let win = e.full_window(0);
    let v = &win.v;
    let w = Weighter::new(v, None, RideType::Road, Mode::Discovery, 0.5, DEFAULT_OFF_TEAM_COST_Q8);
    let s_snap = e.snap(&win, r.start, &|_| true, 0).unwrap().unwrap();
    let sa = make_anchor(e, v, &w, s_snap);
    let disc = Disc::new(r.start.to_e7(), r.start.to_e7(), d, true);
    let (forb, esc) = (Forbidden::default(), Cell::new(false));
    let cx = Ctx { v, w: &w, disc: &disc, s: &sa, e: &sa, forb: &forb, esc: &esc, org: None };
    let mut ws = Workspace::new();
    let ViewWs { t1, t2, .. } = &mut ws.views[0];
    tree_forward(&cx, t1, &mut stop()).unwrap();
    tree_backward(&cx, t2, &mut stop()).unwrap();
    let fx = Fixed {
        e,
        win: &win,
        team: None,
        r: &r,
        cx: &cx,
        s_snap: &s_snap,
        e_snap: &s_snap,
        o,
        d,
        tail_dm: 0,
    };
    let (cands, added) = candidates(&fx, t1, t2);
    (cands, added, v.ngid.clone())
}

fn road_organic(attr: AttrWeights) -> Organic {
    let Figures::Organic(o) = organic() else { unreachable!() };
    Organic { attr, ..o }
}

#[test]
fn an_attractor_less_pleasant_than_the_best_node_of_its_case_is_a_second_candidate() {
    let mut e = engine_of(&mut scenario_graph_dense(5));
    let (none, added0, ngid) = candidates_of(&e, &road_organic(AttrWeights::NONE));
    assert!(none.len() >= 6 && added0 == 0 && none.iter().all(|c| c.w == 0));
    let gid = |c: &Cand| ngid[c.node as usize];
    let road = AttrWeights::general(RideType::Road);
    let zone_of = |nodes: &[u32]| {
        let mut z: Vec<(u32, u8)> = nodes.iter().map(|&g| (g, 1u8 << 2)).collect();
        z.sort_unstable();
        z.dedup();
        AttrData { anchors: Vec::new(), zone: z }
    };

    // Poids nuls (ou aucune zone sur les nœuds) : un seul candidat par case, ceux de 1.12.
    let all_nodes: Vec<u32> = (0..e.views_files[0].header().nodes as u32).collect();
    set_zone(&mut e, 0, &zone_of(&all_nodes));
    let (zero, added, _) = candidates_of(&e, &road_organic(AttrWeights::NONE));
    assert_eq!(
        (zero.iter().map(|c| c.node).collect::<Vec<_>>(), added),
        (none.iter().map(|c| c.node).collect(), 0)
    );

    // L'attracteur est le plus agréable de sa case : un seul candidat (le même nœud, poids 1).
    let pleasant: Vec<u32> = none.iter().map(gid).collect();
    set_zone(&mut e, 0, &zone_of(&pleasant));
    let (same, added, _) = candidates_of(&e, &road_organic(road));
    assert_eq!(added, 0);
    assert_eq!(
        same.iter().map(|c| c.node).collect::<Vec<_>>(),
        none.iter().map(|c| c.node).collect::<Vec<_>>()
    );
    assert!(same.iter().all(|c| c.w == 1));

    // Les nœuds de zone sont tous les autres : chaque case qui en a un autre le donne en second candidat.
    let others: Vec<u32> = all_nodes.iter().copied().filter(|g| !pleasant.contains(g)).collect();
    set_zone(&mut e, 0, &zone_of(&others));
    let (two, added, _) = candidates_of(&e, &road_organic(road));
    assert!(added > 0, "aucun second candidat");
    assert_eq!(two.len(), none.len() + added as usize);
    let kept: Vec<u32> = two.iter().filter(|c| c.w == 0).map(|c| c.node).collect();
    assert_eq!(
        kept,
        none.iter().map(|c| c.node).collect::<Vec<_>>(),
        "les plus agréables, dans le même ordre"
    );
    for (i, c) in two.iter().enumerate().filter(|(_, c)| c.w > 0) {
        assert!(i > 0 && two[i - 1].w == 0, "l'attracteur suit le plus agréable de sa case");
        assert!(c.ratio >= two[i - 1].ratio, "moins agréable que le plus agréable de sa case");
        assert_eq!(c.w, 1);
    }
}

// --- 14. Rang des paires ----------------------------------------------------------------------------------------------

fn pair(n1: u32, r_pair: f64, agr: f64, wsum: u8) -> Pair {
    Pair { n1, n2: n1 + 1000, b1: 0, est_m: 0.0, r_pair, agr, wsum }
}

#[test]
fn a_weight_point_is_worth_one_case_of_the_pair_rank() {
    use std::cmp::Ordering::{Greater, Less};
    let rho = 0.30;
    // `near` à 0,010 de ρ* (case 0), `far` à 0,030 (case 1) : sans poids, `near` passe devant, même si `far` est plus agréable.
    let near = pair(1, 0.31, 0.9, 0);
    let far = pair(2, 0.33, 0.5, 0);
    assert_eq!(pair_order(&near, &far, rho, true), Less);
    // `far` de poids 1 : même case, `far` plus agréable : elle passe devant.
    let far1 = Pair { wsum: 1, ..far };
    assert_eq!(pair_order(&far1, &near, rho, true), Less);
    // Poids 1 mais moins agréable : à rang égal, l'agrément décide.
    let far1_bad = Pair { agr: 1.5, ..far1 };
    assert_eq!(pair_order(&far1_bad, &near, rho, true), Greater);
    // Poids 2 : une case d'avance, devant quelle que soit l'agrément.
    let far2 = Pair { wsum: 2, agr: 9.0, ..far };
    assert_eq!(pair_order(&far2, &near, rho, true), Less);
    // `attr_rank = false` : l'ordre de 1.12.
    assert_eq!(pair_order(&far2, &near, rho, false), Greater);
    assert_eq!(pair_order(&far1, &near, rho, false), Greater);
    // Poids nuls : le rang de 1.12 (case de 0,02, agrément, ids).
    let tie_a = pair(3, 0.30, 0.7, 0);
    let tie_b = pair(4, 0.30, 0.7, 0);
    assert_eq!(pair_order(&tie_a, &tie_b, rho, true), Less);
}

// --- 15. Lentille -------------------------------------------------------------------------------------------------------

#[test]
fn the_lens_takes_a_heavy_node_within_the_tolerance_and_keeps_the_choice_of_1_12_otherwise() {
    // Nœud n : (id, mètres aller (dm), mètres retour (dm)) ; D = 10 000 dm, ρ = 1 : e(n) = |aller + retour − D|.
    let d_dm = 10_000.0;
    let list: Vec<(u32, u32, u32)> = vec![
        (0, 5_000, 5_000), // e = 0 : le choix de 1.12
        (1, 5_200, 5_000), // e = 200 (2 % de D)
        (2, 5_400, 5_300), // e = 700 (7 % de D)
        (3, 5_100, 5_000), // e = 100
    ];
    let masks = [0u8, 0b100, 0b100, 0b001];
    let w = [1u8, 0, 1, 0, 0, 0, 0, 0]; // café 1, point de vue 1 (le masque 0b001 est un café)
    let tried: FxHashSet<u32> = FxHashSet::default();
    let at = |tol: f64| LensAttr { masks: &masks, w: &w, tol_dm: tol };
    let base = lens_pick(&list, &tried, 1.0, d_dm, None);
    assert_eq!(base, (Some((0.0, 0, 5_000, 5_000)), false));
    // Tolérance 5 % (500 dm) : les nœuds 1 (point de vue, poids 1, e 200) et 3 (café, poids 1, e 100) ; à poids égal, le
    // plus proche de D : le nœud 3.
    let (got, replaced) = lens_pick(&list, &tried, 1.0, d_dm, Some(&at(500.0)));
    assert_eq!((got.map(|g| g.1), replaced), (Some(3), true));
    // Le nœud 2 (e 700) est hors de la tolérance : jamais choisi.
    assert!(lens_pick(&list, &tried, 1.0, d_dm, Some(&at(500.0))).0.is_some_and(|g| g.1 != 2));
    // Tolérance trop courte pour tout nœud pesant : le choix de 1.12.
    assert_eq!(lens_pick(&list, &tried, 1.0, d_dm, Some(&at(50.0))), (Some((0.0, 0, 5_000, 5_000)), false));
    // Un nœud essayé n'est plus choisi.
    let tried3: FxHashSet<u32> = [3u32].into_iter().collect();
    let (got, _) = lens_pick(&list, &tried3, 1.0, d_dm, Some(&at(500.0)));
    assert_eq!(got.map(|g| g.1), Some(1));
    // Le choix de 1.12 est lui-même l'attracteur : rien n'est « remplacé ».
    let masks0 = [0b001u8, 0, 0, 0];
    let heavy0 = LensAttr { masks: &masks0, w: &w, tol_dm: 500.0 };
    let (got, replaced) = lens_pick(&list, &tried, 1.0, d_dm, Some(&heavy0));
    assert_eq!((got.map(|g| g.1), replaced), (Some(0), false));
    // Poids : à `e` différent, le plus lourd gagne.
    let w2 = [1u8, 0, 2, 0, 0, 0, 0, 0];
    let a2 = LensAttr { masks: &masks, w: &w2, tol_dm: 500.0 };
    let (got, _) = lens_pick(&list, &tried, 1.0, d_dm, Some(&a2));
    assert_eq!(
        got.map(|g| g.1),
        Some(1),
        "poids 2 au point de vue : le nœud 1 passe devant le café du nœud 3"
    );
    assert_eq!(attr_w(masks[1], &w2), 2);
}

#[test]
fn the_generation_counts_replaced_lens_turns_only_with_attractors() {
    let mut e = engine_zoomed(&mut scenario_graph(7), 11, scenario_params());
    attach_synthetic(&mut e);
    let win = e.full_window(0);
    let mut r = request(ll(3300.0, 3300.0), 7000.0, Mode::Discovery);
    let (mut with, mut without) = (0, 0);
    for seed in 0..40u64 {
        r.seed = seed;
        let (_, _, f1) = generate_with_figures_in(
            &e,
            &win,
            None,
            &with_figures(&r, organic_attr(&r)),
            &mut Workspace::new(),
            &mut stop(),
        );
        let (_, _, f0) = generate_with_figures_in(
            &e,
            &win,
            None,
            &with_figures(&r, organic()),
            &mut Workspace::new(),
            &mut stop(),
        );
        with += f1.attr_lens_picks;
        without += f0.attr_lens_picks + f0.attr_candidates + f0.attr_pairs_searched;
    }
    assert!(with > 0, "aucun demi-tour remplacé sur 40 graines");
    assert_eq!(without, 0, "poids nuls : aucun compteur");
}

// --- 16. Effet (garde) ----------------------------------------------------------------------------------------------------

/// Le point le plus éloigné du départ d'un parcours : le nœud de base de ses étapes le plus loin, en mètres.
fn far_base_node(e: &Engine<Vec<u8>>, r: &Request, route: &crate::Route) -> u32 {
    let loc = crate::geo::Local::new(r.start.to_e7());
    let mut best = (0.0f64, 0u32);
    for st in &route.steps {
        let (a, b) = e.ends(st.base_edge);
        for n in [a, b] {
            let p = loc.xy(e.pos(n));
            let d = p.0.hypot(p.1);
            if d > best.0 {
                best = (d, n);
            }
        }
    }
    best.1
}

#[test]
fn attractor_weights_raise_the_share_of_first_routes_whose_far_point_is_in_a_zone() {
    let mut e = engine_of(&mut scenario_graph_dense(5));
    // Une dizaine de zones de 300 m (points de vue) tirées au hasard dans la bande de r / D du départ.
    let mut rng = SplitMix64(11);
    let view = &e.views_files[0];
    let n = view.header().nodes as u32;
    let anchors: Vec<(u32, u8)> = (0..14).map(|_| (rng.below(n as usize) as u32, 2u8)).collect();
    let mut anchors = anchors;
    anchors.sort_unstable();
    anchors.dedup();
    let zone = ord_graph::attract::zone_for(view, &anchors, 300);
    set_zone(&mut e, 0, &AttrData { anchors, zone });
    let file = e.attr[0].as_ref().unwrap();
    let in_zone = |base_node: u32| {
        let Some(vn) = e.views_files[0].view_node_of_base(base_node) else { return false };
        file.zone_mask(e.views_files[0].node_cell(vn), vn) != 0
    };
    let win = e.full_window(0);
    let share = |attr: AttrWeights| {
        let (mut hit, mut total) = (0usize, 0usize);
        for k in 0..60u64 {
            let mut r = request(
                ll(3300.0 + 40.0 * (k % 7) as f64, 3300.0 + 30.0 * (k % 5) as f64),
                7000.0,
                Mode::Discovery,
            );
            r.seed = k;
            r.figures = Figures::Organic(road_organic(attr));
            let Ok(g) = generate_in(&e, &win, None, &r, &mut Workspace::new(), &mut stop()) else { continue };
            let Some(first) = g.routes.first() else { continue };
            total += 1;
            hit += usize::from(in_zone(far_base_node(&e, &r, first)));
        }
        assert!(total >= 40, "{total} parcours seulement");
        hit as f64 / total as f64
    };
    let (none, general) = (share(AttrWeights::NONE), share(AttrWeights::general(RideType::Road)));
    eprintln!("part en zone : {none:.3} sans poids, {general:.3} avec");
    assert!(
        general > none + 0.05,
        "part en zone : {none:.2} sans poids, {general:.2} avec les poids généraux"
    );
}
