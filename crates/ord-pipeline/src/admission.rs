//! Admission (T01 §3.2) : `max_in_flight` requêtes dans le pipeline, `max_queued` en attente. Une
//! requête qui trouve la file pleine est refusée **aussitôt** (503 `overloaded`) ; l'attente d'un
//! jeton est bornée par le budget (504 `timeout`). Garde d'annulation (T01 §3.3) : un client qui
//! part fait lâcher le futur de la requête, et le garde annule son `Deadline`, que le calcul lit au
//! prochain contrôle coopératif.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ord_domain::budget::Deadline;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::PipelineError;

/// Taille de la file : jetons du pipeline et places d'attente.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmissionLimits {
    pub max_in_flight: usize,
    pub max_queued: usize,
}

impl AdmissionLimits {
    /// Défauts de T07 : un jeton par CPU, file de deux fois plus.
    pub fn from_cpus(cpus: usize) -> Self {
        let n = cpus.max(1);
        Self { max_in_flight: n, max_queued: 2 * n }
    }
}

impl Default for AdmissionLimits {
    fn default() -> Self {
        Self::from_cpus(std::thread::available_parallelism().map_or(1, usize::from))
    }
}

#[derive(Debug)]
struct Shared {
    permits: Arc<Semaphore>,
    limits: AdmissionLimits,
    queued: AtomicUsize,
}

/// File bornée devant le pipeline ; partagée par toutes les requêtes du processus.
#[derive(Clone, Debug)]
pub struct Admission {
    shared: Arc<Shared>,
}

/// Jeton du pipeline, rendu quand il est lâché (fin de requête, client parti, échéance).
#[derive(Debug)]
pub struct AdmissionPermit {
    _permit: OwnedSemaphorePermit,
    shared: Arc<Shared>,
}

impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        // Le jeton est rendu juste après ce `drop` : un de moins en vol.
        let in_flight = self.shared.limits.max_in_flight - self.shared.permits.available_permits() - 1;
        metrics::gauge!("ord_inflight").set(in_flight as f64);
    }
}

/// Place dans la file, libérée en sortie d'attente, y compris quand le futur est lâché.
struct QueueSlot<'a>(&'a Shared);

impl Drop for QueueSlot<'_> {
    fn drop(&mut self) {
        let left = self.0.queued.fetch_sub(1, Ordering::SeqCst) - 1;
        metrics::gauge!("ord_queued").set(left as f64);
    }
}

fn rejected(reason: &'static str) {
    metrics::counter!("ord_rejected_total", "reason" => reason).increment(1);
}

impl Admission {
    pub fn new(limits: AdmissionLimits) -> Self {
        let limits = AdmissionLimits { max_in_flight: limits.max_in_flight.max(1), ..limits };
        Self {
            shared: Arc::new(Shared {
                permits: Arc::new(Semaphore::new(limits.max_in_flight)),
                limits,
                queued: AtomicUsize::new(0),
            }),
        }
    }

    pub fn limits(&self) -> AdmissionLimits {
        self.shared.limits
    }

    /// Requêtes dans le pipeline.
    pub fn in_flight(&self) -> usize {
        self.shared.limits.max_in_flight - self.shared.permits.available_permits()
    }

    /// Requêtes en attente d'un jeton.
    pub fn queued(&self) -> usize {
        self.shared.queued.load(Ordering::SeqCst)
    }

    /// Prend un jeton : tout de suite s'il en reste, sinon en file si elle n'est pas pleine
    /// (`Overloaded` sinon), en attendant au plus l'échéance de la requête (`Timeout`).
    pub async fn admit(&self, deadline: &Deadline) -> Result<AdmissionPermit, PipelineError> {
        let shared = &self.shared;
        let permit = match Arc::clone(&shared.permits).try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                if shared.queued.fetch_add(1, Ordering::SeqCst) >= shared.limits.max_queued {
                    shared.queued.fetch_sub(1, Ordering::SeqCst);
                    rejected("overloaded");
                    return Err(PipelineError::Overloaded);
                }
                let slot = QueueSlot(shared);
                metrics::gauge!("ord_queued").set(self.queued() as f64);
                let at = tokio::time::Instant::from_std(deadline.at);
                let acquired = tokio::time::timeout_at(at, Arc::clone(&shared.permits).acquire_owned()).await;
                drop(slot);
                match acquired {
                    Ok(Ok(p)) => p,
                    Ok(Err(_)) => return Err(PipelineError::Internal("admission closed".into())),
                    Err(_) => {
                        rejected("timeout");
                        return Err(PipelineError::Timeout);
                    }
                }
            }
        };
        metrics::gauge!("ord_inflight").set(self.in_flight() as f64);
        Ok(AdmissionPermit { _permit: permit, shared: Arc::clone(shared) })
    }
}

