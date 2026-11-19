//! L'apprentissage d'un profil de goût (§14.5) : trois pas de recalé itéré sur les boucles retenues d'une équipe, la
//! table commune figée comme point de départ et comme repli.
//!
//! Tout en `f64` dans un ordre de sommes fixé (séquentiel) ; seules les `q8` entières entrent dans le moteur. Le
//! parallélisme est dans le joueur, à résultats indexés : même binaire, même machine, mêmes octets, quel que soit le
//! nombre de fils. Aucune coordonnée ni empreinte de boucle n'en sort : des mètres et des parts par classe.

use ord_loops::{Figures, Mode, Organic, OrganicTune, Request, Taste, TASTE_CLASSES};
use xxhash_rust::xxh3::xxh3_64;

use super::collect::{loop_request, Cap, LearnSet, RetainParams};
use super::shape::mix_for;
use super::Player;

/// Forme du mélange d'une équipe avec la table commune (décision 2 du propriétaire, 17 novembre 2026).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mix {
    /// Le 3b tel que codé : l'équipe **part de la table commune `C`**, et corrige par `ĝ = g_T^w` avec
    /// `w = min(G_T / (G_T + n0 · R), N / (N + n0))` (la cible commune est le point fixe, `g_P = 1`).
    #[default]
    Capped,
    /// Le mélange de la seconde sonde (forme de `rt`, §13.2.4) : l'équipe part de la table **neutre** et, à chaque pas,
    /// `ln ĝ = w · ln g_T + (1 − w) · ln g_P` avec le `g_P` des mètres communs **de ce pas** (`Common::steps`) et le poids
    /// nu `w = G_T / (G_T + n0 · G_P / N_P)`, sans plafond `N / (N + n0)`.
    Sonde,
}

impl Mix {
    pub fn label(self) -> &'static str {
        match self {
            Self::Capped => "capped",
            Self::Sonde => "sonde",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "capped" => Some(Self::Capped),
            "sonde" => Some(Self::Sonde),
            _ => None,
        }
    }
}

/// Les figures que joue l'apprentissage d'un profil (lot 4, 4c, §15.9). `Classic` : la lentille seule, comme 1.10 ;
/// `Organic` : les figures organiques de la requête, avec le mélange de l'équipe (ses formes, sinon le général).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LearnFigures {
    #[default]
    Classic,
    Organic,
}

impl LearnFigures {
    pub fn label(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Organic => "organic",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "classic" => Some(Self::Classic),
            "organic" => Some(Self::Organic),
            _ => None,
        }
    }

    /// Les figures d'une requête de l'apprentissage, pour le type `ty` de l'équipe aux formes `set`.
    pub fn figures_of(self, set: &LearnSet, ty: u8) -> Figures {
        match self {
            Self::Classic => Figures::Classic,
            Self::Organic => Figures::Organic(Organic {
                mix: mix_for(Some(&set.shapes), super::collect::ride_of(ty)),
                tune: OrganicTune::default(),
                // L′ (§17.9) : l'apprentissage ne lit pas les attracteurs (voir `LearnParams::attractors`).
                attr: ord_loops::AttrWeights::NONE,
            }),
        }
    }
}

/// Seuil de kilomètres du réel sous lequel la cible commune d'un pas est neutre : le `min_km` des `TasteParams` du banc
/// (`rt` calcule `g_P` avec lui, `g_T` sans).
pub const SONDE_POOL_MIN_KM: f64 = 200.0;

/// Réglages de l'apprentissage. Leur empreinte ([`LearnParams::hash`]) est écrite dans le profil : un profil appris
/// autrement n'est pas servi.
#[derive(Clone, Debug, PartialEq)]
pub struct LearnParams {
    /// γ initial de chaque classe.
    pub gamma: f64,
    /// Nombre de pas.
    pub steps: u8,
    /// Lissage `m` du rapport réel / généré.
    pub m: f64,
    /// `g` borné à `[1 / step_bound, step_bound]` à chaque pas.
    pub step_bound: f64,
    /// Multiplicateur cumulé borné à `[1 / mult_bound, mult_bound]`.
    pub mult_bound: f64,
    /// Seuil de repli : une équipe pèse `N / (N + n0)` au plus.
    pub n0: f64,
    /// Sous ce `|ln ĝ|`, la correction n'a pas de signe.
    pub sign_eps: f64,
    /// Table normalisée (moyenne pondérée ramenée à 1) : elle va avec la table commune servie.
    pub normalize: bool,
    pub cap: Cap,
    /// Valeurs du curseur jouées, en centièmes (`cursor::hundredths`).
    pub cursors: [u32; 2],
    pub seed: u64,
    pub retain: RetainParams,
    /// Forme du mélange avec la table commune. Hors de l'empreinte tant qu'elle vaut `Capped` (celle d'avant reste la
    /// même), dedans dès `Sonde`.
    pub mix: Mix,
    /// Figures jouées pour apprendre (lot 4, 4c). Hors de l'empreinte tant qu'elles valent `Classic` : les profils écrits
    /// avant le 4c restent compatibles ; `Organic` entre dans l'empreinte (`;figures=organic`) et invalide tous les profils.
    pub figures: LearnFigures,
}

