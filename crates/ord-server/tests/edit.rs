//! La retouche d'un parcours servie par le service (`POST /v1/generator/routes/edit`, api 1.17, lot 7b, note
//! `ord-retouche-lot1.md` §15) : graphe, vues et surcouches **synthétiques** (la grille de 21 × 21 rues de 300 m des tests du
//! générateur, l'équipe roule les lignes et colonnes 6 à 14 paires). Les parcours sont des chemins de carrefours, leur
//! document `network_edges` est fabriqué (`path_document`).

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use axum::Router;
use common::generator::*;
use common::*;
use ord_graph_match::geo::decode_polyline6;
use ord_server::{router, AppState};
use serde_json::{json, Value};

struct World {
    _dir: tempfile::TempDir,
    state: AppState,
    app: Router,
}

impl World {
    fn new() -> Self {
        Self::with(|_| {})
    }

    fn with(tweak: impl FnOnce(&mut ord_server::config::Config)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let files = write_files(dir.path(), "g1", 21);
        let mut config = config_with(test_config(), dir.path(), &files);
        tweak(&mut config);
        let state = AppState::new(&config).expect("état");
        let app = router(state.clone());
        World { _dir: dir, state, app }
    }

    async fn edit(&self, key: Option<&str>, team: Option<&str>, body: &Value) -> Reply {
        send(self.app.clone(), edit(key, team, body)).await
    }

    async fn plain(&self, body: &Value) -> Reply {
        self.edit(None, None, body).await
    }
}

fn resize(distance_m: f64) -> Value {
    json!({"resize": {"distance_m": distance_m}})
}

fn move_end(p: [f64; 2]) -> Value {
    json!({"move_end": {"point": {"lat": p[0], "lon": p[1]}}})
}

/// Un parcours à lobe : la ligne 10 de la colonne 2 à la colonne 6, un détour de 2 400 m par la ligne 12 (au lieu de
/// 1 200 m), puis la ligne 10 jusqu'à la colonne 14. 4 800 m.
fn lobe() -> Vec<(usize, usize)> {
    let mut p = row(10, 2, 6);
    p.extend([(11, 6), (12, 6), (12, 7), (12, 8), (12, 9), (12, 10), (11, 10), (10, 10)]);
    p.extend((11..=14).map(|c| (10, c)));
    p
}

/// Un aller simple en L : la ligne 10 de la colonne 2 à la colonne 14, puis la colonne 14 jusqu'à la ligne 16. 5 400 m.
fn one_way() -> Vec<(usize, usize)> {
    let mut p = row(10, 2, 14);
    p.extend((11..=16).map(|r| (r, 14)));
    p
}

/// Le milieu de la rue entre deux carrefours.
fn mid(a: (usize, usize), b: (usize, usize)) -> [f64; 2] {
    let (p, q) = (node(a.0, a.1), node(b.0, b.1));
    [f64::midpoint(p[0], q[0]), f64::midpoint(p[1], q[1])]
}

/// Un point à `north` et `east` mètres d'un autre.
fn off(p: [f64; 2], north: f64, east: f64) -> [f64; 2] {
    [p[0] + north / M_LAT, p[1] + east / (M_LAT * LAT0.to_radians().cos())]
}

fn ok(r: &Reply) -> Value {
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    r.json()
}

fn reason(r: &Reply, expected: &str) -> Value {
    let v = assert_problem(r, StatusCode::UNPROCESSABLE_ENTITY, "generation_failed");
    assert_eq!(v["details"]["reason"], expected, "{}", r.text);
    v
}

fn pointer(r: &Reply) -> String {
    let v = assert_problem(r, StatusCode::BAD_REQUEST, "invalid_input");
    v["details"]["errors"][0]["pointer"].as_str().unwrap_or("").to_owned()
}

fn points(polyline: &Value) -> Vec<[f64; 2]> {
    decode_polyline6(polyline.as_str().unwrap()).unwrap()
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 2e-6 && (a[1] - b[1]).abs() < 2e-6
}

/// La polyligne rendue commence par celle d'origine, au point près, jusqu'au point de coupe (qui peut être entre deux
/// sommets d'origine : le dernier point avant le tronçon neuf n'est pas comparé).
fn assert_starts_with_the_original(route: &Value, path: &[(usize, usize)]) {
    let (out, orig) = (points(&route["polyline"]), path_points(path));
    let first_new = route["changed"]["points"][0].as_u64().unwrap() as usize;
    assert!(first_new < out.len());
    for i in 0..first_new.saturating_sub(1) {
        assert!(near(out[i], orig[i]), "point {i} : {:?} au lieu de {:?}", out[i], orig[i]);
    }
}

