//! Transports HTTP (T02 §3) : XYZ et WMS `GetMap`. Un client par processus, budget par hôte
//! (cadence, concurrence, report après 429), table des statuts propre à la source. Les relances
//! sont faites par `TileReader`, qui connaît l'échéance ; chaque essai repasse par le budget.

use std::time::Duration;

use bytes::Bytes;
use ord_domain::tiles::TileKey;
use reqwest::header::{self, HeaderMap};
use reqwest::StatusCode;
use std::sync::Arc;

use crate::budget::HostBudget;
use crate::port::{EntryMeta, FetchError, Fetched, SourceDescriptor, StatusMeaning, TileSource};

/// Réglages du client HTTP unique (T07 §1, `http_client`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpSettings {
    pub user_agent: String,
    pub connect_timeout: Duration,
    pub fetch_timeout: Duration,
}

/// Client HTTP partagé par toutes les sources ; jamais d'URL dans ses erreurs.
pub fn build_client(settings: &HttpSettings) -> Result<reqwest::Client, FetchError> {
    reqwest::Client::builder()
        .user_agent(settings.user_agent.clone())
        .connect_timeout(settings.connect_timeout)
        .timeout(settings.fetch_timeout)
        .build()
        .map_err(|e| FetchError::Transport(e.without_url().to_string()))
}

/// Source `{z}/{x}/{y}` en HTTP (Mapterhorn, polygons de couverture).
#[derive(Debug)]
pub struct XyzHttpSource {
    descriptor: SourceDescriptor,
    client: reqwest::Client,
    budget: Arc<HostBudget>,
}

impl XyzHttpSource {
    /// Source avec un budget propre (concurrence du descripteur, sans cadence).
    pub fn new(descriptor: SourceDescriptor, client: reqwest::Client) -> Self {
        let budget = Arc::new(HostBudget::new(None, descriptor.concurrency));
        Self { descriptor, client, budget }
    }

    /// Source qui partage le budget de son hôte (T02 §4.2).
    pub fn with_budget(
        descriptor: SourceDescriptor,
        client: reqwest::Client,
        budget: Arc<HostBudget>,
    ) -> Self {
        Self { descriptor, client, budget }
    }
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned)
}

