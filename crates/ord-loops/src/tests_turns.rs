//! Tests synthétiques de l'étape « virages » du 4c (lot 4, §16.7 de la note) : le modèle des virages de la recherche
//! organique et l'interdiction de l'aller-retour. Aucune donnée de production.

use std::cell::Cell;

use ord_graph::build::MemSource;
use ord_graph::util::SplitMix64;

use crate::cost::RideType;
use crate::engine::{Engine, ViewData};
use crate::generate::generate_with_figures_in;
use crate::search::{
    astar_p2p, make_anchor, tree_backward, tree_forward, Ban, Ctx, Disc, Forbidden, P2pTarget, Tree,
    Weighter, Workspace, INF, NONE,
};
use crate::tests::{engine_of, ll, pt, request, tags};
use crate::tests_organic::test_mix;
use crate::turns::{pair_angle, Lead, OrgSearch, TurnModel};
use crate::verify::{aligned_pair_angle, check_route_aligned_in, Violation};
use crate::{
    Family, FigureDiagnostics, Figures, Mode, Organic, OrganicTune, Position, Request, Route, Step,
    DEFAULT_OFF_TEAM_COST_Q8,
};

// --- Aides ---------------------------------------------------------------------------------------------------

/// Le parcours qui suit les états `s` de bout en bout (chaque chaîne entière).
fn route_of(e: &Engine<Vec<u8>>, v: &ViewData, s: &[u32]) -> Route {
    let steps: Vec<Step> = s
        .iter()
        .flat_map(|&f| v.oriented_links(f))
        .map(|(edge, reverse)| Step { base_edge: edge, reverse })
        .collect();
    let (first, last) = (steps[0], *steps.last().expect("non vide"));
    Route {
        start: Position {
            edge: first.base_edge,
            offset_m: if first.reverse { e.geom_len_m(first.base_edge) } else { 0.0 },
        },
        end: Position {
            edge: last.base_edge,
            offset_m: if last.reverse { 0.0 } else { e.geom_len_m(last.base_edge) },
        },
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

/// Le nœud de vue situé à `north` m au nord et `east` m à l'est du point de référence des tests.
fn node_at(v: &ViewData, north: f64, east: f64) -> u32 {
    let p = ll(north, east).to_e7();
    (0..v.n_nodes as u32)
        .find(|&n| {
            let q = v.pos[n as usize];
            (q.0 - p.0).abs() <= 2 && (q.1 - p.1).abs() <= 2
        })
        .unwrap_or_else(|| panic!("pas de nœud à ({north}, {east})"))
}

/// L'état qui va du nœud `from` au nœud `to`.
fn state(v: &ViewData, from: u32, to: u32) -> u32 {
    *v.out(from).iter().find(|&&f| v.tgt(f) == to).unwrap_or_else(|| panic!("pas d'état {from} → {to}"))
}

fn model() -> TurnModel {
    TurnModel::new(150.0, 5)
}

fn org(turns: bool) -> OrgSearch {
    OrgSearch::new(turns.then(model), false)
}

fn stop() -> impl FnMut() -> bool {
    || false
}

fn violations_of(
    e: &Engine<Vec<u8>>,
    win: &crate::Window,
    r: &Request,
    v: &ViewData,
    s: &[u32],
) -> Vec<Violation> {
    check_route_aligned_in(e, win, None, r, &route_of(e, v, s)).violations
}

// --- Test 2 : le modèle est exact sur des chaînes droites -----------------------------------------------------------

/// Treillis `n × n` de pas `step` m, nœuds bougés de `jitter` m au plus, arêtes droites en ligne, colonne et diagonales :
/// tous les nœuds sont des carrefours (degré 3 ou plus), chaque arête est une chaîne.
fn lattice(seed: u64, n: i64, step: f64, jitter: f64) -> MemSource {
    let mut s = MemSource::default();
    let mut rng = SplitMix64(seed);
    let id = |r: i64, c: i64| r * 100 + c + 1;
    for r in 0..n {
        for c in 0..n {
            let mut j = || (rng.below(2001) as f64 / 1000.0 - 1.0) * jitter;
            let (dn, de) = (j(), j());
            pt(&mut s, id(r, c), 1000.0 + r as f64 * step + dn, 1000.0 + c as f64 * step + de);
        }
    }
    let mut way = 1;
    for r in 0..n {
        for c in 0..n {
            for (dr, dc) in [(0, 1), (1, 0), (1, 1), (1, -1)] {
                let (r2, c2) = (r + dr, c + dc);
                if r2 < n && c2 >= 0 && c2 < n {
                    way += 1;
                    s.way(way, &[id(r, c), id(r2, c2)], &tags("residential", &[]));
                }
            }
        }
    }
    s
}

/// Sur des marches tirées au hasard dans un treillis, l'angle du modèle égale celui du contrôle aligné pour toute paire
/// `(a, b)` dont les fenêtres de cap sont entièrement connues (50 m derrière `a`, 50 m devant `b`). Rend le nombre de
/// paires comparées.
fn model_equals_the_aligned_check(step: f64, jitter: f64, tol: f64) -> usize {
    let mut compared = 0;
    for seed in 0..4u64 {
        let e = engine_of(&mut lattice(seed, 12, step, jitter));
        let win = e.full_window(0);
        let v = &win.v;
        let mut rng = SplitMix64(seed + 100);
        for _ in 0..60 {
            let mut f = ((rng.below(v.n_chains) as u32) << 1) | rng.below(2) as u32;
            let mut s = vec![f];
            while s.len() < 14 {
                let outs: Vec<u32> = v.out(v.tgt(f)).iter().copied().filter(|&x| x != f ^ 1).collect();
                if outs.is_empty() {
                    break;
                }
                f = outs[rng.below(outs.len())];
                s.push(f);
            }
            let len = |k: usize| v.len_dm[(s[k] >> 1) as usize];
            let route = route_of(&e, v, &s);
            for ia in 0..s.len() - 1 {
                let behind: u32 = (0..=ia).map(len).sum();
                let mut run = 0;
                for ib in ia..s.len() - 1 {
                    if ib > ia {
                        run += len(ib);
                    }
                    if run > 1000 {
                        break;
                    }
                    let ahead: u32 = (ib + 1..s.len()).map(len).sum();
                    if behind < 520 || ahead < 520 {
                        continue;
                    }
                    let (m, c) = (pair_angle(v, &s, ia, ib), aligned_pair_angle(&e, &route, ia, ib));
                    assert!(
                        (m - c).abs() <= tol,
                        "paire ({ia}, {ib}) : modèle {m}, contrôle {c}, états {s:?}, longueurs {:?}, positions {:?}",
                        (0..s.len()).map(len).collect::<Vec<_>>(),
                        s.iter().map(|&f| (v.pos[v.src(f) as usize], v.pos[v.tgt(f) as usize])).collect::<Vec<_>>()
                    );
                    compared += 1;
                }
            }
        }
    }
    compared
}

#[test]
fn the_model_angle_equals_the_aligned_check_on_straight_short_chains() {
    // Chaînes de 5 à 45 m : les points des fenêtres sont ceux du contrôle, à l'arrondi flottant près.
    assert!(model_equals_the_aligned_check(20.0, 6.0, 1e-6) > 300);
}

#[test]
fn the_model_angle_equals_the_aligned_check_on_straight_long_chains_up_to_the_stored_heading() {
    // Chaînes de 50 m ou plus : le cap de bout est stocké sur 16 bits (0,0055°).
    assert!(model_equals_the_aligned_check(70.0, 10.0, 0.02) > 100);
}

// --- Tests 3 à 5 : deux chaussées, ouvertures de terre-plein, giratoire ------------------------------------------

/// Deux chaussées à sens unique à `gap` m l'une de l'autre (est au nord, ouest au sud), 13 ouvertures de terre-plein tous les
/// 400 m, un giratoire de six nœuds à 10 m (sens horaire) au bout est, et une impasse de 60 m à chaque bout ouest et à
/// chaque nœud du giratoire qui n'est pas relié aux chaussées.
fn dual_carriageway(gap: f64) -> MemSource {
    const N: i64 = 12;
    let h = gap / 2.0;
    let mut s = MemSource::default();
    let (eb, wb) = (|k: i64| 100 + k, |k: i64| 200 + k);
    for k in 0..=N {
        pt(&mut s, eb(k), h, 400.0 * k as f64);
        pt(&mut s, wb(k), -h, 400.0 * k as f64);
    }
    let end = 400.0 * N as f64;
    let ring: Vec<(f64, f64)> = (0..6)
        .map(|i| (10.0 * (60.0 * i as f64).to_radians().cos(), 10.0 * (60.0 * i as f64).to_radians().sin()))
        .collect();
    // `eb(N)` et `wb(N)` ne sont pas des nœuds de giratoire : le giratoire commence à 20 m, ses sommets 0 et 3 sont
    // reliés aux chaussées.
    let r = |i: i64| 300 + i;
    for (i, (dn, de)) in ring.iter().enumerate() {
        pt(&mut s, r(i as i64), dn * h / 10.0, end + 20.0 + de);
    }
    let mut way = 1000;
    let mut add = |s: &mut MemSource, refs: &[i64], oneway: &'static str| {
        way += 1;
        s.way(way, refs, &tags("residential", &[("oneway", oneway)]));
    };
    for k in 0..N {
        add(&mut s, &[eb(k), eb(k + 1)], "yes");
        add(&mut s, &[wb(k + 1), wb(k)], "yes");
    }
    for k in 0..=N {
        add(&mut s, &[eb(k), wb(k)], "no");
    }
    add(&mut s, &[eb(N), r(0)], "yes");
    add(&mut s, &[r(3), wb(N)], "yes");
    for i in 0..6 {
        add(&mut s, &[r(i), r((i + 1) % 6)], "yes");
    }
    // Impasses : un nœud à 60 m, de plus en plus loin du giratoire ou de la chaussée, pour que ces nœuds aient trois arêtes.
    let mut stub = |s: &mut MemSource, from: i64, id: i64, dn: f64, de: f64, base: (f64, f64)| {
        pt(s, id, base.0 + dn, base.1 + de);
        add(s, &[from, id], "no");
    };
    stub(&mut s, eb(0), 400, 60.0, 0.0, (h, 0.0));
    stub(&mut s, wb(0), 401, -60.0, 0.0, (-h, 0.0));
    for i in [1i64, 2, 4, 5] {
        let (dn, de) = ring[i as usize];
        stub(&mut s, r(i), 410 + i, dn * 6.0, de * 6.0, (dn * h / 10.0, end + 20.0));
    }
    s
}

/// Une recherche point à point depuis l'état `last` (de EB(0) à EB(1)) vers le bout ouest de la chaussée sud : le demi-tour
/// par l'ouverture de EB(1), 120 m plus loin que le départ.
fn p2p_behind(gap: f64, with_model: bool) -> (Vec<Violation>, bool) {
    let e = engine_of(&mut dual_carriageway(gap));
    let win = e.full_window(0);
    let v = &win.v;
    let r = request(ll(gap / 2.0, 100.0), 8000.0, Mode::Discovery);
    let w = Weighter::new(v, None, RideType::Road, Mode::Discovery, 0.5, DEFAULT_OFF_TEAM_COST_Q8);
    let s_snap = e.snap(&win, r.start, &|_| true, 0).unwrap().unwrap();
    let sa = make_anchor(&e, v, &w, s_snap);
    let s_e7 = r.start.to_e7();
    let disc = Disc::new(s_e7, s_e7, 8000.0, true);
    let (forb, esc) = (Forbidden::default(), Cell::new(false));
    let os = org(with_model);
    let cx =
        Ctx { v, w: &w, disc: &disc, s: &sa, e: &sa, forb: &forb, esc: &esc, org: with_model.then_some(&os) };
    let last = state(v, node_at(v, gap / 2.0, 0.0), node_at(v, gap / 2.0, 400.0));
    let target = node_at(v, -gap / 2.0, 0.0);
    let tgt = P2pTarget {
        node: target,
        xy: disc.xy_of(v.pos[target as usize]),
        t1: 0,
        t2: 0,
        f_min: 0.0,
        slack: 0,
    };
    let ban = vec![0u32; v.n_chains];
    let (t1, t2, mut ta) = (Tree::default(), Tree::default(), Tree::default());
    let lead = Lead { path: &[last], dm: sa.out_len[(last & 1) as usize], aux: None };
    let mut budget = u64::MAX / 2;
    let out = astar_p2p(
        &cx,
        &mut ta,
        &t1,
        &t2,
        last,
        NONE,
        &tgt,
        &Ban::plain(&ban, 1),
        &lead,
        false,
        u64::MAX / 2,
        &mut budget,
        &mut stop(),
    )
    .unwrap();
    match out.fields {
        None => (Vec::new(), false),
        Some(fields) => {
            let mut s = vec![last];
            s.extend(fields);
            (violations_of(&e, &win, &r, v, &s), true)
        }
    }
}

/// Test 3 : demi-tour composé. Modèle coupé, la recherche prend le demi-tour par l'ouverture du terre-plein, que le
/// contrôle aligné refuse ; modèle actif, elle ne rend aucun chemin refusé `SHARP_TURN` (le garde : le test ne prouve rien
/// si la recherche ne prenait pas ce demi-tour).
#[test]
fn a_u_turn_through_a_median_opening_is_found_without_the_model_and_never_with_it() {
    for gap in [20.0, 70.0] {
        let (without, found) = p2p_behind(gap, false);
        assert!(found && without.contains(&Violation::SharpTurn), "écart {gap} m, sans modèle : {without:?}");
        let (with, _) = p2p_behind(gap, true);
        assert!(!with.contains(&Violation::SharpTurn), "écart {gap} m, avec le modèle : {with:?}");
    }
}

/// Test 4 : de bout en bout, 50 requêtes organiques : aucun `SHARP_TURN` refusé par le contrôle interne (lentille et
/// triangles) avec le modèle ; au moins un sans.
fn sharp_refusals(gap: f64, turns: bool) -> (u32, usize) {
    let e = engine_of(&mut dual_carriageway(gap));
    let win = e.full_window(0);
    let figures = Figures::Organic(Organic {
        mix: test_mix(),
        tune: OrganicTune { turns, ..OrganicTune::default() },
        attr: crate::AttrWeights::NONE,
    });
    let mut ws = Workspace::new();
    let (mut sharp, mut routes) = (0u32, 0usize);
    for k in 0..50u64 {
        let mut r = request(
            ll(gap / 2.0, 40.0 + 90.0 * (k % 25) as f64),
            3000.0 + 250.0 * (k % 5) as f64,
            Mode::Discovery,
        );
        r.seed = k;
        r.figures = figures;
        let (res, diag, fig): (_, _, FigureDiagnostics) =
            generate_with_figures_in(&e, &win, None, &r, &mut ws, &mut stop());
        let count = |l: &[(Violation, u32)]| {
            l.iter().filter(|x| x.0 == Violation::SharpTurn).map(|x| x.1).sum::<u32>()
        };
        sharp += count(&diag.rejected) + count(&fig.tri_rejected);
        routes += res.map_or(0, |g| g.routes.len());
    }
    (sharp, routes)
}

#[test]
fn no_organic_request_is_refused_for_a_sharp_turn_with_the_model_and_some_are_without_it() {
    for gap in [20.0, 70.0] {
        let (with, _) = sharp_refusals(gap, true);
        assert_eq!(with, 0, "écart {gap} m : refus SHARP_TURN avec le modèle");
        let (without, _) = sharp_refusals(gap, false);
        assert!(without > 0, "écart {gap} m : aucun refus sans le modèle, le test ne prouve rien");
    }
}

/// Test 5 : exemptions. Une grappe dont `b` est à 200 m ou moins du départ (ou `a` à 200 m ou moins de l'arrivée, à rebours)
/// n'est pas refusée. Écart de 70 m : le demi-tour par l'ouverture est une grappe sans virage simple de 150°.
/// Rend (l'arbre aller atteint le tronçon sud de EB(1), l'arbre retour atteint le tronçon nord de EB(0)).
fn reached(with_model: bool, start_north: f64, start_east: f64) -> (bool, bool) {
    let gap = 70.0;
    let e = engine_of(&mut dual_carriageway(gap));
    let win = e.full_window(0);
    let v = &win.v;
    let r = request(ll(start_north, start_east), 8000.0, Mode::Discovery);
    let w = Weighter::new(v, None, RideType::Road, Mode::Discovery, 0.5, DEFAULT_OFF_TEAM_COST_Q8);
    let s_snap = e.snap(&win, r.start, &|_| true, 0).unwrap().unwrap();
    let sa = make_anchor(&e, v, &w, s_snap);
    let s_e7 = r.start.to_e7();
    let disc = Disc::new(s_e7, s_e7, 8000.0, true);
    let (forb, esc) = (Forbidden::default(), Cell::new(false));
    let os = org(with_model);
    let cx =
        Ctx { v, w: &w, disc: &disc, s: &sa, e: &sa, forb: &forb, esc: &esc, org: with_model.then_some(&os) };
    let (mut t1, mut t2) = (Tree::default(), Tree::default());
    tree_forward(&cx, &mut t1, &mut stop()).unwrap();
    tree_backward(&cx, &mut t2, &mut stop()).unwrap();
    let h = gap / 2.0;
    let (eb0, eb1) = (node_at(v, h, 0.0), node_at(v, h, 400.0));
    let (wb1, wb0) = (node_at(v, -h, 400.0), node_at(v, -h, 0.0));
    (t1.get(state(v, wb1, wb0)) != INF, t2.get(state(v, eb0, eb1)) != INF)
}

#[test]
fn a_cluster_within_200_m_of_the_start_or_the_end_is_not_refused() {
    let h = 35.0;
    // Sans modèle : atteint de partout.
    assert!(reached(false, h, 100.0).0);
    assert!(reached(false, -h, 100.0).1);
    // Départ sur la chaussée nord à 100 m de l'ouverture : la grappe (EB(1), WB(1)) finit à 170 m du départ, exemptée.
    assert!(reached(true, h, 300.0).0, "grappe à 170 m du départ : exemptée");
    // À 300 m de l'ouverture, la grappe finit à 370 m du départ : refusée.
    assert!(!reached(true, h, 100.0).0, "grappe à 370 m du départ : refusée");
    // Arrivée sur la chaussée sud à 100 m après l'ouverture : EB(1) est à 170 m de l'arrivée, exemptée (à rebours).
    assert!(reached(true, -h, 300.0).1, "grappe à 170 m de l'arrivée : exemptée");
    assert!(!reached(true, -h, 100.0).1, "grappe à 370 m de l'arrivée : refusée");
}

// --- Tests 6 et 7 : aucun aller-retour dans une jambe -----------------------------------------------------------------

/// Une route d'accès `P1 – J – L` qui finit par une boucle en `L`, et une rue `J – T` qui repart vers l'ouest : le virage de
/// `J` vers `T` depuis l'est fait 158°, interdit. Le chemin le moins cher de `P1` à `T` fait donc le tour de la boucle et
/// revient sur `J – L` en sens inverse.
fn lollipop() -> MemSource {
    let mut s = MemSource::default();
    for (id, n, e) in [
        (1, 0.0, 0.0),
        (2, 0.0, 300.0),
        (3, 0.0, 600.0),
        (4, 80.0, 650.0),
        (5, 80.0, 550.0),
        (6, -40.0, 200.0),
    ] {
        pt(&mut s, id, n, e);
    }
    let res = tags("residential", &[]);
    s.way(11, &[1, 2], &res).way(12, &[2, 3], &res).way(13, &[3, 4, 5, 3], &res).way(14, &[2, 6], &res);
    s
}

fn lollipop_search(no_reuse: bool) -> Option<Vec<u32>> {
    let e = engine_of(&mut lollipop());
    let win = e.full_window(0);
    let v = &win.v;
    let r = request(ll(0.0, 100.0), 8000.0, Mode::Discovery);
    let w = Weighter::new(v, None, RideType::Road, Mode::Discovery, 0.5, DEFAULT_OFF_TEAM_COST_Q8);
    let s_snap = e.snap(&win, r.start, &|_| true, 0).unwrap().unwrap();
    let sa = make_anchor(&e, v, &w, s_snap);
    let s_e7 = r.start.to_e7();
    let disc = Disc::new(s_e7, s_e7, 8000.0, true);
    let (forb, esc) = (Forbidden::default(), Cell::new(false));
    let os = OrgSearch::new(None, no_reuse);
    let cx = Ctx { v, w: &w, disc: &disc, s: &sa, e: &sa, forb: &forb, esc: &esc, org: Some(&os) };
    let last = state(v, node_at(v, 0.0, 0.0), node_at(v, 0.0, 300.0));
    let target = node_at(v, -40.0, 200.0);
    let tgt = P2pTarget {
        node: target,
        xy: disc.xy_of(v.pos[target as usize]),
        t1: 0,
        t2: 0,
        f_min: 0.0,
        slack: 0,
    };
    let ban = vec![0u32; v.n_chains];
    let (t1, t2, mut ta) = (Tree::default(), Tree::default(), Tree::default());
    let lead = Lead { path: &[last], dm: sa.out_len[(last & 1) as usize], aux: None };
    let mut budget = u64::MAX / 2;
    let out = astar_p2p(
        &cx,
        &mut ta,
        &t1,
        &t2,
        last,
        NONE,
        &tgt,
        &Ban::plain(&ban, 1),
        &lead,
        false,
        u64::MAX / 2,
        &mut budget,
        &mut stop(),
    )
    .unwrap();
    if no_reuse {
        // Le garde du compteur : un refus au moins a eu lieu.
        assert!(os.reuse_cut.get() > 0, "aucun aller-retour refusé");
    }
    out.fields
}

fn repeated_chain(fields: &[u32]) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    fields.iter().any(|&f| !seen.insert(f >> 1))
}

