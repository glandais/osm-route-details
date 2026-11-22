//! Retouche d'un parcours, lot 0 : le masque de chaînes interdites et `move_end_in` (note `ord-retouche-lot0.md` §8).
//! Sous `probe` seulement : ce code n'existe pas dans la génération ordinaire.

use crate::generate::generate_with_figures_in;
use crate::probe_graft::{move_end_in, KeepOut, MoveEndFail, MoveEndParams, SeamIn};
use crate::tests::{edge_between, engine_of, grid, ll, request};
use crate::{Engine, Failure, LatLon, Mode, Request, Route, Workspace};

fn id(i: u64, j: u64) -> u64 {
    1 + i * 21 + j
}

fn route_of(
    e: &Engine<Vec<u8>>,
    r: &Request,
    ws: &mut Workspace,
    ko: Option<KeepOut>,
) -> Result<Vec<Route>, Failure> {
    let win = e.full_window(r.ride_type.view_index());
    ws.set_keep_out(ko);
    generate_with_figures_in(e, &win, None, r, ws, &mut || false).0.map(|g| g.routes)
}

fn one_way() -> Request {
    let mut r = request(ll(3000.0, 1500.0), 4200.0, Mode::Discovery);
    r.end = Some(ll(3000.0, 4500.0));
    r
}

#[test]
fn an_empty_mask_gives_the_same_routes_as_no_mask() {
    let e = engine_of(&mut grid(21, 300.0));
    let r = one_way();
    let win = e.full_window(0);
    let plain = route_of(&e, &r, &mut Workspace::new(), None).expect("routes");
    let empty = KeepOut::build(&win, [], []);
    assert_eq!(empty.masked_chains(), 0);
    let masked = route_of(&e, &r, &mut Workspace::new(), Some(empty)).expect("routes");
    assert_eq!(plain.len(), masked.len());
    for (a, b) in plain.iter().zip(&masked) {
        assert_eq!(a.steps, b.steps);
        assert_eq!((a.start, a.end), (b.start, b.end));
    }
}

#[test]
fn a_masked_chain_is_in_no_route_and_the_distance_still_holds() {
    let e = engine_of(&mut grid(21, 300.0));
    let r = one_way();
    let win = e.full_window(0);
    let plain = route_of(&e, &r, &mut Workspace::new(), None).expect("routes");
    // On interdit les arêtes du milieu de la première route, la zone des deux bouts restant libre.
    let steps = &plain[0].steps;
    assert!(steps.len() > 6);
    let mid: Vec<u32> = steps[2..steps.len() - 2].iter().map(|s| s.base_edge).collect();
    let free: Vec<u32> = steps[..2]
        .iter()
        .chain(&steps[steps.len() - 2..])
        .map(|s| s.base_edge)
        .filter(|x| !mid.contains(x))
        .collect();
    let ko = KeepOut::build(&win, mid.iter().copied(), free);
    assert!(ko.masked_chains() > 0, "rien de masqué");
    assert!(ko.masked_m(&win) >= 290.0 * ko.masked_chains() as f64, "{} m", ko.masked_m(&win));
    let routes = route_of(&e, &r, &mut Workspace::new(), Some(ko)).expect("des routes malgré le masque");
    for route in &routes {
        assert!(route.steps.iter().all(|s| !mid.contains(&s.base_edge)), "une arête masquée est reprise");
        // ±10 % de D : la tolérance du moteur, intacte.
        assert!((route.length_m - r.distance_m).abs() <= 0.1 * r.distance_m + 1.0, "{}", route.length_m);
    }
    // Le masque est pris par l'appel : le suivant, sans nouveau masque, rend la route d'avant.
    let mut ws = Workspace::new();
    let _ = route_of(&e, &r, &mut ws, Some(KeepOut::build(&win, mid.iter().copied(), [])));
    let again = route_of(&e, &r, &mut ws, None).expect("routes");
    assert_eq!(again[0].steps, plain[0].steps);
}

#[test]
#[should_panic(expected = "autre fenêtre")]
fn a_mask_built_for_another_window_panics() {
    let e = engine_of(&mut grid(21, 300.0));
    let r = one_way();
    let full = e.full_window(0);
    // Un masque bâti sur le graphe plus petit d'un autre moteur : ni le nombre de chaînes ni les tuiles ne sont les mêmes.
    let small = engine_of(&mut grid(5, 300.0));
    let win_small = small.full_window(0);
    let ko_small = KeepOut::build(&win_small, [], []);
    let mut ws = Workspace::new();
    ws.set_keep_out(Some(ko_small));
    let _ = generate_with_figures_in(&e, &full, None, &r, &mut ws, &mut || false);
}

