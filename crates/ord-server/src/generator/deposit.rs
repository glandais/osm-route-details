//! Dépôt en flux de la surcouche d'une équipe (note lot 3a §4).
//!
//! Le corps est du NDJSON : une ligne par trace, puis la ligne de fin `{"end":{"tracks":N}}`. Le côté asynchrone lit
//! le corps par trames, découpe les lignes (une ligne ≤ `max_document_bytes`) et les passe, une à la fois, à **un seul
//! fil** bloquant qui apparie, agrège et lâche le texte. Mémoire bornée : une ligne en cours, une ligne en file, une
//! ligne lue, la mémoïsation par géométrie et le compte par arête. Un dépôt ne laisse jamais une demi-surcouche :
//! l'écriture est atomique (`Store::commit`) et n'a lieu qu'après la ligne de fin.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use http_body_util::BodyExt;
use ord_graph::overlay::{encode, OverlayMeta};
use ord_graph_match::agg::{params_hash, CAUSES};
use ord_graph_match::doc::{parse_document, type_code, TYPES};
use ord_graph_match::{CellIndex, Deposit, Near, SegIndex};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use tokio::sync::mpsc;
use utoipa::ToSchema;

use super::crypto::Credentials;
use super::store::{FileMeta, ReadyError, StoreError};
use super::Ready;
use crate::problem::{FieldError, LimitKind, Problem, ProblemCode, ProblemDetails};

/// Attente de place au plus longue de la construction en tâche de fond des données d'une équipe.
const WARM_BUDGET: Duration = Duration::from_secs(120);

/// Une ligne de trace : type de surface, drapeau « exclue du générateur », document `network_edges` (`null` :
/// illisible). Les clés inconnues de la ligne sont refusées ; celles du document sont tolérées.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrackLine<'a> {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    excluded: bool,
    #[serde(borrow, default)]
    network_edges: Option<&'a RawValue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndLine {
    end: EndBody,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndBody {
    tracks: u64,
}

enum Msg {
    Line(Vec<u8>),
    Finish,
}

