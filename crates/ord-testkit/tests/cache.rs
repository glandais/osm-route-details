//! Accès données et caches (T05 §2.1) sur un faux serveur local : jamais de réseau.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ord_data::disk::DiskStore;
use ord_data::http::{build_client, HttpSettings, XyzHttpSource};
use ord_data::port::{namespace, ReaderSettings, SourceDescriptor, StatusPolicy, TileReader};
use ord_domain::budget::{Deadline, RequestId};
use ord_domain::ctx::{FailureKind, RequestCtx};
use ord_domain::tiles::{DecodeError, SourceId, TileCodec, TileKey, TileOutcome, TtlClass};
use ord_testkit::{ScriptedResponse, ScriptedServer};

const K: TileKey = TileKey { z: 12, x: 2040, y: 1400 };

/// Codec de test : la tuile est le corps lui-même ; un corps HTML est invalide.
struct RawCodec {
    ttl: TtlClass,
}

impl TileCodec for RawCodec {
    type Tile = Vec<u8>;
    fn decode(&self, _id: TileKey, bytes: &[u8]) -> Result<Vec<u8>, DecodeError> {
        if bytes.starts_with(b"<") {
            Err(DecodeError::Format("html".into()))
        } else {
            Ok(bytes.to_vec())
        }
    }
    fn empty(&self, _id: TileKey) -> Vec<u8> {
        Vec::new()
    }
    fn weight(&self, t: &Vec<u8>) -> u32 {
        t.len() as u32 + 1
    }
    fn ttl_class(&self, _t: &Vec<u8>) -> TtlClass {
        self.ttl
    }
}

struct Setup {
    server: ScriptedServer,
    root: tempfile::TempDir,
    reader: TileReader<RawCodec>,
    namespace: String,
}

fn ctx() -> RequestCtx {
    RequestCtx::new(RequestId("test".into()), Deadline::after(Duration::from_secs(30)))
}

fn reader_for(
    server: &ScriptedServer,
    root: &std::path::Path,
    policy: StatusPolicy,
    ttl: TtlClass,
    offline: bool,
    negative_ttl: Duration,
) -> (TileReader<RawCodec>, String) {
    let ns = namespace("test", &server.url_template);
    let descriptor = SourceDescriptor {
        id: SourceId::Mapterhorn,
        zoom: 12,
        template: server.url_template.clone(),
        namespace: ns.clone(),
        status_policy: policy,
        concurrency: 4,
        max_attempts: 6,
        retry_base: Duration::from_millis(5),
    };
    let client = build_client(&HttpSettings {
        user_agent: "ord-test".into(),
        connect_timeout: Duration::from_secs(2),
        fetch_timeout: Duration::from_secs(5),
    })
    .expect("client");
    let source = Arc::new(XyzHttpSource::new(descriptor, client));
    let settings = ReaderSettings { offline, memory_bytes: 1 << 20, negative_ttl };
    (TileReader::new(source, RawCodec { ttl }, Some(DiskStore::new(root, &ns)), &settings), ns)
}

async fn setup(
    policy: StatusPolicy,
    ttl: TtlClass,
    handler: impl Fn(usize, &ord_testkit::ScriptedRequest) -> ScriptedResponse + Send + Sync + 'static,
) -> Setup {
    let server = ScriptedServer::start(handler).await;
    let root = tempfile::tempdir().expect("dossier temporaire");
    let (reader, namespace) = reader_for(&server, root.path(), policy, ttl, false, Duration::from_secs(60));
    Setup { server, root, reader, namespace }
}

fn disk(s: &Setup) -> DiskStore {
    DiskStore::new(s.root.path(), &s.namespace)
}

#[tokio::test]
async fn error_page_in_200_is_never_cached() {
    let s = setup(StatusPolicy::MAPTERHORN, TtlClass::Immutable, |_, _| {
        ScriptedResponse::ok(b"<html>captive portal</html>".to_vec())
    })
    .await;
    let out = s.reader.get(K, &ctx()).await;
    let TileOutcome::Failed(f) = out else { panic!("échec attendu") };
    assert_eq!(f.kind, FailureKind::Invalid);
    assert!(disk(&s).read(K).is_none(), "rien sur disque");
    assert!(!f.message.contains("127.0.0.1"), "jamais d'URL dans un échec");
}

