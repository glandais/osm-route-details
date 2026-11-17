//! Le recalé itéré, commun ou par équipe (note lot 4, §13.2 et §13.3) : un pas corrige la table recalée d'après l'écart
//! que les parcours de la table précédente laissent entre le réel et le généré, par classe de goût.
//!
//! Tout en `f64`, dans un ordre de sommes fixé (séquentiel, sans `rayon`) : quelques milliers de boucles sur 72 cases.
//! Seules les `q8` entières entrent dans le moteur. Aucune coordonnée ni id : des mètres et des parts par classe.

use std::collections::BTreeMap;

use ord_loops::{Taste, TASTE_CLASSES};
use serde::Serialize;

use super::export::Loop;
use super::play::PlayRec;
use super::taste::{ratio_table, shares, TasteParams, TasteTable, TYPES};
pub use super::variants::Dims;
use super::variants::{ClassMetersOut, OwnerGap, TasteSource, TypeMeters};

/// Dans l'intervalle `]-DEAD_ZONE, DEAD_ZONE[`, une correction n'a pas de signe (§13.2.2, étape 2).
const DEAD_ZONE: f64 = 0.02;
/// Paramètres des effets principaux : 6 voies, 3 revêtements, balisé (2 niveaux).
const N_ME: usize = 11;

// --- Entrées d'un pas ------------------------------------------------------------------------------------------------

/// Mètres par classe d'apprentissage, par type (`RideType::index()`, `usize::from(l.ty).min(2)`).
#[derive(Clone, Debug, PartialEq)]
pub struct Meters {
    /// Mètres du réel, 72 cases par type.
    pub real: [Vec<f64>; 3],
    /// Mètres du premier parcours généré, 72 cases par type.
    pub gen: [Vec<f64>; 3],
    /// Boucles comptées (au moins une paire comptée) et paires (boucle, curseur) comptées.
    pub loops: [u32; 3],
    pub pairs: [u32; 3],
}

impl Default for Meters {
    fn default() -> Self {
        let z = || vec![0.0; TASTE_CLASSES];
        Self { real: [z(), z(), z()], gen: [z(), z(), z()], loops: [0; 3], pairs: [0; 3] }
    }
}

/// Les entrées d'un pas : toutes équipes réunies, et par équipe (indice `t` de `Loop::team`).
#[derive(Clone, Debug, PartialEq)]
pub struct ClassMeters {
    pub pooled: Meters,
    pub team: Vec<Meters>,
}

impl ClassMeters {
    /// Les mètres d'une variante jouée. Une paire (boucle, curseur) compte si la boucle n'est **pas** mise à l'écart,
    /// si le jeu est sur la surcouche `full`, d'issue `OK`, avec `first_class_m`. `pooled` est calculé exactement comme
    /// l'ancien `RcInputs::of` (mêmes conditions, même ordre d'addition : boucles dans l'ordre de `loops`, jeux dans
    /// l'ordre de `plays`), pour que le pas 1 redonne `rc05` au bit près.
    pub fn of(
        loops: &[Loop],
        held: &std::collections::HashSet<usize>,
        real_class_m: &[[f64; TASTE_CLASSES]],
        plays: &[Vec<PlayRec>],
        n_teams: usize,
    ) -> Self {
        let keys: Vec<(usize, u8)> = loops.iter().map(|l| (l.team, l.ty)).collect();
        Self::of_keys(&keys, held, real_class_m, plays, n_teams)
    }

    /// [`ClassMeters::of`] sur les seuls `(équipe, type)` des boucles : tout ce qu'elle lit d'une boucle.
    fn of_keys(
        keys: &[(usize, u8)],
        held: &std::collections::HashSet<usize>,
        real_class_m: &[[f64; TASTE_CLASSES]],
        plays: &[Vec<PlayRec>],
        n_teams: usize,
    ) -> Self {
        let mut pooled = Meters::default();
        let mut team: Vec<Meters> = (0..n_teams).map(|_| Meters::default()).collect();
        for (i, &(t, ty)) in keys.iter().enumerate().filter(|(i, _)| !held.contains(i)) {
            let ty = usize::from(ty).min(2);
            let mut counted = false;
            for p in plays[i].iter().filter(|p| p.overlay == "full" && p.outcome == "OK") {
                let Some(fc) = &p.first_class_m else { continue };
                for c in 0..TASTE_CLASSES {
                    pooled.real[ty][c] += real_class_m[i][c];
                    pooled.gen[ty][c] += fc[c];
                    team[t].real[ty][c] += real_class_m[i][c];
                    team[t].gen[ty][c] += fc[c];
                }
                pooled.pairs[ty] += 1;
                team[t].pairs[ty] += 1;
                counted = true;
            }
            if counted {
                pooled.loops[ty] += 1;
                team[t].loops[ty] += 1;
            }
        }
        Self { pooled, team }
    }
}

/// Les mètres de toutes équipes réunies, tels que le rapport les écrit (§14.3.1).
pub fn pooled_out(m: &ClassMeters) -> ClassMetersOut {
    ClassMetersOut {
        pooled: (0..3)
            .map(|ty| TypeMeters {
                ride: TYPES[ty].0.to_string(),
                real_m: m.pooled.real[ty].clone(),
                gen_m: m.pooled.gen[ty].clone(),
                loops: m.pooled.loops[ty],
                pairs: m.pooled.pairs[ty],
            })
            .collect(),
    }
}

/// L'écart L1 que les parcours d'une variante laissent, par propriétaire (`pooled_name` pour toutes équipes, puis les
/// équipes de `order`) et par type ayant au moins une paire comptée.
pub fn l1_gaps(m: &ClassMeters, order: &[usize], letter_of: &[String], pooled_name: &str) -> Vec<OwnerGap> {
    let gap = |x: &Meters, ty: usize| -> f64 {
        shares(&x.real[ty]).iter().zip(shares(&x.gen[ty])).map(|(a, b)| (a - b).abs()).sum()
    };
    let mut out = Vec::new();
    for (name, x) in std::iter::once((pooled_name, &m.pooled))
        .chain(order.iter().map(|&t| (letter_of[t].as_str(), &m.team[t])))
    {
        for ty in (0..3).filter(|&ty| x.pairs[ty] > 0) {
            out.push(OwnerGap { owner: name.to_string(), ty: TYPES[ty].0.to_string(), l1: gap(x, ty) });
        }
    }
    out
}