/// `Retry-After` en secondes seulement (la forme date HTTP est ignorée).
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    header_str(headers, header::RETRY_AFTER)
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn max_age(headers: &HeaderMap) -> Option<Duration> {
    let cc = header_str(headers, header::CACHE_CONTROL)?;
    cc.split(',')
        .filter_map(|d| d.trim().strip_prefix("max-age="))
        .find_map(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn meta_of(headers: &HeaderMap) -> EntryMeta {
    EntryMeta {
        etag: header_str(headers, header::ETAG),
        last_modified: header_str(headers, header::LAST_MODIFIED),
        fetched_at: std::time::SystemTime::now(),
        server_max_age: max_age(headers),
    }
}

fn meaning(m: StatusMeaning, status: u16) -> Result<Fetched, FetchError> {
    match m {
        StatusMeaning::Empty => Ok(Fetched::Empty),
        StatusMeaning::Absent => Ok(Fetched::Absent),
        StatusMeaning::Failed => Err(FetchError::Http { status }),
    }
}

fn transport(e: reqwest::Error) -> FetchError {
    if e.is_timeout() {
        FetchError::Timeout
    } else {
        FetchError::Transport(e.without_url().to_string())
    }
}

/// Un essai : passage par le budget de l'hôte, puis table des statuts de la source. Jamais
/// d'URL dans les erreurs ni les journaux (une URL de tuile est une coordonnée).
async fn fetch_once(
    client: &reqwest::Client,
    budget: &HostBudget,
    url: String,
    descriptor: &SourceDescriptor,
    revalidate: Option<&EntryMeta>,
) -> Result<Fetched, FetchError> {
    let _permit = budget.acquire().await.ok_or_else(|| FetchError::Transport("source closed".into()))?;
    let mut req = client.get(url);
    if let Some(meta) = revalidate {
        if let Some(etag) = &meta.etag {
            req = req.header(header::IF_NONE_MATCH, etag);
        }
        if let Some(lm) = &meta.last_modified {
            req = req.header(header::IF_MODIFIED_SINCE, lm);
        }
    }
    let response = req.send().await.map_err(transport)?;
    let status = response.status();
    let headers = response.headers().clone();
    let policy = descriptor.status_policy;
    match status {
        StatusCode::OK => {
            let bytes: Bytes = response.bytes().await.map_err(transport)?;
            Ok(Fetched::Body { bytes, meta: meta_of(&headers) })
        }
        StatusCode::NO_CONTENT => meaning(policy.no_content, 204),
        StatusCode::NOT_MODIFIED => Ok(Fetched::NotModified(meta_of(&headers))),
        StatusCode::NOT_FOUND => meaning(policy.not_found, 404),
        StatusCode::TOO_MANY_REQUESTS => {
            tracing::warn!(source = ?descriptor.id, status = 429, "upstream rate limit");
            metrics::counter!("upstream_responses_total", "source" => format!("{:?}", descriptor.id), "status" => "429")
                .increment(1);
            let ra = retry_after(&headers);
            if let Some(ra) = ra {
                budget.push_back(ra);
            }
            Err(FetchError::RateLimited { retry_after: ra })
        }
        StatusCode::SERVICE_UNAVAILABLE if retry_after(&headers).is_some() => {
            Err(FetchError::RateLimited { retry_after: retry_after(&headers) })
        }
        other => Err(FetchError::Http { status: other.as_u16() }),
    }
}

#[async_trait::async_trait]
impl TileSource for XyzHttpSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    async fn fetch(&self, id: TileKey, revalidate: Option<&EntryMeta>) -> Result<Fetched, FetchError> {
        fetch_once(&self.client, &self.budget, self.descriptor.url(id), &self.descriptor, revalidate).await
    }
}

/// IGN LiDAR HD par le WMS-R de la Géoplateforme (T02 §3) : un `GetMap` BIL32 par tuile XYZ,
/// `BBOX` = emprise de la tuile en EPSG:4326, latitude d'abord (WMS 1.3.0). EPSG:4326 plutôt que
/// 3857 : le service rééchantillonne le 3857 une ligne sur deux (vcyclist, `IgnLidarHdLayer.kt`).
#[derive(Debug)]
pub struct WmsGetMapSource {
    descriptor: SourceDescriptor,
    client: reqwest::Client,
    budget: Arc<HostBudget>,
    /// Côté de l'image demandée, en pixels (256).
    size: u16,
}

impl WmsGetMapSource {
    /// `descriptor.template` est l'URL du service (sans requête) ; la couche y est ajoutée.
    pub fn new(
        descriptor: SourceDescriptor,
        client: reqwest::Client,
        budget: Arc<HostBudget>,
        size: u16,
    ) -> Self {
        Self { descriptor, client, budget, size }
    }

    /// Requête `GetMap` d'une tuile.
    pub fn url(&self, key: TileKey) -> String {
        wms_url(&self.descriptor.template, key, self.size)
    }
}

/// Gabarit WMS de la configuration : `<url du service>|<couche>`, séparés pour le hachage de
/// l'espace de noms ; voir `wms_template`.
pub fn wms_template(service_url: &str, layer: &str) -> String {
    format!("{service_url}|{layer}")
}

/// URL `GetMap` BIL32 d'une tuile, à partir d'un gabarit `wms_template`.
pub fn wms_url(template: &str, key: TileKey, size: u16) -> String {
    let (base, layer) = template.split_once('|').unwrap_or((template, ""));
    let (west, south, east, north) = key.bounds();
    let sep = if base.contains('?') { '&' } else { '?' };
    format!(
        "{base}{sep}SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS={layer}&STYLES=normal&CRS=EPSG:4326\
         &BBOX={south},{west},{north},{east}&WIDTH={size}&HEIGHT={size}&FORMAT=image%2Fx-bil%3Bbits%3D32"
    )
}

#[async_trait::async_trait]
impl TileSource for WmsGetMapSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    async fn fetch(&self, id: TileKey, revalidate: Option<&EntryMeta>) -> Result<Fetched, FetchError> {
        fetch_once(&self.client, &self.budget, self.url(id), &self.descriptor, revalidate).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_control_and_retry_after_parsing() {
        let mut h = HeaderMap::new();
        h.insert(header::CACHE_CONTROL, "public, max-age=604800".parse().unwrap());
        h.insert(header::RETRY_AFTER, "7".parse().unwrap());
        assert_eq!(max_age(&h), Some(Duration::from_secs(604_800)));
        assert_eq!(retry_after(&h), Some(Duration::from_secs(7)));
        h.insert(header::RETRY_AFTER, "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap());
        assert_eq!(retry_after(&h), None);
    }

    #[test]
    fn wms_bbox_is_latitude_first_in_epsg_4326() {
        let t = wms_template("https://data.geopf.fr/wms-r/wms", "IGNF_LIDAR-HD_MNT");
        let key = TileKey { z: 15, x: 16_384, y: 11_000 };
        let url = wms_url(&t, key, 256);
        let (w, s, e, n) = key.bounds();
        assert!(url.starts_with("https://data.geopf.fr/wms-r/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap"));
        assert!(url.contains("&LAYERS=IGNF_LIDAR-HD_MNT&"));
        assert!(url.contains(&format!("&BBOX={s},{w},{n},{e}&")), "{url}");
        assert!(url.ends_with("&WIDTH=256&HEIGHT=256&FORMAT=image%2Fx-bil%3Bbits%3D32"));
        assert_eq!(w, 0.0);
        assert!(n > s);
    }
}
