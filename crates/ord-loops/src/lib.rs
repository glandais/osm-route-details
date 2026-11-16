//! Moteur de génération de boucles sur graphe OSM, mode « raccords » (lot 2 du générateur de parcours).
//!
//! Note de conception : `docs/design/technique/ord-loops-lot2.md`. Un départ, une arrivée (le départ par défaut), un
//! cap, une distance D, un type de parcours, un mode — **strict** (arêtes roulées par l'équipe, trous courts
//! comblés), **équipe d'abord** (la route de l'équipe, des arêtes OSM pour raccorder) ou **découverte** (OSM sans
//! préférence) — et une surcouche d'équipe : au plus trois parcours, suites d'arêtes **de base** orientées.
//!
//! Aucune E/S (le graphe est lu par les `GraphFile` fournis), aucun `unsafe` ; l'ouverture de l'[`Engine`] lance
//! des fils pour ses index, la génération n'en lance pas. `Engine`, `TeamData` et [`Window`] sont immuables et
//! partageables, un [`Workspace`] par fil, et l'appelant fournit un `stop` pour couper la recherche.
//!
//! Lot 6 : la recherche ne lit pas le graphe entier mais une [`Window`], les cellules d'une zone relues à plat
//! (`docs/design/technique/ord-loops-lot6.md`).

mod cache;
mod cost;
mod engine;
mod generate;
mod geo;
mod index;
mod reverse;
mod search;
mod selfcheck;
mod team;
#[cfg(any(test, feature = "reference"))]
mod team_reference;
mod verify;
mod window;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_index;
#[cfg(test)]
mod tests_reference;
#[cfg(test)]
mod tests_team_sparse;
#[cfg(test)]
mod tests_window;

pub use cache::{CacheStats, WindowCache};
pub use cost::{multiplier_table, RideType};
pub use engine::{Engine, EngineError, Params};
pub use generate::{
    generate, generate_in, generate_with_diagnostics, generate_with_diagnostics_in, route_geometry,
};
pub use geo::LatLon;
pub use search::Workspace;
pub use selfcheck::SelfCheck;
pub use team::{BuildPlan, TeamCtx, TeamData, TeamStats, WindowTeam};
pub use verify::{check_route, check_route_in, Check, Violation};
pub use window::{tiles_cover, tiles_union, Window, WindowPlan};

/// Les huit caps possibles d'une requête.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bearing8 {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl Bearing8 {
    pub fn degrees(self) -> f64 {
        match self {
            Self::N => 0.0,
            Self::NE => 45.0,
            Self::E => 90.0,
            Self::SE => 135.0,
            Self::S => 180.0,
            Self::SW => 225.0,
            Self::W => 270.0,
            Self::NW => 315.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Seulement les arêtes que l'équipe a roulées (et les trous comblés).
    Strict,
    /// La route de l'équipe fortement préférée, des arêtes OSM pour raccorder (défaut du produit).
    TeamFirst,
    /// OSM sans préférence pour l'équipe.
    Discovery,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub start: LatLon,
    /// Arrivée ; le départ si `None` (boucle).
    pub end: Option<LatLon>,
    pub bearing: Option<Bearing8>,
    pub distance_m: f64,
    pub ride_type: RideType,
    pub mode: Mode,
    /// Familiarité demandée, de 0 (sortir des routes de l'équipe) à 1 (s'y tenir).
    pub familiarity: f32,
    /// λ : coût d'une arête hors équipe en mode équipe d'abord, en 256es (entier : même poids en natif et en WASM).
    /// [`DEFAULT_OFF_TEAM_COST_Q8`] (768) vaut λ = 3, le λ du lot 2 ; 256 vaut λ = 1, le coût OSM seul.
    pub off_team_cost_q8: u32,
    pub seed: u64,
}

/// λ = 3 en 256es : la valeur du lot 2.
pub const DEFAULT_OFF_TEAM_COST_Q8: u32 = 768;

/// Un point du graphe : une arête de base et la distance depuis son extrémité `a`, le long de sa géométrie.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub edge: u32,
    pub offset_m: f64,
}

/// Une arête de base parcourue, de `a` vers `b` ou, si `reverse`, de `b` vers `a`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Step {
    pub base_edge: u32,
    pub reverse: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// Aller jusqu'à un point de demi-tour, retour par un autre chemin.
    Turnaround,
    /// Polygone dans le cap.
    Polygon,
}

#[derive(Clone, Debug)]
pub struct Route {
    pub start: Position,
    pub end: Position,
    pub steps: Vec<Step>,
    pub length_m: f64,
    pub team_m: f64,
    pub bridged_m: f64,
    /// Longueur recouverte (arêtes prises deux fois, une fois comptées).
    pub shared_m: f64,
    pub crossings: u32,
    pub unpaved_m: f64,
    pub presumed_surface_m: f64,
    pub bearing_deviation_deg: Option<f64>,
    pub familiarity: f32,
    pub family: Family,
}

#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    pub t1_settled: u64,
    pub t2_settled: u64,
    pub astar_settled: u64,
    pub astar_runs: u32,
    /// Candidats dont l'aller vient de l'ordre inverse (retour par l'arbre retour, aller cherché à rebours).
    pub reverse_found: u32,
    pub candidates: u32,
    pub within_tolerance: u32,
    /// L / D des candidats hors de la tolérance de distance, dans l'ordre des essais (classement des échecs).
    pub off_tolerance: Vec<f32>,
    /// Nœuds de base des virages refusés par le contrôle interne (`SHARP_TURN`), un par refus.
    pub sharp_nodes: Vec<u32>,
    /// Refus du contrôle interne, par code.
    pub rejected: Vec<(Violation, u32)>,
    pub start_snap_m: f64,
    pub end_snap_m: f64,
    /// Longueur (m) du réseau atteint à moins de D/2, et de celui qui mène aussi à l'arrivée.
    pub reached_m: f64,
    pub way_back_m: f64,
}

#[derive(Clone, Debug)]
pub struct Generation {
    pub routes: Vec<Route>,
    pub network_m: f64,
    pub familiarity_fallback: bool,
    pub diagnostics: Diagnostics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoRouteDetail {
    EndUnreachable,
    NoWayBack,
    TailTooLong,
    NoCandidate,
    Budget,
}

/// Pourquoi rien n'est rendu : les codes de `GeneratorFailure` Java, plus `NoTeamData` et `Stopped`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    StartTooFar,
    EndTooFar,
    DistanceTooShort,
    NetworkTooSmall,
    NoRoute(NoRouteDetail),
    /// Mode strict ou équipe d'abord sans surcouche.
    NoTeamData,
    /// Coupé par l'appelant : jamais de résultat partiel.
    Stopped,
    /// La recherche a touché le bord de la fenêtre (garde-fou du §5.2, jamais attendu avec les tuiles nécessaires) :
    /// le résultat est jeté, l'appelant refait la génération dans une fenêtre plus grande.
    WindowEscape,
    /// La distance dépasse `Params::max_distance_m` : l'index creux des composantes ne répondrait plus exactement.
    TooLong,
}
