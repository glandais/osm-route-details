//! Le profil de goût d'une équipe appris au dépôt (lot 4, 3b) : graphe et dépôts **synthétiques**.
//!
//! Des anneaux de 7,2 km (six rues de 300 m par côté) font les boucles d'apprentissage : au-dessus des 5 km de retenue,
//! sur la grille de 21 × 21 carrefours des tests du générateur.

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::http::StatusCode;
use axum::Router;
use common::generator::*;
use common::*;
use ord_graph_match::geo::{encode_polyline6, haversine};
use ord_server::config::Config;
use ord_server::{router, AppState};
use serde_json::{json, Value};

struct World {
    _dir: tempfile::TempDir,
    state: AppState,
    app: Router,
    files: Files,
}

impl World {
    fn new() -> Self {
        Self::with(|_| {})
    }

    fn with(tweak: impl FnOnce(&mut Config)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let files = write_files(dir.path(), "g1", 21);
        let mut config = config_with(test_config(), dir.path(), &files);
        tweak(&mut config);
        let state = AppState::new(&config).expect("état");
        let app = router(state.clone());
        World { _dir: dir, state, app, files }
    }

    async fn send(&self, req: axum::http::Request<axum::body::Body>) -> Reply {
        send(self.app.clone(), req).await
    }

    async fn deposit(&self, key: &str, team: &str, body: String) -> Reply {
        self.send(put(key, team, body)).await
    }

    async fn status(&self, key: &str, team: &str) -> Value {
        let r = self.send(status(key, team)).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
        r.json()
    }

    fn fingerprint(&self, key: &str, team: &str) -> Option<(String, u8)> {
        self.state.generator().test_profile_fingerprint(key, team)
    }
}

/// Un anneau carré de `side` rues dont le coin sud-ouest est `(r0, c0)`, en pièces de trois arêtes.
fn ring_document(r0: usize, c0: usize, side: usize) -> Value {
    let mut path: Vec<[f64; 2]> = Vec::new();
    for c in c0..=c0 + side {
        path.push(node(r0, c));
    }
    for r in r0 + 1..=r0 + side {
        path.push(node(r, c0 + side));
    }
    for c in (c0..c0 + side).rev() {
        path.push(node(r0 + side, c));
    }
    for r in (r0 + 1..r0 + side).rev() {
        path.push(node(r, c0));
    }
    path.push(node(r0, c0));
    let mut edges = Vec::new();
    for piece in path.windows(4).step_by(3) {
        let trv: f64 = piece.windows(2).map(|w| haversine(w[0], w[1])).sum();
        edges.push(json!({"g": encode_polyline6(piece), "trv": trv, "wk": "minor_road"}));
    }
    json!({"v": 1, "api": "1.8", "edges": edges})
}

/// Un dépôt de quelques anneaux : une trace par anneau.
fn loops_deposit(starts: &[(usize, usize)]) -> String {
    let lines: Vec<String> =
        starts.iter().map(|&(r, c)| track_line("ROAD", false, &ring_document(r, c, 6))).collect();
    ndjson(&lines, Some(lines.len() as u64))
}

const RINGS: [(usize, usize); 4] = [(2, 2), (2, 8), (8, 2), (8, 8)];

fn generate_in_team(key: &str, team: &str, cursor: f64) -> axum::http::Request<axum::body::Body> {
    generate(Some(key), Some(team), &generate_body(cursor))
}