#[tokio::test]
async fn capabilities_publish_the_edit_operations_and_bounds() {
    let w = World::new();
    let c = send(w.app.clone(), get("/v1/capabilities")).await.json();
    assert_eq!(c["api_version"], "1.17");
    assert_eq!(
        c["generator"]["edit"],
        json!({"operations": ["resize", "move_end"], "ride_types": ["road", "gravel", "mtb"],
            "change_share_max": 0.3, "change_m_min": 1000.0, "tolerance": 0.03, "end_snap_m": 500.0,
            "proposals_max": 3, "points_max": 50000, "request_body_bytes": 4_194_304})
    );
    // Les raisons de la retouche sont au contrat.
    let doc = serde_json::to_value(ord_server::openapi()).unwrap();
    let reasons = doc["components"]["schemas"]["GenerationReason"]["enum"].as_array().unwrap();
    for r in [
        "change_too_small",
        "change_too_large",
        "route_off_coverage",
        "route_not_matched",
        "order_anomaly",
        "route_mismatch",
        "no_seam",
        "junction_cut",
        "no_economy",
        "no_window",
        "off_target",
        "end_unreachable",
        "no_candidate",
        "budget",
    ] {
        assert!(reasons.contains(&json!(r)), "{r}");
    }
    // Sans graphe : pas de retouche annoncée, et la route répond 503.
    let off = send(app(), get("/v1/capabilities")).await.json();
    assert_eq!(off["generator"]["enabled"], false);
    assert!(off["generator"].get("edit").is_none());
    let body = edit_body(&one_way(), resize(6_000.0), None);
    assert_problem(
        &send(app(), edit(None, None, &body)).await,
        StatusCode::SERVICE_UNAVAILABLE,
        "data_unavailable",
    );
}

#[tokio::test]
async fn a_loop_is_lengthened_by_twenty_percent_to_within_three_percent() {
    let w = World::new();
    let path = square(5, 5, 6);
    let target = 8_640.0;
    let v = ok(&w.plain(&edit_body(&path, resize(target), None)).await);
    assert_eq!(
        (v["mode"].as_str(), v["taste"].as_str(), v["operation"].as_str()),
        (Some("discovery"), Some("common"), Some("resize"))
    );
    assert!(v["overlay"].is_null() && v["dropped"] == 0);
    assert_eq!(v["graph_id"].as_str().unwrap().len(), 16);
    assert_eq!(v["target_m"], target);
    let original = v["original_m"].as_f64().unwrap();
    assert!((original - 7_200.0).abs() < 5.0, "{original}");
    let routes = v["routes"].as_array().unwrap();
    assert!((1..=3).contains(&routes.len()), "{}", routes.len());
    for r in routes {
        assert_eq!(r["kind"], "lengthen");
        assert!(r["variant"].is_null() && r["abandoned_m"].is_null() && r["end_snap_m"].is_null());
        let d = r["distance_m"].as_f64().unwrap();
        assert!((d - target).abs() / target <= 0.03, "{d}");
        assert!(r["error_share"].as_f64().unwrap() <= 0.03);
        let c = &r["changed"];
        let (from, to, new) =
            (c["from_m"].as_f64().unwrap(), c["to_m"].as_f64().unwrap(), c["new_m"].as_f64().unwrap());
        assert!(0.0 < from && from < to && to < original, "{from} {to}");
        assert!(new > to - from, "allonger : le tronçon neuf est plus long que celui qu'il remplace");
        // Les gardés sont ce qui reste du parcours : ils ne changent pas.
        assert!((r["kept_m"].as_f64().unwrap() - (original - (to - from))).abs() < 3.0);
        assert!(r["kept_share"].as_f64().unwrap() < 1.0);
        assert_eq!(r["unmatched_m"], 0.0);
        assert_starts_with_the_original(r, &path);
        // La polyligne commence et finit par l'origine ; le tronçon neuf est encadré par `points`.
        let (out, orig) = (points(&r["polyline"]), path_points(&path));
        assert!(near(out[0], orig[0]) && near(*out.last().unwrap(), *orig.last().unwrap()));
        let [i, j] = [c["points"][0].as_u64().unwrap() as usize, c["points"][1].as_u64().unwrap() as usize];
        assert!(0 < i && i < j && j < out.len() - 1, "{i} {j} {}", out.len());
        // Sans équipe : aucun mètre d'équipe, et des mesures de tronçon.
        assert_eq!(
            (c["team_m"].as_f64(), c["bridged_m"].as_f64(), c["team_share"].as_f64()),
            (Some(0.0), Some(0.0), Some(0.0))
        );
        assert_eq!(c["offroad_m"], 0.0);
        assert_eq!(c["unpaved"], json!([]));
    }
}