impl Default for LearnParams {
    fn default() -> Self {
        Self {
            gamma: 0.5,
            steps: 3,
            m: 0.5,
            step_bound: 3.0,
            mult_bound: 4.0,
            n0: 25.0,
            sign_eps: 0.02,
            normalize: ord_loops::COMMON_USE_NORMALIZED,
            cap: Cap::Team(500),
            cursors: [25, 100],
            seed: 20_261_117,
            retain: RetainParams { max_km: Some(200.0), ..RetainParams::default() },
            mix: Mix::Capped,
            figures: LearnFigures::Classic,
        }
    }
}

impl LearnParams {
    /// Texte canonique de tous les champs.
    pub fn canonical(&self) -> String {
        let base = format!(
            "ordp/learn/v1;gamma={:?};steps={};m={:?};step_bound={:?};mult_bound={:?};n0={:?};sign_eps={:?};\
             normalize={};cap={};cursors={},{};seed={};retain={:?},{:?},{:?},{:?},{:?}",
            self.gamma,
            self.steps,
            self.m,
            self.step_bound,
            self.mult_bound,
            self.n0,
            self.sign_eps,
            self.normalize,
            self.cap.canonical(),
            self.cursors[0],
            self.cursors[1],
            self.seed,
            self.retain.min_matched,
            self.retain.max_hole_m,
            self.retain.min_km,
            self.retain.max_km,
            self.retain.loop_gap,
        );
        let base = match self.mix {
            Mix::Capped => base,
            Mix::Sonde => format!("{base};mix=sonde"),
        };
        match self.figures {
            LearnFigures::Classic => base,
            LearnFigures::Organic => format!("{base};figures=organic"),
        }
    }

    pub fn hash(&self) -> u64 {
        xxh3_64(self.canonical().as_bytes())
    }
}

/// Les mètres, toutes équipes réunies, que joue un pas de la chaîne commune (la cible du pas dans le mélange `Sonde`) :
/// par type, les mètres du réel et du généré par classe et les boucles comptées.
#[derive(Clone, Debug, PartialEq)]
pub struct StepMeters {
    pub real: [[f64; TASTE_CLASSES]; 3],
    pub gen: [[f64; TASTE_CLASSES]; 3],
    pub loops: [u32; 3],
}

impl Default for StepMeters {
    fn default() -> Self {
        Self { real: [[0.0; TASTE_CLASSES]; 3], gen: [[0.0; TASTE_CLASSES]; 3], loops: [0; 3] }
    }
}

/// La table commune figée : point de départ d'une équipe, et décimètres générés par boucle (le `R` du mélange).
#[derive(Clone, Debug, PartialEq)]
pub struct Common {
    pub q8: [[u16; TASTE_CLASSES]; 3],
    pub gen_dm_per_loop: [[u32; TASTE_CLASSES]; 3],
    /// Les mètres des trois pas de la chaîne commune (mélange `Sonde` seulement : sans effet sur le mélange `Capped`).
    pub steps: [StepMeters; 3],
}

impl Common {
    /// Une table commune sans mètres de pas (les tests, et tout ce qui n'a pas le mélange `Sonde`).
    pub fn without_steps(q8: [[u16; TASTE_CLASSES]; 3], gen_dm_per_loop: [[u32; TASTE_CLASSES]; 3]) -> Self {
        Self { q8, gen_dm_per_loop, steps: Default::default() }
    }

    /// Celle du moteur : brute ou normalisée.
    pub fn engine(normalized: bool) -> Self {
        let t = if normalized { Taste::common_normalized() } else { Taste::common_raw() };
        let (real, gen, loops) = (
            ord_loops::common_step_real_dm(),
            ord_loops::common_step_gen_dm(),
            ord_loops::common_step_loops(),
        );
        let steps = std::array::from_fn(|k| StepMeters {
            real: std::array::from_fn(|t| std::array::from_fn(|c| f64::from(real[k][t][c]) / 10.0)),
            gen: std::array::from_fn(|t| std::array::from_fn(|c| f64::from(gen[k][t][c]) / 10.0)),
            loops: loops[k],
        });
        Self { q8: t.q8, gen_dm_per_loop: *ord_loops::common_gen_dm_per_loop(), steps }
    }

    /// L'empreinte qu'un profil écrit pour la forme `mix` du mélange : `hash()` pour `Capped` (les mètres des pas n'y
    /// servent pas), et `hash()` suivi des mètres des pas, petit-boutiste, pour `Sonde`.
    pub fn hash_for(&self, mix: Mix) -> u64 {
        match mix {
            Mix::Capped => self.hash(),
            Mix::Sonde => {
                let mut b: Vec<u8> = self.hash().to_le_bytes().to_vec();
                for st in &self.steps {
                    for t in 0..3 {
                        for v in st.real[t].iter().chain(&st.gen[t]) {
                            b.extend_from_slice(&v.to_bits().to_le_bytes());
                        }
                        b.extend_from_slice(&st.loops[t].to_le_bytes());
                    }
                }
                xxh3_64(&b)
            }
        }
    }