// --- État d'un propriétaire de table ----------------------------------------------------------------------------------

/// État d'un propriétaire de table (la table commune, ou une équipe), pour un type.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamState {
    /// Multiplicateur cumulé du poids, par classe (72) ; 1 au départ.
    pub mult: Vec<f64>,
    /// γ courant par paramètre (72 à 72 cases, 11 en effets principaux) ; γ de la chaîne au départ.
    pub gamma: Vec<f64>,
    /// Dernier signe non nul de la correction, par paramètre : −1, 0, +1.
    pub sign: Vec<i8>,
}

impl ParamState {
    pub fn new(dims: Dims, gamma0: f64) -> Self {
        let n = match dims {
            Dims::D72 => TASTE_CLASSES,
            Dims::Me => N_ME,
        };
        Self { mult: vec![1.0; TASTE_CLASSES], gamma: vec![gamma0; n], sign: vec![0; n] }
    }

    /// Les `q8` cumulées : `round(256 × mult)` borné à `[1, 65 535]`.
    pub fn q8(&self) -> Vec<u16> {
        self.mult.iter().map(|m| (256.0 * m).round().clamp(1.0, 65_535.0) as u16).collect()
    }

    /// Le signe et l'amortissement du paramètre `k` pour une correction `u` (logarithme) : γ est divisé par deux quand
    /// le signe s'inverse. Rend vrai si γ a été divisé.
    fn damp(&mut self, k: usize, u: f64) -> bool {
        let s: i8 = if u.abs() < DEAD_ZONE {
            0
        } else if u > 0.0 {
            1
        } else {
            -1
        };
        if s == 0 {
            return false;
        }
        let flipped = self.sign[k] != 0 && s != self.sign[k];
        if flipped {
            self.gamma[k] /= 2.0;
        }
        self.sign[k] = s;
        flipped
    }
}

/// Un pas à 72 cases (§13.2.2, étapes 2 et 3) sur `g` (le `g` de `ratio_table`). Rend le nombre de paramètres dont γ a
/// été divisé.
pub fn step72(ps: &mut ParamState, g: &[f64], bound: f64) -> usize {
    let mut halved = 0;
    for c in 0..TASTE_CLASSES {
        halved += usize::from(ps.damp(c, g[c].ln()));
        // Au pas 1 : `1.0 × g^(−γ)`, exactement les bits de `ratio_table`.
        ps.mult[c] = (ps.mult[c] * g[c].powf(-ps.gamma[c])).clamp(1.0 / bound, bound);
    }
    halved
}

// --- Effets principaux ------------------------------------------------------------------------------------------------

/// Voie, revêtement, balisé d'une classe de goût : `((voie × 3 + revêtement) × 2 + balisé) × 2 + destination`.
fn way_of(c: usize) -> usize {
    c / 12
}
fn surf_of(c: usize) -> usize {
    (c / 4) % 3
}
fn mark_of(c: usize) -> usize {
    (c / 2) % 2
}