#[tokio::test]
async fn poisoned_disk_entry_is_removed_then_refetched() {
    let s =
        setup(StatusPolicy::MAPTERHORN, TtlClass::Immutable, |_, _| ScriptedResponse::ok(b"good".to_vec()))
            .await;
    // Entrée qui passe la somme de contrôle mais ne décode plus.
    disk(&s).write(
        K,
        ord_data::disk::StoredIssue::Body,
        &ord_data::port::EntryMeta::now(),
        TtlClass::Immutable,
        b"<bad>",
    );
    let TileOutcome::Data { tile, stale } = s.reader.get(K, &ctx()).await else { panic!("donnée attendue") };
    assert_eq!(tile.as_slice(), b"good");
    assert!(!stale);
    assert_eq!(s.server.hits(), 1);
    assert_eq!(disk(&s).read(K).map(|e| e.body), Some(b"good".to_vec()));
}

#[tokio::test]
async fn not_found_follows_the_source_policy() {
    // Mapterhorn : 404 = au-delà du zoom local, Absent, persisté.
    let s = setup(StatusPolicy::MAPTERHORN, TtlClass::Immutable, |_, _| ScriptedResponse::status(404)).await;
    assert!(matches!(s.reader.get(K, &ctx()).await, TileOutcome::Absent));
    assert_eq!(disk(&s).read(K).map(|e| e.issue), Some(ord_data::disk::StoredIssue::Absent));
    // VersaTiles : 404 = indisponible, rien n'est écrit.
    let s = setup(StatusPolicy::VERSATILES, TtlClass::Immutable, |_, _| ScriptedResponse::status(404)).await;
    assert!(matches!(s.reader.get(K, &ctx()).await, TileOutcome::Failed(_)));
    assert!(disk(&s).read(K).is_none());
}

#[tokio::test]
async fn no_content_is_an_empty_tile_not_a_failure() {
    let s = setup(StatusPolicy::VERSATILES, TtlClass::Immutable, |_, _| ScriptedResponse::status(204)).await;
    assert!(matches!(s.reader.get(K, &ctx()).await, TileOutcome::Empty(_)));
    let s = setup(StatusPolicy::MAPTERHORN, TtlClass::Immutable, |_, _| ScriptedResponse::status(204)).await;
    assert!(matches!(s.reader.get(K, &ctx()).await, TileOutcome::Failed(_)));
}

#[tokio::test]
async fn negative_cache_is_short() {
    let server = ScriptedServer::start(|_, _| ScriptedResponse::status(400)).await;
    let root = tempfile::tempdir().unwrap();
    let (reader, _) = reader_for(
        &server,
        root.path(),
        StatusPolicy::MAPTERHORN,
        TtlClass::Immutable,
        false,
        Duration::from_millis(200),
    );
    let c = ctx();
    assert!(matches!(reader.get(K, &c).await, TileOutcome::Failed(_)));
    assert!(matches!(reader.get(K, &c).await, TileOutcome::Failed(_)));
    assert_eq!(server.hits(), 1, "pendant le TTL négatif, aucune requête");
    assert_eq!(c.log().failures.len(), 1, "échec dédoublonné au journal");
    let other = ctx();
    assert!(matches!(reader.get(K, &other).await, TileOutcome::Failed(_)));
    assert_eq!(other.log().failures.len(), 1, "réinscrit au journal de chaque requête");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(matches!(reader.get(K, &ctx()).await, TileOutcome::Failed(_)));
    assert_eq!(server.hits(), 2, "relu après expiration");
}

#[tokio::test]
async fn concurrent_loads_are_deduplicated() {
    let s = setup(StatusPolicy::MAPTERHORN, TtlClass::Immutable, |_, _| ScriptedResponse {
        delay: Duration::from_millis(100),
        ..ScriptedResponse::ok(b"tile".to_vec())
    })
    .await;
    let reader = Arc::new(s.reader);
    let mut tasks = Vec::new();
    for _ in 0..10 {
        let r = Arc::clone(&reader);
        tasks.push(tokio::spawn(async move { matches!(r.get(K, &ctx()).await, TileOutcome::Data { .. }) }));
    }
    for t in tasks {
        assert!(t.await.unwrap());
    }
    assert_eq!(s.server.hits(), 1, "10 tâches, 1 requête");
}

