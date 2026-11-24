//! L'itinéraire de A à B servi par le service (`POST /v1/router/route`, api 1.15, préférence de voie en 1.16) : graphe, vues et surcouches
//! **synthétiques** (la grille de 21 × 21 rues de 300 m des tests du générateur, l'équipe roule les lignes et colonnes
//! 6 à 14 paires).

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use common::generator::*;
use common::*;
use ord_server::{router, AppState};
use serde_json::{json, Value};

struct World {
    _dir: tempfile::TempDir,
    app: Router,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let files = write_files(dir.path(), "g1", 21);
        let config = config_with(test_config(), dir.path(), &files);
        let app = router(AppState::new(&config).expect("état"));
        World { _dir: dir, app }
    }

    async fn send(&self, req: Request<Body>) -> Reply {
        send(self.app.clone(), req).await
    }

    async fn route(&self, key: Option<&str>, team: Option<&str>, body: &Value) -> Reply {
        self.send(route(key, team, body)).await
    }
}

fn route(key: Option<&str>, team: Option<&str>, body: &Value) -> Request<Body> {
    let mut b = Request::builder()
        .method(Method::POST)
        .uri("/v1/router/route")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(k) = key {
        b = b.header("ord-namespace-key", k);
    }
    if let Some(t) = team {
        b = b.header("ord-team", t);
    }
    b.body(Body::from(body.to_string())).unwrap()
}

fn at(p: [f64; 2]) -> Value {
    json!({"lat": p[0], "lon": p[1]})
}

/// Du carrefour `a` au carrefour `b`, en route.
fn body(a: (usize, usize), b: (usize, usize)) -> Value {
    json!({"start": at(node(a.0, a.1)), "end": at(node(b.0, b.1)), "ride_type": "road"})
}

/// Un point à `north` et `east` mètres d'un carrefour.
fn off(p: [f64; 2], north: f64, east: f64) -> [f64; 2] {
    [p[0] + north / M_LAT, p[1] + east / (M_LAT * LAT0.to_radians().cos())]
}

fn assert_route(r: &Reply) -> Value {
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let v = r.json();
    assert!(!v["polyline"].as_str().unwrap().is_empty());
    assert_eq!(v["graph_id"].as_str().unwrap().len(), 16);
    v
}

#[tokio::test]
async fn without_team_headers_the_route_is_a_discovery_with_the_common_taste() {
    let w = World::new();
    let v = assert_route(&w.route(None, None, &body((10, 2), (10, 18))).await);
    assert_eq!(v["mode"], "discovery");
    assert!(v["overlay"].is_null());
    assert_eq!(
        (v["team_m"].as_f64(), v["bridged_m"].as_f64(), v["team_share"].as_f64()),
        (Some(0.0), Some(0.0), Some(0.0))
    );
    // 16 rues de 300 m : le plus court, sur une grille où tout est de même classe.
    let d = v["distance_m"].as_f64().unwrap();
    assert!((d - 4_800.0).abs() < 10.0, "{d}");
    // Rues résidentielles revêtues : aucune plage non revêtue ; les points sont sur la voie.
    assert_eq!(v["offroad_m"], 0.0);
    assert_eq!(v["unpaved"], json!([]));
    assert!(v["start_snap_m"].as_f64().unwrap() < 1.0 && v["end_snap_m"].as_f64().unwrap() < 1.0);
    assert_eq!(v["sharp_relaxed"], false);
    // Le curseur sans en-têtes d'équipe est ignoré.
    let mut b = body((10, 2), (10, 18));
    b["cursor"] = json!(0.0);
    let again = assert_route(&w.route(None, None, &b).await);
    assert_eq!(again["mode"], "discovery");
    assert_eq!(again["polyline"], v["polyline"]);
    // Un autre type : la vue gravel, toujours la découverte.
    b["ride_type"] = json!("mtb");
    assert_eq!(assert_route(&w.route(None, None, &b).await)["mode"], "discovery");
}