/// L'indice (parmi 11) des trois paramètres d'une classe : voie, revêtement, balisé.
fn levels_of(c: usize) -> [usize; 3] {
    [way_of(c), 6 + surf_of(c), 9 + mark_of(c)]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MainEffects {
    pub a: [f64; 6],
    pub b: [f64; 3],
    pub beta: [f64; 2],
}

impl MainEffects {
    /// Les 11 paramètres à la suite : voies, revêtements, balisé.
    pub fn flat(&self) -> [f64; N_ME] {
        let mut v = [0.0; N_ME];
        v[..6].copy_from_slice(&self.a);
        v[6..9].copy_from_slice(&self.b);
        v[9..].copy_from_slice(&self.beta);
        v
    }

    /// `a[w] + b[s] + β[k]` de la classe.
    pub fn fitted(&self, c: usize) -> f64 {
        self.a[way_of(c)] + self.b[surf_of(c)] + self.beta[mark_of(c)]
    }
}

/// Moindres carrés pondérés de `ln_g` sur `a[w] + b[s] + β[k]`, par relaxation (Gauss-Seidel) : départ à 0, au plus
/// 200 balayages (`a`, puis `b`, puis `β`), arrêt quand la plus grande variation est sous 1e-12. Jauge : les marges
/// `b` et `β` sont centrées (pondérées par le poids de leur niveau), l'écart passant dans `a`, **pour les niveaux qui
/// ont du poids** (un niveau sans poids garde 0). Chaque coefficient est borné à `[−ln bound, ln bound]`.
pub fn fit_main_effects(ln_g: &[f64], w: &[f64], bound: f64) -> MainEffects {
    let mut x = [0.0f64; N_ME];
    let sizes = [6usize, 3, 2];
    let offs = [0usize, 6, 9];
    for _ in 0..200 {
        let mut delta = 0.0f64;
        for (m, (&n, &o)) in sizes.iter().zip(&offs).enumerate() {
            for l in 0..n {
                let (mut num, mut den) = (0.0, 0.0);
                for c in (0..TASTE_CLASSES).filter(|&c| levels_of(c)[m] == o + l) {
                    let others: f64 =
                        levels_of(c).iter().enumerate().filter(|&(j, _)| j != m).map(|(_, &i)| x[i]).sum();
                    num += w[c] * (ln_g[c] - others);
                    den += w[c];
                }
                let new = if den > 0.0 { num / den } else { 0.0 };
                delta = delta.max((new - x[o + l]).abs());
                x[o + l] = new;
            }
        }
        if delta < 1e-12 {
            break;
        }
    }
    // Poids de chaque niveau.
    let mut omega = [0.0f64; N_ME];
    for (c, &wc) in w.iter().enumerate().take(TASTE_CLASSES) {
        for i in levels_of(c) {
            omega[i] += wc;
        }
    }
    // Jauge : revêtement (6..9) et balisé (9..11) centrés, le centre passe dans les voies.
    for (o, n) in [(6usize, 3usize), (9, 2)] {
        let total: f64 = omega[o..o + n].iter().sum();
        if total <= 0.0 {
            continue;
        }
        let mean = (0..n).map(|l| omega[o + l] * x[o + l]).sum::<f64>() / total;
        for l in (0..n).filter(|&l| omega[o + l] > 0.0) {
            x[o + l] -= mean;
        }
        for l in (0..6).filter(|&l| omega[l] > 0.0) {
            x[l] += mean;
        }
    }
    let lim = bound.ln();
    for v in &mut x {
        *v = v.clamp(-lim, lim);
    }
    MainEffects { a: [x[0], x[1], x[2], x[3], x[4], x[5]], b: [x[6], x[7], x[8]], beta: [x[9], x[10]] }
}

/// Un pas en effets principaux (§13.2.3) sur les 11 coefficients `u`. Rend le nombre de paramètres dont γ a été divisé.
pub fn step_me(ps: &mut ParamState, u: &[f64; N_ME], bound: f64) -> usize {
    let mut halved = 0;
    for (k, &x) in u.iter().enumerate() {
        halved += usize::from(ps.damp(k, x));
    }
    for c in 0..TASTE_CLASSES {
        let e: f64 = levels_of(c).iter().map(|&i| ps.gamma[i] * u[i]).sum();
        ps.mult[c] = (ps.mult[c] * (-e).exp()).clamp(1.0 / bound, bound);
    }
    halved
}

// --- Évaluation d'une paire (réel, généré) ----------------------------------------------------------------------------

/// Ce qu'un pas lit d'un couple de vecteurs de mètres : le `g` de `ratio_table` et les parts.
struct Eval {
    g: Vec<f64>,
    pn: Vec<f64>,
    pd: Vec<f64>,
    num_km: f64,
}

impl Eval {
    fn of(real: &[f64], gen: &[f64], gamma: f64, p: &TasteParams) -> Self {
        let num_km = real.iter().sum::<f64>() / 1_000.0;
        let g = ratio_table(real, gen, num_km, gamma, p).1;
        Self { g, pn: shares(real), pd: shares(gen), num_km }
    }

    /// Les 11 coefficients des effets principaux, poids `p_n + p_d` (0 si `p_d = 0`).
    fn coefficients(&self, bound: f64) -> MainEffects {
        let ln_g: Vec<f64> = self.g.iter().map(|g| g.ln()).collect();
        let w: Vec<f64> = (0..TASTE_CLASSES)
            .map(|c| if self.pd[c] > 0.0 { self.pn[c] + self.pd[c] } else { 0.0 })
            .collect();
        fit_main_effects(&ln_g, &w, bound)
    }

    /// `Σ |p_n − p_d|`.
    fn l1_gap(&self) -> f64 {
        self.pn.iter().zip(&self.pd).map(|(a, b)| (a - b).abs()).sum()
    }
}

/// Somme d'un vecteur de 72 classes par niveau de paramètre (voies, revêtements, balisé).
fn level_sums(v: &[f64]) -> [f64; N_ME] {
    let mut s = [0.0; N_ME];
    for (c, x) in v.iter().enumerate() {
        for i in levels_of(c) {
            s[i] += x;
        }
    }
    s
}

/// Poids de l'équipe `G_T / (G_T + n0 · G_P / N_P)` ; 0 si le dénominateur n'est pas un nombre strictement positif.
fn team_weight(g_team: f64, g_pool: f64, n_pool: u32, n0: f64) -> f64 {
    let den = g_team + n0 * g_pool / f64::from(n_pool);
    if den > 0.0 && den.is_finite() {
        g_team / den
    } else {
        0.0
    }
}

/// Mélange d'un logarithme : `w = 0` rend **exactement** la valeur de l'ensemble (une équipe sans donnée reçoit les
/// bits de la table commune), `w = 1` celle de l'équipe.
fn blend(w: f64, team: f64, pool: f64) -> f64 {
    if w <= 0.0 {
        pool
    } else if w >= 1.0 {
        team
    } else {
        (w * team.ln() + (1.0 - w) * pool.ln()).exp()
    }
}

fn blend_lin(w: f64, team: f64, pool: f64) -> f64 {
    if w <= 0.0 {
        pool
    } else if w >= 1.0 {
        team
    } else {
        w * team + (1.0 - w) * pool
    }
}

// --- Diagnostics --------------------------------------------------------------------------------------------------------

/// Diagnostic d'un propriétaire, pour un type, à un pas (§13.2.5).
#[derive(Clone, Debug, Serialize)]
pub struct ChainOwnerStep {
    /// `général` (`rc`), `commun` (la cible de `rt`) ou la lettre d'équipe.
    pub owner: String,
    pub ty: String,
    pub loops: u32,
    pub pairs: u32,
    /// `rt` : `N_T / (N_T + n0)` ; `None` pour `général` et `commun`.
    pub weight: Option<f64>,
    /// `Σ_c |p_n − p_d|` des entrées du pas : l'écart que le pas corrige.
    pub l1_gap: f64,
    /// `Σ_c p_d[c] · mult[c]` : dérive d'échelle (question 6).
    pub mean_mult: f64,
    /// Paramètres dont γ a été divisé à ce pas.
    pub halved: usize,
    pub gamma_min: f64,
    /// Classes dont le multiplicateur cumulé est à `1 / B` ou `B`.
    pub at_bound: usize,
}

/// Ce qu'un pas rend.
#[derive(Debug)]
pub struct StepOut {
    /// `rc` : la `Taste` commune (`None` si neutre : elle n'est pas passée au moteur).
    pub general: Option<Taste>,
    /// `rt` : la `Taste` de chaque équipe jouée.
    pub team: BTreeMap<usize, Option<Taste>>,
    /// Les tables à sortir (jouées seulement : jamais la cible `commun` de `rt`), `q8` cumulées.
    pub tables: Vec<TasteTable>,
    pub diag: Vec<ChainOwnerStep>,
}

// --- L'état d'une chaîne ----------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Owner {
    General,
    Team(usize),
}

#[derive(Clone, Debug)]
pub struct ChainState {
    pub dims: Dims,
    pub gamma0: f64,
    pub owners: BTreeMap<Owner, [ParamState; 3]>,
}

impl ChainState {
    pub fn new(dims: Dims, gamma0: f64) -> Self {
        Self { dims, gamma0, owners: BTreeMap::new() }
    }