/// Annule l'échéance d'une requête quand il est lâché sans avoir été désarmé : le futur de la
/// requête a été abandonné (client parti) ou interrompu (échéance), et les tâches bloquantes qui
/// lisent ce `Deadline` s'arrêtent au prochain contrôle.
#[derive(Debug)]
pub struct CancelOnDrop {
    deadline: Option<Deadline>,
}

impl CancelOnDrop {
    pub fn new(deadline: &Deadline) -> Self {
        Self { deadline: Some(deadline.clone()) }
    }

    /// La requête est allée au bout : rien à annuler.
    pub fn disarm(mut self) {
        self.deadline = None;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(d) = self.deadline.take() {
            d.cancel();
            tracing::info!("request abandoned before completion: computation cancelled");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn admission(max_in_flight: usize, max_queued: usize) -> Admission {
        Admission::new(AdmissionLimits { max_in_flight, max_queued })
    }

    #[tokio::test]
    async fn full_queue_is_refused_at_once() {
        let a = admission(1, 0);
        let d = Deadline::after(Duration::from_secs(60));
        let held = a.admit(&d).await.unwrap();
        assert_eq!(a.in_flight(), 1);
        let started = std::time::Instant::now();
        assert_eq!(a.admit(&d).await.unwrap_err(), PipelineError::Overloaded);
        assert!(started.elapsed() < Duration::from_millis(100), "refus immédiat");
        drop(held);
        assert_eq!(a.in_flight(), 0);
        assert!(a.admit(&d).await.is_ok(), "jeton rendu");
    }

    /// Temps virtuel : la file attend le jeton ; au-delà du budget, 504 (pas 503).
    #[tokio::test(start_paused = true)]
    async fn queued_request_waits_then_times_out_at_its_deadline() {
        let a = admission(1, 1);
        let held = a.admit(&Deadline::after(Duration::from_secs(90))).await.unwrap();
        // Une requête en file obtient le jeton dès qu'il est rendu.
        let waiting = {
            let a = a.clone();
            tokio::spawn(async move { a.admit(&Deadline::after(Duration::from_secs(90))).await.map(drop) })
        };
        tokio::task::yield_now().await;
        assert_eq!(a.queued(), 1);
        assert_eq!(
            a.admit(&Deadline::after(Duration::from_secs(90))).await.unwrap_err(),
            PipelineError::Overloaded,
            "file pleine"
        );
        drop(held);
        waiting.await.unwrap().unwrap();
        assert_eq!((a.in_flight(), a.queued()), (0, 0));

        // Jeton jamais rendu : l'attente s'arrête à l'échéance.
        let _held = a.admit(&Deadline::after(Duration::from_secs(900))).await.unwrap();
        let err = a.admit(&Deadline::after(Duration::from_secs(90))).await.unwrap_err();
        assert_eq!(err, PipelineError::Timeout);
        assert_eq!(a.queued(), 0, "place libérée");
    }

    #[tokio::test]
    async fn abandoned_wait_frees_its_queue_slot() {
        let a = admission(1, 1);
        let _held = a.admit(&Deadline::after(Duration::from_secs(60))).await.unwrap();
        let d = Deadline::after(Duration::from_secs(60));
        let r = tokio::time::timeout(Duration::from_millis(20), a.admit(&d)).await;
        assert!(r.is_err(), "toujours en file");
        assert_eq!(a.queued(), 0, "le futur lâché rend sa place");
    }

    #[test]
    fn cancel_on_drop_cancels_unless_disarmed() {
        let d = Deadline::after(Duration::from_secs(60));
        CancelOnDrop::new(&d).disarm();
        assert!(!d.is_cancelled());
        drop(CancelOnDrop::new(&d));
        assert!(d.is_cancelled());
    }
}
