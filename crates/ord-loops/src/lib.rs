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

mod attract;
mod cache;
mod cost;
mod engine;
mod figures;
mod generate;
mod geo;
mod index;
#[cfg(feature = "probe")]
pub mod probe;
#[cfg(feature = "probe")]
pub mod probe_graft;
mod reverse;
mod search;
mod selfcheck;
mod shape_common;
mod taste_common;
mod team;
#[cfg(any(test, feature = "reference"))]
mod team_reference;
mod turns;
mod verify;
mod window;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_attract;
#[cfg(all(test, feature = "probe"))]
mod tests_graft;
#[cfg(test)]
mod tests_index;
#[cfg(all(test, feature = "probe"))]
mod tests_mu;
#[cfg(test)]
mod tests_organic;
#[cfg(test)]
mod tests_reference;
#[cfg(test)]
mod tests_team_sparse;
#[cfg(test)]
mod tests_turns;
#[cfg(test)]
mod tests_window;

pub use attract::AttrWeights;
pub use cache::{CacheStats, WindowCache};
pub use cost::{
    common_gen_dm_per_loop, common_loops, common_source, common_step_gen_dm, common_step_loops,
    common_step_real_dm, is_greenway, multiplier_table, taste_class, taste_surface, taste_way, taste_weight,
    RideType, Taste, COMMON_USE_NORMALIZED, TASTE_CLASSES,
};
pub use engine::{Engine, EngineError, Params};
pub use generate::{
    generate, generate_in, generate_with_diagnostics, generate_with_diagnostics_in, generate_with_figures_in,
    route_geometry,
};
pub use geo::LatLon;
pub use index::{cap_dm_of, foreign_index, short_components};
pub use search::Workspace;
pub use selfcheck::SelfCheck;
pub use team::{BuildPlan, TeamCtx, TeamData, TeamStats, WindowTeam};
pub use verify::{
    check_route, check_route_aligned, check_route_aligned_in, check_route_in, Check, Violation,
};
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
    /// Figures jouées (lot 4, 4c) : [`Figures::Classic`] rend exactement les parcours de 1.10.
    pub figures: Figures,
}

/// Les figures que la génération essaie (lot 4, 4c, §15.8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Figures {
    /// La lentille d'aujourd'hui seule, sans couloir : les parcours de 1.10, au bit près.
    Classic,
    /// La lentille élargie par un couloir et le triangle, tirés par requête sur la répartition réelle des sommets. Sans
    /// effet pour un aller simple et en mode strict (lentille seule, comme `Classic`).
    Organic(Organic),
}

/// Réglages de la génération organique.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Organic {
    pub mix: FigureMix,
    pub tune: OrganicTune,
    /// Poids des attracteurs OSM (lot 4, 4d) : [`AttrWeights::NONE`] (ou aucun fichier chargé) rend exactement 1.12.
    pub attr: AttrWeights,
}

/// La répartition des figures d'une requête : la part des triangles et la loi du r / D des triangles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FigureMix {
    /// Part des premiers parcours qui sont des triangles, en 256es (0 : aucune recherche de triangle).
    pub tri_q8: u16,
    /// P10, P25, P50, P75, P90 du r / D des triangles réels, en millièmes.
    pub rd_tri_milli: [u16; 5],
}

impl FigureMix {
    /// Lentille seule : aucun triangle.
    pub const LENS_ONLY: Self = Self { tri_q8: 0, rd_tri_milli: [0; 5] };

    /// La répartition générale d'un type (constantes de `shape_common.rs`, tirées des boucles réelles de toutes les
    /// équipes) ; le MTB n'a que des lentilles.
    pub fn general(ride: RideType) -> Self {
        crate::shape_common::general(ride)
    }
}

/// Réglages fins de la génération organique (§15.8) ; `Default` = les valeurs du service.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrganicTune {
    /// Demi-largeur du couloir en millièmes de D (0 : sans couloir).
    pub corridor_w_milli: u16,
    /// Multiplicateur de coût d'une arête dans le couloir, en 256es.
    pub kappa_q8: u16,
    /// Paires de sommets cherchées au plus par requête, triangles gardés au plus, sous-bandes de la bande de distance.
    pub max_pairs: u8,
    pub tri_keep: u8,
    pub sub_bands: u8,
    /// Bande des mètres de T1 des sommets, en millièmes de D, et écart de cap des deux sommets, en degrés.
    pub band_milli: [u16; 2],
    pub sep_deg: [u8; 2],
    /// Biais initial de l'estimation (millièmes), tolérance de l'estimation (millièmes de D), sécante.
    pub beta0_milli: u16,
    pub est_tol_milli: u16,
    pub secant: bool,
    /// Multiplicateur de la part de triangles (256es), écart de note qui fait préférer une figure absente (millièmes).
    pub tri_boost_q8: u16,
    pub near_score_milli: u16,
    /// Épi : angle (degrés) entre l'arrivée et le départ d'un sommet, et distance (m) lue de part et d'autre.
    pub spike_deg: u8,
    pub spike_m: u16,
    /// Une paire dont le triangle est refusé pour un virage serré est reprise une fois à la passe suivante, avec les
    /// transitions que le refus interdit (§15.19 de la note) ; elle ne prend pas une seconde place parmi `max_pairs`.
    pub sharp_retry: bool,
    /// Le contrôle interne prend ses caps comme le contrôleur indépendant (repère centré au nœud) ; `false` : celui de
    /// l'organique d'avant l'étape « virages » (§16.3 de la note du lot 4).
    pub turns: bool,
    /// Marge (degrés) du modèle des virages de la recherche : il refuse à partir de `150 − marge` ; 0 : le seuil du contrôle
    /// (§16.2 de la note).
    pub turn_margin_deg: u8,
    /// Aucune jambe ne repasse sur une de ses chaînes dans l'autre sens, et la jambe 2 ne reprend rien de la jambe 1, queue
    /// comprise (§16.4 de la note) ; `false` : l'organique d'avant l'étape « virages ».
    pub no_reuse: bool,
    /// Tolérance (millièmes de D) de l'écart `e(n)` d'un demi-tour attracteur à celui de la lentille de 1.12 (§17.7) ; 0 :
    /// la lentille de 1.12.
    pub attr_lens_tol_milli: u16,
    /// Les poids des attracteurs entrent dans la clé de rang des paires de sommets (un point de poids vaut une case de
    /// 0,02 de r / D) ; `false` : les attracteurs sont des candidats sans clé de rang.
    pub attr_rank: bool,
}