#[tokio::test]
async fn a_lobe_is_shortened_by_exactly_the_lobe() {
    let w = World::new();
    let path = lobe();
    let v = ok(&w.plain(&edit_body(&path, resize(3_600.0), None)).await);
    assert!((v["original_m"].as_f64().unwrap() - 4_800.0).abs() < 5.0);
    let best = &v["routes"][0];
    assert_eq!(best["kind"], "shorten");
    let c = &best["changed"];
    // Le lobe va du carrefour (10, 6) à (10, 10) : 2 400 m remplacés par 1 200 m de la ligne 10.
    assert!((c["from_m"].as_f64().unwrap() - 1_200.0).abs() < 5.0, "{c}");
    assert!((c["to_m"].as_f64().unwrap() - 3_600.0).abs() < 5.0, "{c}");
    assert!((c["new_m"].as_f64().unwrap() - 1_200.0).abs() < 5.0, "{c}");
    assert!(best["error_share"].as_f64().unwrap() < 0.01);
    assert!((best["distance_m"].as_f64().unwrap() - 3_600.0).abs() < 36.0);
    assert_starts_with_the_original(best, &path);
    let (out, orig) = (points(&best["polyline"]), path_points(&path));
    assert!(near(*out.last().unwrap(), *orig.last().unwrap()), "la fin de l'origine est gardée");
}

#[tokio::test]
async fn a_straight_one_way_ride_has_no_economy_and_the_answer_says_so() {
    let w = World::new();
    let r = w.plain(&edit_body(&row(10, 2, 18), resize(3_600.0), None)).await;
    let v = reason(&r, "no_economy");
    assert_eq!(v["detail"], "no edit could be made for this route");
}

#[tokio::test]
async fn the_end_moves_near_the_old_one_and_keeps_the_route_up_to_a_junction() {
    let w = World::new();
    let path = one_way();
    let end = node(16, 17);
    let v = ok(&w.plain(&edit_body(&path, move_end(end), None)).await);
    assert_eq!(v["operation"], "move_end");
    assert!(v["target_m"].is_null());
    let original = v["original_m"].as_f64().unwrap();
    let routes = v["routes"].as_array().unwrap();
    assert!((1..=3).contains(&routes.len()));
    assert_eq!(routes[0]["kind"], "move_end");
    assert_eq!(routes[0]["variant"], "best");
    for r in routes {
        assert_eq!(r["kind"], "move_end");
        assert!(["best", "keep_all", "nearest"].contains(&r["variant"].as_str().unwrap()));
        let c = &r["changed"];
        assert_eq!(c["to_m"].as_f64().unwrap(), original, "le reste est abandonné");
        assert!(c["from_m"].as_f64().unwrap() <= original);
        assert!(r["end_snap_m"].as_f64().unwrap() <= 500.0);
        assert!(r["error_share"].is_null());
        let abandoned = r["abandoned_m"].as_f64().unwrap();
        assert!((r["kept_m"].as_f64().unwrap() + abandoned - original).abs() < 3.0);
        assert_starts_with_the_original(r, &path);
        // La polyligne rendue finit au point demandé, à l'accroche près (`end_snap_m`).
        let out = points(&r["polyline"]);
        let last = *out.last().unwrap();
        assert!((last[0] - end[0]).abs() < 5e-5 && (last[1] - end[1]).abs() < 5e-5, "{last:?}");
        let [i, j] = [c["points"][0].as_u64().unwrap() as usize, c["points"][1].as_u64().unwrap() as usize];
        assert!(i < j && j == out.len() - 1, "le tronçon neuf finit la polyligne");
    }
}