#[tokio::test]
async fn with_a_team_and_its_overlay_the_route_prefers_the_teams_roads() {
    let w = World::new();
    let (k, team) = (key(1), "team-a");
    let r = w.send(put_digest(&k, team, "d1", team_deposit())).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    // La ligne 6 est roulée par l'équipe, de la colonne 6 à la colonne 14.
    let mut b = body((6, 6), (6, 14));
    b["cursor"] = json!(0.25);
    let v = assert_route(&w.route(Some(&k), Some(team), &b).await);
    assert_eq!(v["mode"], "team_first");
    assert!(v["overlay"]["deposited_at"].as_str().unwrap().ends_with('Z'));
    let share = v["team_share"].as_f64().unwrap();
    assert!(share > 0.9, "{share}");
    assert!(v["team_m"].as_f64().unwrap() > 0.0);
    // Le strict n'est pas offert : un curseur 0 route en équipe d'abord.
    b["cursor"] = json!(0.0);
    assert_eq!(assert_route(&w.route(Some(&k), Some(team), &b).await)["mode"], "team_first");
    // Sans curseur, avec l'équipe : la découverte, la part d'équipe mesurée quand même.
    b.as_object_mut().unwrap().remove("cursor");
    let v = assert_route(&w.route(Some(&k), Some(team), &b).await);
    assert_eq!(v["mode"], "discovery");
    assert!(!v["overlay"].is_null());
    assert!(v["team_share"].as_f64().unwrap() > 0.0);
}

#[tokio::test]
async fn a_team_without_an_overlay_gets_a_discovery_that_says_so() {
    let w = World::new();
    let mut b = body((10, 2), (10, 18));
    b["cursor"] = json!(0.25);
    let v = assert_route(&w.route(Some(&key(1)), Some("team-never-deposited"), &b).await);
    assert_eq!(v["mode"], "discovery");
    assert!(v["overlay"].is_null());
    assert_eq!(v["team_m"], 0.0);
    // Le même itinéraire que sans en-têtes.
    let anonymous = assert_route(&w.route(None, None, &body((10, 2), (10, 18))).await);
    assert_eq!(v["polyline"], anonymous["polyline"]);
}

#[tokio::test]
async fn team_headers_are_optional_but_validated_when_present() {
    let w = World::new();
    let b = body((10, 2), (10, 18));
    for (k, t, name) in [
        (Some(key(1)), None, "Ord-Team"),
        (None, Some("team-a"), "Ord-Namespace-Key"),
        (Some("pas-une-cle".to_owned()), Some("team-a"), "Ord-Namespace-Key"),
        (Some(key(1)), Some("a/b"), "Ord-Team"),
    ] {
        let r = w.route(k.as_deref(), t, &b).await;
        let p = assert_problem(&r, StatusCode::BAD_REQUEST, "invalid_input");
        assert_eq!(p["details"]["errors"][0]["pointer"], format!("/headers/{name}"), "{}", r.text);
        assert!(!r.text.contains("pas-une-cle"), "la valeur reçue ne revient pas");
    }
}

#[tokio::test]
async fn a_point_outside_the_polygon_is_out_of_coverage() {
    let w = World::new();
    let mut b = body((10, 2), (10, 18));
    b["end"] = at(off(node(20, 18), 5_000.0, 0.0));
    let p =
        assert_problem(&w.route(None, None, &b).await, StatusCode::UNPROCESSABLE_ENTITY, "out_of_coverage");
    assert_eq!(p["details"]["coverage"], "graph");
    b["start"] = at(off(node(0, 0), -5_000.0, -5_000.0));
    b["end"] = at(node(10, 18));
    assert_problem(&w.route(None, None, &b).await, StatusCode::UNPROCESSABLE_ENTITY, "out_of_coverage");
}