    fn states(&mut self, o: Owner) -> &mut [ParamState; 3] {
        let (dims, g0) = (self.dims, self.gamma0);
        self.owners.entry(o).or_insert_with(|| [0, 1, 2].map(|_| ParamState::new(dims, g0)))
    }

    /// Un pas sur un propriétaire et un type, depuis les coefficients ou `g` déjà prêts ; rend le diagnostic et la table.
    #[allow(clippy::too_many_arguments)]
    fn apply(
        &mut self,
        o: Owner,
        ty: usize,
        ev: &Eval,
        input: &Input,
        meters: (u32, u32),
        label: &str,
        source: &str,
        weight: Option<f64>,
        bound: f64,
    ) -> (ChainOwnerStep, TasteTable) {
        let dims = self.dims;
        let st = &mut self.states(o)[ty];
        let (halved, g_applied) = match (dims, input) {
            (Dims::D72, Input::G(g)) => (step72(st, g, bound), g.clone()),
            (Dims::Me, Input::Coef(me)) => {
                let h = step_me(st, &me.flat(), bound);
                (h, (0..TASTE_CLASSES).map(|c| me.fitted(c).exp()).collect())
            }
            _ => unreachable!("la forme de la chaîne et celle des entrées vont ensemble"),
        };
        let q8 = st.q8();
        let sd: f64 = ev.pd.iter().sum();
        let mean_mult =
            if sd > 0.0 { ev.pd.iter().zip(&st.mult).map(|(p, m)| p * m).sum::<f64>() / sd } else { 1.0 };
        let (lo, hi) = (1.0 / bound, bound);
        let step = ChainOwnerStep {
            owner: label.to_string(),
            ty: TYPES[ty].0.to_string(),
            loops: meters.0,
            pairs: meters.1,
            weight,
            l1_gap: ev.l1_gap(),
            mean_mult,
            halved,
            gamma_min: st.gamma.iter().copied().fold(f64::INFINITY, f64::min),
            at_bound: st.mult.iter().filter(|&&m| m <= lo || m >= hi).count(),
        };
        let table = TasteTable {
            source: source.to_string(),
            overlay: "full".to_string(),
            ride: TYPES[ty].0.to_string(),
            num_km: ev.num_km,
            neutral: q8.iter().all(|&q| q == 256),
            pn: ev.pn.clone(),
            pd: ev.pd.clone(),
            g: g_applied,
            q8,
        };
        (step, table)
    }

    /// Un pas : `rc` met à jour `General` depuis `m.pooled` ; `rt` met à jour chaque équipe de `order` (et la cible
    /// `commun`, qui n'est jamais jouée : elle figure dans `diag`, jamais dans `tables`).
    pub fn step(
        &mut self,
        src: TasteSource,
        m: &ClassMeters,
        order: &[usize],
        letter_of: &[String],
        p: &TasteParams,
    ) -> StepOut {
        let bound = p.mult_bound;
        let g0 = self.gamma0;
        let dims = self.dims;
        let me_source = if dims == Dims::Me { "recalé-me" } else { "recalé" };
        let mut out = StepOut { general: None, team: BTreeMap::new(), tables: Vec::new(), diag: Vec::new() };
        let input_of = |ev: &Eval| match dims {
            Dims::D72 => Input::G(ev.g.clone()),
            Dims::Me => Input::Coef(ev.coefficients(p.bound)),
        };
        match src {
            TasteSource::Recal => {
                let mut taste = Taste::neutral();
                for ty in 0..3 {
                    let ev = Eval::of(&m.pooled.real[ty], &m.pooled.gen[ty], g0, p);
                    let inp = input_of(&ev);
                    let (d, t) = self.apply(
                        Owner::General,
                        ty,
                        &ev,
                        &inp,
                        (m.pooled.loops[ty], m.pooled.pairs[ty]),
                        "général",
                        me_source,
                        None,
                        bound,
                    );
                    taste.q8[ty].copy_from_slice(&t.q8);
                    out.diag.push(d);
                    out.tables.push(t);
                }
                out.general = (!taste.is_neutral()).then_some(taste);
            }
            TasteSource::RecalTeam => {
                let p_team = TasteParams { min_km: 0.0, ..p.clone() };
                let n0 = p.team_n0;
                // La cible commune : une fois par type, puis l'état de `General` (non joué).
                let eval_p: Vec<Eval> =
                    (0..3).map(|ty| Eval::of(&m.pooled.real[ty], &m.pooled.gen[ty], g0, p)).collect();
                for (ty, ev) in eval_p.iter().enumerate() {
                    let inp = input_of(ev);
                    let (d, _) = self.apply(
                        Owner::General,
                        ty,
                        ev,
                        &inp,
                        (m.pooled.loops[ty], m.pooled.pairs[ty]),
                        "commun",
                        "commun",
                        None,
                        bound,
                    );
                    out.diag.push(d);
                }
                for &t in order {
                    let mt = &m.team[t];
                    let mut taste = Taste::neutral();
                    for (ty, ev_p) in eval_p.iter().enumerate() {
                        let ev_t = Eval::of(&mt.real[ty], &mt.gen[ty], g0, &p_team);
                        let n_p = m.pooled.loops[ty];
                        let n_t = mt.loops[ty];
                        let weight = f64::from(n_t) / (f64::from(n_t) + n0);
                        let inp = match dims {
                            Dims::D72 => {
                                let g: Vec<f64> = (0..TASTE_CLASSES)
                                    .map(|c| {
                                        let w = team_weight(mt.gen[ty][c], m.pooled.gen[ty][c], n_p, n0);
                                        blend(w, ev_t.g[c], ev_p.g[c])
                                    })
                                    .collect();
                                Input::G(g)
                            }
                            Dims::Me => {
                                let (ct, cp) = (ev_t.coefficients(p.bound), ev_p.coefficients(p.bound));
                                let (gt, gp) = (level_sums(&mt.gen[ty]), level_sums(&m.pooled.gen[ty]));
                                let (xt, xp) = (ct.flat(), cp.flat());
                                let mut x = [0.0; N_ME];
                                for k in 0..N_ME {
                                    x[k] = blend_lin(team_weight(gt[k], gp[k], n_p, n0), xt[k], xp[k]);
                                }
                                Input::Coef(MainEffects {
                                    a: [x[0], x[1], x[2], x[3], x[4], x[5]],
                                    b: [x[6], x[7], x[8]],
                                    beta: [x[9], x[10]],
                                })
                            }
                        };
                        let (d, tb) = self.apply(
                            Owner::Team(t),
                            ty,
                            &ev_t,
                            &inp,
                            (n_t, mt.pairs[ty]),
                            &letter_of[t],
                            &letter_of[t],
                            Some(weight),
                            bound,
                        );
                        taste.q8[ty].copy_from_slice(&tb.q8);
                        out.diag.push(d);
                        out.tables.push(tb);
                    }
                    out.team.insert(t, (!taste.is_neutral()).then_some(taste));
                }
            }
            TasteSource::Team | TasteSource::General => {
                unreachable!("`ge` et `gg` ne sont pas des chaînes")
            }
        }
        out
    }
}

