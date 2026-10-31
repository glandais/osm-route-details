//! Faux serveur de tuiles local (modèle `OS/tests/tiles.rs:28-55`) : la réponse à la n-ième
//! requête est scriptée, et le serveur compte les requêtes reçues.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, Response, StatusCode};
use axum::routing::get;
use axum::Router;

/// Requête reçue : tuile demandée (`{z}/{x}/{y}`) ou requête WMS (`/wms?…`, tuile à zéro), et
/// en-têtes (revalidation).
#[derive(Clone, Debug)]
pub struct ScriptedRequest {
    pub z: u8,
    pub x: u32,
    pub y: u32,
    /// Chaîne de requête d'un appel `/wms`.
    pub query: Option<String>,
    pub headers: HeaderMap,
}

impl ScriptedRequest {
    /// Valeur d'un paramètre de la requête WMS (`BBOX`, `LAYERS`…).
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query
            .as_deref()?
            .split('&')
            .find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == name).map(|(_, v)| v))
    }
}

#[derive(Clone, Debug, Default)]
pub struct ScriptedResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub delay: Duration,
}

impl ScriptedResponse {
    pub fn ok(body: Vec<u8>) -> Self {
        Self { status: 200, body, ..Self::default() }
    }

    pub fn status(status: u16) -> Self {
        Self { status, ..Self::default() }
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

/// `n` (à partir de 0) est le rang de la requête parmi toutes celles reçues par le serveur.
pub type Handler = Arc<dyn Fn(usize, &ScriptedRequest) -> ScriptedResponse + Send + Sync>;

#[derive(Clone)]
struct Shared {
    hits: Arc<AtomicUsize>,
    handler: Handler,
}

/// Serveur scripté sur `127.0.0.1:0`, arrêté quand il est lâché.
pub struct ScriptedServer {
    /// Gabarit `{z}/{x}/{y}` à mettre dans la configuration de la source.
    pub url_template: String,
    /// URL du service WMS (`elevation.ign.wms_url`).
    pub wms_url: String,
    pub hits: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for ScriptedServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedServer").field("url_template", &self.url_template).finish_non_exhaustive()
    }
}

async fn serve(
    State(shared): State<Shared>,
    Path((z, x, file)): Path<(u8, u32, String)>,
    headers: HeaderMap,
) -> Response<Body> {
    let y = file.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(u32::MAX);
    reply(&shared, ScriptedRequest { z, x, y, query: None, headers }).await
}

async fn serve_wms(
    State(shared): State<Shared>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response<Body> {
    reply(&shared, ScriptedRequest { z: 0, x: 0, y: 0, query, headers }).await
}

async fn reply(shared: &Shared, req: ScriptedRequest) -> Response<Body> {
    let n = shared.hits.fetch_add(1, Ordering::SeqCst);
    let reply = (shared.handler)(n, &req);
    if !reply.delay.is_zero() {
        tokio::time::sleep(reply.delay).await;
    }
    let mut builder =
        Response::builder().status(StatusCode::from_u16(reply.status).unwrap_or(StatusCode::OK));
    for (k, v) in &reply.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    builder.body(Body::from(reply.body)).unwrap_or_default()
}

impl ScriptedServer {
    pub async fn start(
        handler: impl Fn(usize, &ScriptedRequest) -> ScriptedResponse + Send + Sync + 'static,
    ) -> Self {
        let hits = Arc::new(AtomicUsize::new(0));
        let shared = Shared { hits: Arc::clone(&hits), handler: Arc::new(handler) };
        let app = Router::new()
            .route("/{z}/{x}/{file}", get(serve))
            .route("/wms", get(serve_wms))
            .with_state(shared);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("port local libre");
        let addr = listener.local_addr().expect("adresse locale");
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            url_template: format!("http://{addr}/{{z}}/{{x}}/{{y}}.webp"),
            wms_url: format!("http://{addr}/wms"),
            hits,
            task,
        }
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

impl Drop for ScriptedServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