#[tokio::test]
async fn a_point_too_far_from_any_way_is_refused_with_its_side() {
    let w = World::new();
    // 800 m au sud de la grille : dans le polygone (1 km), à plus de `snap_m` (500 m) de toute voie.
    let far = at(off(node(0, 10), -800.0, 0.0));
    let mut b = body((10, 2), (10, 18));
    b["start"] = far.clone();
    let p =
        assert_problem(&w.route(None, None, &b).await, StatusCode::UNPROCESSABLE_ENTITY, "generation_failed");
    assert_eq!(p["details"]["reason"], "start_too_far");
    let mut b = body((10, 2), (10, 18));
    b["end"] = far;
    let p =
        assert_problem(&w.route(None, None, &b).await, StatusCode::UNPROCESSABLE_ENTITY, "generation_failed");
    assert_eq!(p["details"]["reason"], "end_too_far");
    // À 300 m de la grille : accroché, la distance d'accroche est rendue.
    let mut b = body((10, 2), (10, 18));
    b["start"] = at(off(node(0, 10), -300.0, 0.0));
    let v = assert_route(&w.route(None, None, &b).await);
    let snap = v["start_snap_m"].as_f64().unwrap();
    assert!((snap - 300.0).abs() < 5.0, "{snap}");
}

#[tokio::test]
async fn a_leg_too_long_as_the_crow_flies_is_an_invalid_input_naming_the_limit() {
    let w = World::new();
    let mut b = body((10, 2), (10, 18));
    // 150 km au nord : refusé avant la couverture.
    b["end"] = at(off(node(10, 18), 150_000.0, 0.0));
    let r = w.route(None, None, &b).await;
    let p = assert_problem(&r, StatusCode::BAD_REQUEST, "invalid_input");
    assert_eq!(p["details"]["errors"][0]["pointer"], "/end");
    assert!(p["details"]["errors"][0]["message"].as_str().unwrap().contains("100000"), "{}", r.text);
}

#[tokio::test]
async fn invalid_fields_are_listed_by_pointer() {
    let w = World::new();
    let mut b = body((10, 2), (10, 18));
    b["ride_type"] = json!("bike");
    assert_problem(&w.route(None, None, &b).await, StatusCode::BAD_REQUEST, "invalid_input");
    let mut b = body((10, 2), (10, 18));
    b["cursor"] = json!(1.5);
    b["start"] = json!({"lat": 91.0, "lon": 0.0});
    let p = assert_problem(&w.route(None, None, &b).await, StatusCode::BAD_REQUEST, "invalid_input");
    let pointers: Vec<&str> =
        p["details"]["errors"].as_array().unwrap().iter().map(|e| e["pointer"].as_str().unwrap()).collect();
    assert_eq!(pointers, ["/start/lat", "/cursor"]);
    // Une clé inconnue, un champ manquant.
    let mut b = body((10, 2), (10, 18));
    b["distance_m"] = json!(3000.0);
    assert_problem(&w.route(None, None, &b).await, StatusCode::BAD_REQUEST, "invalid_input");
    let mut b = body((10, 2), (10, 18));
    b.as_object_mut().unwrap().remove("end");
    assert_problem(&w.route(None, None, &b).await, StatusCode::BAD_REQUEST, "invalid_input");
}

