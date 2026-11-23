//! La consigne de part de non-revêtu (lot 5, §4.3, §4.7 et §11) : la recherche de μ, la mise en commun des candidats, le pas
//! coupé, la cible inatteignable. Graphe de deux couloirs parallèles de même longueur, l'un résidentiel, l'autre en piste
//! gravillonnée : la part d'un parcours y vaut 0 ou 0,85, rien entre les deux.

use std::cell::Cell;

use ord_graph::build::MemSource;

use crate::cost::RideType;
use crate::team::{TeamCtx, TeamData, WindowTeam};
use crate::tests::{edge_between, engine_of, ll, overlay_of, pt, request, tags, Counts, GRAVEL1};
use crate::{
    generate_steered_in, generate_with_figures_in, Engine, Failure, Mode, OffroadTarget, Request, Steer,
    Workspace,
};

/// Deux couloirs entre `1` et `2` (`1-3-2` au nord, `1-4-2` au sud) ; impasses résidentielles de 300 m en `5` et `6`. Le
/// couloir sud est en piste gravillonnée si `south_unpaved`.
fn corridors(south_unpaved: bool) -> MemSource {
    corridors_of(
        Corridor { highway: "residential", unpaved: false, east: 700.0 },
        Corridor {
            highway: if south_unpaved { "track" } else { "residential" },
            unpaved: south_unpaved,
            east: -700.0,
        },
    )
}

/// Un couloir : sa classe, en gravier ou non, son écart latéral au milieu (m, signé ; ±700 : 3,3 km, ±1 500 : 4,2 km, hors
/// de la tolérance de distance de [`gravel_one_way`]).
struct Corridor {
    highway: &'static str,
    unpaved: bool,
    east: f64,
}

fn corridors_of(north: Corridor, south: Corridor) -> MemSource {
    let mut s = MemSource::default();
    pt(&mut s, 1, 0.0, 0.0);
    pt(&mut s, 2, 0.0, 3000.0);
    pt(&mut s, 3, north.east, 1500.0);
    pt(&mut s, 4, south.east, 1500.0);
    pt(&mut s, 5, 0.0, -300.0);
    pt(&mut s, 6, 0.0, 3300.0);
    let way = |s: &mut MemSource, id: i64, nodes: [i64; 2], c: &Corridor| {
        let extra: &[(&str, &str)] = if c.unpaved { &[("surface", "gravel")] } else { &[] };
        s.way(id, &nodes, &tags(c.highway, extra));
    };
    way(&mut s, 101, [1, 3], &north);
    way(&mut s, 105, [3, 2], &north);
    way(&mut s, 102, [1, 4], &south);
    way(&mut s, 106, [4, 2], &south);
    s.way(103, &[5, 1], &tags("residential", &[]));
    s.way(104, &[2, 6], &tags("residential", &[]));
    s
}

fn gravel_one_way() -> Request {
    let mut r = request(ll(0.0, -300.0), 4150.0, Mode::Discovery);
    r.end = Some(ll(0.0, 3300.0));
    r.ride_type = RideType::Gravel;
    r
}

fn target(milli: u16, max_steps: u8, until_ms: u32) -> Steer {
    Steer {
        offroad: Some(OffroadTarget {
            target_milli: milli,
            tolerance_milli: 50,
            max_steps,
            steps_until_ms: until_ms,
        }),
    }
}

fn share_of(e: &Engine<Vec<u8>>, r: &crate::Route) -> f64 {
    let (mut off, mut all) = (0.0, 0.0);
    for s in &r.steps {
        let ev = e.base().edge(s.base_edge);
        all += f64::from(ev.len_dm);
        if ev.attrs.offroad() {
            off += f64::from(ev.len_dm);
        }
    }
    off / all
}

fn steered(
    e: &Engine<Vec<u8>>,
    r: &Request,
    steer: &Steer,
    stop: &mut dyn FnMut() -> bool,
) -> Result<crate::SteeredGeneration, Failure> {
    let win = e.full_window(r.ride_type.view_index());
    generate_steered_in(e, &win, None, r, steer, &mut Workspace::new(), stop).0
}