impl Default for OrganicTune {
    fn default() -> Self {
        Self {
            corridor_w_milli: 30,
            kappa_q8: 512,
            max_pairs: 4,
            tri_keep: 2,
            sub_bands: 3,
            band_milli: [270, 390],
            sep_deg: [40, 90],
            beta0_milli: 1_040,
            est_tol_milli: 50,
            secant: true,
            tri_boost_q8: 256,
            near_score_milli: 1_000,
            spike_deg: 150,
            spike_m: 500,
            sharp_retry: true,
            turns: true,
            turn_margin_deg: 5,
            no_reuse: true,
            attr_lens_tol_milli: 50,
            attr_rank: true,
        }
    }
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
    /// Aller jusqu'à un point de demi-tour, retour par un autre chemin (la lentille).
    Turnaround,
    /// Polygone dans le cap : jamais rendu (documenté dans le contrat, retiré au prochain changement cassant).
    Polygon,
    /// Boucle à deux sommets (lot 4, 4c) : jambe d'aller, jambe transverse, jambe de retour.
    Triangle,
}

/// Ce que la génération organique a fait pour une requête (compteurs, aucune donnée privée) ; vide pour `Classic`.
/// Les compteurs des triangles ne vont pas dans [`Diagnostics`], dont `rejected` classe les échecs de la lentille.
#[derive(Clone, Debug, Default)]
pub struct FigureDiagnostics {
    /// La génération organique a joué (boucle, mode autre que strict, [`Figures::Organic`]).
    pub applied: bool,
    /// La figure tirée pour le premier parcours est le triangle, et le r / D visé tiré.
    pub tri_first: bool,
    pub rho_target: f64,
    /// Candidats (nœuds lointains), paires écartées, paires admissibles ; cumulés sur les passes.
    pub candidates: u32,
    pub pairs: u32,
    pub pairs_admissible: u32,
    /// Recherches de paires (reprises comprises), dont les reprises d'une paire refusée pour un virage serré ; jambes 2
    /// trouvées ou abandonnées au plafond. `searched − retried` ≤ `max_pairs` (paires distinctes de la requête).
    pub searched: u32,
    pub retried: u32,
    pub leg2_found: u32,
    pub leg2_capped: u32,
    /// Jambes 3 : chemin de l'arbre retour gardé, A* d'arrivée trouvé, échec.
    pub leg3_t2: u32,
    pub leg3_astar: u32,
    pub leg3_failed: u32,
    /// Triangles complets (trois jambes), à ±tolérance de D, jetés pour un épi, épis relevés et réparés.
    pub complete: u32,
    pub within_tol: u32,
    pub spikes: u32,
    pub spikes_repaired: u32,
    pub spikes_dropped: u32,
    /// Triangles refusés par le contrôle, par code ; triangles gardés (notés).
    pub tri_rejected: Vec<(Violation, u32)>,
    pub tri_scored: u32,
    /// États fixés par les recherches de la jambe 2 et par les A* de la jambe 3.
    pub p2p_settled: u64,
    pub leg3_settled: u64,
    /// Biais final de la sécante.
    pub beta_final: f64,
    /// Relaxations refusées par le modèle des virages, relaxations qu'il a évaluées hors voie rapide (le coût) ; toute la
    /// requête, lentille et triangles.
    pub turn_cut: u64,
    pub turn_evals: u64,
    /// Relaxations refusées parce qu'elles reprenaient en sens inverse une chaîne du chemin (aller-retour dans une jambe).
    pub reuse_cut: u64,
    /// Attracteurs (lot 4, 4d) : sommets candidats ajoutés pour leur poids, paires de poids non nul cherchées, demi-tours de
    /// la lentille remplacés par un attracteur.
    pub attr_candidates: u32,
    pub attr_pairs_searched: u32,
    pub attr_lens_picks: u32,
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
