//! Retouche d'un parcours, lot 7a (note `ord-retouche-lot1.md`) : ce que `move_end` et `resize` ont en commun.
//!
//! - [`KeepOut`] : des chaînes de la fenêtre interdites à **toute** une recherche (`Weighter::chain`). Né dans la sonde du
//!   lot 0 (`probe_graft`, qui le réexporte), il sert maintenant hors `probe` : une génération ou un itinéraire ordinaires
//!   ne le posent jamais.
//! - la **table des coutures** d'une fenêtre ([`SeamState`]) : pour chaque couture, l'état orienté de la chaîne gardée qui
//!   arrive au nœud, son prédécesseur, celui de la chaîne qui repart, et les états gardés des 200 m de part et d'autre. C'est
//!   ce qui donne à une recherche **le cap de la jambe gardée** (§4 de la note).
//! - les **accroches de couture** ([`seam_anchor_start`], [`seam_anchor_end`]) : un morceau de longueur nulle sur la chaîne
//!   gardée, dont le cap d'entrée (ou de sortie) est celui de la jambe gardée, et dont le demi-tour est fermé.
//! - [`normalize_between`] : ramène un tronçon rendu par la lentille aux nœuds de ses coutures.
//! - les types publics de la retouche : [`RetouchRequest`], [`RetouchTune`], [`Retouch`], [`Retouched`],
//!   [`RetouchFailure`], [`RetouchDiagnostics`].

use std::collections::BTreeMap;

use ord_graph::format::DIR_BIT;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::cost::RideType;
use crate::engine::{Engine, Snap, ViewData};
use crate::generate::route_geometry;
use crate::geo::LatLon;
use crate::posed::{PoseError, PosedJoin, PosedRoute};
use crate::search::{in_head_ext, out_head, Anchor, INF, NONE};
use crate::verify::Violation;
use crate::window::Window;
use crate::{Failure, Mode, Position, Route, Step};

// --- Le masque ---------------------------------------------------------------------------------------------------------