#[test]
fn no_steer_is_the_plain_engine() {
    let e = engine_of(&mut corridors(true));
    let r = gravel_one_way();
    let win = e.full_window(1);
    let plain = generate_with_figures_in(&e, &win, None, &r, &mut Workspace::new(), &mut || false).0.unwrap();
    let s = steered(&e, &r, &Steer::NONE, &mut || false).unwrap();
    assert!(s.offroad.is_none());
    assert_eq!(format!("{:?}", plain), format!("{:?}", s.generation));
}

#[test]
fn a_target_on_a_road_request_is_refused() {
    let e = engine_of(&mut corridors(true));
    let mut r = gravel_one_way();
    r.ride_type = RideType::Road;
    let win = e.full_window(0);
    let res =
        generate_steered_in(&e, &win, None, &r, &target(380, 4, 10_000), &mut Workspace::new(), &mut || {
            false
        })
        .0;
    assert_eq!(res.err(), Some(Failure::OffroadOnRoad));
}

/// La cible d'une piste (0,85) : la piste l'emporte déjà à μ = 1 (× 0,75 du gravel), le pas 0 y est joué (le prior, μ = 1/4,
/// est plus bas que 1) et l'atteint.
#[test]
fn a_reachable_target_is_reached_at_the_first_step() {
    let e = engine_of(&mut corridors(true));
    let r = gravel_one_way();
    let s = steered(&e, &r, &target(850, 4, 10_000), &mut || false).unwrap();
    let out = s.offroad.unwrap();
    assert!(out.reached, "{out:?}");
    assert_eq!(out.steps, 1);
    assert_eq!(out.mu_q8, 256);
    assert!(share_of(&e, &s.generation.routes[0]) > 0.8);
    // Cible 0 : le couloir revêtu, μ = 2 au pas 0 (un prior au-dessus de 1 est joué tel quel).
    let s = steered(&e, &r, &target(0, 4, 10_000), &mut || false).unwrap();
    let out = s.offroad.unwrap();
    assert!(out.reached && out.steps == 1 && out.mu_q8 == 512, "{out:?}");
    assert!(share_of(&e, &s.generation.routes[0]) < 0.05);
}

/// C4 (relevé du 23 novembre, §10.5) : un pas à μ < 1 coûte plus cher que la génération sans cible ; le pas 0, seul à
/// garder le budget entier du service, n'est donc jamais joué sous μ = 1, et y est **la génération sans cible** : mêmes
/// candidats, même stop. Le prior est joué au pas 1 quand la part est trop basse.
#[test]
fn step_zero_is_never_below_one_and_the_prior_comes_next() {
    // Piste gravillonnée au sud, voie cyclable revêtue au nord : à μ = 1 la voie cyclable (48 contre 54), à μ = 1/4 la piste.
    let e = engine_of(&mut corridors_of(
        Corridor { highway: "cycleway", unpaved: false, east: 700.0 },
        Corridor { highway: "track", unpaved: true, east: -700.0 },
    ));
    let r = gravel_one_way();
    let win = e.full_window(1);
    let plain = generate_with_figures_in(&e, &win, None, &r, &mut Workspace::new(), &mut || false).0.unwrap();
    assert!(share_of(&e, &plain.routes[0]) < 0.05, "précondition : la voie cyclable sans cible");
    for t in [470, 600, 850] {
        // Un seul pas : le pas 0, à μ = 1, rend les parcours de la génération sans cible.
        let one = steered(&e, &r, &target(t, 4, 0), &mut || false).unwrap();
        let out = one.offroad.unwrap();
        assert_eq!((out.steps, out.mu_q8), (1, 256), "cible {t}");
        let mut a: Vec<String> = one.generation.routes.iter().map(|x| format!("{:?}", x.steps)).collect();
        let mut b: Vec<String> = plain.routes.iter().map(|x| format!("{:?}", x.steps)).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b, "cible {t}");
    }
    // Quatre pas permis : le prior (μ = 1/4 pour 0,85) au pas 1, qui trouve la piste.
    let s = steered(&e, &r, &target(850, 4, 10_000), &mut || false).unwrap();
    let out = s.offroad.unwrap();
    assert!(out.reached, "{out:?}");
    assert_eq!((out.steps, out.mu_q8), (2, 64));
    assert!(share_of(&e, &s.generation.routes[0]) > 0.8);
}