#[tokio::test]
async fn an_end_on_the_route_cuts_it_without_a_new_stretch() {
    let w = World::new();
    let path = one_way();
    // Au milieu de la rue (10, 8)-(10, 9) : coupé au dernier passage.
    let v = ok(&w.plain(&edit_body(&path, move_end(mid((10, 8), (10, 9))), None)).await);
    let t = &v["routes"][0];
    assert_eq!((t["kind"].as_str(), t["changed"]["new_m"].as_f64()), (Some("truncate"), Some(0.0)));
    assert!(t["variant"].is_null());
    let out = points(&t["polyline"]);
    let last = (out.len() - 1) as u64;
    assert_eq!(t["changed"]["points"], json!([last, last]));
    assert_eq!(t["changed"]["unpaved"], json!([]));
    assert_eq!(t["changed"]["crossings"], 0);
    // 1 800 m (6 rues) plus la demi-rue : coupé à 1 950 m environ de l'origine.
    let cut = t["changed"]["from_m"].as_f64().unwrap();
    assert!((cut - 1_950.0).abs() < 20.0, "{cut}");
    assert!((t["distance_m"].as_f64().unwrap() - t["kept_m"].as_f64().unwrap()).abs() < 1e-6);
    assert!(t["end_snap_m"].as_f64().unwrap() < 5.0);
    assert!((t["abandoned_m"].as_f64().unwrap() - (v["original_m"].as_f64().unwrap() - cut)).abs() < 3.0);
    assert_starts_with_the_original(t, &path);
}

#[tokio::test]
async fn the_change_is_bounded_before_the_queue_and_the_queue_is_the_generations() {
    // Une seule place de calcul, aucune attente : une retouche qui prendrait la file serait refusée en 503.
    let w = World::with(|c| {
        c.generator.max_concurrent = 1;
        c.generator.max_queued = 0;
    });
    let path = square(5, 5, 6);
    let held =
        w.state.generator().test_compute_slot(Duration::from_secs(1)).await.expect("prêt").expect("place");
    // Les bornes sont jugées avant la file : elles répondent, la file est pleine.
    reason(&w.plain(&edit_body(&path, resize(7_200.0 * 1.4), None)).await, "change_too_large");
    reason(&w.plain(&edit_body(&path, resize(7_200.0 + 500.0), None)).await, "change_too_small");
    reason(&w.plain(&edit_body(&path, resize(7_200.0 - 500.0), None)).await, "change_too_small");
    reason(&w.plain(&edit_body(&path, resize(7_200.0 * 0.6), None)).await, "change_too_large");
    // Une retouche valide, elle, attend la place des générations : la file est pleine, 503.
    let r = w.plain(&edit_body(&path, resize(8_640.0), None)).await;
    assert_problem(&r, StatusCode::SERVICE_UNAVAILABLE, "overloaded");
    assert!(r.headers.get("retry-after").is_some());
    // Un document dont une arête (ou le total) dépasse nettement la polyligne : 422 sans prendre la file ni calculer.
    let mut huge_g = edit_body(&path, resize(8_640.0), None);
    let zigzag: Vec<[f64; 2]> = (0..400).flat_map(|_| [node(5, 5), node(5, 11)]).collect();
    huge_g["network_edges"]["edges"][0]["g"] = json!(ord_graph_match::geo::encode_polyline6(&zigzag));
    reason(&w.plain(&huge_g).await, "route_mismatch");
    let mut huge_trv = edit_body(&path, resize(8_640.0), None);
    huge_trv["network_edges"]["edges"][0]["trv"] = json!(5.0e7);
    reason(&w.plain(&huge_trv).await, "route_mismatch");
    drop(held);
    ok(&w.plain(&edit_body(&path, resize(8_640.0), None)).await);
}