/// Empreinte de la liste des tuiles d'une fenêtre (FNV-1a).
fn tiles_hash(tiles: &[u32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &t in tiles {
        h = (h ^ u64::from(t)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Chaînes locales d'une fenêtre interdites pour toute une requête (bitset de `n_chains` bits).
#[derive(Clone, Debug)]
pub struct KeepOut {
    bits: Vec<u64>,
    n_chains: usize,
    tiles_hash: u64,
    masked: usize,
}

impl KeepOut {
    /// Masque les chaînes de `win` qui portent au moins une arête de `mask_edges` et aucune arête de `free_edges`. Les
    /// `free_edges` sont la zone des coutures du lot 0 : une chaîne qui en porte une reste libre. La retouche du lot 7a n'en
    /// libère aucune (ses recherches partent d'un état, pas d'une accroche posée sur une chaîne gardée).
    pub fn build(
        win: &Window,
        mask_edges: impl IntoIterator<Item = u32>,
        free_edges: impl IntoIterator<Item = u32>,
    ) -> Self {
        let mask: FxHashSet<u32> = mask_edges.into_iter().collect();
        let free: FxHashSet<u32> = free_edges.into_iter().collect();
        let n = win.v.n_chains;
        let mut bits = vec![0u64; n.div_ceil(64).max(1)];
        let mut masked = 0;
        if !mask.is_empty() {
            for c in 0..n {
                let (mut m, mut f) = (false, false);
                for &l in win.v.links(c as u32) {
                    let e = l & !DIR_BIT;
                    m |= mask.contains(&e);
                    f |= free.contains(&e);
                }
                if m && !f {
                    bits[c >> 6] |= 1 << (c & 63);
                    masked += 1;
                }
            }
        }
        Self { bits, n_chains: n, tiles_hash: tiles_hash(win.tiles()), masked }
    }

    pub fn masked_chains(&self) -> usize {
        self.masked
    }

    /// Mètres de vue masqués : à rapporter, pour voir ce que la chaîne entière ajoute à l'arête gardée.
    pub fn masked_m(&self, win: &Window) -> f64 {
        self.assert_window(win);
        let v = &win.v;
        (0..v.n_chains).filter(|&c| self.is_masked(c as u32)).map(|c| f64::from(v.len_dm[c])).sum::<f64>()
            / 10.0
    }

    pub fn is_masked(&self, c: u32) -> bool {
        self.bits[(c >> 6) as usize] >> (c & 63) & 1 != 0
    }

    pub(crate) fn bits(&self) -> &[u64] {
        &self.bits
    }

    /// Refuse un masque bâti pour une autre fenêtre : ses numéros de chaînes ne diraient rien ici.
    pub(crate) fn assert_window(&self, win: &Window) {
        assert!(
            self.n_chains == win.v.n_chains && self.tiles_hash == tiles_hash(win.tiles()),
            "le masque de chaînes interdites a été bâti pour une autre fenêtre"
        );
    }
}

// --- Types publics -----------------------------------------------------------------------------------------------------

/// Ce que la retouche lit de la requête : le type, le mode, la familiarité, λ et la graine.
#[derive(Clone, Copy, Debug)]
pub struct RetouchRequest {
    pub ride_type: RideType,
    pub mode: Mode,
    pub familiarity: f32,
    pub off_team_cost_q8: u32,
    pub seed: u64,
}

/// Réglages de la retouche ; `Default` = les valeurs du moteur. Ceux que la campagne du banc confirme ou change sont
/// marqués dans la note (§9.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RetouchTune {
    /// Accroche du point d'arrivée de `move_end` : distance maximale à une voie du type (m).
    pub end_snap_m: f64,
    /// Rayon du disque de l'arbre retour : `clamp(facteur × vol d'oiseau + marge ; min ; max)` ; rejoué à `2ρ` (au plus le
    /// maximum) si aucune couture n'est atteinte.
    pub move_end_radius: (f64, f64, f64, f64),
    /// Tolérance de la proposition à la distance visée (plan §4.3).
    pub tolerance: f64,
    /// Seuil de choix d'une paire de coutures (raccourcir) : écart prévu à la cible.
    pub pick_tol: f64,
    /// Changement relatif maximal (D3) ; au-delà, `ChangeTooLarge`.
    pub max_change: f64,
    /// Changement minimal, m ; en deçà, `ChangeTooSmall`.
    pub min_change_m: f64,
    /// Longueur minimale du tronçon neuf, m.
    pub min_segment_m: f64,
    /// Les coutures `i` d'un raccourcissement sont prises entre `ends_share × L` et `(1 − ends_share) × L`.
    pub ends_share: f64,
    pub shorten_trees: usize,
    pub shorten_radius_max_m: f64,
    pub shorten_j_per_i: usize,
    pub lengthen_windows: usize,
    /// La partie remplacée `[i, j]` est libre pour l'allongement (comme au lot 0 : 75,0 → 95,9 % mesuré) ; `false` : masquée aussi.
    pub free_section: bool,
    /// Raccourcir : la partie remplacée `(i, j_max]` n'est pas masquée dans l'arbre de la couture `i` ; une lecture dont le
    /// chemin reprend une jambe d'après `j` est refusée (contrôle C3 local).
    pub free_shorten: bool,
    /// Raccourcir : une couture `j` est lue si l'écart prévu à la cible est d'au plus cette valeur (`pick_tol` à l'arrêt
    /// anticipé, `tolerance` au plus ici).
    pub shorten_read_tol: f64,
    /// Propositions rendues au plus.
    pub proposals: usize,
    /// Passes de `move_end` (reprises après un virage refusé par le contrôle).
    pub passes: usize,
    /// Marge (degrés) du modèle des virages : il refuse à partir de `150 − marge`.
    pub turn_margin_deg: u8,
    /// Échéance douce : passée avec au moins une proposition à 3 %, on s'arrête (ms).
    pub soft_ms: u32,
    /// Échéance dure : ce qui est tenu est rendu, rien de tenu → `Timeout` (ms).
    pub budget_ms: u32,
}

impl Default for RetouchTune {
    fn default() -> Self {
        Self {
            end_snap_m: 500.0,
            move_end_radius: (1.6, 3_000.0, 5_000.0, 40_000.0),
            tolerance: 0.03,
            pick_tol: 0.01,
            max_change: 0.30,
            min_change_m: 1_000.0,
            min_segment_m: 1_000.0,
            ends_share: 0.05,
            shorten_trees: 6,
            shorten_radius_max_m: 20_000.0,
            shorten_j_per_i: 2,
            lengthen_windows: 12,
            free_section: true,
            free_shorten: true,
            shorten_read_tol: 0.03,
            proposals: 3,
            passes: 4,
            turn_margin_deg: 1,
            soft_ms: 1_000,
            budget_ms: 1_800,
        }
    }
}

/// La sortie choisie d'un `move_end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveEndVariant {
    /// Minimise `longueur abandonnée + raccord`.
    Best,
    /// La couture atteinte la plus tardive (on garde le plus).
    KeepAll,
    /// Le plus petit raccord.
    Nearest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetouchKind {
    Shorten,
    Lengthen,
    MoveEnd(MoveEndVariant),
    /// Le point d'arrivée est sur le parcours : coupé au dernier passage, sans tronçon neuf.
    Truncate,
}

/// Une proposition.
#[derive(Clone, Debug)]
pub struct Retouched {
    pub kind: RetouchKind,
    /// Indice (dans `PosedRoute::seams`) de la couture où le tronçon neuf part ; `None` pour `Truncate`.
    pub from_seam: Option<usize>,
    /// Indice de la couture où il arrive ; `None` pour `move_end` et `Truncate`.
    pub to_seam: Option<usize>,
    /// Abscisses d'origine de la partie remplacée (`to_s_m = L` pour `move_end` et `Truncate`).
    pub from_s_m: f64,
    pub to_s_m: f64,
    /// Le tronçon neuf, mesuré ; vide pour `Truncate`.
    pub new: Route,
    /// `L′ = s_i + longueur(new) + (L − s_j)`.
    pub length_m: f64,
    /// Mètres d'origine gardés.
    pub kept_m: f64,
    /// `|L′ − D′| / D′` (`resize`).
    pub err: Option<f64>,
    /// Mètres abandonnés (`move_end`).
    pub abandon_m: Option<f64>,
}

impl Retouched {
    /// La géométrie complète : la polyligne d'origine jusqu'à `from_s_m`, le tronçon neuf, puis l'origine depuis `to_s_m`
    /// (raccourcir ou allonger seulement). La couture est le nœud ; le point de coupe de l'origine en est à `proj_tol_m` au
    /// plus (15 m).
    pub fn geometry<B: AsRef<[u8]>>(&self, e: &Engine<B>, posed: &PosedRoute) -> Vec<LatLon> {
        let mut out = posed.line_between(0.0, self.from_s_m);
        if !self.new.steps.is_empty() {
            out.extend(route_geometry(e.base(), &self.new));
        }
        if matches!(self.kind, RetouchKind::Shorten | RetouchKind::Lengthen) {
            out.extend(posed.line_between(self.to_s_m, posed.length_m()));
        }
        out
    }
}

/// Ce qu'une retouche a coûté, et ce qu'elle a vu (compteurs ; aucune donnée privée).
#[derive(Clone, Debug, Default)]
pub struct RetouchDiagnostics {
    /// Coutures proposées à la fenêtre ; écartées par la fenêtre : règle 2 (arête hors vue ou hors fenêtre), nœud incomplet,
    /// chaîne d'arrivée partielle.
    pub seams_offered: u32,
    pub seams_refused_window: [u32; 3],
    /// Coutures dont les états gardés des 200 m voisins sont incomplets.
    pub short_leads: u32,
    /// Arbres bâtis, états fixés par eux, fenêtres de lentille jouées.
    pub trees: u32,
    pub settled: u64,
    pub windows_tried: u32,
    /// Sorties refusées par la règle des grappes à la couture (`junction_ok`).
    pub junction_cut: u32,
    /// Raccourcir : coutures `j` lues dans un arbre (une arrivée atteinte), `j` dont toutes les arrivées sont refusées par
    /// le virage d'entrée dans la jambe gardée (`turn_cost`, `retro_ok`), chemins dont la tête n'est pas l'état gardé
    /// d'entrée, et, par arbre lu, le meilleur écart `err₀` de la longueur lue à la cible (`None` : aucun `j` atteint).
    pub shorten_reached: u32,
    pub shorten_turn_refused: u32,
    pub shorten_head_mismatch: u32,
    /// Raccourcir, partie remplacée libre : chemins refusés parce qu'ils reprennent une jambe gardée d'après `j`.
    pub shorten_reuse_refused: u32,
    pub tree_best_err0: Vec<Option<f64>>,
    /// Refus de `check_spliced_in`, par code.
    pub spliced_refused: BTreeMap<Violation, u32>,
    pub passes: u32,
    pub stopped_early: bool,
    pub deadline_hit: bool,
    pub radius_m: Option<f64>,
    pub widened: bool,
    pub elapsed_us: u64,
    /// Propositions à `pick_tol` ou moins, et à `tolerance` ou moins.
    pub proposals_tight: u32,
}

/// Une retouche : les propositions (la meilleure d'abord) et les diagnostics.
#[derive(Clone, Debug)]
pub struct Retouch {
    pub proposals: Vec<Retouched>,
    pub diagnostics: RetouchDiagnostics,
}

/// Un refus et ce que la recherche a vu avant de refuser : les compteurs ne se perdent pas sur un échec (rendu en `Box` : le
/// refus est le chemin rare, le succès n'en paie pas la taille).
#[derive(Clone, Debug)]
pub struct Refused {
    pub failure: RetouchFailure,
    pub diagnostics: RetouchDiagnostics,
}

/// Pourquoi rien n'est rendu (note §9.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RetouchFailure {
    /// L'entrée est refusée.
    Pose(PoseError),
    /// Aucune couture valide pour l'opération (marges, règle 2).
    NoSeam,
    /// Le nouveau point d'arrivée est à plus de `end_snap_m` d'une voie du type.
    EndTooFar,
    /// `move_end` : aucune sortie de couture dans l'arbre, rayon maximal compris.
    NoSeamReached,
    /// Des sorties de couture atteintes, toutes coupées par la règle des grappes (`junction_ok`) : aucun chemin n'a été
    /// proposé au contrôle du recollé.
    JunctionCut,
    /// `|Δ| < min_change_m`.
    ChangeTooSmall,
    /// `|δ| > max_change` : renvoi à la génération (D3).
    ChangeTooLarge,
    /// Raccourcir : aucune paire n'offre l'économie à `pick_tol` près dans les arbres joués.
    NoEconomy,
    /// Allonger : aucune paire `(i, j)` à l'échelle de `ℓ*`.
    NoWindow,
    /// Toutes les recherches ont échoué (échec dominant du moteur pour la lentille).
    NoProposal {
        dominant: Option<Failure>,
    },
    /// Des chemins, tous refusés par la règle des grappes ou le contrôle du recollé.
    AllRefused {
        dominant: Violation,
    },
    /// Des propositions contrôlées, aucune à `tolerance`.
    OffTarget {
        best_err: f64,
    },
    /// `budget_ms` passé sans proposition.
    Timeout,
    NoTeamData,
    WindowEscape,
    Stopped,
    TooLong,
}

// --- La table des coutures ---------------------------------------------------------------------------------------------

/// Une couture vue de la fenêtre : les états gardés autour du nœud (ids locaux).
#[derive(Clone, Debug)]
pub(crate) struct SeamState {
    /// Indice dans `PosedRoute::seams`.
    pub seam: usize,
    /// Nœud local.
    pub nl: u32,
    /// État orienté de la chaîne gardée qui arrive au nœud, et celui qui le précède.
    pub f_in: u32,
    pub pred_in: u32,
    /// État de la chaîne gardée qui repart, et celui qui le suit ; `NONE` si l'opération ne garde pas la suite.
    pub f_out: u32,
    pub succ_out: u32,
    /// Les états gardés jusqu'à 200 m avant le nœud (`f_in` en dernier, au plus 48), dans l'ordre de la marche.
    pub lead_before: Vec<u32>,
    /// Les états gardés après `f_out`, jusqu'à 200 m.
    pub lead_after: Vec<u32>,
    /// Mètres de marche (dm) du départ au nœud.
    pub dm: u32,
}

/// Mètres (dm) de chaînes que le modèle des virages relit de part et d'autre : 100 m de grappe, 50 m de cap, 50 m de
/// nœuds candidats (`turns.rs`), plus une marge.
const LEAD_DM: u32 = 2_000;
const MAX_LEAD: usize = 48;
/// Sous cette longueur de chaîne (dm), le cap est celui de 50 m et il faut le prédécesseur.
const CAP_DM: u32 = 500;

/// Les états de chaînes que la marche prend, jambe par jambe.
struct WalkStates<'a, B> {
    e: &'a Engine<B>,
    v: &'a ViewData,
    posed: &'a PosedRoute,
    /// Par jambe : l'état orienté et la position de la jambe dans ses liens orientés.
    st: Vec<Option<(u32, usize)>>,
}

impl<'a, B: AsRef<[u8]>> WalkStates<'a, B> {
    fn new(e: &'a Engine<B>, win: &'a Window, posed: &'a PosedRoute) -> Self {
        let v = &win.v;
        let wanted: FxHashSet<u32> = posed.legs().iter().map(|l| l.edge).collect();
        let mut chain_of: FxHashMap<u32, (u32, usize, bool)> =
            FxHashMap::with_capacity_and_hasher(wanted.len(), Default::default());
        for c in 0..v.n_chains {
            for (i, &l) in v.links(c as u32).iter().enumerate() {
                let ed = l & !DIR_BIT;
                if wanted.contains(&ed) {
                    chain_of.insert(ed, (c as u32, i, l & DIR_BIT != 0));
                }
            }
        }
        let st = posed
            .legs()
            .iter()
            .map(|l| {
                let &(c, idx, stored_rev) = chain_of.get(&l.edge)?;
                let n = v.links(c).len();
                // Le sens du lien enregistré : la jambe le suit si elle a le même sens `a → b`.
                if l.reverse == stored_rev {
                    Some((c << 1, idx))
                } else {
                    Some(((c << 1) | 1, n - 1 - idx))
                }
            })
            .collect();
        Self { e, v, posed, st }
    }

    fn n_links(&self, f: u32) -> usize {
        self.v.links(f >> 1).len()
    }

    fn node_join(&self, j: usize) -> bool {
        matches!(self.posed.joins()[j], PosedJoin::Node(_))
    }

    /// Les états de la marche qui finissent à la jambe `j` (qui doit être le dernier lien du dernier état), en remontant
    /// jusqu'à `LEAD_DM` : `(états dans l'ordre, le premier est complet ou fait au moins 50 m, le premier est complet,
    /// l'arrêt vient d'un état manquant)`.
    fn back(&self, j: usize) -> Option<(Vec<u32>, bool, bool, bool)> {
        let mut out: Vec<u32> = Vec::new();
        let (mut first_ok, mut first_all, mut short) = (false, false, false);
        let mut k = j;
        let mut acc = 0u32;
        loop {
            let Some((f, p)) = self.st[k] else {
                short = !out.is_empty();
                break;
            };
            let n = self.n_links(f);
            if p + 1 != n {
                short = !out.is_empty();
                break;
            }
            if let Some(&last) = out.last() {
                // L'état suivant (déjà posé) doit partir du nœud où celui-ci arrive.
                if self.v.src(last) != self.v.tgt(f) {
                    short = true;
                    break;
                }
            }
            let (mut t, mut dm) = (0usize, 0u32);
            while t < n
                && t <= k
                && self.st[k - t] == Some((f, n - 1 - t))
                && (t == 0 || self.node_join(k - t))
            {
                dm += self.e.len_dm(self.posed.legs()[k - t].edge);
                t += 1;
            }
            let all = t == n;
            if out.is_empty() {
                first_all = all;
                first_ok = all || dm >= CAP_DM;
                if !first_ok {
                    return None;
                }
            }
            if !all {
                if out.is_empty() {
                    out.push(f);
                } else {
                    short = true;
                }
                break;
            }
            out.push(f);
            acc += self.v.len_dm[(f >> 1) as usize];
            if acc >= LEAD_DM || out.len() >= MAX_LEAD || k + 1 == n {
                break;
            }
            let start = k + 1 - n;
            if !self.node_join(start - 1) {
                break;
            }
            k = start - 1;
        }
        out.reverse();
        Some((out, first_ok, first_all, short))
    }

    /// Les états qui commencent à la jambe `j` (premier lien du premier état), en avançant jusqu'à `LEAD_DM`.
    fn forward(&self, j: usize) -> Option<(Vec<u32>, bool, bool, bool)> {
        let n_legs = self.posed.legs().len();
        let mut out: Vec<u32> = Vec::new();
        let (mut first_ok, mut first_all, mut short) = (false, false, false);
        let mut k = j;
        let mut acc = 0u32;
        loop {
            let Some((f, p)) = self.st[k] else {
                short = !out.is_empty();
                break;
            };
            let n = self.n_links(f);
            if p != 0 {
                short = !out.is_empty();
                break;
            }
            if let Some(&prev) = out.last() {
                if self.v.tgt(prev) != self.v.src(f) {
                    short = true;
                    break;
                }
            }
            let (mut t, mut dm) = (0usize, 0u32);
            while t < n
                && k + t < n_legs
                && self.st[k + t] == Some((f, t))
                && (t == 0 || self.node_join(k + t - 1))
            {
                dm += self.e.len_dm(self.posed.legs()[k + t].edge);
                t += 1;
            }
            let all = t == n;
            if out.is_empty() {
                first_all = all;
                first_ok = all || dm >= CAP_DM;
                if !first_ok {
                    return None;
                }
            }
            if !all {
                if out.is_empty() {
                    out.push(f);
                } else {
                    short = true;
                }
                break;
            }
            out.push(f);
            acc += self.v.len_dm[(f >> 1) as usize];
            if acc >= LEAD_DM || out.len() >= MAX_LEAD || k + n >= n_legs {
                break;
            }
            if !self.node_join(k + n - 1) {
                break;
            }
            k += n;
        }
        Some((out, first_ok, first_all, short))
    }
}

/// La table des coutures de `wanted` (indices dans `PosedRoute::seams`) dans la fenêtre, avec ce que la fenêtre écarte
/// (règle 2 du lot 0 : arête hors vue ou hors fenêtre ; nœud incomplet ; chaîne partielle). `with_out` : l'opération garde la
/// suite de la marche (`resize`), la couture doit avoir son état de sortie.
pub(crate) fn seam_table<B: AsRef<[u8]>>(
    e: &Engine<B>,
    win: &Window,
    posed: &PosedRoute,
    wanted: &[usize],
    with_out: bool,
    diag: &mut RetouchDiagnostics,
) -> Vec<Option<SeamState>> {
    let ws = WalkStates::new(e, win, posed);
    let v = &win.v;
    let vi = win.vi;
    wanted
        .iter()
        .map(|&k| {
            diag.seams_offered += 1;
            let seam = posed.seams()[k];
            let j = seam.join;
            let (Some((fa, pa)), Some((fb, pb))) = (ws.st[j], ws.st[j + 1]) else {
                diag.seams_refused_window[0] += 1;
                return None;
            };
            if pa + 1 != ws.n_links(fa) || pb != 0 || v.tgt(fa) != v.src(fb) {
                diag.seams_refused_window[0] += 1;
                return None;
            }
            let nl = v.tgt(fa);
            let gn = e.views_files[vi].view_node_of_base(seam.node);
            if gn.is_none_or(|g| v.ngid.binary_search(&g).ok() != Some(nl as usize))
                || !v.complete[nl as usize]
            {
                diag.seams_refused_window[1] += 1;
                return None;
            }
            let Some((before, ok_a, _, short_a)) = ws.back(j) else {
                diag.seams_refused_window[2] += 1;
                return None;
            };
            if !ok_a {
                diag.seams_refused_window[2] += 1;
                return None;
            }
            let (f_out, succ_out, lead_after) = if with_out {
                let Some((after, ok_b, _, short_b)) = ws.forward(j + 1) else {
                    diag.seams_refused_window[2] += 1;
                    return None;
                };
                if !ok_b {
                    diag.seams_refused_window[2] += 1;
                    return None;
                }
                diag.short_leads += u32::from(short_b);
                (after[0], after.get(1).copied().unwrap_or(NONE), after[1..].to_vec())
            } else {
                (NONE, NONE, Vec::new())
            };
            diag.short_leads += u32::from(short_a);
            let pred_in = if before.len() >= 2 { before[before.len() - 2] } else { NONE };
            Some(SeamState {
                seam: k,
                nl,
                f_in: fa,
                pred_in,
                f_out,
                succ_out,
                lead_before: before,
                lead_after,
                dm: (posed.leg_end_m(j) * 10.0).min(f64::from(u32::MAX / 2)) as u32,
            })
        })
        .collect()
}

// --- Accroches de couture ----------------------------------------------------------------------------------------------

/// L'accroche d'un état entier : sur son premier (`at_end` faux) ou son dernier lien orienté, au bout correspondant.
pub(crate) fn link_snap<B: AsRef<[u8]>>(e: &Engine<B>, v: &ViewData, f: u32, at_end: bool) -> Snap {
    let links = v.links(f >> 1);
    let n = links.len();
    let rev_dir = f & 1 == 1;
    let stored = if at_end != rev_dir { n - 1 } else { 0 };
    let l = links[stored];
    let (edge, stored_rev) = (l & !DIR_BIT, l & DIR_BIT != 0);
    let oriented_rev = stored_rev != rev_dir;
    let g = e.geom_len_m(edge);
    // Parcourue de `a` vers `b` : le début est à 0, la fin à `g` ; à l'envers, l'inverse.
    let offset_m = if at_end != oriented_rev { g } else { 0.0 };
    Snap { chain: f >> 1, link: stored, edge, rev: stored_rev, offset_m, dist_m: 0.0 }
}

/// Départ d'une recherche à une couture : un morceau de longueur nul le long de la chaîne gardée `f_in`. Le sens qui repart
/// le long de `f_in` (le demi-tour sur la jambe gardée) coûte `INF` ; le sens de longueur nulle garde un coût nul et pour
/// **cap d'arrivée** celui de la jambe gardée, prolongé par son prédécesseur si elle fait moins de 50 m : la première
/// relaxation des arbres lit justement ce cap (`hin_over`).
pub(crate) fn seam_anchor_start<B: AsRef<[u8]>>(e: &Engine<B>, v: &ViewData, s: &SeamState) -> Anchor {
    let dir = (s.f_in & 1) as usize;
    let mut a = blank_anchor(link_snap(e, v, s.f_in, true));
    a.out_cost[dir] = 0;
    a.out_len[dir] = 0;
    a.out_head[dir] = in_head_ext(v, s.f_in, s.pred_in);
    a
}

/// Arrivée d'une recherche à une couture : le symétrique, sur la chaîne gardée `f_out`. Le cap de départ du morceau est celui
/// de la jambe gardée qui repart ; le sens qui arriverait en remontant `f_out` coûte `INF`.
pub(crate) fn seam_anchor_end<B: AsRef<[u8]>>(e: &Engine<B>, v: &ViewData, s: &SeamState) -> Anchor {
    let dir = (s.f_out & 1) as usize;
    let mut a = blank_anchor(link_snap(e, v, s.f_out, false));
    a.in_cost[dir] = 0;
    a.in_len[dir] = 0;
    a.in_head[dir] = out_head(v, s.f_out);
    a
}

fn blank_anchor(snap: Snap) -> Anchor {
    Anchor {
        snap,
        out_cost: [INF; 2],
        out_len: [0; 2],
        in_cost: [INF; 2],
        in_len: [0; 2],
        out_head: [0; 2],
        in_head: [0; 2],
    }
}

/// Un tronçon rendu par une recherche de couture, ramené aux nœuds : sans le morceau de longueur nulle de l'accroche.
pub(crate) struct Normalized {
    pub route: Route,
    /// Le premier (le dernier) pas a été retiré : à répercuter sur l'état de chaque pas.
    pub head_removed: bool,
    pub tail_removed: bool,
}

/// Ramène un tronçon aux nœuds de ses coutures : l'accroche tombe à un bout d'arête, ce qui fait un morceau de départ ou
/// d'arrivée de longueur nulle (ou de quelques mètres). Un morceau de 5 m ou moins qui touche la couture est retiré ; un
/// morceau qui part de la couture est prolongé jusqu'à elle. `None` si le tronçon ne part pas (ou n'arrive pas) à la couture.
pub(crate) fn normalize_between<B: AsRef<[u8]>>(
    e: &Engine<B>,
    r: &Route,
    node_i: u32,
    node_j: u32,
) -> Option<Normalized> {
    let mut steps = r.steps.clone();
    let (mut start, mut end) = (r.start, r.end);
    let (mut head_removed, mut tail_removed) = (false, false);
    let ends = |s: &Step| {
        let (a, b) = e.ends(s.base_edge);
        if s.reverse {
            (b, a)
        } else {
            (a, b)
        }
    };
    let s0 = *steps.first()?;
    let (f0, t0) = ends(&s0);
    if f0 == node_i {
        start = Position {
            edge: s0.base_edge,
            offset_m: if s0.reverse { e.geom_len_m(s0.base_edge) } else { 0.0 },
        };
    } else if t0 == node_i {
        let len = e.geom_len_m(s0.base_edge);
        let piece = if s0.reverse { start.offset_m } else { len - start.offset_m };
        if piece > 5.0 || steps.len() < 2 {
            return None;
        }
        steps.remove(0);
        head_removed = true;
        let s1 = steps[0];
        if ends(&s1).0 != node_i {
            return None;
        }
        start = Position {
            edge: s1.base_edge,
            offset_m: if s1.reverse { e.geom_len_m(s1.base_edge) } else { 0.0 },
        };
    } else {
        return None;
    }
    let sl = *steps.last()?;
    let (fl, tl) = ends(&sl);
    if tl == node_j {
        end = Position {
            edge: sl.base_edge,
            offset_m: if sl.reverse { 0.0 } else { e.geom_len_m(sl.base_edge) },
        };
    } else if fl == node_j {
        let len = e.geom_len_m(sl.base_edge);
        let piece = if sl.reverse { len - end.offset_m } else { end.offset_m };
        if piece > 5.0 || steps.len() < 2 {
            return None;
        }
        steps.pop();
        tail_removed = true;
        let s1 = *steps.last()?;
        if ends(&s1).1 != node_j {
            return None;
        }
        end = Position {
            edge: s1.base_edge,
            offset_m: if s1.reverse { 0.0 } else { e.geom_len_m(s1.base_edge) },
        };
    } else {
        return None;
    }
    Some(Normalized { route: Route { start, end, steps, ..r.clone() }, head_removed, tail_removed })
}

/// Le tronçon neuf d'une suite d'états, du premier lien du premier au dernier lien du dernier (`start`, `end`), non mesuré.
pub(crate) fn route_between_snaps(
    v: &ViewData,
    fields: &[u32],
    start: &Snap,
    end: &Snap,
    familiarity: f32,
) -> (Route, Vec<u32>) {
    let (steps, step_field) = crate::generate::steps_of(v, fields, start, end);
    let route = Route {
        start: Position { edge: start.edge, offset_m: start.offset_m },
        end: Position { edge: end.edge, offset_m: end.offset_m },
        steps,
        length_m: 0.0,
        team_m: 0.0,
        bridged_m: 0.0,
        shared_m: 0.0,
        crossings: 0,
        unpaved_m: 0.0,
        offroad_m: 0.0,
        presumed_surface_m: 0.0,
        bearing_deviation_deg: None,
        familiarity,
        family: crate::Family::Turnaround,
    };
    (route, step_field)
}