/// C4, la Galles à 40 km : aucun candidat à μ < 1 (les chemins y font sortir de la tolérance de distance), un candidat à
/// μ = 1. Avant, la recherche partait du prior (μ = 0,35 pour 0,60) et épuisait ses quatre pas sous 1 : 422 là où la
/// requête sans cible rendait 200. Avec une consigne, la réponse n'est jamais pire que sans.
#[test]
fn a_target_never_fails_where_the_plain_engine_succeeds() {
    // Au nord le couloir résidentiel dans la tolérance ; au sud une piste de 4,2 km, hors tolérance, moins chère dès μ = 0,7.
    let e = engine_of(&mut corridors_of(
        Corridor { highway: "residential", unpaved: false, east: 700.0 },
        Corridor { highway: "track", unpaved: true, east: -1500.0 },
    ));
    let r = gravel_one_way();
    let win = e.full_window(1);
    let at = |mu| {
        crate::generate::run_step(&e, &win, None, &r, &mut Workspace::new(), &mut || false, Some(mu))
            .0
            .is_ok()
    };
    assert!(at(256) && !at(179) && !at(128) && !at(90) && !at(64), "précondition");
    for t in [470, 600, 800] {
        for max in [1u8, 4] {
            let s = steered(&e, &r, &target(t, max, 10_000), &mut || false).unwrap();
            let out = s.offroad.unwrap();
            assert!(!out.reached && out.mu_q8 == 256, "cible {t}, {max} pas : {out:?}");
            assert!(share_of(&e, &s.generation.routes[0]) < 0.05);
        }
    }
}

/// Le dernier recours : aucun pas n'a de candidat et μ = 1 n'a pas été joué (cible basse, prior au-dessus de 1, un seul pas
/// permis ou aucun délai) : la génération sans cible est jouée, dans le budget du service seul.
#[test]
fn when_no_step_has_a_candidate_mu_one_is_played() {
    // Au nord une piste dans la tolérance ; au sud le résidentiel de 4,2 km, hors tolérance, moins cher à μ = 2.
    let e = engine_of(&mut corridors_of(
        Corridor { highway: "track", unpaved: true, east: 700.0 },
        Corridor { highway: "residential", unpaved: false, east: -1500.0 },
    ));
    let r = gravel_one_way();
    let win = e.full_window(1);
    let at = |mu| {
        crate::generate::run_step(&e, &win, None, &r, &mut Workspace::new(), &mut || false, Some(mu))
            .0
            .is_ok()
    };
    assert!(at(256) && !at(512), "précondition");
    for (max, until) in [(1u8, 10_000), (4, 0)] {
        let s = steered(&e, &r, &target(0, max, until), &mut || false).unwrap();
        let out = s.offroad.unwrap();
        assert!(!out.reached, "{out:?}");
        assert_eq!((out.steps, out.mu_q8), (2, 256), "{max} pas, {until} ms : {out:?}");
        assert!(share_of(&e, &s.generation.routes[0]) > 0.8);
    }
    // Le service a coupé : le dernier recours n'est pas joué, le verdict des pas joués tient (jamais un 504 après eux).
    let calls = Cell::new(0u32);
    let n0 = {
        let _ = steered(&e, &r, &target(0, 1, 0), &mut || {
            calls.set(calls.get() + 1);
            false
        });
        calls.get()
    };
    let k = Cell::new(0u32);
    let cut = steered(&e, &r, &target(0, 1, 0), &mut || {
        k.set(k.get() + 1);
        k.get() > n0 / 2 + 1
    });
    assert_eq!(cut.err(), Some(Failure::DistanceTooShort));
}