#[tokio::test]
async fn a_deposit_with_loops_learns_a_profile_in_three_steps_and_the_team_is_served_with_it() {
    let w = World::new();
    let k = key(1);
    let r = w.deposit(&k, "team-a", loops_deposit(&RINGS)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let p = &r.json()["profile"];
    assert_eq!(p["outcome"], "learned", "{p}");
    assert_eq!(p["steps"], 3);
    assert_eq!(p["loops"], 4);
    assert_eq!(p["plays"], 4 * 3 * 2, "quatre boucles, trois pas, deux curseurs");
    let st = w.status(&k, "team-a").await;
    assert_eq!(st["state"], "ready");
    assert_eq!(
        (st["profile"]["state"].as_str(), st["profile"]["steps"].as_u64()),
        (Some("present"), Some(3))
    );
    let g = w.send(generate_in_team(&k, "team-a", 0.25)).await;
    assert_eq!(g.status, StatusCode::OK, "{}", g.text);
    assert_eq!(g.json()["taste"], "team");
    // Sans équipe, la table commune sert.
    let d = w.send(generate(None, None, &generate_body(1.0))).await;
    assert_eq!(d.status, StatusCode::OK, "{}", d.text);
    assert_eq!(d.json()["taste"], "common");
}

#[tokio::test]
async fn a_deposit_with_no_loop_writes_the_common_table_as_the_profile_and_stays_ready() {
    let w = World::new();
    let k = key(1);
    let r = w.deposit(&k, "team-a", team_deposit()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let p = &r.json()["profile"];
    assert_eq!(
        (p["outcome"].as_str(), p["steps"].as_u64(), p["plays"].as_u64()),
        (Some("empty"), Some(0), Some(0))
    );
    assert_eq!(w.status(&k, "team-a").await["state"], "ready", "le backend ne redépose pas pour rien");
}

#[tokio::test]
async fn the_profile_does_not_depend_on_the_number_of_threads() {
    let mut prints = Vec::new();
    for threads in [1usize, 4] {
        let w = World::with(|c| c.generator.profile.threads = threads);
        let k = key(1);
        let r = w.deposit(&k, "team-a", loops_deposit(&RINGS)).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
        let j = r.json();
        prints.push((
            w.fingerprint(&k, "team-a"),
            j["profile"]["loops"].clone(),
            j["profile"]["plays"].clone(),
        ));
    }
    assert!(prints[0].0.is_some());
    assert_eq!(prints[0], prints[1]);
}

#[tokio::test]
async fn the_service_learns_what_the_reference_player_learns_on_the_same_loops() {
    use ord_graph::mmap::MappedGraph;
    use ord_graph_match::profile::collect::LoopCollector;
    use ord_graph_match::profile::learn::{learn, Common, LearnParams};
    use ord_graph_match::profile::FlatPlayer;
    use ord_graph_match::{Deposit, SegIndex};
    use ord_loops::{Engine, Params, TeamData};
    use ord_loops_check::{Checker, Config as CheckConfig, GraphIndex, TeamCheck};
    use xxhash_rust::xxh3::xxh3_64;

    let w = World::new();
    let (k, body) = (key(1), loops_deposit(&RINGS));
    let r = w.deposit(&k, "team-a", body.clone()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let (served, steps) = w.fingerprint(&k, "team-a").expect("profil servi");
    assert_eq!(steps, 3);

    // La référence : le même dépôt rejoué ici, `FlatPlayer` sur la même surcouche.
    let open = |p: &std::path::Path| MappedGraph::open_mmap(p).unwrap();
    let base = open(&w.files.graph);
    let poly = ord_graph::util::Poly::read(&w.files.poly).unwrap();
    // Les figures d'apprentissage du service par défaut (organiques depuis 1.12) ; celles du moteur restent classiques.
    let mut learn_params = LearnParams {
        figures: ord_graph_match::profile::learn::LearnFigures::Organic,
        ..LearnParams::default()
    };
    let (max_km, margin_m) = w.state.generator().test_limits().unwrap();
    learn_params.retain.max_km = Some(max_km);
    let seg = SegIndex::build(&base);
    let mut dep = Deposit::with_collector(
        &base,
        &seg,
        &poly,
        1000,
        LoopCollector::new(learn_params.retain, learn_params.cap),
    );
    for line in body.lines().filter(|l| !l.starts_with("{\"end\"")) {
        let v: Value = serde_json::from_str(line).unwrap();
        let edges = ord_graph_match::doc::parse_document(v["network_edges"].to_string().as_bytes());
        dep.push_track(ord_graph_match::doc::type_code("ROAD"), false, edges);
    }
    let (edges, counts, _stats, set) = dep.finish_learn();
    let meta = ord_graph::overlay::OverlayMeta {
        graph: base.graph_identity(),
        team_id: 0,
        export_xxh3: 0,
        export_rows: 4,
        params_hash: ord_graph_match::agg::params_hash(),
        tool_version: "t".into(),
        build_epoch: 0,
        types: ord_graph_match::doc::TYPES.iter().map(|t| (*t).to_string()).collect(),
    };
    let overlay =
        ord_graph::overlay::Overlay::decode(&ord_graph::overlay::encode(&meta, &edges, &counts).unwrap())
            .unwrap();
    let engine = Engine::new(
        open(&w.files.graph),
        open(&w.files.road),
        open(&w.files.gravel),
        Params { window_margin_m: margin_m, ..Params::default() },
    )
    .unwrap();
    let gi = GraphIndex::build(engine.base());
    let team = TeamData::build(&engine, &overlay).unwrap();
    let check = TeamCheck::build(engine.base(), &gi, &overlay, &CheckConfig::default());
    let checker =
        Checker::with_tables(engine.base(), Some(&overlay), CheckConfig::default(), &gi, Some(&check));
    let player = FlatPlayer { engine: &engine, team: &team, checker: &checker, gf: engine.base() };
    let cursors: Vec<_> = learn_params
        .cursors
        .iter()
        .map(|&h| {
            let a = ord_server::generator::cursor::apply(h);
            (a.mode, a.familiarity, a.off_team_cost_q8)
        })
        .collect();
    let learned =
        learn(&set.unwrap(), &cursors, &player, &Common::engine(learn_params.normalize), &learn_params);
    assert_eq!(learned.steps_done, 3);
    let bytes: Vec<u8> = learned.taste.q8.iter().flatten().flat_map(|q| q.to_le_bytes()).collect();
    assert_eq!(format!("{:016x}", xxh3_64(&bytes)), served, "ServicePlayer = FlatPlayer");
}

#[tokio::test]
async fn a_team_with_the_common_table_as_profile_is_served_exactly_like_a_team_without_profile() {
    let on = World::new();
    let off = World::with(|c| c.generator.profile.enabled = false);
    let k = key(1);
    for w in [&on, &off] {
        assert_eq!(w.deposit(&k, "team-a", team_deposit()).await.status, StatusCode::OK);
    }
    for cursor in [0.0, 0.25, 0.6, 1.0] {
        let (a, b) = (
            on.send(generate_in_team(&k, "team-a", cursor)).await,
            off.send(generate_in_team(&k, "team-a", cursor)).await,
        );
        assert_eq!((a.status, b.status), (StatusCode::OK, StatusCode::OK), "{} {}", a.text, b.text);
        let (mut x, mut y) = (a.json(), b.json());
        assert_eq!((x["taste"].as_str(), y["taste"].as_str()), (Some("team"), Some("common")));
        for v in [&mut x, &mut y] {
            v.as_object_mut().unwrap().remove("taste");
            v.as_object_mut().unwrap().remove("overlay");
        }
        assert_eq!(x, y, "curseur {cursor} : mêmes parcours");
    }
    // Profils désactivés : la surcouche seule est `ready`, jamais `stale` pour la raison `profile`.
    let st = off.status(&k, "team-a").await;
    assert_eq!(st["state"], "ready");
    assert!(st.get("profile").is_none());
}

#[tokio::test]
async fn a_missing_profile_makes_the_state_stale_profile_but_generations_are_served_with_the_common_table() {
    let w = World::new();
    let k = key(1);
    assert_eq!(w.deposit(&k, "team-a", loops_deposit(&RINGS)).await.status, StatusCode::OK);
    // Le profil disparaît (retour arrière, suppression par une 1.9.x...).
    let overlays = w._dir.path().join("overlays");
    let mut removed = 0;
    for entry in walk(&overlays) {
        if entry.to_string_lossy().ends_with(".ordp.enc") {
            std::fs::remove_file(entry).unwrap();
            removed += 1;
        }
    }
    assert_eq!(removed, 1);
    let st = w.status(&k, "team-a").await;
    assert_eq!((st["state"].as_str(), st["stale_reason"].as_str()), (Some("stale"), Some("profile")));
    assert_eq!(st["profile"]["state"], "absent");
    // Aucune génération n'est refusée pour autant : la table commune sert.
    let g = w.send(generate_in_team(&k, "team-a", 0.25)).await;
    assert_eq!(g.status, StatusCode::OK, "{}", g.text);
    assert_eq!(g.json()["taste"], "common");
    // Un nouveau dépôt réapprend le profil : retour à `ready`.
    assert_eq!(w.deposit(&k, "team-a", loops_deposit(&RINGS)).await.status, StatusCode::OK);
    assert_eq!(w.status(&k, "team-a").await["state"], "ready");
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok) {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[tokio::test]
async fn a_budget_spent_before_the_first_step_keeps_the_previous_profile_and_still_writes_the_overlay() {
    // 4 s: enough for the three steps of the first deposit on a slow runner (2 of 3 in 1 s on the
    // runner of the image build), and the hold below outlasts it.
    let w = World::with(|c| c.generator.profile.budget_s = 4);
    let k = key(1);
    let first = w.deposit(&k, "team-a", loops_deposit(&RINGS)).await;
    assert_eq!(first.json()["profile"]["outcome"], "learned", "{}", first.text);
    let before = w.fingerprint(&k, "team-a");
    let learned_at = w.status(&k, "team-a").await["profile"]["learned_at"].clone();
    // La phase attend plus que son budget : aucun pas fini.
    let hooks = w.state.generator().hooks().expect("prêt");
    hooks.profile_hold.store(true, Ordering::Relaxed);
    let release = {
        let hooks = std::sync::Arc::clone(&hooks);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(4500)).await;
            hooks.profile_hold.store(false, Ordering::Relaxed);
        })
    };
    let second = w.send(put_digest(&k, "team-a", "d2", loops_deposit(&RINGS))).await;
    release.await.unwrap();
    assert_eq!(second.status, StatusCode::OK, "{}", second.text);
    assert_eq!(second.json()["profile"]["outcome"], "skipped");
    let st = w.status(&k, "team-a").await;
    assert_eq!(st["source_digest"], "d2", "la surcouche est écrite quoi qu'il arrive");
    assert_eq!(w.fingerprint(&k, "team-a"), before, "l'ancien profil est gardé");
    assert_eq!(st["profile"]["learned_at"], learned_at);
}

#[tokio::test]
async fn a_profile_phase_held_open_does_not_block_the_deposit_slot_of_another_environment() {
    let w = World::new();
    let hooks = w.state.generator().hooks().expect("prêt");
    hooks.profile_hold.store(true, Ordering::Relaxed);
    let (ka, kb) = (key(1), key(2));
    let held = {
        let app = w.app.clone();
        let req = put(&ka, "team-a", loops_deposit(&RINGS));
        tokio::spawn(async move { send(app, req).await })
    };
    // Laisse le dépôt A finir son appariement et entrer dans sa phase de profil, retenue.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let t = std::time::Instant::now();
    let b = w.send(put(&kb, "team-b", team_deposit())).await;
    assert_eq!(b.status, StatusCode::OK, "pas de 503 : {}", b.text);
    assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
    hooks.profile_hold.store(false, Ordering::Relaxed);
    let a = held.await.unwrap();
    assert_eq!(a.status, StatusCode::OK, "{}", a.text);
    assert_eq!(a.json()["profile"]["outcome"], "learned");
}

#[tokio::test]
async fn a_panic_in_the_profile_phase_still_writes_the_overlay_and_gives_its_memory_back() {
    let w = World::new();
    let k = key(1);
    let g = w.state.generator();
    let hooks = g.hooks().expect("prêt");
    hooks.profile_panic.store(true, Ordering::Relaxed);
    let r = w.send(put_digest(&k, "team-a", "d1", loops_deposit(&RINGS))).await;
    hooks.profile_panic.store(false, Ordering::Relaxed);
    // Un échec du profil ne fait jamais échouer le dépôt (§14.7.4) : 200, profil `skipped`, surcouche écrite.
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["profile"]["outcome"], "skipped");
    let st = w.status(&k, "team-a").await;
    assert_eq!(st["source_digest"], "d1");
    assert_eq!((st["state"].as_str(), st["stale_reason"].as_str()), (Some("stale"), Some("profile")));
    // Les octets de la nouvelle surcouche tenus par la phase sont rendus au budget : une fois l'équipe supprimée (son
    // entrée résidente avec elle), plus rien n'est compté.
    let gen = w.send(generate_in_team(&k, "team-a", 0.25)).await;
    assert_eq!(gen.status, StatusCode::OK, "{}", gen.text);
    assert_eq!(w.send(delete(&k, "team-a")).await.status, StatusCode::NO_CONTENT);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while g.test_team_bytes() != Some(0) && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(g.test_team_bytes(), Some(0), "octets de la phase de profil jamais rendus");
}

