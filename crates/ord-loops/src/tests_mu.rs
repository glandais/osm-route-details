//! Le multiplicateur μ du non-revêtu (lot 5, §3.4, §4.2 et §11) : le multiplicateur du non-revêtu, sur un graphe à deux couloirs
//! parallèles de même longueur (l'un résidentiel, l'autre en `track` `gravel`).

use ord_graph::build::MemSource;

use crate::cost::RideType;
use crate::generate::{finish, generate_with_figures_in, run_step};
use crate::search::{Weighter, INF};
use crate::team::{TeamCtx, TeamData, WindowTeam};
use crate::tests::{
    chain_of_edge, edge_between, engine_of, ll, overlay_of, pt, request, tags, Counts, GRAVEL1,
};
use crate::{Engine, Mode, Request, Workspace};

/// Deux couloirs entre `1` et `2` : `1-3-2` résidentiel (au nord), `1-4-2` en piste gravillonnée (au sud), de même
/// longueur ; des impasses de 300 m en `5` et `6` portent le départ et l'arrivée.
fn two_corridors() -> MemSource {
    let mut s = MemSource::default();
    pt(&mut s, 1, 0.0, 0.0);
    pt(&mut s, 2, 0.0, 3000.0);
    pt(&mut s, 3, 700.0, 1500.0);
    pt(&mut s, 4, -700.0, 1500.0);
    pt(&mut s, 5, 0.0, -300.0);
    pt(&mut s, 6, 0.0, 3300.0);
    s.way(101, &[1, 3], &tags("residential", &[]));
    s.way(105, &[3, 2], &tags("residential", &[]));
    s.way(102, &[1, 4], &tags("track", &[("surface", "gravel")]));
    s.way(106, &[4, 2], &tags("track", &[("surface", "gravel")]));
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

/// Part de la longueur du premier parcours rendu qui roule sur des arêtes `offroad`.
fn share(e: &Engine<Vec<u8>>, r: &Request, mu_q8: Option<u32>) -> f64 {
    let win = e.full_window(r.ride_type.view_index());
    let c = run_step(e, &win, None, r, &mut Workspace::new(), &mut || false, mu_q8).0.expect("des parcours");
    let routes = finish(e, r, c.scored, c.organic, None);
    let (mut off, mut all) = (0.0, 0.0);
    for s in &routes[0].steps {
        let ev = e.base().edge(s.base_edge);
        let l = f64::from(ev.len_dm);
        all += l;
        if ev.attrs.offroad() {
            off += l;
        }
    }
    off / all
}

#[test]
fn the_corridor_graph_has_one_paved_and_one_unpaved_way() {
    let e = engine_of(&mut two_corridors());
    assert!(!e.base().edge(edge_between(&e, 1, 3)).attrs.offroad());
    assert!(e.base().edge(edge_between(&e, 1, 4)).attrs.offroad());
}

#[test]
fn mu_one_is_bit_for_bit_the_plain_engine() {
    let e = engine_of(&mut two_corridors());
    let r = gravel_one_way();
    let win = e.full_window(1);
    let plain = generate_with_figures_in(&e, &win, None, &r, &mut Workspace::new(), &mut || false).0.unwrap();
    let c = run_step(&e, &win, None, &r, &mut Workspace::new(), &mut || false, Some(256)).0.unwrap();
    let one = finish(&e, &r, c.scored, c.organic, None);
    assert_eq!(plain.routes.len(), one.len());
    for (a, b) in plain.routes.iter().zip(&one) {
        assert_eq!(a.steps, b.steps);
        assert_eq!(a.length_m.to_bits(), b.length_m.to_bits());
    }
    // Et chaque poids de chaîne et d'arête, sans surcouche (le `return w`) comme avec : même nombre.
    let v = &win.v;
    let (a, b) = (
        Weighter::new(v, None, RideType::Gravel, Mode::Discovery, 0.5, 768),
        Weighter::new(v, None, RideType::Gravel, Mode::Discovery, 0.5, 768).with_mu_q8(Some(256)),
    );
    for c in 0..v.n_chains as u32 {
        assert_eq!(a.chain(c), b.chain(c));
    }
}

#[test]
fn the_unpaved_share_grows_as_mu_drops() {
    let e = engine_of(&mut two_corridors());
    let r = gravel_one_way();
    // La grille de la sonde, du plus cher au moins cher.
    let shares: Vec<f64> =
        [1024u32, 512, 358, 256, 179, 128, 90, 64].iter().map(|&mu| share(&e, &r, Some(mu))).collect();
    for w in shares.windows(2) {
        assert!(w[1] >= w[0] - 1e-9, "la part ne décroît jamais quand μ baisse : {shares:?}");
    }
    assert!(shares[0] < 0.01, "μ = 4 : le couloir revêtu, {shares:?}");
    assert!(shares[7] > 0.5, "μ = 1/4 : la piste, {shares:?}");
}

/// Les trois branches de `chain` portent μ : sans surcouche, en équipe d'abord, et en strict ; une chaîne revêtue n'est
/// jamais touchée, `INF` reste `INF`.
#[test]
fn mu_scales_the_unpaved_chain_in_every_branch_and_leaves_the_paved_one() {
    let e = engine_of(&mut two_corridors());
    let win = e.full_window(1);
    let v = &win.v;
    let c_off = chain_of_edge(v, edge_between(&e, 1, 4));
    let c_pav = chain_of_edge(v, edge_between(&e, 1, 3));
    assert!(v.off_q8[c_off as usize] >= 250, "le couloir en piste : {}", v.off_q8[c_off as usize]);
    assert_eq!(v.off_q8[c_pav as usize], 0);
    let rows: Vec<(u32, Counts)> = vec![
        (edge_between(&e, 1, 4), GRAVEL1),
        (edge_between(&e, 4, 2), GRAVEL1),
        (edge_between(&e, 5, 1), GRAVEL1),
    ];
    let team = TeamData::build(&e, &overlay_of(&e, &rows)).unwrap();
    let wt = WindowTeam::build(&e, &win, &team);
    let tc = TeamCtx { data: &team, win: &wt };
    for mode in [Mode::Discovery, Mode::TeamFirst, Mode::Strict] {
        let with_team = mode != Mode::Discovery;
        let mk = |mu: Option<u32>| {
            Weighter::new(v, with_team.then_some(tc), RideType::Gravel, mode, 0.5, 768).with_mu_q8(mu)
        };
        let (n, lo, hi) = (mk(None), mk(Some(64)), mk(Some(1024)));
        assert!(lo.chain(c_off) < n.chain(c_off), "{mode:?} : μ < 1 allège la piste");
        assert!(hi.chain(c_off) > n.chain(c_off), "{mode:?} : μ > 1 l'alourdit");
        assert_eq!(lo.chain(c_pav), n.chain(c_pav), "{mode:?} : le revêtu n'est pas touché");
        assert_eq!(hi.chain(c_pav), n.chain(c_pav));
    }
    // Strict, hors réseau d'équipe : la chaîne reste infinie sous μ.
    let strict = Weighter::new(v, Some(tc), RideType::Gravel, Mode::Strict, 0.5, 768).with_mu_q8(Some(64));
    assert_eq!(strict.chain(c_pav), INF);
    // Les arêtes partielles du départ et de l'arrivée suivent la même règle.
    let (n, lo) = (
        Weighter::new(v, None, RideType::Gravel, Mode::Discovery, 0.5, 768),
        Weighter::new(v, None, RideType::Gravel, Mode::Discovery, 0.5, 768).with_mu_q8(Some(64)),
    );
    let (eo, ep) = (edge_between(&e, 1, 4), edge_between(&e, 1, 3));
    assert!(lo.edge(&e, eo) < n.edge(&e, eo));
    assert_eq!(lo.edge(&e, ep), n.edge(&e, ep));
}

/// La vue route n'a pas de tableau `f` : μ n'y touche ni les chaînes ni les arêtes partielles (même règle des deux
/// côtés, sinon μ n'agirait que sur le départ et l'arrivée).
#[test]
fn mu_has_no_effect_on_the_road_view_not_even_on_partial_edges() {
    let e = engine_of(&mut two_corridors());
    let win = e.full_window(0);
    let v = &win.v;
    assert!(v.off_q8.is_empty());
    let mk =
        |mu: Option<u32>| Weighter::new(v, None, RideType::Road, Mode::Discovery, 0.5, 768).with_mu_q8(mu);
    let (n, lo, hi) = (mk(None), mk(Some(64)), mk(Some(1024)));
    for c in 0..v.n_chains as u32 {
        assert_eq!(lo.chain(c), n.chain(c));
        assert_eq!(hi.chain(c), n.chain(c));
    }
    let eo = edge_between(&e, 1, 4);
    assert!(e.base().edge(eo).attrs.offroad());
    assert_eq!(lo.edge(&e, eo), n.edge(&e, eo));
    assert_eq!(hi.edge(&e, eo), n.edge(&e, eo));
}

/// Bornes de l'arithmétique entière (§11) : aucun débordement aux poids maxima, 256 neutre, monotone en μ et en `f`,
/// toujours hors de `INF`.
#[test]
fn mu_arithmetic_is_exact_and_stays_in_range() {
    let e = engine_of(&mut two_corridors());
    let win = e.full_window(1);
    let w = |mu: u32| {
        Weighter::new(&win.v, None, RideType::Gravel, Mode::Discovery, 0.5, 768).with_mu_q8(Some(mu))
    };
    let probe = w(64);
    for wt in [0u32, 1, 3, 255, 256, 1_000_000, u32::MAX / 8, u32::MAX / 2 - 1] {
        for f in [0u8, 1, 128, 254, 255] {
            let mut last = 0;
            for mu in [64u32, 90, 128, 255, 256, 257, 512, 1024] {
                let x = w(mu).mu_of(wt, f);
                assert!(x <= u32::MAX / 2, "w = {wt}, f = {f}, μ = {mu} : {x}");
                assert!(x >= last, "croissant en μ");
                if mu == 256 || f == 0 {
                    assert_eq!(x, wt, "neutre");
                }
                last = x;
            }
        }
    }
    // ⌈⌉ : 3 × (256 + (128 − 256) × 255 / 256) / 256 = 3 × 128,5 / 256 ≈ 1,506, soit 2.
    assert_eq!(probe.mu_of(3, 255), 1);
    assert_eq!(w(128).mu_of(3, 255), 2);
}
