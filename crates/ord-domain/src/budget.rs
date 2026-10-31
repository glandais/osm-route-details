//! Budget de temps et annulation coopérative (T01 §3.3).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Lu par le cœur aux frontières d'étape et dans les boucles longues.
pub trait Budget: Send + Sync {
    fn expired(&self) -> bool;
    fn remaining(&self) -> Duration;
}

/// Échéance d'une requête (horloge monotone), annulable quand le client part.
#[derive(Clone, Debug)]
pub struct Deadline {
    pub at: Instant,
    cancelled: Arc<AtomicBool>,
}

impl Deadline {
    pub fn after(budget: Duration) -> Self {
        Self::at(Instant::now() + budget)
    }

    pub fn at(at: Instant) -> Self {
        Self { at, cancelled: Arc::new(AtomicBool::new(false)) }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Budget for Deadline {
    fn expired(&self) -> bool {
        self.is_cancelled() || Instant::now() >= self.at
    }

    fn remaining(&self) -> Duration {
        if self.is_cancelled() {
            Duration::ZERO
        } else {
            self.at.saturating_duration_since(Instant::now())
        }
    }
}

/// Budget sans limite, pour les tests du cœur.
#[derive(Clone, Copy, Debug, Default)]
pub struct Unbounded;

impl Budget for Unbounded {
    fn expired(&self) -> bool {
        false
    }

    fn remaining(&self) -> Duration {
        Duration::MAX
    }
}

/// Identifiant de requête (`X-Request-Id` repris ou UUID v7), aussi `instance` des problèmes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(pub String);

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_deadline_is_expired() {
        let d = Deadline::after(Duration::from_secs(60));
        assert!(!d.expired());
        d.clone().cancel();
        assert!(d.expired());
        assert_eq!(d.remaining(), Duration::ZERO);
        assert!(!Unbounded.expired());
    }
}