/// Test 6.
#[test]
fn a_leg_never_turns_back_on_a_chain_it_has_just_taken() {
    let without = lollipop_search(false).expect("sans l'interdiction, le chemin fait le tour de la boucle");
    assert!(repeated_chain(&without), "garde : le chemin le moins cher revient sur ses pas : {without:?}");
    assert!(lollipop_search(true).is_none_or(|f| !repeated_chain(&f)));
}

/// Test 7 : la jambe 2 ne reprend aucune chaîne de la jambe 1, queue comprise ; la jambe 3 et la lentille gardent la queue.
#[test]
fn leg_two_bans_the_whole_of_leg_one_and_the_tail_stays_for_the_other_searches() {
    let e = engine_of(&mut lollipop());
    let win = e.full_window(0);
    let v = &win.v;
    let leg1 = [
        state(v, node_at(v, 0.0, 0.0), node_at(v, 0.0, 300.0)),
        state(v, node_at(v, 0.0, 300.0), node_at(v, 0.0, 600.0)),
    ];
    let mut t1 = Tree::default();
    t1.begin(2 * v.n_chains, v.n_nodes);
    t1.set(leg1[0], 10, NONE, 3000);
    t1.set(leg1[1], 20, leg1[0], 6000);
    let tail_dm = 4000;
    let mut results = Vec::new();
    for all_ban in [false, true] {
        let (mut ta, mut ban, mut gen, mut cor) =
            (Tree::default(), vec![0u32; v.n_chains], 0u32, Default::default());
        let mut sc = crate::figures::Scratch { ta: &mut ta, ban: &mut ban, ban_gen: &mut gen, cor: &mut cor };
        sc.stamp_leg1(&t1, &leg1, tail_dm, all_ban);
        let bits = |f: u32| ban[(f >> 1) as usize] & 7;
        results.push((bits(leg1[0]), bits(leg1[1])));
    }
    assert_eq!(results[0], (crate::search::TAIL, crate::search::BAN), "la queue garde son interdit léger");
    assert_eq!(results[1], (crate::search::BAN, crate::search::BAN), "la jambe 2 interdit toute la jambe 1");
}

/// I2′ : sans le modèle des virages ni l'interdiction de l'aller-retour, l'organique est celui d'avant l'étape (la
/// référence de `ea6ef63`, gardée à part : `organic_reference.txt` est re-bénie par l'étape).
#[test]
fn the_organic_reference_is_replayed_with_turns_and_no_reuse_off() {
    let off = Figures::Organic(Organic {
        mix: test_mix(),
        tune: OrganicTune { turns: false, no_reuse: false, ..OrganicTune::default() },
        attr: crate::AttrWeights::NONE,
    });
    crate::tests_reference::assert_matches_fixture(
        &|e, team, r, ws| {
            crate::generate::generate_with_diagnostics(
                e,
                team,
                &Request { figures: off, ..r.clone() },
                ws,
                &mut stop(),
            )
        },
        crate::tests_reference::scenario_params,
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/organic_reference_before_turns.txt"),
        false,
    );
}
