//! Bornes de roulabilité (lot 5, §6 et §11), côté moteur : le niveau d'une arête par type (chaque bord de code), le
//! contournement d'une arête non roulable, l'exemption d'équipe (parcours du type seulement, jamais pour `impassable`),
//! le mode strict inchangé, `Rideability::Off` = 1.14.

use ord_graph::attrs::{Attrs, Fields, SURFACES};
use ord_graph::build::MemSource;

use crate::cost::RideType;
use crate::engine::{ride_level, Rideability, RIDE_HARD, RIDE_OK, RIDE_SOFT};
use crate::generate::generate_with_figures_in;
use crate::search::{Weighter, INF};
use crate::team::{TeamCtx, TeamData, WindowTeam};
use crate::tests::{
    chain_of_edge, edge_between, engine_with, ll, neutral_params, overlay_of, pt, request, tags, Counts,
    GRAVEL1, MIXED1, ROAD1,
};
use crate::{Engine, Mode, Params, Request, Workspace};

const MTB1: Counts = [0, 0, 0, 0, 1, 1, 0, 0];

fn params(rideability: Rideability) -> Params {
    Params { rideability, ..neutral_params() }
}

/// Deux couloirs entre `1` et `2` : au nord `1-3-2` (même longueur, mais moins cher : piste), dont le premier tronçon porte `bad` ; au sud `1-4-2`
/// (long, résidentiel propre). Des impasses de 300 m en `5` et `6` portent le départ et l'arrivée.
fn corridors(bad: &[(&str, &str)]) -> MemSource {
    let mut s = MemSource::default();
    pt(&mut s, 1, 0.0, 0.0);
    pt(&mut s, 2, 0.0, 3000.0);
    pt(&mut s, 3, 700.0, 1500.0);
    pt(&mut s, 4, -700.0, 1500.0);
    pt(&mut s, 5, 0.0, -300.0);
    pt(&mut s, 6, 0.0, 3300.0);
    s.way(101, &[1, 3], &tags("track", bad));
    s.way(105, &[3, 2], &tags("residential", &[]));
    s.way(102, &[1, 4], &tags("residential", &[]));
    s.way(106, &[4, 2], &tags("residential", &[]));
    s.way(103, &[5, 1], &tags("residential", &[]));
    s.way(104, &[2, 6], &tags("residential", &[]));
    s
}

fn one_way(ride: RideType) -> Request {
    let mut r = request(ll(0.0, -300.0), 4150.0, Mode::Discovery);
    r.end = Some(ll(0.0, 3300.0));
    r.ride_type = ride;
    r
}

fn uses_north(e: &Engine<Vec<u8>>, r: &Request, tc: Option<TeamCtx<'_>>) -> bool {
    let win = e.full_window(1);
    let g = generate_with_figures_in(e, &win, tc, r, &mut Workspace::new(), &mut || false)
        .0
        .expect("des parcours");
    let north = edge_between(e, 1, 3);
    g.routes[0].steps.iter().any(|s| s.base_edge == north)
}

fn attrs(f: Fields) -> Attrs {
    Attrs::pack(&f)
}