    pub fn taste(&self) -> Taste {
        Taste { q8: self.q8 }
    }

    /// `xxh3` des `q8` puis des `R`, petit-boutiste.
    pub fn hash(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(3 * TASTE_CLASSES * 6);
        for t in &self.q8 {
            for q in t {
                b.extend_from_slice(&q.to_le_bytes());
            }
        }
        for t in &self.gen_dm_per_loop {
            for r in t {
                b.extend_from_slice(&r.to_le_bytes());
            }
        }
        xxh3_64(&b)
    }
}

/// Diagnostic d'un pas pour un type (agrégats seulement).
#[derive(Clone, Debug, PartialEq)]
pub struct StepDiag {
    /// De 1 à `steps`.
    pub step: u8,
    /// 0 ROAD, 1 GRAVEL, 2 MTB.
    pub ty: u8,
    pub loops: u32,
    pub pairs: u32,
    /// `N / (N + n0)`.
    pub weight: f64,
    /// `Σ_c |p_n − p_d|` des entrées du pas : l'écart que le pas corrige.
    pub l1_gap: f64,
    /// `Σ_c p_d[c] · mult[c]` : la dérive d'échelle.
    pub mean_mult: f64,
    /// Classes dont γ a été divisé à ce pas.
    pub halved: usize,
    pub gamma_min: f64,
    /// Classes dont le multiplicateur cumulé est à une borne.
    pub at_bound: usize,
}

/// Ce que l'apprentissage rend.
#[derive(Clone, Debug)]
pub struct Learned {
    pub taste: Taste,
    /// Pas finis ; 0 : la table commune (ensemble vide, ou arrêt avant la fin du premier pas).
    pub steps_done: u8,
    /// Boucles et paires comptées au dernier pas fini.
    pub loops: [u32; 3],
    pub pairs: [u32; 3],
    /// Requêtes jouées, tous pas finis ou non.
    pub plays: u64,
    pub diag: Vec<StepDiag>,
}

/// Mètres du réel et du généré par classe, pour un type.
struct Meters {
    real: [f64; TASTE_CLASSES],
    gen: [f64; TASTE_CLASSES],
    loops: u32,
    pairs: u32,
}

/// `g(c) = (p_n + m · p_d) / ((1 + m) · p_d)` borné à `[1 / bound, bound]` ; neutre (1 partout) si l'un des totaux n'est
/// pas strictement positif ; `g = 1` si `p_d = 0`. La `ratio_table` du banc à `min_km = 0`, même expression.
fn ratio(
    real: &[f64; TASTE_CLASSES],
    gen: &[f64; TASTE_CLASSES],
    m: f64,
    bound: f64,
) -> [f64; TASTE_CLASSES] {
    let (sn, sd): (f64, f64) = (real.iter().sum(), gen.iter().sum());
    let mut g = [1.0; TASTE_CLASSES];
    if sd <= 0.0 || sn <= 0.0 {
        return g;
    }
    for c in 0..TASTE_CLASSES {
        let (pn, pd) = (real[c] / sn, gen[c] / sd);
        g[c] = if pd > 0.0 { ((pn + m * pd) / ((1.0 + m) * pd)).clamp(1.0 / bound, bound) } else { 1.0 };
    }
    g
}

fn shares(v: &[f64; TASTE_CLASSES]) -> [f64; TASTE_CLASSES] {
    let s: f64 = v.iter().sum();
    let mut out = [0.0; TASTE_CLASSES];
    if s > 0.0 {
        for (o, x) in out.iter_mut().zip(v) {
            *o = x / s;
        }
    }
    out
}

/// Normalise une table : `M̄ = Σ w·q8 / Σ w`, `q8n = round(q8 × 256 / M̄)` borné à `[256 / bound, 256 × bound]` ;
/// inchangée si `Σ w = 0`. La normalisation de la table commune par l'outil d'export (poids `R`).
pub fn normalize_q8(
    q8: &[u16; TASTE_CLASSES],
    weights: &[f64; TASTE_CLASSES],
    bound: f64,
) -> [u16; TASTE_CLASSES] {
    let sw: f64 = weights.iter().sum();
    if sw <= 0.0 {
        return *q8;
    }
    let mean: f64 = weights.iter().zip(q8).map(|(w, &q)| w * f64::from(q)).sum::<f64>() / sw;
    let (lo, hi) = ((256.0 / bound).round(), (256.0 * bound).round());
    let mut out = [0u16; TASTE_CLASSES];
    for (o, &q) in out.iter_mut().zip(q8) {
        *o = (256.0 * f64::from(q) / mean).round().clamp(lo, hi) as u16;
    }
    out
}

/// L'état d'un type : multiplicateurs cumulés, γ par classe, dernier signe non nul.
struct TypeState {
    mult: [f64; TASTE_CLASSES],
    gamma: [f64; TASTE_CLASSES],
    sign: [i8; TASTE_CLASSES],
}