/// Un parcours en L : la rangée 0 vers l'est jusqu'à `(0, 10)`, puis la colonne 10 vers le nord jusqu'à `(10, 10)`.
fn l_route(e: &Engine<Vec<u8>>) -> (Vec<u32>, Vec<SeamIn>) {
    let mut nodes: Vec<(u64, u64)> = (0..=10).map(|j| (0, j)).collect();
    nodes.extend((1..=10).map(|i| (i, 10)));
    let g = e.base();
    let node_of = |edge: u32, osm: u64| {
        let (a, b) = g.edge_ends(edge);
        if g.osm_node_id(a) == osm {
            a
        } else {
            b
        }
    };
    let total = (nodes.len() - 1) as f64 * 300.0;
    let (mut edges, mut seams) = (Vec::new(), Vec::new());
    for (k, w) in nodes.windows(2).enumerate() {
        let (p, q) = (id(w[0].0, w[0].1), id(w[1].0, w[1].1));
        let edge = edge_between(e, p, q);
        edges.push(edge);
        // Couture au nœud d'arrivée de cette jambe, sauf à l'arrivée du parcours.
        if k + 1 < nodes.len() - 1 {
            let n = node_of(edge, q);
            let (a, _) = g.edge_ends(edge);
            seams.push(SeamIn {
                node: n,
                in_edge: edge,
                in_reverse: a != node_of(edge, p),
                abandon_m: total - (k + 1) as f64 * 300.0,
            });
        }
    }
    (edges, seams)
}

fn end_request(at: LatLon) -> Request {
    let mut r = request(at, 40_000.0, Mode::Discovery);
    r.end = None;
    r
}

fn params() -> MoveEndParams {
    MoveEndParams { radius_m: 4_000.0, max_radius_m: 8_000.0, min_radius_m: 1_000.0 }
}

#[test]
fn move_end_picks_the_seam_that_minimises_abandoned_plus_connector() {
    let e = engine_of(&mut grid(21, 300.0));
    let win = e.full_window(0);
    let (edges, seams) = l_route(&e);
    let ko = KeepOut::build(&win, edges.iter().copied(), []);
    let mut ws = Workspace::new();
    // `E′` voisin de l'arrivée : on quitte à la dernière couture.
    let near = move_end_in(&e, &win, None, &end_request(ll(2700.0, 2700.0)), &seams, &ko, &mut ws, &params())
        .expect("raccord");
    assert_eq!(near.best.seam, seams.len() - 1, "dernière couture");
    assert!((near.best.abandon_m - 300.0).abs() < 1e-6);
    assert!(near.best.raccord_m > 250.0 && near.best.raccord_m < 400.0, "{}", near.best.raccord_m);
    assert!(!near.best.raccord.steps.is_empty());
    // Aucun mètre du raccord n'est une arête du parcours gardé.
    assert!(near.best.raccord.steps.iter().all(|s| !edges.contains(&s.base_edge)));
    // `E′` au-delà du point lointain du parcours, vers l'ouest : plusieurs coutures atteintes, la meilleure est
    // strictement meilleure que « garder tout » ou égale.
    let far = move_end_in(&e, &win, None, &end_request(ll(3000.0, 900.0)), &seams, &ko, &mut ws, &params())
        .expect("raccord");
    let total = |p: &crate::probe_graft::MoveEndPick| p.abandon_m + p.raccord_m;
    assert!(far.seams_reached >= 2);
    if let Some(all) = &far.keep_all {
        assert!(total(&far.best) <= total(all) + 1e-6);
    }
    if let Some(n) = &far.nearest {
        assert!(total(&far.best) <= total(n) + 1e-6);
        assert!(n.raccord_m <= far.best.raccord_m + 1e-6);
    }
}

#[test]
fn move_end_classes_its_refusals() {
    let e = engine_of(&mut grid(21, 300.0));
    let win = e.full_window(0);
    let (edges, seams) = l_route(&e);
    let ko = KeepOut::build(&win, edges.iter().copied(), []);
    let mut ws = Workspace::new();
    // `E′` sur le parcours.
    let on = move_end_in(&e, &win, None, &end_request(ll(0.0, 1500.0)), &seams, &ko, &mut ws, &params());
    assert!(matches!(on, Err(MoveEndFail::EndOnRoute { edge }) if edges.contains(&edge)), "{on:?}");
    // `E′` injoignable : loin de tout le graphe.
    let far =
        move_end_in(&e, &win, None, &end_request(ll(60_000.0, 60_000.0)), &seams, &ko, &mut ws, &params());
    assert!(matches!(far, Err(MoveEndFail::EndTooFar)), "{far:?}");
    // Un disque trop petit pour atteindre une couture.
    let small = MoveEndParams { radius_m: 600.0, max_radius_m: 600.0, min_radius_m: 300.0 };
    let none = move_end_in(&e, &win, None, &end_request(ll(5400.0, 900.0)), &seams, &ko, &mut ws, &small);
    assert!(matches!(none, Err(MoveEndFail::NoSeamReached)), "{none:?}");
    // Un mode d'équipe sans données d'équipe.
    let mut r = end_request(ll(2700.0, 2700.0));
    r.mode = Mode::TeamFirst;
    let no_team = move_end_in(&e, &win, None, &r, &seams, &ko, &mut ws, &params());
    assert!(matches!(no_team, Err(MoveEndFail::NoTeamData)), "{no_team:?}");
}