/// Chaque bord de code du §6.1, pour chaque type.
#[test]
fn levels_at_every_code_edge() {
    let (g, m) = (RideType::Gravel, RideType::Mtb);
    let base = Fields { class: 7, ..Fields::default() };
    let lv = |ride, f: Fields| ride_level(ride, attrs(f));
    // smoothness : 5 (very_bad) et 15 (inconnu) permis, 6 et 7 interdits en gravel seulement, 8 toujours.
    for (code, gravel, mtb) in [
        (0, RIDE_OK, RIDE_OK),
        (5, RIDE_OK, RIDE_OK),
        (6, RIDE_SOFT, RIDE_OK),
        (7, RIDE_SOFT, RIDE_OK),
        (8, RIDE_HARD, RIDE_HARD),
        (15, RIDE_OK, RIDE_OK),
    ] {
        let f = Fields { smoothness: code, ..base };
        assert_eq!(lv(g, f), gravel, "smoothness {code}, gravel");
        assert_eq!(lv(m, f), mtb, "smoothness {code}, vtt");
        assert_eq!(lv(RideType::Road, f), RIDE_OK, "la route n'a aucune borne");
    }
    // mtb:scale : code 2 (échelle 1) permis, 3 (échelle 2) interdit en gravel ; code 4 (échelle 3) permis en VTT, 5 non.
    for (code, gravel, mtb) in [
        (0, RIDE_OK, RIDE_OK),
        (2, RIDE_OK, RIDE_OK),
        (3, RIDE_SOFT, RIDE_OK),
        (4, RIDE_SOFT, RIDE_OK),
        (5, RIDE_SOFT, RIDE_SOFT),
        (7, RIDE_SOFT, RIDE_SOFT),
    ] {
        let f = Fields { mtb_scale: code, ..base };
        assert_eq!(lv(g, f), gravel, "mtb_scale {code}, gravel");
        assert_eq!(lv(m, f), mtb, "mtb_scale {code}, vtt");
    }
    // tracktype : grade4 (4) permis, grade5 (5) interdit en gravel, 7 (inconnu) permis ; le VTT roule tout.
    for (code, gravel) in [(1, RIDE_OK), (4, RIDE_OK), (5, RIDE_SOFT), (7, RIDE_OK)] {
        let f = Fields { tracktype: code, ..base };
        assert_eq!(lv(g, f), gravel, "tracktype {code}");
        assert_eq!(lv(m, f), RIDE_OK);
    }
    // surface : mud interdit en gravel ; sand, grass et tout le reste permis.
    for (i, name) in SURFACES.iter().enumerate() {
        let f = Fields { surface: i as u8 + 1, ..base };
        let want = if *name == "mud" { RIDE_SOFT } else { RIDE_OK };
        assert_eq!(lv(g, f), want, "surface {name}");
        assert_eq!(lv(m, f), RIDE_OK, "surface {name}, vtt");
    }
    // Un niveau dur l'emporte sur un niveau mou, pour le type où il est dur.
    let both = Fields { smoothness: 8, mtb_scale: 5, ..base };
    assert_eq!(lv(g, both), RIDE_HARD);
}

#[test]
fn a_banned_edge_is_avoided_in_gravel_but_taken_in_mtb() {
    let e = engine_with(
        &mut corridors(&[("surface", "gravel"), ("smoothness", "horrible")]),
        params(Rideability::On),
    );
    assert!(!uses_north(&e, &one_way(RideType::Gravel), None), "gravel : le couloir sud");
    assert!(uses_north(&e, &one_way(RideType::Mtb), None), "VTT : `horrible` roule");
    // Sans bornes : le même graphe se prend par le nord, 1.14.
    let off = engine_with(
        &mut corridors(&[("surface", "gravel"), ("smoothness", "horrible")]),
        params(Rideability::Off),
    );
    assert!(uses_north(&off, &one_way(RideType::Gravel), None));
    // Et le VTT s'interdit `mtb:scale` ≥ 4 (code 5).
    let hard =
        engine_with(&mut corridors(&[("surface", "gravel"), ("mtb:scale", "4")]), params(Rideability::On));
    assert!(!uses_north(&hard, &one_way(RideType::Mtb), None));
    let ok =
        engine_with(&mut corridors(&[("surface", "gravel"), ("mtb:scale", "3")]), params(Rideability::On));
    assert!(uses_north(&ok, &one_way(RideType::Mtb), None), "mtb:scale 3 roule en VTT");
    assert!(!uses_north(&ok, &one_way(RideType::Gravel), None), "mais pas en gravel (≥ 2)");
}

#[test]
fn off_is_the_engine_of_1_14_and_the_road_view_is_never_marked() {
    let bad = [("surface", "gravel"), ("smoothness", "impassable")];
    let off = engine_with(&mut corridors(&bad), params(Rideability::Off));
    let win = off.full_window(1);
    assert!(win.v.unrideable.is_empty());
    let w = Weighter::new(&win.v, None, RideType::Gravel, Mode::Discovery, 0.5, 768);
    assert!((0..win.v.n_chains as u32).all(|c| w.chain(c) != INF));
    let on = engine_with(&mut corridors(&bad), params(Rideability::On));
    assert!(on.full_window(0).v.unrideable.is_empty(), "la vue route n'a pas de borne");
    assert!(!on.full_window(1).v.unrideable.is_empty());
}

/// Poids de la chaîne du tronçon nord, pour un contexte d'équipe et un mode.
fn north_weight(
    e: &Engine<Vec<u8>>,
    ride: RideType,
    mode: Mode,
    rows: Option<&[(u32, Counts)]>,
) -> (u32, u32) {
    let win = e.full_window(1);
    let north = edge_between(e, 1, 3);
    let c = chain_of_edge(&win.v, north);
    let Some(rows) = rows else {
        let w = Weighter::new(&win.v, None, ride, mode, 0.5, 768);
        return (w.chain(c), w.edge(e, north));
    };
    let team = TeamData::build(e, &overlay_of(e, rows)).unwrap();
    let wt = WindowTeam::build(e, &win, &team);
    let w = Weighter::new(&win.v, Some(TeamCtx { data: &team, win: &wt }), ride, mode, 0.5, 768);
    (w.chain(c), w.edge(e, north))
}

