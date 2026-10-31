//! Chaîne typologie, façade asynchrone (T03 §10) : tuiles du couloir lues par le port partagé,
//! puis calcul pur (graphe, HMM, tronçons) dans une tâche bloquante. Une tuile en échec n'est
//! jamais une erreur de la chaîne : elle devient une plage de `TypologyCoverage`.

use std::sync::Arc;

use ord_core::rules::TypologyRules;
use ord_core::typology::{compute, corridor_tiles, TileState, TypologyError, TypologyOutput};
use ord_domain::budget::Budget;
use ord_domain::ctx::RequestCtx;
use ord_domain::geo::Grid;
use ord_domain::route::PreparedRoute;
use ord_domain::street::{SchemaCapabilities, StreetTile};
use ord_domain::tiles::{TileOutcome, TileStore};
use ord_domain::LatLon;

#[async_trait::async_trait]
pub trait TypologyChain: Send + Sync {
    async fn run(&self, route: Arc<PreparedRoute>, ctx: &RequestCtx)
        -> Result<TypologyOutput, TypologyError>;
}

/// Typologie sur des tuiles de rues (VersaTiles Shortbread au MVP).
pub struct TileTypology {
    store: Arc<dyn TileStore<StreetTile>>,
    grid: Grid,
    caps: SchemaCapabilities,
    rules: TypologyRules,
}

impl std::fmt::Debug for TileTypology {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TileTypology").field("grid", &self.grid).finish_non_exhaustive()
    }
}

impl TileTypology {
    pub fn new(
        store: Arc<dyn TileStore<StreetTile>>,
        grid: Grid,
        caps: SchemaCapabilities,
        rules: TypologyRules,
    ) -> Self {
        Self { store, grid, caps, rules }
    }

    pub fn store(&self) -> &Arc<dyn TileStore<StreetTile>> {
        &self.store
    }
}

#[async_trait::async_trait]
impl TypologyChain for TileTypology {
    async fn run(
        &self,
        route: Arc<PreparedRoute>,
        ctx: &RequestCtx,
    ) -> Result<TypologyOutput, TypologyError> {
        let inputs: Vec<LatLon> =
            route.input_pos.iter().filter_map(|&i| route.samples.get(i as usize)).map(|s| s.at).collect();
        let keys = corridor_tiles(self.grid, &inputs, self.rules.corridor_radius_m());
        let outcomes = self.store.get_many(&keys, ctx).await;
        let tiles: Vec<_> = keys
            .into_iter()
            .zip(outcomes)
            .map(|(k, o)| {
                let state = match o {
                    TileOutcome::Data { tile, .. } => TileState::Data(tile),
                    TileOutcome::Empty(_) => TileState::Empty,
                    TileOutcome::Absent => TileState::Absent,
                    TileOutcome::Failed(_) => TileState::Failed,
                };
                (k, state)
            })
            .collect();
        if ctx.deadline.expired() {
            return Err(if ctx.deadline.is_cancelled() {
                TypologyError::Cancelled
            } else {
                TypologyError::Timeout
            });
        }
        let (grid, caps, rules, budget) = (self.grid, self.caps, self.rules.clone(), ctx.deadline.clone());
        let started = std::time::Instant::now();
        let out = tokio::task::spawn_blocking(move || compute(&route, grid, &tiles, &caps, &rules, &budget))
            .await
            .map_err(|e| TypologyError::Internal(format!("typology task failed: {e}")))?;
        tracing::debug!(
            compute_ms = started.elapsed().as_millis() as u64,
            stats = ?out.as_ref().map(|o| o.stats).ok(),
            "typology computed"
        );
        out
    }
}
