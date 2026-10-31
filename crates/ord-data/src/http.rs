//! Transport HTTP XYZ (T02 §3) : un client par processus, concurrence bornée par hôte, table des
//! statuts propre à la source. Les relances sont faites par `TileReader`, qui connaît l'échéance.

use std::time::Duration;

use bytes::Bytes;
use ord_domain::tiles::TileKey;
use reqwest::header::{self, HeaderMap};
use reqwest::StatusCode;
use tokio::sync::Semaphore;

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

/// Source `{z}/{x}/{y}` en HTTP (Mapterhorn public à l'incrément 1).
#[derive(Debug)]
pub struct XyzHttpSource {
    descriptor: SourceDescriptor,
    client: reqwest::Client,
    in_flight: Semaphore,
}

impl XyzHttpSource {
    pub fn new(descriptor: SourceDescriptor, client: reqwest::Client) -> Self {
        let permits = descriptor.concurrency.max(1);
        Self { descriptor, client, in_flight: Semaphore::new(permits) }
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

#[async_trait::async_trait]
impl TileSource for XyzHttpSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    async fn fetch(&self, id: TileKey, revalidate: Option<&EntryMeta>) -> Result<Fetched, FetchError> {
        let _permit =
            self.in_flight.acquire().await.map_err(|_| FetchError::Transport("source closed".into()))?;
        let mut req = self.client.get(self.descriptor.url(id));
        if let Some(meta) = revalidate {
            if let Some(etag) = &meta.etag {
                req = req.header(header::IF_NONE_MATCH, etag);
            }
            if let Some(lm) = &meta.last_modified {
                req = req.header(header::IF_MODIFIED_SINCE, lm);
            }
        }
        let response = req.send().await.map_err(|e| {
            if e.is_timeout() {
                FetchError::Timeout
            } else {
                FetchError::Transport(e.without_url().to_string())
            }
        })?;
        let status = response.status();
        let headers = response.headers().clone();
        let policy = self.descriptor.status_policy;
        match status {
            StatusCode::OK => {
                let bytes: Bytes = response.bytes().await.map_err(|e| {
                    if e.is_timeout() {
                        FetchError::Timeout
                    } else {
                        FetchError::Transport(e.without_url().to_string())
                    }
                })?;
                Ok(Fetched::Body { bytes, meta: meta_of(&headers) })
            }
            StatusCode::NO_CONTENT => meaning(policy.no_content, 204),
            StatusCode::NOT_MODIFIED => Ok(Fetched::NotModified(meta_of(&headers))),
            StatusCode::NOT_FOUND => meaning(policy.not_found, 404),
            StatusCode::TOO_MANY_REQUESTS => {
                tracing::warn!(source = ?self.descriptor.id, status = 429, "upstream rate limit");
                Err(FetchError::RateLimited { retry_after: retry_after(&headers) })
            }
            StatusCode::SERVICE_UNAVAILABLE if retry_after(&headers).is_some() => {
                Err(FetchError::RateLimited { retry_after: retry_after(&headers) })
            }
            other => Err(FetchError::Http { status: other.as_u16() }),
        }
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
}
