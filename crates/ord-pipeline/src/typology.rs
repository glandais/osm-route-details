//! Chaîne typologie, façade asynchrone (T03 §10) : tuiles du couloir lues par le port partagé,
//! puis calcul pur (graphe, HMM, tronçons) dans une tâche bloquante, puis les ouvrages : tant
//! qu'une extrémité d'ouvrage sort des tuiles lues, les tuiles voisines sont lues (au plus
//! `max_extension_rounds` tours et `max_extension_tiles` tuiles). Une tuile en échec n'est jamais
//! une erreur de la chaîne : elle devient une plage de `TypologyCoverage`.

use std::sync::Arc;

use ord_core::rules::TypologyRules;
use ord_core::typology::{corridor_tiles, finish, match_route, TileState, TypologyError, TypologyOutput};
use ord_domain::budget::Budget;
use ord_domain::ctx::RequestCtx;
use ord_domain::geo::Grid;
use ord_domain::route::PreparedRoute;
use ord_domain::street::{SchemaCapabilities, StreetTile};
use ord_domain::tiles::{TileKey, TileOutcome, TileStore};
use ord_domain::LatLon;
use tokio::sync::Semaphore;

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
    /// Jetons de calcul partagés avec l'orchestrateur (T01 §3.1) ; sans eux, pas de borne.
    compute: Option<Arc<Semaphore>>,
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
        Self { store, grid, caps, rules, compute: None }
    }

    /// Calculs bloquants sous les jetons de calcul du processus.
    pub fn with_compute_permits(mut self, compute: Arc<Semaphore>) -> Self {
        self.compute = Some(compute);
        self
    }

    /// Tâche bloquante, sous un jeton de calcul déplacé dans la tâche.
    async fn blocking<R: Send + 'static>(
        &self,
        f: impl FnOnce() -> R + Send + 'static,
    ) -> Result<R, TypologyError> {
        let permit = match &self.compute {
            Some(c) => Some(
                Arc::clone(c)
                    .acquire_owned()
                    .await
                    .map_err(|_| TypologyError::Internal("compute permits closed".into()))?,
            ),
            None => None,
        };
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            f()
        })
        .await
        .map_err(|e| TypologyError::Internal(format!("typology task failed: {e}")))
    }

    pub fn store(&self) -> &Arc<dyn TileStore<StreetTile>> {
        &self.store
    }
}

fn state_of(o: TileOutcome<StreetTile>) -> TileState {
    match o {
        TileOutcome::Data { tile, .. } => TileState::Data(tile),
        TileOutcome::Empty(_) => TileState::Empty,
        TileOutcome::Absent => TileState::Absent,
        TileOutcome::Failed(_) => TileState::Failed,
    }
}

fn deadline_error(ctx: &RequestCtx) -> Option<TypologyError> {
    ctx.deadline.expired().then(|| {
        if ctx.deadline.is_cancelled() {
            TypologyError::Cancelled
        } else {
            TypologyError::Timeout
        }
    })
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
        let mut tiles: Vec<(TileKey, TileState)> =
            keys.into_iter().zip(outcomes).map(|(k, o)| (k, state_of(o))).collect();
        if let Some(e) = deadline_error(ctx) {
            return Err(e);
        }
        let (grid, caps, rules, budget) = (self.grid, self.caps, self.rules.clone(), ctx.deadline.clone());
        let started = std::time::Instant::now();
        let shared = Arc::new(tiles.clone());
        let matched = {
            let (route, rules, shared) = (Arc::clone(&route), rules.clone(), Arc::clone(&shared));
            self.blocking(move || match_route(&route, grid, &shared, &caps, &rules, &budget)).await??
        };
        let matched = Arc::new(matched);
        let limits = &self.rules.structures;
        let (mut rounds, mut extra) = (0u32, 0usize);
        let out = loop {
            let allow = rounds < limits.max_extension_rounds && extra < limits.max_extension_tiles;
            let (m, r) = (Arc::clone(&matched), rules.clone());
            let current = Arc::new(std::mem::take(&mut tiles));
            let step = {
                let current = Arc::clone(&current);
                self.blocking(move || finish(&m, &current, &r, allow)).await?
            };
            tiles = Arc::try_unwrap(current).unwrap_or_else(|a| (*a).clone());
            match step {
                Ok(out) => break out,
                Err(mut need) => {
                    need.retain(|k| tiles.binary_search_by(|(t, _)| t.cmp(k)).is_err());
                    need.truncate(limits.max_extension_tiles - extra);
                    rounds += 1;
                    if need.is_empty() {
                        // Rien de lisible de plus : dernier tour sans extension.
                        rounds = limits.max_extension_rounds;
                        continue;
                    }
                    extra += need.len();
                    let outcomes = self.store.get_many(&need, ctx).await;
                    tiles.extend(need.into_iter().zip(outcomes).map(|(k, o)| (k, state_of(o))));
                    tiles.sort_by_key(|a| a.0);
                    if let Some(e) = deadline_error(ctx) {
                        return Err(e);
                    }
                }
            }
        };
        tracing::debug!(
            compute_ms = started.elapsed().as_millis() as u64,
            stats = ?out.stats,
            structures = out.structures.len(),
            passages = out.passages.len(),
            extension_rounds = rounds,
            extension_tiles = extra,
            "typology computed"
        );
        Ok(out)
    }
}