/// Deposit report.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DepositReportV1 {
    /// Identity of the graph the overlay was built for.
    pub graph_id: String,
    pub deposited_at: String,
    pub expires_at: String,
    /// As received (`Ord-Source-Digest` header); absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_digest: Option<String>,
    /// Tracks read, tracks excluded from the generator, tracks whose document was unreadable.
    pub tracks: u64,
    pub tracks_excluded: u64,
    pub tracks_unreadable: u64,
    /// Graph edges in the overlay.
    pub edges: u64,
    /// Kilometres counted in the overlay.
    pub team_km: f64,
    pub km: DepositKm,
    /// Bytes of the stored file.
    pub stored_bytes: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DepositKm {
    pub total: f64,
    /// Outside the region the graph covers.
    pub outside: f64,
    /// Matched to the graph.
    pub matched: f64,
    /// Not matched, by cause (`border`, `invalid`, `no_osm_edge`, `chain_not_connected`, `geometry`,
    /// `ambiguous_parallel`, `way_kind`, `direction_closed`).
    #[schema(value_type = Object)]
    pub unmatched: serde_json::Map<String, serde_json::Value>,
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn bad_line(n: u64, message: &str) -> Problem {
    Problem::invalid_input("invalid deposit", vec![FieldError::new(format!("/lines/{n}"), message)])
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Ce que renvoie le fil bloquant.
struct Worker {
    ready: Arc<Ready>,
    creds: Arc<Credentials>,
    digest: Option<String>,
    /// Fin du budget du dépôt : le fil s'arrête de lui-même, même si le client est encore là.
    deadline: Instant,
}

impl Worker {
    /// Consomme les lignes jusqu'à `Finish`, puis écrit. `Err(None)` : le client est parti, rien n'est écrit.
    fn run(self, mut rx: mpsc::Receiver<Msg>) -> Result<DepositReportV1, Option<Problem>> {
        let r = &self.ready;
        let gf = r.engine.base();
        let cell;
        let full;
        let idx: &dyn Near = if r.cell_index_ok {
            cell = CellIndex::new(gf, r.cfg.deposit.index_cache_mb * 1024 * 1024);
            &cell
        } else {
            full = SegIndex::build(gf);
            &full
        };
        let mut dep = Deposit::new(gf, idx, &r.poly, r.cfg.deposit.memo_entries);
        let mut n = 0u64; // numéro de ligne, à partir de 1
        let mut tracks = 0u64;
        let mut end: Option<u64> = None;
        let over_budget = || Problem::new(ProblemCode::Timeout, "deposit budget exceeded");
        loop {
            match rx.blocking_recv() {
                None => return Err(None),
                Some(Msg::Finish) => break,
                Some(Msg::Line(raw)) => {
                    if Instant::now() >= self.deadline {
                        return Err(Some(over_budget()));
                    }
                    n += 1;
                    let line = trim(&raw);
                    if line.is_empty() {
                        continue;
                    }
                    if end.is_some() {
                        return Err(Some(bad_line(n, "data after the end line")));
                    }
                    if line.starts_with(b"{\"end\"") {
                        let e: EndLine = serde_json::from_slice(line)
                            .map_err(|_| Some(bad_line(n, "unreadable end line")))?;
                        end = Some(e.end.tracks);
                        continue;
                    }
                    if tracks >= r.cfg.deposit.max_tracks {
                        return Err(Some(Problem::too_large(
                            LimitKind::Tracks,
                            "too many tracks in the deposit",
                        )));
                    }
                    let t: TrackLine<'_> = serde_json::from_slice(line)
                        .map_err(|_| Some(bad_line(n, "unreadable track line")))?;
                    tracks += 1;
                    let edges = t.network_edges.and_then(|d| parse_document(d.get().as_bytes()));
                    dep.push_track(type_code(&t.ty), t.excluded, edges);
                }
            }
        }
        match end {
            None => return Err(Some(bad_line(n + 1, "missing end line"))),
            Some(e) if e != tracks => {
                return Err(Some(bad_line(n, "end line count does not match the tracks")))
            }
            Some(_) => {}
        }
        if Instant::now() >= self.deadline {
            return Err(Some(over_budget()));
        }
        let (edges, counts, stats) = dep.finish();
        let now = unix_now();
        let meta = FileMeta {
            deposited_at: now,
            source_digest: self.digest.clone(),
            matcher_hash: r.store.matcher_hash_hex(),
            edges: edges.len() as u64,
            team_km: r3(stats.km_counted),
            tracks: stats.tracks,
            tracks_excluded: stats.excluded,
            tracks_unreadable: stats.unreadable,
        };
        // Le `team_id` d'une surcouche du service vaut 0 : l'équipe n'est connue que par son nom dérivé.
        let ordo_meta = OverlayMeta {
            graph: gf.graph_identity(),
            team_id: 0,
            export_xxh3: 0,
            export_rows: stats.tracks,
            params_hash: params_hash(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            build_epoch: now,
            types: TYPES.iter().map(|t| (*t).to_string()).collect(),
        };
        let ordo = encode(&ordo_meta, &edges, &counts).map_err(|_| Some(Problem::internal()))?;
        if Instant::now() >= self.deadline {
            return Err(Some(over_budget()));
        }
        let stored = r.store.commit(&self.creds, &meta, &ordo).map_err(|e| match e {
            StoreError::Quota => {
                Some(Problem::new(ProblemCode::InsufficientStorage, "storage quota reached"))
            }
            StoreError::Io => {
                tracing::error!(ns8 = self.creds.ns8(), "overlay write failed");
                Some(Problem::internal())
            }
        })?;
        let mut unmatched = serde_json::Map::new();
        for (name, km) in CAUSES.iter().zip(stats.km_unmatched) {
            unmatched.insert((*name).to_string(), serde_json::json!(r3(km)));
        }
        tracing::info!(
            ns8 = self.creds.ns8(),
            tracks = stats.tracks,
            excluded = stats.excluded,
            unreadable = stats.unreadable,
            edges = meta.edges,
            km_total = r3(stats.km_total),
            km_matched = r3(stats.km_matched),
            km_outside = r3(stats.km_outside),
            km_counted = meta.team_km,
            memo_hits = stats.memo_hits,
            memo_misses = stats.memo_misses,
            bytes = stored,
            "overlay deposited"
        );
        Ok(DepositReportV1 {
            graph_id: r.graph_id.clone(),
            deposited_at: super::iso8601(now),
            expires_at: super::iso8601(now + i64::try_from(r.store.ttl().as_secs()).unwrap_or(i64::MAX)),
            source_digest: self.digest,
            tracks: stats.tracks,
            tracks_excluded: stats.excluded,
            tracks_unreadable: stats.unreadable,
            edges: meta.edges,
            team_km: meta.team_km,
            km: DepositKm {
                total: r3(stats.km_total),
                outside: r3(stats.km_outside),
                matched: r3(stats.km_matched),
                unmatched,
            },
            stored_bytes: stored,
        })
    }
}

/// Ligne sans son saut de ligne ni son retour chariot, ni espaces de bord.
fn trim(raw: &[u8]) -> &[u8] {
    let start = raw.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(raw.len());
    let end = raw.iter().rposition(|b| !b.is_ascii_whitespace()).map_or(start, |i| i + 1);
    &raw[start..end]
}

/// Ce qui reste du budget d'un dépôt.
fn budget_left(started: Instant, cfg: &crate::config::GeneratorConfig) -> Duration {
    Duration::from_secs(cfg.deposit.budget_s).saturating_sub(started.elapsed())
}

fn count(outcome: &'static str) {
    metrics::counter!("ord_overlay_deposits_total", "outcome" => outcome).increment(1);
}

impl Ready {
    /// Reçoit le dépôt d'une équipe. `creds` est dérivé de la clé de l'appelant : rien d'autre ne nomme un fichier.
    pub(crate) async fn deposit(
        self: &Arc<Self>,
        creds: Credentials,
        digest: Option<String>,
        body: Body,
    ) -> Result<DepositReportV1, Problem> {
        let started = Instant::now();
        let result = self.deposit_inner(creds, digest, body, started).await;
        metrics::histogram!("ord_overlay_deposit_seconds").record(started.elapsed().as_secs_f64());
        count(match &result {
            Ok(_) => "ok",
            Err(p) => match p.code {
                ProblemCode::InvalidInput => "invalid",
                ProblemCode::RouteTooLarge => "too_large",
                ProblemCode::Timeout => "timeout",
                ProblemCode::InsufficientStorage => "storage",
                ProblemCode::DepositInProgress | ProblemCode::Overloaded => "busy",
                _ => "error",
            },
        });
        result
    }

    async fn deposit_inner(
        self: &Arc<Self>,
        creds: Credentials,
        digest: Option<String>,
        body: Body,
        started: Instant,
    ) -> Result<DepositReportV1, Problem> {
        let creds = Arc::new(creds);
        let guard = self.store.try_lock(&creds).ok_or_else(|| {
            Problem::new(ProblemCode::DepositInProgress, "a deposit of this team is already running")
        })?;
        let busy = |what: &str| {
            let mut p = Problem::new(ProblemCode::Overloaded, what.to_owned());
            p.retry_after = Some(Duration::from_secs(30));
            p
        };
        // Un environnement ne tient qu'une place (en cours ou en attente) : il ne peut pas en retenir plusieurs, ni
        // faire la queue derrière lui-même.
        if self.store.namespace_locks(&creds) > 1 {
            return Err(busy("a deposit of this environment is already running"));
        }
        // La place de dépôt est une file équitable (première arrivée, première servie) : un environnement qui vient de
        // finir repasse derrière ceux qui attendent. L'attente est bornée, puis 503.
        let wait = Duration::from_secs(self.cfg.deposit.queue_wait_s).min(budget_left(started, &self.cfg));
        let permit = match tokio::time::timeout(wait, Arc::clone(&self.deposit_slots).acquire_owned()).await {
            Ok(Ok(p)) => p,
            _ => return Err(busy("deposit capacity is in use")),
        };
        {
            let store = Arc::clone(&self.store);
            let c = Arc::clone(&creds);
            let ok = tokio::task::spawn_blocking(move || store.precheck_quota(&c))
                .await
                .map_err(|_| Problem::internal())?;
            ok.map_err(|_| Problem::new(ProblemCode::InsufficientStorage, "storage quota reached"))?;
        }

        let (tx, rx) = mpsc::channel::<Msg>(1);
        let deadline = started + Duration::from_secs(self.cfg.deposit.budget_s);
        let warm_creds = Arc::clone(&creds);
        let worker = Worker { ready: Arc::clone(self), creds, digest, deadline };
        // Le verrou de l'équipe et la place de dépôt vivent autant que le fil : un client qui part ne les rend pas
        // avant que l'écriture en cours soit finie.
        let mut join = tokio::task::spawn_blocking(move || {
            let _guard = guard;
            let _permit = permit;
            worker.run(rx)
        });

        // `feed` possède l'émetteur : s'il échoue, l'émetteur tombe, le fil abandonne et rien n'est écrit.
        let feed = self.feed(body, tx, deadline);
        tokio::pin!(feed);
        let fed = tokio::select! {
            r = &mut feed => r,
            // Le fil s'est arrêté avant la fin du flux : sa raison est la réponse.
            done = &mut join => return Self::joined(done),
        };
        if let Err(p) = fed {
            let _ = (&mut join).await;
            return Err(p);
        }
        let report = Self::joined((&mut join).await)?;
        self.warm_team(warm_creds);
        Ok(report)
    }

    /// Construit en tâche de fond les données de l'équipe qui vient de déposer : sa première génération les trouve au
    /// cache au lieu de les construire dans son budget (pages froides, une grosse équipe y passait plus de 8 s : 504).
    /// Seul un dépôt livre la clé qui ouvre la surcouche : au redémarrage, la première demande les construit.
    fn warm_team(self: &Arc<Self>, creds: Arc<Credentials>) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else { return };
        let r = Arc::clone(self);
        drop(handle.spawn_blocking(move || {
            let deadline = Instant::now() + WARM_BUDGET;
            let reserve = |bytes: usize| -> Result<Box<dyn Send + '_>, ReadyError> {
                r.windows
                    .reserve_build(bytes, deadline, &|| false)
                    .map(|g| Box::new(g) as Box<dyn Send + '_>)
                    .map_err(|_| ReadyError::Overloaded)
            };
            // Un échec (place, surcouche déjà remplacée) n'est pas une erreur : la première génération construira.
            let _ = r.store.ready(&creds, &r.engine, &r.graph_index, &reserve);
        }));
    }

    fn joined(
        done: Result<Result<DepositReportV1, Option<Problem>>, tokio::task::JoinError>,
    ) -> Result<DepositReportV1, Problem> {
        match done {
            Ok(Ok(report)) => Ok(report),
            Ok(Err(Some(p))) => Err(p),
            Ok(Err(None)) => Err(Problem::new(ProblemCode::InvalidInput, "deposit aborted")),
            Err(_) => Err(Problem::internal()),
        }
    }

    /// Lit le corps, coupe les lignes et les envoie. Bornes : corps décompressé, ligne, silence, budget.
    async fn feed(&self, mut body: Body, tx: mpsc::Sender<Msg>, deadline: Instant) -> Result<(), Problem> {
        let d = &self.cfg.deposit;
        let idle = Duration::from_secs(d.idle_timeout_s);
        let tokio_deadline = tokio::time::Instant::from_std(deadline);
        let timeout = |what: &str| Problem::new(ProblemCode::Timeout, format!("deposit {what}"));
        let too_big_line = || {
            Problem::with_details(
                ProblemCode::RouteTooLarge,
                "a track document exceeds the limit",
                ProblemDetails { limit: Some(LimitKind::DocumentBytes), ..Default::default() },
            )
        };
        // Envoi à la file du fil, dans le budget : un fil lent ne prolonge pas le dépôt. `Ok(false)` : le fil s'est arrêté.
        let send = |msg: Msg| {
            let tx = &tx;
            async move {
                match tokio::time::timeout_at(tokio_deadline, tx.send(msg)).await {
                    Err(_) => Err(timeout("budget exceeded")),
                    Ok(r) => Ok(r.is_ok()),
                }
            }
        };
        let mut buf: Vec<u8> = Vec::new();
        let mut total = 0u64;
        loop {
            let remaining =
                deadline.checked_duration_since(Instant::now()).ok_or_else(|| timeout("budget exceeded"))?;
            let frame = match tokio::time::timeout(idle.min(remaining), body.frame()).await {
                Err(_) if remaining <= idle => return Err(timeout("budget exceeded")),
                Err(_) => return Err(timeout("stalled")),
                Ok(None) => break,
                Ok(Some(Err(_))) => {
                    return Err(Problem::invalid_input("deposit body could not be read", Vec::new()));
                }
                Ok(Some(Ok(f))) => f,
            };
            let Ok(data) = frame.into_data() else { continue };
            total += data.len() as u64;
            if total > d.max_body_bytes {
                return Err(Problem::too_large(LimitKind::BodyBytes, "deposit body exceeds the limit"));
            }
            // Ce qui précède `scan` ne contient aucun saut de ligne : seuls les octets nouveaux sont parcourus (une
            // ligne de 16 Mio en trames de 4 Kio ne coûte pas un parcours entier par trame).
            let mut scan = buf.len();
            buf.extend_from_slice(&data);
            // Une seule passe : toutes les lignes complètes, puis un seul `drain`.
            let mut from = 0usize;
            while let Some(i) = buf[scan..].iter().position(|&b| b == b'\n') {
                let line = &buf[from..scan + i];
                if line.len() > d.max_document_bytes {
                    return Err(too_big_line());
                }
                if !send(Msg::Line(line.to_vec())).await? {
                    // Le fil s'est arrêté : l'appelant lit sa raison dans `join`.
                    return std::future::pending().await;
                }
                from = scan + i + 1;
                scan = from;
            }
            buf.drain(..from);
            if buf.len() > d.max_document_bytes {
                return Err(too_big_line());
            }
        }
        if !buf.is_empty() && !send(Msg::Line(buf)).await? {
            return std::future::pending().await;
        }
        if !send(Msg::Finish).await? {
            return std::future::pending().await;
        }
        Ok(())
    }
}
