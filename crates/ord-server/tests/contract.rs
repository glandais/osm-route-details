//! Contrat HTTP et OpenAPI (T05 §2.5, points 1, 2, 3 et 5).

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use axum::http::{Method, StatusCode};
use common::*;
use ord_server::app::public_routes;
use ord_server::{internal_router, openapi, render_contract};
use serde_json::json;

fn contract_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contract/openapi.json")
}

fn committed_contract() -> String {
    std::fs::read_to_string(contract_path()).expect("contract/openapi.json versionné")
}

/// Point 1 : le contrat commité est celui rendu par le code, et le rendu est déterministe.
#[test]
fn committed_contract_matches_code() {
    let rendered = render_contract();
    assert_eq!(rendered, render_contract(), "rendu non déterministe");
    assert!(
        committed_contract() == rendered,
        "contract/openapi.json est périmé : lancer `cargo run -p ord-server -- openapi`"
    );
}

/// Point 2 : chaque route exposée a une opération décrite (operationId, résumé), et réciproquement.
#[tokio::test]
async fn every_route_has_an_operation_and_vice_versa() {
    let doc = serde_json::to_value(openapi()).expect("OpenAPI sérialisable");
    let mut documented = BTreeSet::new();
    for (path, item) in doc["paths"].as_object().expect("paths") {
        for (method, op) in item.as_object().expect("path item") {
            let id = op["operationId"].as_str().unwrap_or("");
            let summary = op["summary"].as_str().unwrap_or("");
            assert!(!id.is_empty() && !summary.is_empty(), "{method} {path} sans operationId ou résumé");
            documented.insert((path.clone(), method.to_uppercase()));
        }
    }
    let exposed: BTreeSet<_> =
        public_routes().into_iter().map(|(path, method, _)| (path.to_owned(), method.to_string())).collect();
    assert_eq!(exposed, documented);

    // Chaque opération documentée est bien servie par le routeur (ni 404 ni 405).
    for (path, method) in &documented {
        let method: Method = method.parse().expect("méthode HTTP");
        let req = axum::http::Request::builder()
            .method(method.clone())
            .uri(path)
            .body(axum::body::Body::empty())
            .expect("requête");
        let reply = send(app(), req).await;
        assert!(
            ![StatusCode::NOT_FOUND, StatusCode::METHOD_NOT_ALLOWED].contains(&reply.status),
            "{method} {path} → {}",
            reply.status
        );
    }
}

/// Point 3 : le contrat servi sur le port interne est le contrat commité.
#[tokio::test]
async fn served_contract_is_committed_contract() {
    let reply = send(internal_router(None), get("/openapi.json")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.text, committed_contract());
}

/// Point 5 : chaque erreur est un problème RFC 9457 cohérent (type, code, statut, instance).
#[tokio::test]
async fn errors_are_rfc9457_problems() {
    let valid = ord_testkit::routes::request_body(
        &ord_testkit::routes::meridian(ord_testkit::routes::START, 1_000.0, 3),
        None,
    );
    let mut small = ord_server::config::Config::default();
    small.limits.max_body_bytes = 64;
    let cases = [
        (send(app(), get("/nope")).await, StatusCode::NOT_FOUND, "not_found"),
        (send(app(), get("/v1/route-details")).await, StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"),
        (
            send(app(), post_raw(Some("text/plain"), valid.to_string())).await,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        ),
        (
            send(app(), post_raw(None, valid.to_string())).await,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        ),
        (
            send(app(), post_raw(Some("application/json"), "{")).await,
            StatusCode::BAD_REQUEST,
            "invalid_input",
        ),
        (post(&json!({"points": [], "foo": 1})).await, StatusCode::BAD_REQUEST, "invalid_input"),
        (send(app_with(&small), post_json(&valid)).await, StatusCode::PAYLOAD_TOO_LARGE, "route_too_large"),
        (post(&valid).await, StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    ];
    for (reply, status, code) in &cases {
        assert_problem(reply, *status, code);
    }
}
