//! Appel du routeur en mémoire (`tower::ServiceExt::oneshot`), sans réseau.

#![allow(dead_code)]

use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use ord_server::config::Config;
use ord_server::{router, AppState};
use serde_json::Value;
use tower::ServiceExt;

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub text: String,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or_else(|e| panic!("JSON attendu ({e}) : {}", self.text))
    }

    pub fn content_type(&self) -> &str {
        self.headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("")
    }
}

pub fn app() -> Router {
    app_with(&Config::default())
}

pub fn app_with(config: &Config) -> Router {
    router(AppState::new(config))
}

pub async fn send(app: Router, req: Request<Body>) -> Reply {
    let response = app.oneshot(req).await.expect("le routeur ne renvoie pas d'erreur");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.expect("corps lisible").to_bytes();
    Reply { status, headers, text: String::from_utf8_lossy(&bytes).into_owned() }
}

pub fn get(uri: &str) -> Request<Body> {
    Request::builder().method(Method::GET).uri(uri).body(Body::empty()).expect("requête valide")
}

pub fn post_raw(content_type: Option<&str>, body: impl Into<Body>) -> Request<Body> {
    let mut builder = Request::builder().method(Method::POST).uri("/v1/route-details");
    if let Some(ct) = content_type {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }
    builder.body(body.into()).expect("requête valide")
}

pub fn post_json(body: &Value) -> Request<Body> {
    post_raw(Some("application/json"), body.to_string())
}

pub async fn post(body: &Value) -> Reply {
    send(app(), post_json(body)).await
}

/// Vérifie la forme RFC 9457 d'un problème et renvoie son corps (T05 §2.5, point 5).
pub fn assert_problem(reply: &Reply, status: StatusCode, code: &str) -> Value {
    assert_eq!(reply.status, status, "{}", reply.text);
    assert_eq!(reply.content_type(), "application/problem+json");
    let body = reply.json();
    assert_eq!(body["type"], format!("urn:osm-route-details:problem:{code}"));
    assert_eq!(body["code"], code);
    assert_eq!(body["status"], status.as_u16());
    assert!(body["title"].as_str().is_some_and(|t| !t.is_empty()));
    assert!(body["detail"].is_string());
    let request_id = reply.headers.get("x-request-id").and_then(|v| v.to_str().ok());
    assert_eq!(body["instance"].as_str(), request_id, "instance = X-Request-Id");
    body
}
