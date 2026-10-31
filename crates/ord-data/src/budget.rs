//! Budget par hôte, pour tout le processus (T02 §4.2) : cadence, requêtes en vol, report de tout
//! l'hôte après un `Retry-After`. Modèle : `HostPacers` et `RequestPacer` de vcyclist
//! (`TileRequestPacing.kt`), créneaux de départ espacés de `1/débit` dans l'ordre d'appel.
//!
//! Les relances elles-mêmes sont faites par `TileReader`, qui connaît l'échéance de la requête :
//! chaque essai repasse par le budget. Écrit avec `tokio::time` (testable en temps virtuel).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::{Semaphore, SemaphorePermit};
use tokio::time::Instant;

/// Budget d'un hôte : la limite IGN (40 req/s) est par IP, donc partagée par le MNT et le MNS.
#[derive(Debug)]
pub struct HostBudget {
    /// `None` : pas de cadence (Mapterhorn, VersaTiles), seulement la concurrence.
    spacing: Option<Duration>,
    in_flight: Semaphore,
    /// Prochain créneau de départ libre (cadence et report après un 429).
    next_slot: Mutex<Option<Instant>>,
}

impl HostBudget {
    /// `rate_per_s` : départs par seconde (`None` ou ≤ 0 : sans cadence) ; `in_flight` ≥ 1.
    pub fn new(rate_per_s: Option<f64>, in_flight: usize) -> Self {
        let spacing =
            rate_per_s.filter(|r| r.is_finite() && *r > 0.0).map(|r| Duration::from_secs_f64(1.0 / r));
        Self { spacing, in_flight: Semaphore::new(in_flight.max(1)), next_slot: Mutex::new(None) }
    }

    /// Attend une place en vol puis un créneau de départ ; le jeton libère la place.
    pub async fn acquire(&self) -> Option<SemaphorePermit<'_>> {
        let permit = self.in_flight.acquire().await.ok()?;
        let wait = {
            let mut next = self.next_slot.lock().unwrap_or_else(PoisonError::into_inner);
            let now = Instant::now();
            let slot = next.map_or(now, |n| n.max(now));
            *next = Some(match self.spacing {
                Some(s) => slot + s,
                None => slot,
            });
            slot.saturating_duration_since(now)
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        Some(permit)
    }

    /// Un 429 avec `Retry-After` repousse le prochain départ de **tout** l'hôte (limite par IP).
    pub fn push_back(&self, retry_after: Duration) {
        let mut next = self.next_slot.lock().unwrap_or_else(PoisonError::into_inner);
        let until = Instant::now() + retry_after;
        *next = Some(next.map_or(until, |n| n.max(until)));
    }
}

/// Registre des budgets, un par hôte, partagé par toutes les sources du processus.
#[derive(Debug, Default)]
pub struct HostBudgets {
    by_host: Mutex<HashMap<String, Arc<HostBudget>>>,
}

impl HostBudgets {
    /// Budget de l'hôte de `url` ; le premier appel pour un hôte fixe ses réglages.
    pub fn for_url(&self, url: &str, rate_per_s: Option<f64>, in_flight: usize) -> Arc<HostBudget> {
        let host = host_of(url);
        let mut map = self.by_host.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(map.entry(host).or_insert_with(|| Arc::new(HostBudget::new(rate_per_s, in_flight))))
    }
}

/// `https://data.geopf.fr/wms-r/wms?…` → `data.geopf.fr`.
pub fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?']).next().unwrap_or(rest).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_parsing_and_sharing() {
        assert_eq!(host_of("https://data.geopf.fr/wms-r/wms?SERVICE=WMS"), "data.geopf.fr");
        assert_eq!(host_of("http://127.0.0.1:8080/{z}/{x}/{y}.webp"), "127.0.0.1:8080");
        let budgets = HostBudgets::default();
        let a = budgets.for_url("https://data.geopf.fr/wms-r/wms?LAYERS=MNT", Some(20.0), 12);
        let b = budgets.for_url("https://data.geopf.fr/wms-r/wms?LAYERS=MNS", Some(5.0), 1);
        assert!(Arc::ptr_eq(&a, &b), "MNT et MNS partagent l'hôte");
    }

    #[tokio::test(start_paused = true)]
    async fn departures_are_spaced_by_the_rate() {
        let budget = HostBudget::new(Some(20.0), 100);
        let t0 = Instant::now();
        let mut starts = Vec::new();
        for _ in 0..41 {
            let _p = budget.acquire().await;
            starts.push(Instant::now() - t0);
        }
        // 41 départs à 20/s : le dernier part après 2 s, jamais avant.
        assert_eq!(starts[0], Duration::ZERO);
        assert_eq!(starts[40], Duration::from_secs(2));
        assert!(starts.windows(2).all(|w| w[1] - w[0] >= Duration::from_millis(50)));
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_callers_never_exceed_the_rate_and_push_back_delays_the_host() {
        let budget = Arc::new(HostBudget::new(Some(20.0), 4));
        let t0 = Instant::now();
        let tasks: Vec<_> = (0..20)
            .map(|_| {
                let b = Arc::clone(&budget);
                tokio::spawn(async move {
                    let _p = b.acquire().await;
                    let start = Instant::now() - t0;
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    start
                })
            })
            .collect();
        let mut starts = Vec::new();
        for t in tasks {
            starts.push(t.await.unwrap());
        }
        starts.sort();
        // Sur toute fenêtre d'une seconde : au plus 20 départs (cadence), en vol ≤ 4.
        for (i, s) in starts.iter().enumerate() {
            let in_window = starts[i..].iter().take_while(|t| **t < *s + Duration::from_secs(1)).count();
            assert!(in_window <= 20);
        }
        budget.push_back(Duration::from_secs(7));
        let before = Instant::now();
        let _p = budget.acquire().await;
        assert!(Instant::now() - before >= Duration::from_secs(7), "Retry-After appliqué à tout l'hôte");
    }

    #[tokio::test(start_paused = true)]
    async fn no_rate_means_no_wait() {
        let budget = HostBudget::new(None, 2);
        let t0 = Instant::now();
        for _ in 0..10 {
            let _p = budget.acquire().await;
        }
        assert_eq!(Instant::now(), t0);
    }
}
