//! Chaîne typologie, partie pure (T03) : couloir, graphe par niveaux, HMM, tronçons typés,
//! ouvrages et passages (graphe des ouvrages, extension aux tuiles voisines), ponts au-dessus du
//! parcours, segments homogènes.

pub mod corridor;
pub mod edges;
pub mod graph;
pub mod matcher;
pub mod overpasses;
pub mod runs;
pub mod segments;
pub mod structures;

use std::ops::Range;
use std::sync::Arc;

use ord_domain::budget::Budget;
use ord_domain::geo::Grid;
use ord_domain::route::PreparedRoute;
use ord_domain::street::{SchemaCapabilities, StreetTile};
use ord_domain::tiles::TileKey;
use rustc_hash::FxHashMap;

use crate::rules::TypologyRules;
pub use corridor::{corridor_tiles, Corridor};
pub use edges::{EdgeNode, EdgeOneway, MatchedEdge};
pub use graph::{ClassId, EdgeClass, LineId, StreetGraph};
pub use overpasses::Overpass;
pub use segments::{build_segments, InputIndex, TypedSegment, UnmatchedReason};
pub use structures::{
    EndKind, PassageDoubt, PassageId, Structure, StructureElement, StructureEnd, StructureId, StructureKind,
    StructurePassage, StructuresOut, UnresolvedEnd,
};

/// Élément d'ouvrage : une ligne de tuile recollée aux bords.
pub type ElementId = LineId;

/// Ce que l'appariement dit d'un tronçon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMatch {
    Matched {
        class: ClassId,
        element: Option<ElementId>,
    },
    Unmatched,
    /// Route fermée au vélo suivie (état « hors graphe » du HMM, T03 §5.3) : non appariée au
    /// graphe vélo, typologie de cette route rendue avec l'avertissement `bicycle_forbidden`.
    OffGraph {
        class: ClassId,
    },
    /// Pas de donnée : tuile absente de l'emprise, ou en panne (`failed`).
    NoData {
        failed: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypedRun {
    pub from_m: f64,
    pub to_m: f64,
    pub matched: RunMatch,
}

/// Plages sans tuile : hors emprise (`missing`) ou en panne (`unavailable`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TypologyCoverage {
    pub missing: Vec<Range<f64>>,
    pub unavailable: Vec<Range<f64>>,
}

/// Compteurs (journaux, banc) ; jamais de coordonnée.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypologyStats {
    pub tiles: usize,
    pub segments: usize,
    pub classes: usize,
    pub observations: usize,
    pub matched_observations: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypologyOutput {
    pub runs: Vec<TypedRun>,
    pub coverage: TypologyCoverage,
    pub classes: Arc<[EdgeClass]>,
    pub stats: TypologyStats,
    /// Ouvrages appariés, avec leurs extrémités sur l'ouvrage.
    pub structures: Vec<Structure>,
    /// Passages du parcours sur ces ouvrages, triés par distance.
    pub passages: Vec<StructurePassage>,
    /// Ponts des tuiles qui croisent la trace (§6.4), triés par distance ; la fusion décide si la
    /// trace passe dessous (T04 §3.4).
    pub overpasses: Vec<Overpass>,
    /// Arêtes du graphe des rues parcourues, dans l'ordre de la trace (api 1.7, `edges[]`).
    pub edges: Vec<MatchedEdge>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TypologyError {
    #[error("typology budget exceeded")]
    Timeout,
    #[error("typology cancelled")]
    Cancelled,
    #[error("typology internal error: {0}")]
    Internal(String),
}

/// Issue de lecture d'une tuile du couloir, vue par le calcul.
#[derive(Clone, Debug)]
pub enum TileState {
    Data(Arc<StreetTile>),
    Empty,
    Absent,
    Failed,
}

/// Observations du HMM : chaque point d'entrée, plus une station tous les `step_m` au moins.
fn observations(
    route: &PreparedRoute,
    grid: Grid,
    states: &FxHashMap<TileKey, &TileState>,
    rules: &TypologyRules,
) -> Vec<matcher::Obs> {
    let step = rules.matcher.step_m;
    let mut out = Vec::with_capacity(route.samples.len() / 2 + 2);
    let mut last = f64::NEG_INFINITY;
    let n = route.samples.len();
    for (i, s) in route.samples.iter().enumerate() {
        if s.input_index.is_none() && s.d_m - last < step - 1e-6 && i + 1 != n {
            continue;
        }
        last = s.d_m;
        let p = grid.to_units(s.at);
        let r = rules.matcher.search_radius_m / grid.metres_per_unit(s.at.lat);
        let mut no_data = None;
        for dx in [-r, r] {
            for dy in [-r, r] {
                match states.get(&grid.tile_of([p[0] + dx, p[1] + dy])) {
                    Some(TileState::Failed) => no_data = Some(true),
                    Some(TileState::Absent) | None => {
                        no_data.get_or_insert(false);
                    }
                    Some(TileState::Data(_) | TileState::Empty) => {}
                }
            }
        }
        out.push(matcher::Obs { p, at: s.at, d_m: s.d_m, no_data });
    }
    out
}

fn ranges(runs: &[TypedRun], want: impl Fn(&RunMatch) -> bool) -> Vec<Range<f64>> {
    let mut out: Vec<Range<f64>> = Vec::new();
    for r in runs.iter().filter(|r| want(&r.matched)) {
        match out.last_mut() {
            Some(last) if (last.end - r.from_m).abs() < 1e-9 => last.end = r.to_m,
            _ => out.push(r.from_m..r.to_m),
        }
    }
    out
}

/// Appariement fait, avant les ouvrages : graphe, chemin, tronçons et couverture.
pub struct Matched {
    graph: StreetGraph,
    path: matcher::MatchedPath,
    runs: Vec<TypedRun>,
    coverage: TypologyCoverage,
    stats: TypologyStats,
}

impl std::fmt::Debug for Matched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Matched").field("stats", &self.stats).finish_non_exhaustive()
    }
}