/// Ce que `apply` lit : le `g` de 72 cases, ou les coefficients des effets principaux.
enum Input {
    G(Vec<f64>),
    Coef(MainEffects),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> TasteParams {
        TasteParams::default()
    }

    /// Un générateur congruentiel : des vecteurs de mètres reproductibles.
    fn lcg(seed: u64, n: usize, lo: f64, hi: f64) -> Vec<f64> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                lo + (hi - lo) * ((x >> 11) as f64 / (1u64 << 53) as f64)
            })
            .collect()
    }

    fn near(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() < tol, "{a} != {b}");
    }

    fn meters(real: &[f64], gen: &[f64], loops: u32) -> Meters {
        let mut m = Meters::default();
        m.real[0] = real.to_vec();
        m.gen[0] = gen.to_vec();
        m.loops[0] = loops;
        m.pairs[0] = 2 * loops;
        m
    }

    fn one_team(real: &[f64], gen: &[f64]) -> ClassMeters {
        let m = meters(real, gen, 100);
        ClassMeters { pooled: m.clone(), team: vec![m] }
    }

    #[test]
    fn the_first_step_is_ratio_table_bit_for_bit() {
        // Trois jeux de vecteurs, γ ∈ {0,5 ; 1} (et 1,5 avec une borne cumulée de 10).
        for (seed, gamma, bound) in
            [(1, 0.5, 4.0), (2, 0.5, 4.0), (3, 1.0, 4.0), (4, 1.0, 4.0), (5, 1.5, 10.0)]
        {
            let real = lcg(seed, 72, 100.0, 9_000.0);
            let gen = lcg(seed + 100, 72, 100.0, 9_000.0);
            let params = TasteParams { mult_bound: bound, ..p() };
            let m = one_team(&real, &gen);
            let mut st = ChainState::new(Dims::D72, gamma);
            let out = st.step(TasteSource::Recal, &m, &[0], &["A".to_string()], &params);
            let num = [real.clone(), vec![0.0; 72], vec![0.0; 72]];
            let den = [gen.clone(), vec![0.0; 72], vec![0.0; 72]];
            let (taste, tables) =
                super::super::taste::learn("recalé", "full", &num, &den, 1_000.0, gamma, &params);
            let taste = taste.expect("table non neutre");
            assert_eq!(out.general.expect("goût").q8[0], taste.q8[0], "seed {seed}, γ {gamma}");
            assert_eq!(out.tables.len(), 3);
            assert_eq!(out.tables[0].q8, tables[0].q8);
            assert_eq!(out.tables[0].g, tables[0].g);
            assert_eq!(out.tables[0].pn, tables[0].pn);
            assert!(out.tables[1].neutral && out.tables[2].neutral);
        }
    }

    #[test]
    fn real_equal_to_generated_moves_nothing() {
        let v = lcg(7, 72, 100.0, 9_000.0);
        let mut st = ChainState::new(Dims::D72, 0.5);
        let m = one_team(&v, &v);
        let out = st.step(TasteSource::Recal, &m, &[0], &["A".to_string()], &p());
        let ps = &st.owners[&Owner::General][0];
        for &x in &ps.mult {
            near(x, 1.0, 1e-12);
        }
        assert!(ps.gamma.iter().all(|&g| g == 0.5) && ps.sign.iter().all(|&s| s == 0));
        assert!(out.general.is_none() || out.tables[0].q8.iter().all(|&q| q == 256));
        assert_eq!(out.diag[0].halved, 0);
        // Directement sur `g = 1` : rien ne bouge, au bit près.
        let mut ps = ParamState::new(Dims::D72, 0.5);
        assert_eq!(step72(&mut ps, &[1.0; 72], 4.0), 0);
        assert_eq!(ps, ParamState::new(Dims::D72, 0.5));
    }

    #[test]
    fn a_sign_change_halves_gamma_of_that_class_alone() {
        let mut ps = ParamState::new(Dims::D72, 0.5);
        let mut g = vec![1.0; 72];
        g[3] = 1.5;
        g[4] = 1.5;
        assert_eq!(step72(&mut ps, &g, 4.0), 0);
        assert_eq!((ps.sign[3], ps.sign[4], ps.sign[5]), (1, 1, 0));
        // Pas 2 : la classe 3 change de signe, la 4 garde le sien, la 5 (jamais corrigée) apparaît.
        g[3] = 0.6;
        g[5] = 0.5;
        assert_eq!(step72(&mut ps, &g, 4.0), 1);
        assert_eq!(ps.gamma[3], 0.25);
        assert_eq!(ps.gamma[4], 0.5);
        assert_eq!(ps.gamma[5], 0.5, "un premier signe n'est pas un changement");
        assert_eq!((ps.sign[3], ps.sign[5]), (-1, -1));
        near(ps.mult[3], 1.5f64.powf(-0.5) * 0.6f64.powf(-0.25), 1e-15);
        // Une correction sous 0,02 (en logarithme) ne compte pas comme changement de signe, ni ne change le signe retenu.
        g[4] = 0.99;
        assert_eq!(step72(&mut ps, &g, 4.0), 0);
        assert_eq!((ps.gamma[4], ps.sign[4]), (0.5, 1));
        // Au-delà de la zone, le même signe inversé divise bien.
        g[4] = 0.9;
        assert_eq!(step72(&mut ps, &g, 4.0), 1);
        assert_eq!(ps.gamma[4], 0.25);
    }

    #[test]
    fn repeated_extreme_inputs_stay_inside_the_cumulative_bound() {
        // Une classe qui prend tout, une qui ne prend rien, cinq fois de suite : g = 3 et 1/3 à chaque pas.
        let mut real = vec![1.0; 72];
        let mut gen = vec![1_000.0; 72];
        real[10] = 1.0e9;
        gen[10] = 1.0;
        real[11] = 0.0;
        let m = one_team(&real, &gen);
        let mut st = ChainState::new(Dims::D72, 1.0);
        let mut last = Vec::new();
        for _ in 0..5 {
            last = st.step(TasteSource::Recal, &m, &[0], &["A".to_string()], &p()).diag;
        }
        let ps = &st.owners[&Owner::General][0];
        assert!(ps.mult.iter().all(|&x| (0.25..=4.0).contains(&x)), "{:?}", ps.mult);
        assert_eq!(ps.mult[10], 0.25);
        assert_eq!(ps.mult[11], 4.0);
        assert!(last[0].at_bound >= 2);
        assert_eq!(ps.q8()[10], 64);
        assert_eq!(ps.q8()[11], 1_024);
    }

    /// Le générateur jouet : `gen[c] ∝ avail[c] · mult[c]^(−ε[c])`.
    fn toy_gen(avail: &[f64], mult: &[f64], eps: &[f64]) -> Vec<f64> {
        let raw: Vec<f64> = (0..72).map(|c| avail[c] * mult[c].powf(-eps[c])).collect();
        let s: f64 = raw.iter().sum();
        raw.iter().map(|x| 1_000_000.0 * x / s).collect()
    }

    fn l1(a: &[f64], b: &[f64]) -> f64 {
        let (sa, sb): (f64, f64) = (a.iter().sum(), b.iter().sum());
        a.iter().zip(b).map(|(x, y)| (x / sa - y / sb).abs()).sum()
    }

    #[test]
    fn the_toy_generator_converges_and_the_elastic_class_is_damped() {
        let avail = vec![1.0; 72];
        let mut eps = vec![1.0; 72];
        eps[0] = 3.2;
        let mut real = vec![0.0; 72];
        for (c, x) in real.iter_mut().enumerate() {
            *x = if c == 0 { 50_000.0 } else { 950_000.0 / 71.0 };
        }
        let mut st = ChainState::new(Dims::D72, 0.5);
        let mut gen = toy_gen(&avail, &[1.0; 72], &eps);
        let gap0 = l1(&real, &gen);
        let mut gammas = Vec::new();
        let mut gaps = vec![gap0];
        for _ in 0..3 {
            let m = one_team(&real, &gen);
            st.step(TasteSource::Recal, &m, &[0], &["A".to_string()], &p());
            let ps = &st.owners[&Owner::General][0];
            gammas.push((ps.gamma[0], ps.gamma[1]));
            gen = toy_gen(&avail, &ps.mult, &eps);
            gaps.push(l1(&real, &gen));
        }
        // Le pas 1 dépasse la cible (gain 1,6 sur la classe élastique), le pas 2 change de signe : γ divisé par deux ;
        // la classe de pente 1 ne s'inverse jamais.
        assert_eq!(gammas[0], (0.5, 0.5));
        assert_eq!(gammas[1].0, 0.25, "{gammas:?}");
        assert_eq!(gammas[1].1, 0.5);
        assert_eq!(gammas[2].1, 0.5);
        assert!(gaps[3] < 0.5 * gap0, "{gaps:?}");
    }

    #[test]
    fn a_team_without_data_gets_exactly_the_common_table() {
        // Deux équipes ; la seconde n'a rien du type 0 : elle reçoit les q8 de la table commune, au pas 1 et au suivant.
        let real = lcg(11, 72, 100.0, 9_000.0);
        let gen = lcg(12, 72, 100.0, 9_000.0);
        let mut pooled = meters(&real, &gen, 80);
        pooled.pairs[0] = 160;
        let mut a = pooled.clone();
        a.loops[0] = 80;
        let b = Meters::default();
        let m = ClassMeters { pooled, team: vec![a, b] };
        for dims in [Dims::D72, Dims::Me] {
            let mut st = ChainState::new(dims, 0.5);
            let letters = ["A".to_string(), "B".to_string()];
            for _ in 0..2 {
                let out = st.step(TasteSource::RecalTeam, &m, &[0, 1], &letters, &p());
                let common = st.owners[&Owner::General][0].q8();
                let b_table = out.tables.iter().find(|t| t.source == "B" && t.ride == "ROAD").unwrap();
                assert_eq!(b_table.q8, common, "{dims:?}");
                assert_eq!(st.owners[&Owner::Team(1)][0], st.owners[&Owner::General][0], "{dims:?}");
                // L'équipe de pleine donnée a elle aussi une table jouée.
                assert!(out.team.contains_key(&0));
            }
        }
    }

    #[test]
    fn a_team_at_the_common_pace_weighs_n_over_n_plus_n0_and_n0_zero_is_its_own_table() {
        // L'équipe génère chaque classe au même rythme par boucle que l'ensemble : w = N_T / (N_T + n0).
        let gen_pool = lcg(21, 72, 1_000.0, 9_000.0);
        let gen_team: Vec<f64> = gen_pool.iter().map(|x| x * 30.0 / 120.0).collect();
        let real_pool = lcg(22, 72, 1_000.0, 9_000.0);
        let real_team = lcg(23, 72, 1_000.0, 9_000.0);
        let mut pooled = meters(&real_pool, &gen_pool, 120);
        pooled.pairs[0] = 240;
        let mut team = meters(&real_team, &gen_team, 30);
        team.pairs[0] = 60;
        let m = ClassMeters { pooled, team: vec![team] };
        for c in [0usize, 17, 71] {
            let w = team_weight(gen_team[c], gen_pool[c], 120, 25.0);
            near(w, 30.0 / (30.0 + 25.0), 1e-12);
        }
        let mut st = ChainState::new(Dims::D72, 0.5);
        let out = st.step(TasteSource::RecalTeam, &m, &[0], &["A".to_string()], &p());
        let d = out.diag.iter().find(|d| d.owner == "A" && d.ty == "ROAD").unwrap();
        near(d.weight.unwrap(), 30.0 / 55.0, 1e-12);
        // Le mélange sur une classe : exp(w ln g_T + (1 − w) ln g_P) avec les g de ratio_table.
        let gt = ratio_table(
            &real_team,
            &gen_team,
            real_team.iter().sum::<f64>() / 1_000.0,
            0.5,
            &TasteParams { min_km: 0.0, ..p() },
        )
        .1;
        let gp = ratio_table(&real_pool, &gen_pool, real_pool.iter().sum::<f64>() / 1_000.0, 0.5, &p()).1;
        let w = 30.0 / 55.0;
        let want = (w * gt[5].ln() + (1.0 - w) * gp[5].ln()).exp();
        assert_eq!(out.tables[0].g[5].to_bits(), want.to_bits());
        // n0 = 0 : sa propre table (toutes les classes sont générées).
        let own = TasteParams { team_n0: 0.0, ..p() };
        let mut st = ChainState::new(Dims::D72, 0.5);
        let out = st.step(TasteSource::RecalTeam, &m, &[0], &["A".to_string()], &own);
        let mut alone = ChainState::new(Dims::D72, 0.5);
        let m_alone = ClassMeters { pooled: m.team[0].clone(), team: vec![m.team[0].clone()] };
        let own_min = TasteParams { min_km: 0.0, ..own.clone() };
        let solo = alone.step(TasteSource::Recal, &m_alone, &[0], &["A".to_string()], &own_min);
        assert_eq!(out.tables[0].q8, solo.tables[0].q8);
    }

    fn additive(a: [f64; 6], b: [f64; 3], beta: [f64; 2]) -> Vec<f64> {
        (0..72).map(|c| a[c / 12] + b[(c / 4) % 3] + beta[(c / 2) % 2]).collect()
    }

    #[test]
    fn main_effects_recover_an_exactly_additive_target() {
        let (a, b, beta) = ([0.2, -0.1, 0.05, 0.3, -0.25, 0.0], [0.1, -0.2, 0.15], [-0.05, 0.12]);
        let target = additive(a, b, beta);
        let w = lcg(31, 72, 0.1, 2.0);
        let fit = fit_main_effects(&target, &w, 3.0);
        for (c, t) in target.iter().enumerate() {
            near(fit.fitted(c), *t, 1e-9);
        }
        // Après jauge (poids uniformes sur les niveaux : marges centrées).
        let flat = fit.flat();
        assert_eq!(flat.len(), 11);
        // La destination est ignorée : deux classes qui ne diffèrent que par ce bit ont le même ajusté.
        for c in (0..72).step_by(2) {
            assert_eq!(fit.fitted(c).to_bits(), fit.fitted(c + 1).to_bits());
        }
    }

    #[test]
    fn main_effects_gauge_empty_levels_and_bounds() {
        let t = additive([0.5, 0.3, -0.2, 0.1, 0.0, 0.4], [0.3, -0.1, 0.2], [0.2, -0.2]);
        let w = lcg(41, 72, 0.1, 2.0);
        let fit = fit_main_effects(&t, &w, 3.0);
        // (d) Après jauge, Σ Ω_s b[s] = 0 et Σ Ω_k β[k] = 0.
        let mut omega = [0.0; 11];
        for (c, &wc) in w.iter().enumerate() {
            for i in levels_of(c) {
                omega[i] += wc;
            }
        }
        let flat = fit.flat();
        near((6..9).map(|i| omega[i] * flat[i]).sum::<f64>(), 0.0, 1e-9);
        near((9..11).map(|i| omega[i] * flat[i]).sum::<f64>(), 0.0, 1e-9);
        // (c) Une voie sans poids garde 0, et ne gêne pas les autres.
        let mut w2 = w.clone();
        for (c, x) in w2.iter_mut().enumerate() {
            if c / 12 == 2 {
                *x = 0.0;
            }
        }
        let fit2 = fit_main_effects(&t, &w2, 3.0);
        assert_eq!(fit2.a[2], 0.0);
        for c in (0..72).filter(|c| c / 12 != 2) {
            near(fit2.fitted(c), t[c], 1e-9);
        }
        // Un niveau de revêtement sans poids garde 0 aussi.
        let mut w3 = w.clone();
        for (c, x) in w3.iter_mut().enumerate() {
            if (c / 4) % 3 == 1 {
                *x = 0.0;
            }
        }
        assert_eq!(fit_main_effects(&t, &w3, 3.0).b[1], 0.0);
        // Aucun poids : tout à 0.
        assert_eq!(fit_main_effects(&t, &[0.0; 72], 3.0).flat(), [0.0; 11]);
        // (e) Bornés à ln bound.
        let big = additive([5.0, -5.0, 0.0, 0.0, 0.0, 0.0], [0.0; 3], [0.0; 2]);
        let fit = fit_main_effects(&big, &w, 3.0);
        assert!(fit.flat().iter().all(|x| x.abs() <= 3.0f64.ln() + 1e-15));
        near(fit.a[0], 3.0f64.ln(), 1e-12);
        near(fit.a[1], -(3.0f64.ln()), 1e-12);
    }

    #[test]
    fn main_effects_step_gives_the_same_multiplier_to_the_two_destination_classes() {
        let real = lcg(51, 72, 100.0, 9_000.0);
        let gen = lcg(52, 72, 100.0, 9_000.0);
        let m = one_team(&real, &gen);
        let mut st = ChainState::new(Dims::Me, 0.5);
        let out = st.step(TasteSource::Recal, &m, &[0], &["A".to_string()], &p());
        let ps = &st.owners[&Owner::General][0];
        assert_eq!(ps.gamma.len(), 11);
        for c in (0..72).step_by(2) {
            assert_eq!(ps.mult[c].to_bits(), ps.mult[c + 1].to_bits(), "classe {c}");
        }
        assert_eq!(out.tables[0].source, "recalé-me");
        // Pas 1 ≠ rc05 : une autre table.
        let mut st72 = ChainState::new(Dims::D72, 0.5);
        let o72 = st72.step(TasteSource::Recal, &m, &[0], &["A".to_string()], &p());
        assert_ne!(out.tables[0].q8, o72.tables[0].q8);
        // Un niveau qui change de signe divise γ de ce paramètre seul.
        let mut ps = ParamState::new(Dims::Me, 0.5);
        let mut u = [0.0; 11];
        u[1] = 0.3;
        u[7] = 0.3;
        assert_eq!(step_me(&mut ps, &u, 4.0), 0);
        u[1] = -0.3;
        assert_eq!(step_me(&mut ps, &u, 4.0), 1);
        assert_eq!((ps.gamma[1], ps.gamma[7], ps.gamma[0]), (0.25, 0.5, 0.5));
    }

    /// L'ancien `RcInputs::of` du §12, gardé ici comme référence : `pooled` doit lui être égal au bit près.
    fn rc_inputs_reference(
        keys: &[(usize, u8)],
        held: &std::collections::HashSet<usize>,
        real_class_m: &[[f64; TASTE_CLASSES]],
        plays: &[Vec<PlayRec>],
    ) -> ([Vec<f64>; 3], [Vec<f64>; 3]) {
        let z = || vec![0.0; TASTE_CLASSES];
        let (mut num, mut den) = ([z(), z(), z()], [z(), z(), z()]);
        for (i, &(_, ty)) in keys.iter().enumerate().filter(|(i, _)| !held.contains(i)) {
            let ty = usize::from(ty).min(2);
            for p in plays[i].iter().filter(|p| p.overlay == "full" && p.outcome == "OK") {
                let Some(fc) = &p.first_class_m else { continue };
                for c in 0..TASTE_CLASSES {
                    num[ty][c] += real_class_m[i][c];
                    den[ty][c] += fc[c];
                }
            }
        }
        (num, den)
    }

    fn play(cursor: f64, overlay: &str, outcome: &str, fc: Option<Vec<f64>>) -> PlayRec {
        PlayRec {
            cursor,
            overlay: overlay.to_string(),
            outcome: outcome.to_string(),
            n_routes: 1,
            n_kept: 1,
            dropped: Default::default(),
            ms: 1.0,
            routes: Vec::new(),
            best_shape: None,
            best_recovered: None,
            first_class_m: fc,
        }
    }

    #[test]
    fn class_meters_match_the_old_rc_inputs_bit_for_bit_and_skip_what_must_be_skipped() {
        // 6 boucles : équipes 0, 1, 0, 2, 1, 0 ; types ROAD, GRAVEL, ROAD, ROAD, GRAVEL, MTB (3 → 2 par `min`).
        let keys: Vec<(usize, u8)> = vec![(0, 0), (1, 1), (0, 0), (2, 0), (1, 1), (0, 3)];
        let held: std::collections::HashSet<usize> = [2].into_iter().collect();
        let real: Vec<[f64; TASTE_CLASSES]> = (0..6)
            .map(|i| {
                let v = lcg(60 + i, TASTE_CLASSES, 0.0, 5_000.0);
                let mut a = [0.0; TASTE_CLASSES];
                a.copy_from_slice(&v);
                a
            })
            .collect();
        let fc = |seed: u64| Some(lcg(seed, TASTE_CLASSES, 0.0, 5_000.0));
        let plays: Vec<Vec<PlayRec>> = vec![
            vec![play(0.25, "full", "OK", fc(1)), play(1.0, "full", "OK", fc(2))],
            vec![play(0.25, "full", "OK", fc(3)), play(1.0, "full", "NO_ROUTES", None)],
            // La boucle mise à l'écart : jamais comptée, quels que soient ses jeux.
            vec![play(0.25, "full", "OK", fc(4)), play(0.25, "holdout", "OK", fc(5))],
            // Un jeu « écart » n'y compte pas, un jeu `OK` sans classes non plus.
            vec![play(0.25, "holdout", "OK", fc(6)), play(1.0, "full", "OK", None)],
            vec![play(0.25, "full", "OK", fc(7)), play(1.0, "full", "OK", fc(8))],
            vec![play(0.25, "full", "OK", fc(9))],
        ];
        let m = ClassMeters::of_keys(&keys, &held, &real, &plays, 3);
        let (num, den) = rc_inputs_reference(&keys, &held, &real, &plays);
        for ty in 0..3 {
            for c in 0..TASTE_CLASSES {
                assert_eq!(m.pooled.real[ty][c].to_bits(), num[ty][c].to_bits());
                assert_eq!(m.pooled.gen[ty][c].to_bits(), den[ty][c].to_bits());
            }
        }
        // La somme des équipes égale `pooled` à 1e-9 relatif.
        for ty in 0..3 {
            for c in 0..TASTE_CLASSES {
                let sr: f64 = m.team.iter().map(|t| t.real[ty][c]).sum();
                let sg: f64 = m.team.iter().map(|t| t.gen[ty][c]).sum();
                near(sr, m.pooled.real[ty][c], 1e-9 * m.pooled.real[ty][c].max(1.0));
                near(sg, m.pooled.gen[ty][c], 1e-9 * m.pooled.gen[ty][c].max(1.0));
            }
        }
        // Boucles et paires : la boucle 1 n'a qu'un jeu compté ; la 2 (écart) et la 3 (jeu « écart », jeu sans classes)
        // n'en ont aucun.
        assert_eq!(m.pooled.loops, [1, 2, 1]);
        assert_eq!(m.pooled.pairs, [2, 3, 1]);
        assert_eq!(m.team[0].loops, [1, 0, 1]);
        assert_eq!(m.team[1].pairs, [0, 3, 0]);
        assert_eq!(m.team[2].loops, [0, 0, 0]);
        assert!(m.team[2].real[0].iter().all(|&x| x == 0.0));
        // Les écarts L1 : un propriétaire par type ayant des paires.
        let gaps = l1_gaps(&m, &[0, 1], &["A".to_string(), "B".to_string(), "C".to_string()], "général");
        assert_eq!(gaps.iter().filter(|g| g.owner == "général").count(), 3);
        assert_eq!(gaps.iter().filter(|g| g.owner == "A").count(), 2);
        assert!(gaps.iter().all(|g| (0.0..=2.0).contains(&g.l1)));
        // Parts égales : écart nul.
        let eq = one_team(&[1.0; 72], &[3.0; 72]);
        assert!(l1_gaps(&eq, &[0], &["A".to_string()], "commun")[0].l1.abs() < 1e-12);
    }
}