impl TypeState {
    fn new(common: &[u16; TASTE_CLASSES], gamma: f64) -> Self {
        let mut mult = [1.0; TASTE_CLASSES];
        for (m, &q) in mult.iter_mut().zip(common) {
            *m = f64::from(q) / 256.0;
        }
        Self { mult, gamma: [gamma; TASTE_CLASSES], sign: [0; TASTE_CLASSES] }
    }

    fn q8(&self) -> [u16; TASTE_CLASSES] {
        let mut out = [0u16; TASTE_CLASSES];
        for (o, m) in out.iter_mut().zip(&self.mult) {
            *o = (256.0 * m).round().clamp(1.0, 65_535.0) as u16;
        }
        out
    }
}

/// Poids de l'équipe du mélange `Sonde` : `G_T / (G_T + n0 · G_P / N_P)` ; 0 si le dénominateur n'est pas un nombre
/// strictement positif (`N_P = 0` compris). L'expression du `team_weight` du banc, même ordre d'opérations.
fn pool_weight(g_team: f64, g_pool: f64, n_pool: u32, n0: f64) -> f64 {
    let den = g_team + n0 * g_pool / f64::from(n_pool);
    if den > 0.0 && den.is_finite() {
        g_team / den
    } else {
        0.0
    }
}

/// Mélange d'un `g` du mélange `Sonde` : `w = 0` rend **exactement** celui de l'ensemble, `w = 1` celui de l'équipe.
/// Le `blend` du banc.
fn pool_blend(w: f64, team: f64, pool: f64) -> f64 {
    if w <= 0.0 {
        pool
    } else if w >= 1.0 {
        team
    } else {
        (w * team.ln() + (1.0 - w) * pool.ln()).exp()
    }
}

/// Un pas sur un type (§14.5.2, étape 4) : met à jour `st` et rend le diagnostic. `r_common` : `R` en mètres par boucle.
/// `pool` : pour le mélange `Sonde`, les mètres communs du pas et le type.
fn step_type(
    st: &mut TypeState,
    x: &Meters,
    r_common: &[f64; TASTE_CLASSES],
    pool: Option<(&StepMeters, usize)>,
    p: &LearnParams,
    step: u8,
    ty: u8,
) -> StepDiag {
    let n = f64::from(x.loops);
    let g = ratio(&x.real, &x.gen, p.m, p.step_bound);
    // La cible commune du pas : le `g` de ses mètres, neutre sous le seuil de kilomètres (le `ratio_table` du banc).
    let g_pool = pool.map(|(sm, t)| {
        if sm.real[t].iter().sum::<f64>() / 1_000.0 < SONDE_POOL_MIN_KM {
            [1.0; TASTE_CLASSES]
        } else {
            ratio(&sm.real[t], &sm.gen[t], p.m, p.step_bound)
        }
    });
    let mut halved = 0;
    for c in 0..TASTE_CLASSES {
        let g_hat = match (pool, &g_pool) {
            (Some((sm, t)), Some(gp)) => {
                pool_blend(pool_weight(x.gen[c], sm.gen[t][c], sm.loops[t], p.n0), g[c], gp[c])
            }
            _ => {
                // Poids de l'équipe : jamais plus que la part que ses boucles lui donnent.
                let w = if x.gen[c] <= 0.0 {
                    0.0
                } else {
                    (x.gen[c] / (x.gen[c] + p.n0 * r_common[c])).min(n / (n + p.n0))
                };
                if w <= 0.0 {
                    1.0
                } else if w >= 1.0 {
                    g[c]
                } else {
                    (w * g[c].ln()).exp()
                }
            }
        };
        let u = g_hat.ln();
        let s: i8 = if u.abs() < p.sign_eps {
            0
        } else if u > 0.0 {
            1
        } else {
            -1
        };
        if s != 0 {
            if st.sign[c] != 0 && s != st.sign[c] {
                st.gamma[c] /= 2.0;
                halved += 1;
            }
            st.sign[c] = s;
        }
        st.mult[c] = (st.mult[c] * g_hat.powf(-st.gamma[c])).clamp(1.0 / p.mult_bound, p.mult_bound);
    }
    if p.normalize {
        let sg: f64 = x.gen.iter().sum();
        if sg > 0.0 {
            let mean: f64 = x.gen.iter().zip(&st.mult).map(|(g, m)| g * m).sum::<f64>() / sg;
            for m in &mut st.mult {
                *m = (*m / mean).clamp(1.0 / p.mult_bound, p.mult_bound);
            }
        }
    }
    let (pn, pd) = (shares(&x.real), shares(&x.gen));
    let sd: f64 = pd.iter().sum();
    let (lo, hi) = (1.0 / p.mult_bound, p.mult_bound);
    StepDiag {
        step,
        ty,
        loops: x.loops,
        pairs: x.pairs,
        weight: n / (n + p.n0),
        l1_gap: pn.iter().zip(&pd).map(|(a, b)| (a - b).abs()).sum(),
        mean_mult: if sd > 0.0 { pd.iter().zip(&st.mult).map(|(a, m)| a * m).sum::<f64>() / sd } else { 1.0 },
        halved,
        gamma_min: st.gamma.iter().copied().fold(f64::INFINITY, f64::min),
        at_bound: st.mult.iter().filter(|&&m| m <= lo || m >= hi).count(),
    }
}