#[tokio::test]
async fn capabilities_publish_the_router_next_to_the_generator() {
    let w = World::new();
    let c = w.send(get("/v1/capabilities")).await.json();
    assert_eq!(c["api_version"], "1.17");
    assert_eq!(
        c["router"],
        json!({"enabled": true, "ride_types": ["road", "gravel", "mtb"], "crow_m_max": 100000.0, "snap_m": 500.0,
            "use_roads": true, "avoid_main_roads": true})
    );
    assert_eq!(c["generator"]["enabled"], true);
    // La raison `no_route` est au contrat.
    let doc = serde_json::to_value(ord_server::openapi()).unwrap();
    let reasons = doc["components"]["schemas"]["GenerationReason"]["enum"].as_array().unwrap();
    assert!(reasons.contains(&json!("no_route")));
    let op = &doc["paths"]["/v1/router/route"]["post"];
    let names: Vec<&str> =
        op["parameters"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Ord-Namespace-Key", "Ord-Team"]);
    // Les deux champs de la préférence de voie sont au contrat, facultatifs.
    let req = &doc["components"]["schemas"]["RouterRequestV1"];
    for f in ["use_roads", "use_roads_neutral", "avoid_main_roads"] {
        assert!(req["properties"][f].is_object(), "{f}");
        assert!(!req["required"].as_array().unwrap().contains(&json!(f)), "{f}");
    }
}

#[tokio::test]
async fn a_road_setting_equal_to_its_neutral_changes_nothing() {
    let w = World::new();
    let plain = assert_route(&w.route(None, None, &body((10, 2), (10, 18))).await);
    for (ride, u) in [("road", 0.25), ("gravel", 1.0), ("mtb", 0.0)] {
        let mut b = body((10, 2), (10, 18));
        b["ride_type"] = json!(ride);
        let without = assert_route(&w.route(None, None, &b).await);
        b["use_roads"] = json!(u);
        b["use_roads_neutral"] = json!(u);
        let same = assert_route(&w.route(None, None, &b).await);
        assert_eq!(same, without, "{ride} {u}");
        // Le neutre seul : sans effet.
        b.as_object_mut().unwrap().remove("use_roads");
        assert_eq!(assert_route(&w.route(None, None, &b).await), without, "{ride}");
    }
    assert_eq!(
        plain["polyline"],
        assert_route(&w.route(None, None, &body((10, 2), (10, 18))).await)["polyline"]
    );
}

#[tokio::test]
async fn a_road_setting_away_from_its_neutral_is_served_with_or_without_a_team() {
    let w = World::new();
    // La grille n'a qu'une classe de voie (rues résidentielles) : tout est multiplié pareil, le plus court reste le
    // même ; l'effet sur le tracé est vérifié par les tests du moteur (`tests_route.rs`).
    for (u, n) in [(0.0, 1.0), (1.0, 0.25), (0.5, 0.0)] {
        let mut b = body((10, 2), (10, 18));
        b["use_roads"] = json!(u);
        b["use_roads_neutral"] = json!(n);
        let v = assert_route(&w.route(None, None, &b).await);
        assert_eq!(v["mode"], "discovery");
        let d = v["distance_m"].as_f64().unwrap();
        assert!((d - 4_800.0).abs() < 10.0, "{u} {n} {d}");
    }
    let (k, team) = (key(1), "team-a");
    let r = w.send(put_digest(&k, team, "d1", team_deposit())).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let mut b = body((6, 6), (6, 14));
    b["cursor"] = json!(0.25);
    b["use_roads"] = json!(0.0);
    b["use_roads_neutral"] = json!(1.0);
    let v = assert_route(&w.route(Some(&k), Some(team), &b).await);
    assert_eq!(v["mode"], "team_first");
    assert!(v["team_share"].as_f64().unwrap() > 0.9, "{}", v["team_share"]);
}

#[tokio::test]
async fn avoiding_main_roads_is_optional_and_false_changes_nothing() {
    let w = World::new();
    for ride in ["road", "gravel", "mtb"] {
        let mut b = body((10, 2), (10, 18));
        b["ride_type"] = json!(ride);
        let without = assert_route(&w.route(None, None, &b).await);
        for off in [json!(false), Value::Null] {
            b["avoid_main_roads"] = off;
            assert_eq!(assert_route(&w.route(None, None, &b).await), without, "{ride}");
        }
    }
    // `true` : la grille n'a qu'une classe de voie (rues résidentielles), aucune grande route à éviter ; l'effet sur le
    // tracé est vérifié par les tests du moteur (`tests_route.rs`). Seul, puis avec `use_roads`, puis avec l'équipe.
    let mut b = body((10, 2), (10, 18));
    b["avoid_main_roads"] = json!(true);
    let v = assert_route(&w.route(None, None, &b).await);
    assert!((v["distance_m"].as_f64().unwrap() - 4_800.0).abs() < 10.0, "{}", v["distance_m"]);
    b["use_roads"] = json!(0.0);
    b["use_roads_neutral"] = json!(1.0);
    let v = assert_route(&w.route(None, None, &b).await);
    assert!((v["distance_m"].as_f64().unwrap() - 4_800.0).abs() < 10.0, "{}", v["distance_m"]);
    let (k, team) = (key(1), "team-a");
    let r = w.send(put_digest(&k, team, "d1", team_deposit())).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let mut b = body((6, 6), (6, 14));
    b["cursor"] = json!(0.25);
    b["avoid_main_roads"] = json!(true);
    let v = assert_route(&w.route(Some(&k), Some(team), &b).await);
    assert_eq!(v["mode"], "team_first");
    // Pas un booléen.
    for bad in [json!("true"), json!(1), json!(0.0)] {
        let mut b = body((10, 2), (10, 18));
        b["avoid_main_roads"] = bad.clone();
        assert_problem(&w.route(None, None, &b).await, StatusCode::BAD_REQUEST, "invalid_input");
    }
}

#[tokio::test]
async fn a_road_setting_needs_its_neutral_and_both_stay_within_bounds() {
    let w = World::new();
    let pointers = |p: &Value| -> Vec<String> {
        p["details"]["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["pointer"].as_str().unwrap().to_owned())
            .collect()
    };
    // `use_roads` seul : le neutre dépend du profil, le service ne le devine pas.
    let mut b = body((10, 2), (10, 18));
    b["use_roads"] = json!(0.5);
    let p = assert_problem(&w.route(None, None, &b).await, StatusCode::BAD_REQUEST, "invalid_input");
    assert_eq!(pointers(&p), ["/use_roads_neutral"]);
    // Hors bornes, chacun à son pointeur.
    for (u, n, expected) in [
        (json!(1.5), json!(0.25), vec!["/use_roads"]),
        (json!(-0.1), json!(0.25), vec!["/use_roads"]),
        (json!(0.5), json!(2.0), vec!["/use_roads_neutral"]),
        (json!(-1.0), json!(1.01), vec!["/use_roads", "/use_roads_neutral"]),
    ] {
        let mut b = body((10, 2), (10, 18));
        b["use_roads"] = u;
        b["use_roads_neutral"] = n;
        let r = w.route(None, None, &b).await;
        let p = assert_problem(&r, StatusCode::BAD_REQUEST, "invalid_input");
        assert_eq!(pointers(&p), expected, "{}", r.text);
    }
    // Le neutre seul est validé lui aussi.
    let mut b = body((10, 2), (10, 18));
    b["use_roads_neutral"] = json!(-0.5);
    let p = assert_problem(&w.route(None, None, &b).await, StatusCode::BAD_REQUEST, "invalid_input");
    assert_eq!(pointers(&p), ["/use_roads_neutral"]);
    // Pas un nombre.
    let mut b = body((10, 2), (10, 18));
    b["use_roads"] = json!("routes");
    b["use_roads_neutral"] = json!(0.25);
    assert_problem(&w.route(None, None, &b).await, StatusCode::BAD_REQUEST, "invalid_input");
}

#[tokio::test]
async fn without_a_graph_the_router_answers_503_like_the_generator() {
    let app = router(AppState::new(&test_config()).expect("le service démarre"));
    let c = send(app.clone(), get("/v1/capabilities")).await.json();
    assert_eq!(c["router"]["enabled"], false);
    assert_eq!(c["generator"]["enabled"], false);
    let r = send(app, route(None, None, &body((10, 2), (10, 18)))).await;
    let p = assert_problem(&r, StatusCode::SERVICE_UNAVAILABLE, "data_unavailable");
    assert_eq!(p["details"]["source"], "graph");
    assert_eq!(r.headers.get("retry-after").unwrap(), "3600");
}

#[tokio::test]
async fn the_same_point_twice_is_not_an_error_of_the_service() {
    let w = World::new();
    let r = w.route(None, None, &body((10, 10), (10, 10))).await;
    assert!(
        r.status == StatusCode::OK || r.status == StatusCode::UNPROCESSABLE_ENTITY,
        "{} {}",
        r.status,
        r.text
    );
    if r.status == StatusCode::OK {
        assert!(r.json()["distance_m"].as_f64().unwrap() < 1.0, "{}", r.text);
    }
}