/// Aucun pas k ≥ 1 n'est lancé si le temps écoulé plus sa durée estimée, au moins une fois et demie le plus long pas joué
/// (le pas 0 compris), dépasse `until`.
#[test]
fn a_step_is_launched_only_if_it_fits_before_the_limit() {
    use std::time::Duration;
    let ms = Duration::from_millis;
    assert!(crate::offroad::fits(ms(1_000), ms(2_000), ms(4_000)));
    assert!(!crate::offroad::fits(ms(1_000), ms(2_000), ms(3_999)));
    assert!(!crate::offroad::fits(ms(0), ms(1), ms(0)));
    // Un pas 0 lent (le stop dort pendant ses appels), des pas suivants rapides : `until` à deux fois le pas 0, aucun pas 1.
    let e = engine_of(&mut corridors(true));
    let r = gravel_one_way();
    let calls = Cell::new(0u32);
    let _ = steered(&e, &r, &target(600, 1, 0), &mut || {
        calls.set(calls.get() + 1);
        false
    });
    let n0 = calls.get();
    let slow = |k: &Cell<u32>| {
        k.set(k.get() + 1);
        if k.get() <= n0 {
            std::thread::sleep(ms(25));
        }
        false
    };
    let k = Cell::new(0u32);
    let t = std::time::Instant::now();
    let s = steered(&e, &r, &target(600, 4, 0), &mut || slow(&k)).unwrap();
    let dur0 = t.elapsed();
    assert_eq!(s.offroad.unwrap().steps, 1);
    let until = u32::try_from((dur0 * 2).as_millis()).unwrap();
    let k = Cell::new(0u32);
    let s = steered(&e, &r, &target(600, 4, until), &mut || slow(&k)).unwrap();
    assert_eq!(s.offroad.unwrap().steps, 1, "pas 0 de {dur0:?}, délai de {until} ms");
    // Avec de la marge, la recherche continue.
    let k = Cell::new(0u32);
    let s = steered(&e, &r, &target(600, 4, until * 5), &mut || slow(&k)).unwrap();
    assert!(s.offroad.unwrap().steps > 1);
}

/// Cible entre les deux couloirs : le pas 0 (μ = 1, la piste à 0,85) est trop haut, la recherche essaie des μ plus chers, au
/// plus quatre pas, et le candidat du pas 0 (plus proche de 0,60 que le couloir revêtu) est rendu : la mise en commun.
#[test]
fn the_pool_keeps_the_closest_candidate_of_any_step() {
    let e = engine_of(&mut corridors(true));
    let r = gravel_one_way();
    // Précondition : à μ = 2, le couloir revêtu ; à μ = 1, la piste.
    let win = e.full_window(1);
    let at = |mu| {
        let c =
            crate::generate::run_step(&e, &win, None, &r, &mut Workspace::new(), &mut || false, Some(mu)).0;
        let routes = crate::generate::finish(&e, &r, c.unwrap().scored, None, None);
        share_of(&e, &routes[0])
    };
    assert!(at(512) < 0.05 && at(256) > 0.8);
    let s = steered(&e, &r, &target(600, 4, 10_000), &mut || false).unwrap();
    let out = s.offroad.unwrap();
    assert!(!out.reached);
    assert!((2..=4).contains(&out.steps), "{out:?}");
    assert_eq!(out.mu_q8, 256, "le candidat du pas 0 gagne");
    assert!(share_of(&e, &s.generation.routes[0]) > 0.8);
}