/// Appariement sur les tuiles lues du couloir (`tiles` triées par clé).
pub fn match_route(
    route: &PreparedRoute,
    grid: Grid,
    tiles: &[(TileKey, TileState)],
    caps: &SchemaCapabilities,
    rules: &TypologyRules,
    budget: &dyn Budget,
) -> Result<Matched, TypologyError> {
    let states: FxHashMap<TileKey, &TileState> = tiles.iter().map(|(k, s)| (*k, s)).collect();
    let data: Vec<Arc<StreetTile>> = tiles
        .iter()
        .filter_map(|(_, s)| match s {
            TileState::Data(t) => Some(Arc::clone(t)),
            _ => None,
        })
        .collect();
    let inputs: Vec<([f64; 2], f64)> = route
        .input_pos
        .iter()
        .filter_map(|&i| route.samples.get(i as usize))
        .map(|s| (grid.to_units(s.at), s.at.lat))
        .collect();
    let corridor = Corridor::new(grid, &inputs, rules.corridor_radius_m());
    let graph = StreetGraph::build(grid, &data, &corridor, caps);
    if budget.expired() {
        return Err(TypologyError::Timeout);
    }
    let obs = observations(route, grid, &states, rules);
    let path = matcher::match_obs(&graph, obs, &rules.matcher, budget)?;
    let runs = runs::project_runs(&graph, &path, route.length_m);
    let coverage = TypologyCoverage {
        missing: ranges(&runs, |m| *m == RunMatch::NoData { failed: false }),
        unavailable: ranges(&runs, |m| *m == RunMatch::NoData { failed: true }),
    };
    let stats = TypologyStats {
        tiles: tiles.len(),
        segments: graph.segs.len(),
        classes: graph.classes.len(),
        observations: path.obs.len(),
        matched_observations: path
            .state
            .iter()
            .filter(|s| matches!(s, matcher::ObsState::Matched { .. }))
            .count(),
    };
    Ok(Matched { graph, path, runs, coverage, stats })
}

/// Ouvrages et passages sur toutes les tuiles lues (couloir et extensions). `Err(tuiles)` : il
/// faut lire ces tuiles voisines et recommencer (seulement si `allow_need`).
pub fn finish(
    m: &Matched,
    tiles: &[(TileKey, TileState)],
    rules: &TypologyRules,
    allow_need: bool,
) -> Result<TypologyOutput, Vec<TileKey>> {
    let s = structures::resolve(&m.graph, &m.path, &m.runs, tiles, &rules.structures, allow_need)?;
    Ok(TypologyOutput {
        runs: m.runs.clone(),
        coverage: m.coverage.clone(),
        classes: m.graph.classes.clone().into(),
        stats: TypologyStats { tiles: tiles.len(), ..m.stats },
        structures: s.structures,
        passages: s.passages,
        overpasses: overpasses::find(m.graph.grid, &m.path.obs, tiles),
        edges: edges::extract(&m.graph, &m.path),
    })
}

/// Calcul complet sur les tuiles lues, sans lecture de tuile voisine : une extrémité d'ouvrage
/// hors des tuiles reste non résolue.
pub fn compute(
    route: &PreparedRoute,
    grid: Grid,
    tiles: &[(TileKey, TileState)],
    caps: &SchemaCapabilities,
    rules: &TypologyRules,
    budget: &dyn Budget,
) -> Result<TypologyOutput, TypologyError> {
    let m = match_route(route, grid, tiles, caps, rules, budget)?;
    finish(&m, tiles, rules, false).map_err(|_| TypologyError::Internal("extension without need".into()))
}

#[cfg(test)]
pub(crate) mod tests;