#[test]
fn team_exemption_needs_a_ride_of_the_type_and_never_frees_impassable() {
    let soft = [("surface", "gravel"), ("smoothness", "horrible")];
    let e = engine_with(&mut corridors(&soft), params(Rideability::On));
    let north = edge_between(&e, 1, 3);
    let g = |row: Counts, ride, mode| north_weight(&e, ride, mode, Some(&[(north, row)]));
    for mode in [Mode::TeamFirst, Mode::Discovery] {
        // Un parcours GRAVEL ou MIXED libère l'arête, la chaîne et l'arête partielle ; ROAD seul, non (§6.2).
        for row in [GRAVEL1, MIXED1] {
            let (c, p) = g(row, RideType::Gravel, mode);
            assert!(c != INF && p != INF, "{mode:?} : un parcours du type libère");
        }
        let (c, p) = g(ROAD1, RideType::Gravel, mode);
        assert!(c == INF && p == INF, "{mode:?} : un parcours ROAD ne libère pas");
        let (c, p) = g(MTB1, RideType::Gravel, mode);
        assert!(c == INF && p == INF, "{mode:?} : un parcours VTT ne libère pas le gravel");
    }
    // Sans surcouche, aucune exemption.
    let (c, p) = north_weight(&e, RideType::Gravel, Mode::Discovery, None);
    assert!(c == INF && p == INF);
    // Impassable : jamais, même d'équipe.
    let hard = engine_with(
        &mut corridors(&[("surface", "gravel"), ("smoothness", "impassable")]),
        params(Rideability::On),
    );
    let north = edge_between(&hard, 1, 3);
    for row in [GRAVEL1, MIXED1, MTB1] {
        for ride in [RideType::Gravel, RideType::Mtb] {
            let (c, p) = north_weight(&hard, ride, Mode::TeamFirst, Some(&[(north, row)]));
            assert!(c == INF && p == INF, "impassable, {ride:?}");
        }
    }
}

#[test]
fn team_exemption_in_mtb_accepts_mtb_gravel_or_mixed_rides() {
    let e =
        engine_with(&mut corridors(&[("surface", "gravel"), ("mtb:scale", "4")]), params(Rideability::On));
    let north = edge_between(&e, 1, 3);
    for row in [MTB1, GRAVEL1, MIXED1] {
        let (c, p) = north_weight(&e, RideType::Mtb, Mode::TeamFirst, Some(&[(north, row)]));
        assert!(c != INF && p != INF);
    }
    let (c, p) = north_weight(&e, RideType::Mtb, Mode::TeamFirst, Some(&[(north, ROAD1)]));
    assert!(c == INF && p == INF);
}

#[test]
fn a_gravel_ride_through_the_exempted_edge_is_taken() {
    let e = engine_with(
        &mut corridors(&[("surface", "gravel"), ("smoothness", "horrible")]),
        params(Rideability::On),
    );
    let win = e.full_window(1);
    let north = edge_between(&e, 1, 3);
    let r = one_way(RideType::Gravel);
    for (row, expect) in [(GRAVEL1, true), (ROAD1, false)] {
        let team = TeamData::build(&e, &overlay_of(&e, &[(north, row)])).unwrap();
        let wt = WindowTeam::build(&e, &win, &team);
        let mut r = r.clone();
        r.mode = Mode::TeamFirst;
        assert_eq!(uses_north(&e, &r, Some(TeamCtx { data: &team, win: &wt })), expect);
    }
}

/// Strict : les bornes n'ont aucun effet, l'arête d'équipe (de n'importe quel type, comme en 1.14) reste prise.
#[test]
fn strict_mode_ignores_the_bounds() {
    let e = engine_with(
        &mut corridors(&[("surface", "gravel"), ("smoothness", "impassable")]),
        params(Rideability::On),
    );
    let north = edge_between(&e, 1, 3);
    let rows = [(north, ROAD1), (edge_between(&e, 3, 2), ROAD1)];
    let (c, p) = north_weight(&e, RideType::Gravel, Mode::Strict, Some(&rows));
    assert!(c != INF && p != INF, "strict : arête d'équipe prise, même impassable : {c} {p}");
    // Le même poids qu'avec les bornes coupées.
    let off = engine_with(
        &mut corridors(&[("surface", "gravel"), ("smoothness", "impassable")]),
        params(Rideability::Off),
    );
    let rows = [(edge_between(&off, 1, 3), ROAD1), (edge_between(&off, 3, 2), ROAD1)];
    assert_eq!(north_weight(&off, RideType::Gravel, Mode::Strict, Some(&rows)), (c, p));
}