/// Apprend un profil. `cursors` : les valeurs du curseur déjà appliquées (`cursor::apply`), dans l'ordre de
/// `p.cursors`. Pas `k` (1 à `p.steps`) : jouer la table `T_{k-1}` (`T_0` = la table commune) sur toutes les requêtes,
/// sommer les mètres, corriger. Un type sans boucle garde la table commune au bit près. Partiel : si le joueur
/// s'arrête au pas `k`, le résultat est `T_{k-1}`.
pub fn learn(
    set: &LearnSet,
    cursors: &[(Mode, f32, u32)],
    player: &dyn Player,
    common: &Common,
    p: &LearnParams,
) -> Learned {
    let sonde = p.mix == Mix::Sonde;
    // `Capped` part de la table commune ; `Sonde` de la table neutre, comme `rt` (la cible commune entre à chaque pas).
    let mut states: [TypeState; 3] = std::array::from_fn(|t| {
        TypeState::new(if sonde { &[256; TASTE_CLASSES] } else { &common.q8[t] }, p.gamma)
    });
    let mut seen = [false; 3];
    let mut out = Learned {
        taste: common.taste(),
        steps_done: 0,
        loops: [0; 3],
        pairs: [0; 3],
        plays: 0,
        diag: Vec::new(),
    };
    if set.loops.is_empty() || cursors.is_empty() {
        return out;
    }
    // Les requêtes ne dépendent pas de la table : construites une fois, dans l'ordre (boucle, curseur).
    let reqs: Vec<Request> = set
        .loops
        .iter()
        .flat_map(|l| {
            cursors.iter().map(move |&(mode, fam, q8)| {
                loop_request(
                    l.start,
                    l.bearing,
                    l.distance_m,
                    l.ty,
                    mode,
                    fam,
                    q8,
                    l.key ^ p.seed,
                    p.figures.figures_of(set, l.ty),
                )
            })
        })
        .collect();
    let nc = cursors.len();
    let r_common: [[f64; TASTE_CLASSES]; 3] =
        std::array::from_fn(|t| std::array::from_fn(|c| f64::from(common.gen_dm_per_loop[t][c]) / 10.0));
    let mut taste = if sonde { Taste::neutral() } else { common.taste() };
    for step in 1..=p.steps {
        let Ok(results) = player.play_all(&reqs, &taste) else { return out };
        out.plays += reqs.len() as u64;
        let mut m: [Meters; 3] = std::array::from_fn(|_| Meters {
            real: [0.0; TASTE_CLASSES],
            gen: [0.0; TASTE_CLASSES],
            loops: 0,
            pairs: 0,
        });
        for (i, l) in set.loops.iter().enumerate() {
            let t = usize::from(l.ty).min(2);
            let mut counted = false;
            for j in 0..nc {
                let Some(gen) = results.get(i * nc + j).and_then(Option::as_ref) else { continue };
                for c in 0..TASTE_CLASSES {
                    m[t].real[c] += l.real_m[c];
                    m[t].gen[c] += gen[c];
                }
                m[t].pairs += 1;
                counted = true;
            }
            m[t].loops += u32::from(counted);
        }
        for t in 0..3 {
            if m[t].loops > 0 {
                // Au-delà du troisième pas, le mélange `Sonde` relit les mètres du dernier.
                let pool = sonde.then(|| (&common.steps[usize::from(step - 1).min(2)], t));
                let d = step_type(&mut states[t], &m[t], &r_common[t], pool, p, step, t as u8);
                out.diag.push(d);
                seen[t] = true;
            }
            // Un type sans donnée garde la table commune au bit près, dans les deux formes.
            out.taste.q8[t] = if seen[t] || !sonde { states[t].q8() } else { common.q8[t] };
            out.loops[t] = m[t].loops;
            out.pairs[t] = m[t].pairs;
        }
        out.steps_done = step;
        taste = if sonde {
            // Ce que l'équipe joue au pas suivant : sa table pour les types déjà vus, la neutre pour les autres.
            Taste { q8: std::array::from_fn(|t| if seen[t] { states[t].q8() } else { [256; TASTE_CLASSES] }) }
        } else {
            out.taste.clone()
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::collect::LearnLoop;
    use crate::profile::Stopped;

    /// Un joueur jouet : sur chaque requête, le généré de la classe `c` est `avail[c] · mult(c)^-ε`, normalisé à 1 000 m ;
    /// la classe `elastic` est très sensible, les autres peu.
    struct Toy {
        avail: [f64; TASTE_CLASSES],
        elastic: usize,
        stop_at_call: Option<usize>,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl Toy {
        fn new(elastic: usize) -> Self {
            let mut avail = [0.0; TASTE_CLASSES];
            for (c, a) in avail.iter_mut().enumerate().take(6) {
                *a = 1.0 + c as f64 * 0.3;
            }
            Self { avail, elastic, stop_at_call: None, calls: 0.into() }
        }
    }

    impl Player for Toy {
        fn play_all(
            &self,
            reqs: &[Request],
            taste: &Taste,
        ) -> Result<Vec<Option<[f64; TASTE_CLASSES]>>, Stopped> {
            let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.stop_at_call == Some(n) {
                return Err(Stopped);
            }
            Ok(reqs
                .iter()
                .map(|r| {
                    let t = r.ride_type.index();
                    let mut out = [0.0; TASTE_CLASSES];
                    for c in 0..TASTE_CLASSES {
                        let eps = if c == self.elastic { 4.0 } else { 0.8 };
                        out[c] = self.avail[c] * (f64::from(taste.q8[t][c]) / 256.0).powf(-eps);
                    }
                    let s: f64 = out.iter().sum();
                    for o in &mut out {
                        *o *= 1_000.0 / s;
                    }
                    Some(out)
                })
                .collect())
        }
    }

    fn set_of(types: &[u8], real: &[(usize, f64)]) -> LearnSet {
        let loops = types
            .iter()
            .enumerate()
            .map(|(i, &ty)| {
                let mut real_m = [0.0; TASTE_CLASSES];
                for &(c, m) in real {
                    real_m[c] = m;
                }
                LearnLoop {
                    key: i as u64 + 1,
                    ty,
                    start: (47.0, -1.5),
                    bearing: None,
                    distance_m: 20_000.0,
                    real_m,
                }
            })
            .collect();
        LearnSet { loops, counts: Default::default(), shapes: Default::default() }
    }

    fn cursors() -> Vec<(Mode, f32, u32)> {
        vec![(Mode::TeamFirst, 0.5, 640), (Mode::TeamFirst, 0.5, 256)]
    }

    /// Une table commune non neutre, pour que le départ de l'équipe ne soit pas `K`.
    fn common() -> Common {
        let mut c = Common::without_steps([[256; TASTE_CLASSES]; 3], [[0; TASTE_CLASSES]; 3]);
        c.q8[0][0] = 300;
        c.q8[0][1] = 200;
        for row in &mut c.gen_dm_per_loop {
            row[..6].fill(1_000);
        }
        c
    }

    #[test]
    fn an_empty_set_returns_the_common_table_without_playing() {
        let toy = Toy::new(2);
        let cm = common();
        let out = learn(&LearnSet::default(), &cursors(), &toy, &cm, &LearnParams::default());
        assert_eq!((out.steps_done, out.plays), (0, 0));
        assert_eq!(out.taste.q8, cm.q8);
        assert_eq!(toy.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn a_type_with_no_data_keeps_the_common_table_bit_for_bit() {
        let toy = Toy::new(2);
        let cm = common();
        // Que des boucles ROAD : GRAVEL et MTB gardent `C`.
        let set = set_of(&[0, 0, 0], &[(0, 600.0), (1, 400.0)]);
        let out = learn(&set, &cursors(), &toy, &cm, &LearnParams::default());
        assert_eq!(out.steps_done, 3);
        assert_eq!(out.taste.q8[1], cm.q8[1]);
        assert_eq!(out.taste.q8[2], cm.q8[2]);
        assert_ne!(out.taste.q8[0], cm.q8[0]);
        assert_eq!(out.loops, [3, 0, 0]);
        assert_eq!(out.plays, 3 * 3 * 2);
    }

    #[test]
    fn a_class_the_common_table_never_generates_does_not_reach_a_weight_of_one_on_one_loop() {
        // `R = 0` pour toutes les classes : `w = N / (N + n0)` (test 10h), jamais 1 avec une seule boucle.
        let mut cm = common();
        cm.gen_dm_per_loop = [[0; TASTE_CLASSES]; 3];
        let set = set_of(&[0], &[(0, 100.0), (1, 900.0)]);
        let one = LearnParams { steps: 1, ..LearnParams::default() };
        let out = learn(&set, &cursors(), &Toy::new(2), &cm, &one);
        let d = &out.diag[0];
        assert!((d.weight - 1.0 / 26.0).abs() < 1e-12);
        // Le même sans la borne `N / (N + n0)` irait à `g` plein : la correction est bien amortie.
        let free = LearnParams { n0: 0.0, steps: 1, ..LearnParams::default() };
        let full = learn(&set, &cursors(), &Toy::new(2), &cm, &free);
        let dist = |a: &Learned| (f64::from(a.taste.q8[0][1]) - f64::from(cm.q8[0][1])).abs();
        assert!(dist(&out) < dist(&full));
    }

    #[test]
    fn three_steps_shrink_the_gap_and_damp_only_the_elastic_class() {
        let toy = Toy::new(1);
        let cm = Common::without_steps([[256; TASTE_CLASSES]; 3], [[0; TASTE_CLASSES]; 3]);
        // Le réel veut beaucoup plus de la classe 1 et moins de la classe 5.
        let set = set_of(&[0; 40], &[(0, 100.0), (1, 250.0), (2, 200.0), (3, 200.0), (4, 150.0), (5, 100.0)]);
        let p = LearnParams { n0: 0.0, ..LearnParams::default() };
        let out = learn(&set, &cursors(), &toy, &cm, &p);
        assert_eq!(out.steps_done, 3);
        let road: Vec<&StepDiag> = out.diag.iter().filter(|d| d.ty == 0).collect();
        assert_eq!(road.len(), 3);
        assert!(road[2].l1_gap < road[0].l1_gap / 2.0, "{:?}", road);
        // γ de la classe élastique (la seule qui dépasse sa cible) est divisé au pas 2, et un seul γ l'est.
        assert_eq!((road[0].halved, road[1].halved), (0, 1), "{road:?}");
        assert!((road[1].gamma_min - 0.25).abs() < 1e-12, "{road:?}");
        // Les multiplicateurs restent dans [1/4, 4] : q8 dans [64, 1 024].
        assert!(out.taste.q8.iter().flatten().all(|&q| (64..=1_024).contains(&q)));
        assert!(out.taste.q8[0][1] < 256, "la classe voulue est moins chère : {}", out.taste.q8[0][1]);
    }

    #[test]
    fn a_player_that_stops_at_step_two_leaves_the_first_table() {
        let mut toy = Toy::new(1);
        toy.stop_at_call = Some(1);
        let cm = Common::without_steps([[256; TASTE_CLASSES]; 3], [[0; TASTE_CLASSES]; 3]);
        let set = set_of(&[0; 5], &[(1, 800.0), (2, 200.0)]);
        let out = learn(&set, &cursors(), &toy, &cm, &LearnParams::default());
        assert_eq!(out.steps_done, 1);
        let one =
            learn(&set, &cursors(), &Toy::new(1), &cm, &LearnParams { steps: 1, ..LearnParams::default() });
        assert_eq!(out.taste.q8, one.taste.q8);
        // Arrêt dès le premier pas : la table commune, zéro pas.
        let mut toy = Toy::new(1);
        toy.stop_at_call = Some(0);
        let out = learn(&set, &cursors(), &toy, &cm, &LearnParams::default());
        assert_eq!((out.steps_done, out.taste.q8), (0, cm.q8));
    }

    #[test]
    fn the_normalised_step_keeps_the_weighted_mean_near_one() {
        let cm = Common::without_steps([[256; TASTE_CLASSES]; 3], [[0; TASTE_CLASSES]; 3]);
        let set = set_of(&[0; 30], &[(0, 100.0), (1, 700.0), (2, 200.0)]);
        let p = LearnParams { normalize: true, n0: 0.0, steps: 1, ..LearnParams::default() };
        let out = learn(&set, &cursors(), &Toy::new(1), &cm, &p);
        // Les mètres joués au pas 1 sont ceux de la table `C` : la moyenne des `mult` pondérée par ces mètres vaut 1.
        let played = Toy::new(1)
            .play_all(
                &[set_of(&[0], &[])
                    .loops
                    .iter()
                    .map(|l| {
                        loop_request(
                            l.start,
                            l.bearing,
                            l.distance_m,
                            0,
                            Mode::TeamFirst,
                            0.5,
                            640,
                            1,
                            Figures::Classic,
                        )
                    })
                    .next()
                    .unwrap()],
                &cm.taste(),
            )
            .unwrap();
        let gen = played[0].unwrap();
        let sg: f64 = gen.iter().sum();
        let mean: f64 =
            gen.iter().zip(&out.taste.q8[0]).map(|(g, &q)| g * f64::from(q) / 256.0).sum::<f64>() / sg;
        // `q8` arrondi à l'entier : 1/256 près au pire.
        assert!((mean - 1.0).abs() < 0.01, "{mean}");
    }

    /// Un joueur qui note la table jouée à chaque appel.
    struct Spy {
        inner: Toy,
        tastes: std::sync::Mutex<Vec<Taste>>,
    }

    impl Player for Spy {
        fn play_all(
            &self,
            reqs: &[Request],
            taste: &Taste,
        ) -> Result<Vec<Option<[f64; TASTE_CLASSES]>>, Stopped> {
            self.tastes.lock().unwrap().push(taste.clone());
            self.inner.play_all(reqs, taste)
        }
    }

    fn with_steps(mut c: Common) -> Common {
        for (k, st) in c.steps.iter_mut().enumerate() {
            for t in 0..2 {
                for cl in 0..TASTE_CLASSES {
                    st.real[t][cl] = ((cl * 31 + k * 5 + t) % 17 + 1) as f64 * 2_000.0;
                    st.gen[t][cl] = ((cl * 23 + k * 3 + t) % 13) as f64 * 1_800.0;
                }
                st.loops[t] = 200;
            }
        }
        c
    }

    #[test]
    fn the_sonde_mix_starts_from_the_neutral_table_and_keeps_the_common_one_for_a_type_without_data() {
        let cm = with_steps(common());
        let set = set_of(&[0, 0, 0, 0], &[(0, 600.0), (1, 400.0)]);
        let sonde = LearnParams { mix: Mix::Sonde, ..LearnParams::default() };
        let spy = Spy { inner: Toy::new(2), tastes: Default::default() };
        let out = learn(&set, &cursors(), &spy, &cm, &sonde);
        assert_eq!(out.steps_done, 3);
        let played = spy.tastes.lock().unwrap().clone();
        // Pas 1 : la table neutre (`rt` part de `K`), pas la table commune ; les pas suivants : la table de l'équipe.
        assert_eq!(played[0], Taste::neutral());
        assert_ne!(played[1].q8[0], [256; TASTE_CLASSES]);
        // GRAVEL et MTB n'ont aucune boucle : la table commune, au bit près.
        assert_eq!((out.taste.q8[1], out.taste.q8[2]), (cm.q8[1], cm.q8[2]));
        // La forme `Capped`, elle, joue la table commune au pas 1.
        let spy = Spy { inner: Toy::new(2), tastes: Default::default() };
        learn(&set, &cursors(), &spy, &cm, &LearnParams::default());
        assert_eq!(spy.tastes.lock().unwrap()[0], cm.taste());
        // Arrêt avant le premier pas fini : la table commune, zéro pas, pour la forme `Sonde` aussi.
        let mut toy = Toy::new(2);
        toy.stop_at_call = Some(0);
        let out = learn(&set, &cursors(), &toy, &cm, &sonde);
        assert_eq!((out.steps_done, out.taste.q8), (0, cm.q8));
    }

    #[test]
    fn the_sonde_mix_is_bare_weight_without_the_n_over_n_plus_n0_cap() {
        // Une seule boucle : en `Capped`, w <= 1 / 26 ; en `Sonde`, w nu `G_T / (G_T + n0 G_P / N_P)`, bien plus grand
        // quand les mètres communs par boucle sont faibles : la correction de l'équipe pèse plus.
        let mut cm = common();
        for st in &mut cm.steps {
            for t in 0..2 {
                st.real[t] = [3_000.0; TASTE_CLASSES];
                st.gen[t] = [10.0; TASTE_CLASSES];
                st.loops[t] = 300;
            }
        }
        let set = set_of(&[0], &[(0, 100.0), (1, 900.0)]);
        let one = |mix| LearnParams { steps: 1, mix, ..LearnParams::default() };
        let capped = learn(&set, &cursors(), &Toy::new(2), &cm, &one(Mix::Capped));
        let sonde = learn(&set, &cursors(), &Toy::new(2), &cm, &one(Mix::Sonde));
        // Le déplacement de la classe 1 depuis le point de départ de chaque forme (la table commune, ou la neutre).
        let move_of = |t: &Learned, start: u16| (f64::from(t.taste.q8[0][1]) - f64::from(start)).abs();
        let (mc, ms) = (move_of(&capped, cm.q8[0][1]), move_of(&sonde, 256));
        assert!(ms > mc * 2.0, "{ms} {mc}");
    }

    #[test]
    fn the_mix_enters_the_hash_only_when_it_is_not_the_default() {
        let a = LearnParams::default();
        let s = LearnParams { mix: Mix::Sonde, ..a.clone() };
        assert_ne!(a.hash(), s.hash());
        assert!(!a.canonical().contains("mix") && s.canonical().ends_with(";mix=sonde"));
        let c = Common::engine(false);
        // `Capped` : l'empreinte d'avant ; `Sonde` : celle des mètres des pas en plus.
        assert_eq!(c.hash_for(Mix::Capped), c.hash());
        assert_ne!(c.hash_for(Mix::Sonde), c.hash());
        let mut d = c.clone();
        d.steps[2].gen[0][3] += 0.1;
        assert_eq!(d.hash_for(Mix::Capped), c.hash());
        assert_ne!(d.hash_for(Mix::Sonde), c.hash_for(Mix::Sonde));
        assert_eq!(Mix::parse("sonde"), Some(Mix::Sonde));
        assert_eq!(Mix::parse("capped"), Some(Mix::Capped));
        assert_eq!(Mix::parse("x"), None);
    }

    #[test]
    fn hashes_follow_every_field() {
        let a = LearnParams::default();
        let h = a.hash();
        let variants = [
            LearnParams { gamma: 0.6, ..a.clone() },
            LearnParams { steps: 2, ..a.clone() },
            LearnParams { m: 0.4, ..a.clone() },
            LearnParams { step_bound: 2.0, ..a.clone() },
            LearnParams { mult_bound: 5.0, ..a.clone() },
            LearnParams { n0: 10.0, ..a.clone() },
            LearnParams { sign_eps: 0.03, ..a.clone() },
            LearnParams { normalize: !a.normalize, ..a.clone() },
            LearnParams { cap: Cap::PerType(500), ..a.clone() },
            LearnParams { cap: Cap::Team(400), ..a.clone() },
            LearnParams { cursors: [25, 90], ..a.clone() },
            LearnParams { seed: 1, ..a.clone() },
            LearnParams { retain: RetainParams { min_km: 6.0, ..a.retain }, ..a.clone() },
            LearnParams { retain: RetainParams { max_km: None, ..a.retain }, ..a.clone() },
        ];
        for v in &variants {
            assert_ne!(v.hash(), h, "{}", v.canonical());
        }
        assert_eq!(a.hash(), LearnParams::default().hash());
        let c = Common::engine(false);
        let mut d = c.clone();
        d.gen_dm_per_loop[2][71] += 1;
        assert_ne!(c.hash(), d.hash());
    }
}