#[tokio::test]
async fn retries_429_and_503_with_retry_after_then_backoff() {
    let s = setup(StatusPolicy::MAPTERHORN, TtlClass::Immutable, |n, _| match n {
        0 => ScriptedResponse::status(429).header("retry-after", "0"),
        1..=3 => ScriptedResponse::status(503),
        4 => ScriptedResponse::status(502),
        _ => ScriptedResponse::ok(b"finally".to_vec()),
    })
    .await;
    assert!(matches!(s.reader.get(K, &ctx()).await, TileOutcome::Data { .. }));
    assert_eq!(s.server.hits(), 6, "6 essais au total");

    let s = setup(StatusPolicy::MAPTERHORN, TtlClass::Immutable, |_, _| ScriptedResponse::status(503)).await;
    let TileOutcome::Failed(f) = s.reader.get(K, &ctx()).await else { panic!("échec attendu") };
    assert_eq!(s.server.hits(), 6, "jamais plus de 6 essais");
    assert_eq!(f.status, Some(503));
}

#[tokio::test]
async fn retries_stop_at_the_request_deadline() {
    let s = setup(StatusPolicy::MAPTERHORN, TtlClass::Immutable, |_, _| {
        ScriptedResponse::status(429).header("retry-after", "120")
    })
    .await;
    let c = RequestCtx::new(RequestId("t".into()), Deadline::after(Duration::from_secs(5)));
    let TileOutcome::Failed(f) = s.reader.get(K, &c).await else { panic!("échec attendu") };
    assert_eq!(f.kind, FailureKind::Deadline);
    assert_eq!(f.retry_after, Some(Duration::from_secs(120)));
    assert_eq!(s.server.hits(), 1);
}

#[tokio::test]
async fn stale_entry_served_on_error_and_revalidated_with_304() {
    let calls = Arc::new(AtomicU64::new(0));
    let seen = Arc::clone(&calls);
    let s = setup(StatusPolicy::MAPTERHORN, TtlClass::Expires(Duration::ZERO), move |n, req| {
        if req.headers.get("if-none-match").is_some() {
            seen.fetch_add(1, Ordering::SeqCst);
        }
        match n {
            0 => ScriptedResponse::ok(b"v1".to_vec()).header("etag", "\"v1\""),
            1 => ScriptedResponse::status(304),
            _ => ScriptedResponse::status(500),
        }
    })
    .await;
    assert!(matches!(s.reader.get(K, &ctx()).await, TileOutcome::Data { stale: false, .. }));
    // Un nouveau lecteur (mémoire vide) sur le même disque : l'entrée est périmée (TTL nul).
    let (r2, _) = reader_for(
        &s.server,
        s.root.path(),
        StatusPolicy::MAPTERHORN,
        TtlClass::Expires(Duration::ZERO),
        false,
        Duration::from_secs(60),
    );
    let c = ctx();
    let TileOutcome::Data { tile, stale } = r2.get(K, &c).await else { panic!("304 : ancien corps") };
    assert_eq!((tile.as_slice(), stale), (b"v1".as_slice(), false));
    assert_eq!(c.log().counters.not_modified, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "revalidation par If-None-Match");
    let (r3, _) = reader_for(
        &s.server,
        s.root.path(),
        StatusPolicy::MAPTERHORN,
        TtlClass::Expires(Duration::ZERO),
        false,
        Duration::from_secs(60),
    );
    let c = ctx();
    let TileOutcome::Data { tile, stale } = r3.get(K, &c).await else {
        panic!("périmée servie en secours")
    };
    assert_eq!((tile.as_slice(), stale), (b"v1".as_slice(), true));
    assert_eq!(c.log().stale_served.len(), 1);
}

#[tokio::test]
async fn offline_reads_disk_only() {
    let server = ScriptedServer::start(|_, _| ScriptedResponse::ok(b"x".to_vec())).await;
    let root = tempfile::tempdir().unwrap();
    let (reader, ns) = reader_for(
        &server,
        root.path(),
        StatusPolicy::MAPTERHORN,
        TtlClass::Immutable,
        true,
        Duration::from_secs(60),
    );
    let TileOutcome::Failed(f) = reader.get(K, &ctx()).await else { panic!("hors ligne : échec") };
    assert_eq!(f.kind, FailureKind::Offline);
    assert_eq!(server.hits(), 0);
    let other = TileKey { x: K.x + 1, ..K };
    DiskStore::new(root.path(), &ns).write(
        other,
        ord_data::disk::StoredIssue::Body,
        &ord_data::port::EntryMeta::now(),
        TtlClass::Immutable,
        b"cached",
    );
    assert!(matches!(reader.get(other, &ctx()).await, TileOutcome::Data { .. }));
    assert_eq!(server.hits(), 0);
}