#[tokio::test]
async fn invalid_input_is_a_400_with_a_pointer_and_never_the_value() {
    let w = World::new();
    let path = one_way();
    let good = edit_body(&path, resize(6_000.0), None);
    let with = |k: &str, v: Value| {
        let mut b = good.clone();
        b[k] = v;
        b
    };
    // La polyligne.
    assert_eq!(pointer(&w.plain(&with("polyline", json!("\u{7f}\u{7f}"))).await), "/polyline");
    assert_eq!(pointer(&w.plain(&with("polyline", json!(""))).await), "/polyline");
    let one = ord_graph_match::geo::encode_polyline6(&[node(10, 2), node(10, 2)]);
    assert_eq!(pointer(&w.plain(&with("polyline", json!(one))).await), "/polyline");
    // Le document.
    assert_eq!(pointer(&w.plain(&with("network_edges", json!("x"))).await), "/network_edges");
    assert_eq!(pointer(&w.plain(&with("network_edges", json!([[], null]))).await), "/network_edges");
    assert_eq!(pointer(&w.plain(&with("network_edges", json!({"edges": 3}))).await), "/network_edges");
    assert_eq!(
        pointer(&w.plain(&with("network_edges", json!({"edges": [{"trv": "x"}]}))).await),
        "/network_edges"
    );
    // Le curseur, avec ou sans équipe.
    assert_eq!(pointer(&w.plain(&with("cursor", json!(1.5))).await), "/cursor");
    assert_eq!(pointer(&w.plain(&with("cursor", json!(-0.1))).await), "/cursor");
    // L'opération : aucune clé, deux clés, la distance, le point.
    assert_eq!(pointer(&w.plain(&with("operation", json!({}))).await), "/operation");
    let both = json!({"resize": {"distance_m": 6000.0}, "move_end": {"point": {"lat": 47.2, "lon": -1.5}}});
    assert_eq!(pointer(&w.plain(&with("operation", both)).await), "/operation");
    for bad in [-5.0, 0.0, 300_000.0] {
        assert_eq!(
            pointer(&w.plain(&with("operation", resize(bad))).await),
            "/operation/resize/distance_m",
            "{bad}"
        );
    }
    assert_eq!(
        pointer(&w.plain(&with("operation", json!({"resize": {"distance_m": "6000"}}))).await),
        "/operation/resize/distance_m"
    );
    assert_eq!(
        pointer(&w.plain(&with("operation", move_end([91.0, -1.5]))).await),
        "/operation/move_end/point/lat"
    );
    assert_eq!(
        pointer(&w.plain(&with("operation", move_end([47.2, 181.0]))).await),
        "/operation/move_end/point/lon"
    );
    assert_eq!(pointer(&w.plain(&with("ride_type", json!("tandem"))).await), "/ride_type");
    // Le type : la valeur reçue n'est jamais rendue.
    let r = w.plain(&with("operation", json!({"resize": {"distance_m": "6000.123"}}))).await;
    assert!(!r.text.contains("6000.123"), "{}", r.text);
}