/// Aucun chemin dans le secteur, cible 0,5 : on rend le parcours le plus proche, `reached = false`, jamais un refus ; au
/// plus quatre pas.
#[test]
fn an_unreachable_target_gives_the_closest_route() {
    let e = engine_of(&mut corridors(false));
    let r = gravel_one_way();
    for max in [1u8, 2, 4, 9] {
        let s = steered(&e, &r, &target(500, max, 10_000), &mut || false).unwrap();
        let out = s.offroad.unwrap();
        assert!(!out.reached);
        // Rien ne rapproche la part de la cible : la recherche joue tous les pas permis, sur la grille de sept points.
        assert_eq!(out.steps, max.min(7), "{out:?}");
        assert!(!s.generation.routes.is_empty());
        assert!(share_of(&e, &s.generation.routes[0]) < 0.05);
    }
}

#[test]
fn zero_steps_until_runs_a_single_step() {
    let e = engine_of(&mut corridors(true));
    let r = gravel_one_way();
    let s = steered(&e, &r, &target(600, 4, 0), &mut || false).unwrap();
    assert_eq!(s.offroad.unwrap().steps, 1);
    assert!(!s.generation.routes.is_empty());
}

/// Le stop du service : levé pendant le pas 0, `Stopped` ; levé entre deux pas ou pendant un pas k ≥ 1, la recherche
/// rend ce que le pas 0 avait trouvé (jamais un 504 pour une génération qui a abouti), le pas coupé jeté.
#[test]
fn a_cut_step_is_discarded_and_never_turns_a_result_into_a_failure() {
    let e = engine_of(&mut corridors(true));
    let r = gravel_one_way();
    let calls = Cell::new(0u32);
    let one = steered(&e, &r, &target(600, 1, 10_000), &mut || {
        calls.set(calls.get() + 1);
        false
    })
    .unwrap();
    let n0 = calls.get();
    assert!(n0 >= 4, "le pas 0 appelle le stop {n0} fois");
    assert!(steered(&e, &r, &target(600, 4, 10_000), &mut || true).is_err());
    // Entre le pas 0 et le pas 1.
    let k = Cell::new(0u32);
    let between = steered(&e, &r, &target(600, 4, 10_000), &mut || {
        k.set(k.get() + 1);
        k.get() > n0
    })
    .unwrap();
    assert_eq!(between.offroad.unwrap().steps, 1);
    assert_eq!(format!("{:?}", between.generation.routes), format!("{:?}", one.generation.routes));
    // Au milieu du pas 1.
    let k = Cell::new(0u32);
    let mid = steered(&e, &r, &target(600, 4, 10_000), &mut || {
        k.set(k.get() + 1);
        k.get() > n0 + 3
    })
    .unwrap();
    assert_eq!(mid.offroad.unwrap().steps, 1);
    assert_eq!(format!("{:?}", mid.generation.routes), format!("{:?}", one.generation.routes));
}

/// Mode strict : la cible est acceptée et sans effet, un seul pas à μ = 1, `reached` dit la part obtenue.
#[test]
fn strict_mode_does_not_search() {
    let e = engine_of(&mut corridors(true));
    let rows: Vec<(u32, Counts)> =
        [(5, 1), (1, 4), (4, 2), (2, 6)].iter().map(|&(a, b)| (edge_between(&e, a, b), GRAVEL1)).collect();
    let team = TeamData::build(&e, &overlay_of(&e, &rows)).unwrap();
    let win = e.full_window(1);
    let wt = WindowTeam::build(&e, &win, &team);
    let tc = TeamCtx { data: &team, win: &wt };
    let mut r = gravel_one_way();
    r.mode = Mode::Strict;
    let res = generate_steered_in(
        &e,
        &win,
        Some(tc),
        &r,
        &target(0, 4, 10_000),
        &mut Workspace::new(),
        &mut || false,
    )
    .0
    .unwrap();
    let out = res.offroad.unwrap();
    assert_eq!((out.steps, out.mu_q8), (1, 256));
    assert!(!out.reached, "la piste est le seul réseau d'équipe : part 0,85 pour une cible de 0");
    let plain =
        generate_with_figures_in(&e, &win, Some(tc), &r, &mut Workspace::new(), &mut || false).0.unwrap();
    assert_eq!(format!("{:?}", plain.routes), format!("{:?}", res.generation.routes));
}