#[tokio::test]
async fn deleting_a_team_removes_its_profile_with_its_overlay() {
    let w = World::new();
    let k = key(1);
    assert_eq!(w.deposit(&k, "team-a", loops_deposit(&RINGS)).await.status, StatusCode::OK);
    assert!(w.fingerprint(&k, "team-a").is_some());
    let d = w.send(delete(&k, "team-a")).await;
    assert_eq!(d.status, StatusCode::NO_CONTENT);
    assert!(w.fingerprint(&k, "team-a").is_none());
    assert!(walk(&w._dir.path().join("overlays")).iter().all(|p| !p.to_string_lossy().ends_with(".enc")));
}

// --- 4c : figures et formes de l'équipe -----------------------------------------------------------------------------

#[tokio::test]
async fn the_deposit_writes_the_shapes_of_the_team_in_its_profile_header() {
    let w = World::new();
    let k = key(1);
    assert_eq!(w.deposit(&k, "team-a", loops_deposit(&RINGS)).await.status, StatusCode::OK);
    let shapes =
        w.state.generator().test_profile_shapes(&k, "team-a").expect("formes écrites avec le profil");
    // Quatre anneaux carrés de 7,2 km, tous ROAD : trois sommets à 10 % de D chacun.
    let road = &shapes.types[0];
    assert_eq!((road.loops, road.k1, road.k2, road.k3), (4, 0, 0, 4), "{road:?}");
    assert!(shapes.types[1].loops == 0 && shapes.types[2].loops == 0);
    // Sous le seuil de 50 boucles : le mélange général sert (jamais celui d'une équipe de quatre boucles).
    use ord_graph_match::profile::shape::mix_for;
    assert_eq!(
        mix_for(Some(&shapes), ord_loops::RideType::Road),
        ord_loops::FigureMix::general(ord_loops::RideType::Road)
    );
    // Un dépôt sans boucle : pas de formes.
    assert_eq!(w.deposit(&k, "team-b", team_deposit()).await.status, StatusCode::OK);
    assert!(w.state.generator().test_profile_shapes(&k, "team-b").is_none());
}