#[tokio::test]
async fn the_json_is_strict_outside_the_document_and_tolerant_inside_it() {
    let w = World::new();
    let path = one_way();
    let mut b = edit_body(&path, resize(6_000.0), None);
    b["inconnue"] = json!(1);
    b["operation"]["resize"]["bis"] = json!(2);
    let r = w.plain(&b).await;
    let v = assert_problem(&r, StatusCode::BAD_REQUEST, "invalid_input");
    let mut keys: Vec<String> = v["details"]["unknown_keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap().to_owned())
        .collect();
    keys.sort();
    assert_eq!(keys, ["/inconnue", "/operation/resize/bis"]);
    // Les clés inconnues du document (`inconnue` sur chaque arête, `v`, `api`) sont tolérées : même réponse qu'avec elles
    // retirées, et un `len` racine d'un autre type ne rend pas le document illisible.
    let body = edit_body(&path, resize(6_000.0), None);
    let mut ok_body = body.clone();
    ok_body["network_edges"]["autre"] = json!({"x": [1, 2]});
    ok_body["network_edges"]["v"] = json!("2");
    ok_body["network_edges"]["api"] = json!(9);
    for e in ok_body["network_edges"]["edges"].as_array_mut().unwrap() {
        e["encore"] = json!([1, {"a": null}]);
    }
    let mut clean = body.clone();
    for e in clean["network_edges"]["edges"].as_array_mut().unwrap() {
        e.as_object_mut().unwrap().remove("inconnue");
    }
    let (a, b) = (w.plain(&ok_body).await, w.plain(&clean).await);
    assert_ne!(a.status, StatusCode::BAD_REQUEST);
    assert_eq!(a.status, b.status);
    let mut odd_len = body.clone();
    odd_len["network_edges"]["len"] = json!("beaucoup");
    assert_ne!(w.plain(&odd_len).await.status, StatusCode::BAD_REQUEST);
    // Un corps qui n'est pas du JSON, un autre type de contenu.
    let text = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/generator/routes/edit")
        .header("content-type", "text/plain")
        .body(axum::body::Body::from("x"))
        .unwrap();
    assert_eq!(send(w.app.clone(), text).await.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let raw = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/generator/routes/edit")
        .header("content-type", "application/json")
        .body(axum::body::Body::from("{"))
        .unwrap();
    assert_eq!(pointer(&send(w.app.clone(), raw).await), "");
}

#[tokio::test]
async fn the_headers_are_optional_but_checked_as_soon_as_one_is_there() {
    let w = World::new();
    let body = edit_body(&square(5, 5, 6), resize(8_640.0), Some(0.25));
    // Une clé sans équipe, une équipe sans clé, une clé mal formée : 400 au pointeur de l'en-tête.
    let r = w.edit(Some(&key(1)), None, &body).await;
    assert_eq!(pointer(&r), "/headers/Ord-Team");
    assert_eq!(pointer(&w.edit(None, Some("team-a"), &body).await), "/headers/Ord-Namespace-Key");
    let r = w.edit(Some("pas-une-cle"), Some("team-a"), &body).await;
    assert_eq!(pointer(&r), "/headers/Ord-Namespace-Key");
    assert!(!r.text.contains("pas-une-cle"));
    // Sans équipe, le curseur est ignoré (mais validé) : la découverte.
    let v = ok(&w.plain(&body).await);
    assert_eq!(v["mode"], "discovery");
    // Une équipe sans surcouche : la découverte, et la réponse le dit.
    let v = ok(&w.edit(Some(&key(1)), Some("team-never-deposited"), &body).await);
    assert_eq!(v["mode"], "discovery");
    assert!(v["overlay"].is_null());
}

#[tokio::test]
async fn the_input_bounds_are_413s() {
    let w = World::new();
    let path = one_way();
    // Le corps : 4 Mio.
    let mut big = edit_body(&path, resize(6_000.0), None);
    big["network_edges"]["pad"] = json!("x".repeat(4 * 1024 * 1024));
    let v = assert_problem(&w.plain(&big).await, StatusCode::PAYLOAD_TOO_LARGE, "route_too_large");
    assert_eq!(v["details"]["limit"], "body_bytes");
    // Trop de points : 50 001 points distincts.
    let line: Vec<[f64; 2]> = (0..50_001).map(|i| [47.2 + f64::from(i) * 1e-6, -1.5]).collect();
    let mut b = edit_body(&path, resize(6_000.0), None);
    b["polyline"] = json!(ord_graph_match::geo::encode_polyline6(&line));
    let v = assert_problem(&w.plain(&b).await, StatusCode::PAYLOAD_TOO_LARGE, "route_too_large");
    assert_eq!(v["details"]["limit"], "points");
    // Trop long : 222 km en deux points.
    b["polyline"] = json!(ord_graph_match::geo::encode_polyline6(&[[47.2, -1.5], [49.2, -1.5]]));
    let v = assert_problem(&w.plain(&b).await, StatusCode::PAYLOAD_TOO_LARGE, "route_too_large");
    assert_eq!(v["details"]["limit"], "length_m");
}

#[tokio::test]
async fn coverage_is_judged_before_the_engine() {
    let w = World::new();
    let path = one_way();
    // Une arrivée hors du polygone.
    let v = assert_problem(
        &w.plain(&edit_body(&path, move_end(off(node(10, 14), 50_000.0, 0.0)), None)).await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "out_of_coverage",
    );
    assert_eq!(v["details"]["coverage"], "graph");
    // Une arrivée dans le polygone mais à plus de 500 m d'une voie : jugée par le moteur, après la file.
    let corner = node(20, 20);
    reason(&w.plain(&edit_body(&path, move_end(off(corner, 800.0, 800.0)), None)).await, "end_too_far");
    // Un seul point du parcours hors du polygone.
    let mut b = edit_body(&path, resize(6_000.0), None);
    let mut line = path_points(&path);
    line.push(off(node(16, 14), 30_000.0, 0.0));
    b["polyline"] = json!(ord_graph_match::geo::encode_polyline6(&line));
    reason(&w.plain(&b).await, "route_off_coverage");
}

#[tokio::test]
async fn a_route_that_cannot_be_posed_is_refused_with_the_reason() {
    let w = World::new();
    let sq = square(5, 5, 6);
    let op = resize(8_640.0);
    // Aucune arête : rien n'est apparié.
    let mut b = edit_body(&sq, op.clone(), None);
    b["network_edges"] = json!({"edges": []});
    reason(&w.plain(&b).await, "route_not_matched");
    let mut b = edit_body(&sq, op.clone(), None);
    b["network_edges"] = json!({});
    reason(&w.plain(&b).await, "route_not_matched");
    // Des arêtes dans le désordre, en double : les longueurs ne correspondent plus à la polyligne.
    let edges = edit_body(&sq, op.clone(), None)["network_edges"]["edges"].as_array().unwrap().clone();
    let mut b = edit_body(&sq, op.clone(), None);
    let mut reversed = edges.clone();
    reversed.reverse();
    b["network_edges"]["edges"] = json!(reversed);
    reason(&w.plain(&b).await, "route_mismatch");
    let mut b = edit_body(&sq, op.clone(), None);
    let mut twice = edges.clone();
    twice.extend(edges[..4].to_vec());
    b["network_edges"]["edges"] = json!(twice);
    reason(&w.plain(&b).await, "route_mismatch");
    // Le document d'un autre carré de même longueur : les coutures se projettent dans le désordre de la polyligne.
    let mut b = edit_body(&sq, op.clone(), None);
    b["network_edges"] = path_document(&square(8, 8, 6));
    reason(&w.plain(&b).await, "order_anomaly");
    // Un parcours d'une seule rue n'a aucune couture.
    let street = vec![(10, 10), (10, 11)];
    reason(&w.plain(&edit_body(&street, move_end(node(14, 14)), None)).await, "no_seam");
}

#[tokio::test]
async fn with_a_team_the_cursor_gives_the_mode_and_the_new_stretch_is_measured() {
    let w = World::new();
    let (k, team) = (key(1), "team-a");
    assert_eq!(send(w.app.clone(), put_digest(&k, team, "d1", team_deposit())).await.status, StatusCode::OK);
    // Le carré (6, 6) de 6 rues de côté est sur les rues de l'équipe ; ses rues intérieures aussi, en partie.
    let path = square(6, 6, 6);
    let body = edit_body(&path, resize(8_640.0), Some(0.25));
    let v = ok(&w.edit(Some(&k), Some(team), &body).await);
    assert_eq!(v["mode"], "team_first");
    assert!(v["overlay"]["deposited_at"].as_str().unwrap().ends_with('Z'));
    let routes = v["routes"].as_array().unwrap();
    assert!(
        routes.iter().any(|r| r["changed"]["team_m"].as_f64().unwrap() > 0.0),
        "un tronçon pris sur les routes de l'équipe : {}",
        serde_json::to_string(&routes.iter().map(|r| r["changed"].clone()).collect::<Vec<_>>()).unwrap()
    );
    for r in routes {
        let c = &r["changed"];
        let share =
            (c["team_m"].as_f64().unwrap() + c["bridged_m"].as_f64().unwrap()) / c["new_m"].as_f64().unwrap();
        assert!((c["team_share"].as_f64().unwrap() - share).abs() < 0.002);
    }
    // Un curseur 0 route en équipe d'abord (le strict n'est pas offert), un curseur 1 découvre.
    let strictish = edit_body(&path, resize(8_640.0), Some(0.0));
    assert_eq!(ok(&w.edit(Some(&k), Some(team), &strictish).await)["mode"], "team_first");
    let discovery = edit_body(&path, resize(8_640.0), Some(1.0));
    let v = ok(&w.edit(Some(&k), Some(team), &discovery).await);
    assert_eq!(v["mode"], "discovery");
    assert!(!v["overlay"].is_null());
    // Une autre équipe de la même clé : pas de surcouche, la découverte.
    let v = ok(&w.edit(Some(&k), Some("team-b"), &body).await);
    assert_eq!((v["mode"].as_str(), v["overlay"].is_null()), (Some("discovery"), true));
}

#[tokio::test]
async fn unmatched_metres_are_counted_in_the_kept_parts_only() {
    let w = World::new();
    let path = square(5, 5, 6);
    let op = resize(8_640.0);
    let edges = path_document(&path)["edges"].as_array().unwrap().clone();
    // Une arête illisible : un trou dans la marche, une étendue non appariée de l'axe du document.
    let broken = |i: usize| {
        let mut b = edit_body(&path, op.clone(), None);
        let mut e = edges.clone();
        e[i]["g"] = json!("");
        b["network_edges"]["edges"] = json!(e);
        (b, e[i]["fm"].as_f64().unwrap(), e[i]["tm"].as_f64().unwrap())
    };
    // Chaque arête intérieure rendue illisible, l'une après l'autre : quand la retouche répond, les mètres non appariés d'une
    // proposition sont ceux de l'étendue de l'arête qui tombent dans ses parties gardées.
    let (mut inside, mut outside, mut usable) = (0, 0, None);
    for i in 1..edges.len() - 1 {
        let (b, fm, tm) = broken(i);
        let r = w.plain(&b).await;
        if r.status != StatusCode::OK {
            continue;
        }
        let v = r.json();
        let original = v["original_m"].as_f64().unwrap();
        for route in v["routes"].as_array().unwrap() {
            let c = &route["changed"];
            let (from, to) = (c["from_m"].as_f64().unwrap(), c["to_m"].as_f64().unwrap());
            let kept = [(0.0, from), (to, original)];
            let expected: f64 = kept.iter().map(|&(a, b)| (tm.min(b) - fm.max(a)).max(0.0)).sum();
            let got = route["unmatched_m"].as_f64().unwrap();
            assert!((got - expected).abs() < 5.0, "arête {i} : {got} au lieu de {expected}");
            if expected > 50.0 {
                inside += 1;
                usable = Some((i, expected));
            } else if expected == 0.0 {
                outside += 1;
            }
        }
    }
    assert!(inside > 0, "un trou dans une partie gardée est compté");
    assert!(outside > 0, "un trou dans la partie remplacée ne l'est pas");
    // Sans `fm`, rien n'est localisable : la somme des mètres non appariés, plafonnée aux mètres gardés (une borne haute).
    let (i, _) = usable.expect("une retouche d'un document troué");
    let (mut b, fm, tm) = broken(i);
    let trv = tm - fm;
    for e in b["network_edges"]["edges"].as_array_mut().unwrap() {
        e.as_object_mut().unwrap().remove("fm");
    }
    let v = ok(&w.plain(&b).await);
    for route in v["routes"].as_array().unwrap() {
        let unmatched = route["unmatched_m"].as_f64().unwrap();
        assert!(unmatched > 0.0 && unmatched <= route["kept_m"].as_f64().unwrap() + 0.1, "{unmatched}");
        assert!((unmatched - trv.min(route["kept_m"].as_f64().unwrap())).abs() < 5.0, "{unmatched}");
    }
}

#[tokio::test]
async fn the_same_request_gives_the_same_bytes() {
    let w = World::new();
    let body = edit_body(&square(5, 5, 6), resize(8_640.0), None);
    let a = w.plain(&body).await;
    let b = w.plain(&body).await;
    assert_eq!(a.status, StatusCode::OK, "{}", a.text);
    assert_eq!(a.text, b.text);
    let mut other = body.clone();
    other["seed"] = json!(3);
    ok(&w.plain(&other).await);
    let m = edit_body(&one_way(), move_end(node(16, 17)), None);
    assert_eq!(w.plain(&m).await.text, w.plain(&m).await.text);
}

#[tokio::test]
async fn the_independent_checker_drops_what_it_refuses() {
    let w = World::new();
    let hooks = w.state.generator().hooks().expect("prêt");
    let body = edit_body(&square(5, 5, 6), resize(8_640.0), None);
    let clean = ok(&w.plain(&body).await);
    let n = clean["routes"].as_array().unwrap().len();
    assert!(n >= 1);
    // La première proposition est altérée : le contrôleur la jette, les autres restent (ou, seule, plus rien).
    hooks.corrupt_first_route.store(true, std::sync::atomic::Ordering::Relaxed);
    let r = w.plain(&body).await;
    if n > 1 {
        let v = ok(&r);
        assert_eq!(v["dropped"], 1);
        assert_eq!(v["routes"].as_array().unwrap().len(), n - 1);
    } else {
        reason(&r, "checker_rejected");
    }
    hooks.corrupt_first_route.store(false, std::sync::atomic::Ordering::Relaxed);
    // Toutes altérées : `checker_rejected`.
    hooks.corrupt_routes.store(true, std::sync::atomic::Ordering::Relaxed);
    reason(&w.plain(&body).await, "checker_rejected");
    let m = edit_body(&one_way(), move_end(node(16, 17)), None);
    reason(&w.plain(&m).await, "checker_rejected");
}