#[tokio::test]
async fn a_request_names_its_figures_or_takes_the_service_setting_and_the_answer_says_which() {
    use ord_server::config::GeneratorFigures;
    let w = World::new();
    let k = key(1);
    assert_eq!(w.deposit(&k, "team-a", loops_deposit(&RINGS)).await.status, StatusCode::OK);
    let body = |figures: Option<&str>, seed: u64| {
        let mut b = generate_body(0.25);
        b["seed"] = json!(seed);
        if let Some(f) = figures {
            b["figures"] = json!(f);
        }
        b
    };
    let run = |b: Value| {
        let (w, k) = (&w, k.clone());
        async move { w.send(generate(Some(&k), Some("team-a"), &b)).await }
    };
    // Absent : le réglage du service (organique par défaut depuis 1.12) ; nommé : ce qu'il dit.
    let r = run(body(None, 0)).await;
    assert_eq!((r.status, r.json()["figures"].as_str()), (StatusCode::OK, Some("organic")), "{}", r.text);
    let r = run(body(Some("organic"), 0)).await;
    assert_eq!((r.status, r.json()["figures"].as_str()), (StatusCode::OK, Some("organic")), "{}", r.text);
    for route in r.json()["routes"].as_array().unwrap() {
        assert!(["turnaround", "triangle"].contains(&route["family"].as_str().unwrap()), "{route}");
    }
    // Classique : jamais de triangle ; organique : des triangles sur quelques graines, tous contrôlés par le contrôleur.
    let (mut triangles, mut routes) = (0, 0);
    for seed in 0..30u64 {
        let c = run(body(Some("classic"), seed)).await.json();
        assert!(c["routes"].as_array().unwrap().iter().all(|x| x["family"] == "turnaround"), "{c}");
        let o = run(body(Some("organic"), seed)).await.json();
        assert_eq!(o["dropped"], 0, "le contrôleur indépendant refuse un parcours organique : {o}");
        for x in o["routes"].as_array().unwrap() {
            routes += 1;
            triangles += usize::from(x["family"] == "triangle");
        }
    }
    assert!(routes > 0);
    eprintln!("triangles rendus : {triangles} parcours sur {routes}");
    // Un nom inconnu est refusé.
    let r = run(body(Some("quadrilateral"), 0)).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text);
    // Le réglage du service change le défaut, la requête garde la main.
    let w2 = World::with(|c| c.generator.figures = GeneratorFigures::Classic);
    assert_eq!(w2.deposit(&k, "team-a", loops_deposit(&RINGS)).await.status, StatusCode::OK);
    let r = w2.send(generate(Some(&k), Some("team-a"), &body(None, 0))).await;
    assert_eq!(r.json()["figures"], "classic", "{}", r.text);
    let r = w2.send(generate(Some(&k), Some("team-a"), &body(Some("organic"), 0))).await;
    assert_eq!(r.json()["figures"], "organic", "{}", r.text);
    let caps = w2.send(get("/v1/capabilities")).await.json();
    assert_eq!(caps["generator"]["figures"]["default"], "classic");
}
