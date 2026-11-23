//! Figures organiques (lot 4, 4c, §15.3 à §15.7 de `docs/design/technique/ord-loops-lot4.md`) : le tirage de la figure,
//! les sommets lointains et leurs paires, le triangle (trois jambes, épi, sécante) et le choix des trois parcours.
//!
//! Tout ce qui est ici est appelé par `generate.rs` **seulement** pour [`crate::Figures::Organic`] : `Classic` n'y
//! passe jamais, ce qui garde les parcours de 1.10 au bit près.

use std::collections::BTreeMap;

use ord_graph::util::SplitMix64;
use rustc_hash::FxHashSet;

use crate::attract::attr_w;
use crate::engine::{Engine, Snap, ViewData};
use crate::generate::{overlap_share, scored_route, steps_of, Scored};
use crate::geo::{heading_deg, turn_angle_deg};
use crate::search::{
    astar, astar_p2p, retro_ok, t1_min_factor, turn_cost, Ban, CorGrid, Ctx, P2pEnd, P2pTarget, Tree, BAN,
    NONE, P2P_SLACK, TAIL,
};
use crate::team::TeamCtx;
use crate::turns::Lead;
use crate::verify::{check_route_aligned_in, check_route_in, Violation};
use crate::window::Window;
use crate::{Failure, Family, FigureDiagnostics, Organic, Position, Request, Route};

/// Secteurs de 30° autour du départ.
const SECTORS_PER_TURN: usize = 12;

/// Sel du générateur de la figure : à part de celui de la lentille, dont la suite reste celle de `Classic`.
pub(crate) const FIGURE_SALT: u64 = 0x4f52_4746_4947_0001;

/// Ce que le tirage de la figure décide pour une requête.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Draw {
    /// Le premier parcours préféré est un triangle.
    pub tri_first: bool,
    /// r / D visé des triangles : clé de rang des paires, jamais un filtre.
    pub rho: f64,
    /// Sens du triangle : P2 après P1 dans le sens des aiguilles d'une montre vu du départ.
    pub clockwise: bool,
}

/// Part effective de triangles, en 256es : `min(256, tri_q8 · boost / 256)`.
pub(crate) fn tri_q8_eff(o: &Organic) -> u32 {
    (u32::from(o.mix.tri_q8) * u32::from(o.tune.tri_boost_q8) / 256).min(256)
}

/// Tirage de la figure (§15.3), dans l'ordre : figure du premier parcours, r / D visé, sens.
pub(crate) fn draw(seed: u64, o: &Organic) -> Draw {
    let mut fr = SplitMix64(seed);
    let tri_first = (fr.below(256) as u32) < tri_q8_eff(o);
    let u = 0.1 + 0.8 * fr.below(10_000) as f64 / 10_000.0;
    let clockwise = fr.below(2) == 0;
    Draw { tri_first, rho: interp_rho(&o.mix.rd_tri_milli, u), clockwise }
}

/// r / D du quantile `u` (de 0,10 à 0,90) : interpolation linéaire par morceaux entre P10, P25, P50, P75, P90.
fn interp_rho(rd: &[u16; 5], u: f64) -> f64 {
    const US: [f64; 5] = [0.10, 0.25, 0.50, 0.75, 0.90];
    let k = (0..4).find(|&k| u <= US[k + 1]).unwrap_or(3);
    let t = ((u - US[k]) / (US[k + 1] - US[k])).clamp(0.0, 1.0);
    (f64::from(rd[k]) + t * (f64::from(rd[k + 1]) - f64::from(rd[k]))) / 1000.0
}

/// État de la recherche des triangles, gardé d'une passe à l'autre de la requête.
#[derive(Debug)]
pub(crate) struct TriState {
    pub draw: Draw,
    /// Biais de l'estimation (sécante).
    pub beta: f64,
    /// Paires `(P1, P2)` déjà cherchées (au plus `max_pairs` par requête).
    pub searched: Vec<(u32, u32)>,
    /// Paires refusées pour un virage serré à la passe d'avant, reprises une fois à celle-ci (`sharp_retry`).
    pub retry: Vec<(u32, u32)>,
    /// Triangles gardés.
    pub kept: u32,
    /// Plus de triangle à chercher : sans part de triangles, assez de paires ou de triangles, ou plus de paire admissible.
    pub done: bool,
}

impl TriState {
    pub fn new(draw: Draw, o: &Organic) -> Self {
        Self {
            draw,
            beta: f64::from(o.tune.beta0_milli) / 1000.0,
            searched: Vec::new(),
            retry: Vec::new(),
            kept: 0,
            done: tri_q8_eff(o) == 0,
        }
    }
}

/// Ce que la phase des triangles tient fixe pour la requête.
pub(crate) struct Fixed<'a, B: AsRef<[u8]>> {
    pub e: &'a Engine<B>,
    pub win: &'a Window,
    pub team: Option<TeamCtx<'a>>,
    pub r: &'a Request,
    pub cx: &'a Ctx<'a>,
    pub s_snap: &'a Snap,
    pub e_snap: &'a Snap,
    pub o: &'a Organic,
    pub d: f64,
    pub tail_dm: u32,
}

/// Les tableaux de travail de la vue : arbre de recherche, interdits, couloir.
pub(crate) struct Scratch<'a> {
    pub ta: &'a mut Tree,
    pub ban: &'a mut Vec<u32>,
    pub ban_gen: &'a mut u32,
    pub cor: &'a mut CorGrid,
}

impl Scratch<'_> {
    /// Nouvelle génération d'interdits (même règle que la lentille).
    fn next_gen(&mut self) -> u32 {
        *self.ban_gen += 1;
        if *self.ban_gen >= (1 << 28) {
            self.ban.iter_mut().for_each(|x| *x = 0);
            *self.ban_gen = 1;
        }
        *self.ban_gen
    }

    /// Pose les interdits d'une jambe de départ : hors queue `BAN`, dans la queue `TAIL` (aller repris au coût × 2) ; avec
    /// `all_ban`, `BAN` partout (la jambe 2 ne reprend rien de la jambe 1, §16.4 de la note).
    pub(crate) fn stamp_leg1(&mut self, t1: &Tree, leg1: &[u32], tail_dm: u32, all_ban: bool) {
        let gen = *self.ban_gen;
        for &f in leg1 {
            let c = (f >> 1) as usize;
            let bits = if !all_ban && t1.meters(f) <= tail_dm { TAIL } else { BAN };
            let cur = if self.ban[c] >> 3 == gen { self.ban[c] & 7 } else { 0 };
            let bits = if cur & BAN != 0 { BAN } else { bits };
            self.ban[c] = (gen << 3) | bits;
        }
    }
}

/// Un sommet lointain.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cand {
    pub(crate) node: u32,
    /// Cap du départ au nœud.
    pub(crate) ang: f64,
    /// Agrément : coût T1 / mètres T1.
    pub(crate) ratio: f64,
    /// Meilleurs états du nœud dans T1 et T2.
    pub(crate) b1: u32,
    pub(crate) b2: u32,
    /// Poids d'attracteur du nœud (lot 4, 4d) ; 0 quand les attracteurs ne sont pas actifs.
    pub(crate) w: u8,
}

/// Une paire de sommets admissible à un biais donné.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pair {
    pub(crate) n1: u32,
    pub(crate) n2: u32,
    pub(crate) b1: u32,
    pub(crate) est_m: f64,
    pub(crate) r_pair: f64,
    pub(crate) agr: f64,
    /// Somme des poids d'attracteur des deux sommets (0 quand les attracteurs ne sont pas actifs).
    pub(crate) wsum: u8,
}

/// Rang d'une paire : l'écart de son r / D au r / D visé (par cases de 0,02), moins la somme des poids d'attracteur de ses
/// sommets quand `attr_rank` (un point de poids vaut une case, §17.7) ; poids nuls : le rang de 1.12.
fn pair_rank(p: &Pair, rho: f64, attr_rank: bool) -> i64 {
    ((p.r_pair - rho).abs() / 0.02).floor() as i64 - if attr_rank { i64::from(p.wsum) } else { 0 }
}

/// L'ordre des paires : le rang, l'agrément, puis les ids des sommets.
pub(crate) fn pair_order(a: &Pair, b: &Pair, rho: f64, attr_rank: bool) -> std::cmp::Ordering {
    pair_rank(a, rho, attr_rank)
        .cmp(&pair_rank(b, rho, attr_rank))
        .then(a.agr.total_cmp(&b.agr))
        .then(a.n1.cmp(&b.n1))
        .then(a.n2.cmp(&b.n2))
}

/// Les attracteurs jouent pour cette requête : poids non nuls, et la fenêtre porte les masques du fichier (§17.7).
pub(crate) fn attr_active(v: &ViewData, o: &Organic) -> bool {
    !o.attr.is_none() && !v.attr.is_empty()
}

/// Sommets lointains (§15.4) : nœud atteint par T1 et T2, degré de vue ≥ 3, mètres T1 dans la bande, secteur dans le
/// cap ; par secteur et par sous-bande, le plus agréable (départage par id de nœud). Avec des attracteurs actifs (§17.7), un
/// second tableau garde par case le nœud de poids non nul le meilleur selon `(poids décroissant, agrément, id)` : la sortie,
/// case par case, est le plus agréable puis l'attracteur s'il existe et diffère. Rend aussi le nombre d'attracteurs ajoutés.
pub(crate) fn candidates<B: AsRef<[u8]>>(fx: &Fixed<'_, B>, t1: &Tree, t2: &Tree) -> (Vec<Cand>, u32) {
    let (v, disc, t) = (fx.cx.v, fx.cx.disc, &fx.o.tune);
    let nb = usize::from(t.sub_bands.max(1));
    let (lo, hi) = (f64::from(t.band_milli[0]) / 1000.0, f64::from(t.band_milli[1]) / 1000.0);
    let d_dm = fx.d * 10.0;
    let attr_on = attr_active(v, fx.o);
    let mut best: Vec<Option<Cand>> = vec![None; SECTORS_PER_TURN * nb];
    let mut best_attr: Vec<Option<Cand>> =
        if attr_on { vec![None; SECTORS_PER_TURN * nb] } else { Vec::new() };
    for &node in &t1.reached {
        if v.deg[node as usize] < 3 {
            continue;
        }
        let Some(b2) = t2.node_best(node) else { continue };
        let b1 = t1.node_best(node).expect("atteint");
        let m1 = f64::from(t1.meters(b1));
        let s = m1 / d_dm;
        if s < lo || s >= hi {
            continue;
        }
        let ang = disc.bearing_of(v.pos[node as usize]);
        let sec = ((ang / 30.0) as usize) % SECTORS_PER_TURN;
        if let Some(b) = fx.r.bearing {
            if turn_angle_deg(b.degrees(), sec as f64 * 30.0 + 15.0) > 50.0 {
                continue;
            }
        }
        let sb = (((s - lo) / (hi - lo) * nb as f64) as usize).min(nb - 1);
        let ratio = f64::from(t1.get(b1)) / m1.max(1.0);
        let w = if attr_on { attr_w(v.attr[node as usize], &fx.o.attr.w) } else { 0 };
        let cand = Cand { node, ang, ratio, b1, b2, w };
        let slot = &mut best[sec * nb + sb];
        if slot.is_none_or(|c| ratio < c.ratio || (ratio == c.ratio && node < c.node)) {
            *slot = Some(cand);
        }
        if w > 0 {
            let slot = &mut best_attr[sec * nb + sb];
            let better =
                |c: &Cand| w > c.w || (w == c.w && (ratio < c.ratio || (ratio == c.ratio && node < c.node)));
            if slot.as_ref().is_none_or(better) {
                *slot = Some(cand);
            }
        }
    }
    if !attr_on {
        return (best.into_iter().flatten().collect(), 0);
    }
    let mut out = Vec::with_capacity(best.len());
    let mut added = 0u32;
    for (pleasant, attractor) in best.into_iter().zip(best_attr) {
        if let Some(p) = pleasant {
            out.push(p);
        }
        if let Some(a) = attractor {
            if pleasant.is_none_or(|p| p.node != a.node) {
                out.push(a);
                added += 1;
            }
        }
    }
    (out, added)
}

/// Les paires de sommets écartées de `sep_deg` vu du départ, ordonnées selon le sens tiré, avec l'estimation de la
/// longueur `m₁ + φ · c + m₃` (la formule de la sonde, `ord-loops-lot4c-sonde.md` §3).
fn pairs_of<B: AsRef<[u8]>>(
    fx: &Fixed<'_, B>,
    t1: &Tree,
    t2: &Tree,
    cands: &[Cand],
    clockwise: bool,
) -> Vec<Pair> {
    let (v, disc, t) = (fx.cx.v, fx.cx.disc, &fx.o.tune);
    let (sep_lo, sep_hi) = (f64::from(t.sep_deg[0]), f64::from(t.sep_deg[1]));
    let xy = |n: u32| disc.xy_of(v.pos[n as usize]);
    let mut out = Vec::new();
    for i in 0..cands.len() {
        for j in (i + 1)..cands.len() {
            let (ci, cj) = (&cands[i], &cands[j]);
            let sep = turn_angle_deg(ci.ang, cj.ang);
            if sep < sep_lo || sep > sep_hi {
                continue;
            }
            let (a, b) = if (cj.ang - ci.ang).rem_euclid(360.0) <= 180.0 { (ci, cj) } else { (cj, ci) };
            let (a, b) = if clockwise { (a, b) } else { (b, a) };
            let (p1, p2) = (xy(a.node), xy(b.node));
            let (r1, r2) = (p1.0.hypot(p1.1).max(1.0), p2.0.hypot(p2.1).max(1.0));
            let (m1, m2) = (f64::from(t1.meters(a.b1)) / 10.0, f64::from(t1.meters(b.b1)) / 10.0);
            let phi = (m1 / r1 + m2 / r2) / 2.0;
            let crow = (p1.0 - p2.0).hypot(p1.1 - p2.1);
            let m3 = f64::from(t2.meters(b.b2)) / 10.0;
            out.push(Pair {
                n1: a.node,
                n2: b.node,
                b1: a.b1,
                est_m: m1 + phi * crow + m3,
                r_pair: r1.max(r2) / fx.d,
                agr: (a.ratio + b.ratio) / 2.0,
                wsum: a.w + b.w,
            });
        }
    }
    out
}

/// Les nœuds d'une jambe, avec la distance cumulée (m) au premier : le nœud de départ du premier état, puis l'arrivée
/// de chaque état.
fn nodes_of(v: &ViewData, fields: &[u32]) -> Vec<(u32, f64)> {
    let mut out = Vec::with_capacity(fields.len() + 1);
    out.push((v.src(fields[0]), 0.0));
    let mut acc = 0.0;
    for &f in fields {
        acc += f64::from(v.len_dm[(f >> 1) as usize]) / 10.0;
        out.push((v.tgt(f), acc));
    }
    out
}

/// Épi au sommet entre la jambe d'arrivée `arr` et la jambe de départ `dep` : l'angle entre le cap des `spike_m` derniers
/// mètres d'arrivée et celui des `spike_m` premiers mètres de départ atteint `spike_deg` (le parcours repart presque par
/// où il est venu).
pub(crate) fn is_spike<B: AsRef<[u8]>>(fx: &Fixed<'_, B>, arr: &[u32], dep: &[u32]) -> bool {
    let (v, disc, t) = (fx.cx.v, fx.cx.disc, &fx.o.tune);
    let xy = |n: u32| disc.xy_of(v.pos[n as usize]);
    let spike_m = f64::from(t.spike_m);
    let (na, nd) = (nodes_of(v, arr), nodes_of(v, dep));
    let (apex, total) = (na[na.len() - 1].0, na[na.len() - 1].1);
    let from = na.iter().rposition(|&(_, c)| total - c >= spike_m).map_or(na[0].0, |i| na[i].0);
    let to = nd.iter().find(|&&(_, c)| c >= spike_m).map_or(nd[nd.len() - 1].0, |&(n, _)| n);
    let (pa, pf, pt) = (xy(apex), xy(from), xy(to));
    let h_in = heading_deg(pa.0 - pf.0, pa.1 - pf.1);
    let h_out = heading_deg(pt.0 - pa.0, pt.1 - pa.1);
    turn_angle_deg(h_in, h_out) >= f64::from(t.spike_deg)
}

/// P' d'une réparation : le dernier nœud de degré ≥ 3 de la jambe d'arrivée situé à `spike_m` au moins avant le sommet ;
/// rend le nombre d'états du préfixe qui y mène.
pub(crate) fn repair_prefix<B: AsRef<[u8]>>(fx: &Fixed<'_, B>, arr: &[u32]) -> Option<usize> {
    let v = fx.cx.v;
    let spike_m = f64::from(fx.o.tune.spike_m);
    let na = nodes_of(v, arr);
    let total = na[na.len() - 1].1;
    (1..na.len() - 1).rev().find(|&i| total - na[i].1 >= spike_m && v.deg[na[i].0 as usize] >= 3)
}

/// Issue d'une recherche de jambe.
enum Leg {
    Found(Vec<u32>),
    /// Rien (pas de chemin, plafond de la recherche atteint) : la paire est jetée.
    None,
    /// Budget de la passe épuisé : la phase s'arrête.
    OutOfBudget,
}

/// Jambe 2 : A* de la fin de `leg1` au nœud `n2`, sur le graphe privé de la jambe 1 et son couloir.
#[allow(clippy::too_many_arguments)]
fn run_leg2<B: AsRef<[u8]>>(
    fx: &Fixed<'_, B>,
    t1: &Tree,
    t2: &Tree,
    sc: &mut Scratch<'_>,
    leg1: &[u32],
    n2: u32,
    f_min: f64,
    budget: &mut u64,
    fig: &mut FigureDiagnostics,
    stop: &mut dyn FnMut() -> bool,
) -> Result<Leg, Failure> {
    let (cx, v) = (fx.cx, fx.cx.v);
    let last = *leg1.last().expect("jambe non vide");
    let last_pred = if leg1.len() >= 2 { leg1[leg1.len() - 2] } else { NONE };
    sc.next_gen();
    sc.stamp_leg1(t1, leg1, fx.tail_dm, fx.o.tune.no_reuse);
    let w_m = f64::from(fx.o.tune.corridor_w_milli) / 1000.0 * fx.d;
    let use_cor = fx.o.tune.corridor_w_milli > 0;
    if use_cor {
        sc.cor.begin(fx.d, w_m);
        sc.cor.mark_leg(v, cx.disc, leg1, fx.d * fx.e.params.tail_reuse_share, 2.0 * w_m);
    }
    let ban = Ban {
        stamp: sc.ban,
        gen: *sc.ban_gen,
        cor: use_cor.then_some(&*sc.cor),
        kappa_q8: u32::from(fx.o.tune.kappa_q8),
    };
    let tgt = P2pTarget {
        node: n2,
        xy: cx.disc.xy_of(v.pos[n2 as usize]),
        t1: t1.node_best(n2).map_or(0, |x| t1.get(x)),
        t2: t2.node_best(n2).map_or(0, |x| t2.get(x)),
        f_min,
        slack: P2P_SLACK,
    };
    let cap = t1.settled.len() as u64;
    // Aucune exemption à l'arrivée : la jambe 3 suit la jambe 2, dont aucun nœud n'est à 200 m de l'arrivée le long du
    // parcours, même quand la jambe passe près du départ ; lue sur T2, l'exemption y laisserait des grappes refusées.
    let lead = Lead { path: leg1, dm: t1.meters(last), aux: None };
    let out = astar_p2p(cx, sc.ta, t1, t2, last, last_pred, &tgt, &ban, &lead, true, cap, budget, stop)
        .map_err(|_| halted(cx))?;
    fig.p2p_settled += out.settled;
    Ok(match out.end {
        P2pEnd::Found => {
            fig.leg2_found += 1;
            Leg::Found(out.fields.expect("trouvé"))
        }
        P2pEnd::Capped => {
            fig.leg2_capped += 1;
            Leg::None
        }
        P2pEnd::NotFound => Leg::None,
        P2pEnd::BudgetOut => Leg::OutOfBudget,
    })
}

/// Mètres (dm) des jambes 1 et 2 posées : ceux de T1 au bout de la jambe 1, puis les chaînes entières de la jambe 2.
fn leg_dm(cx: &Ctx<'_>, t1: &Tree, leg1: &[u32], leg2: &[u32]) -> u32 {
    let v = cx.v;
    t1.meters(*leg1.last().expect("jambe non vide"))
        + leg2.iter().map(|&f| v.len_dm[(f >> 1) as usize]).sum::<u32>()
}

/// Le chemin d'arbre `ret` posé après les jambes 1 et 2 tient la règle des virages du contrôle aux jonctions (§16.2).
/// Sans modèle (`Classic`, `turns` coupé), toujours vrai.
fn junction_ok(cx: &Ctx<'_>, t1: &Tree, t2: &Tree, leg1: &[u32], leg2: &[u32], ret: &[u32]) -> bool {
    let Some(o) = cx.org else { return true };
    if o.model.is_none() {
        return true;
    }
    let ctx: Vec<u32> = [leg1, leg2].concat();
    o.junction_ok(cx, &ctx, leg_dm(cx, t1, leg1, leg2), ret, Some(t2))
}

fn halted(cx: &Ctx<'_>) -> Failure {
    if cx.esc.get() {
        Failure::WindowEscape
    } else {
        Failure::Stopped
    }
}

/// Jambe 3, depuis la fin de `leg2` (le nœud P2) : le chemin de l'arbre retour s'il ne reprend rien de la jambe 1 (hors
/// queue) ni de la jambe 2 et que le virage en P2 est permis ; sinon l'A* d'heuristique T2 sur le graphe privé des deux
/// jambes, couloir des deux jambes.
#[allow(clippy::too_many_arguments)]
fn run_leg3<B: AsRef<[u8]>>(
    fx: &Fixed<'_, B>,
    t1: &Tree,
    t2: &Tree,
    sc: &mut Scratch<'_>,
    leg1: &[u32],
    leg2: &[u32],
    budget: &mut u64,
    fig: &mut FigureDiagnostics,
    stop: &mut dyn FnMut() -> bool,
) -> Result<Leg, Failure> {
    let (cx, v) = (fx.cx, fx.cx.v);
    let last2 = *leg2.last().expect("jambe non vide");
    let pred2 = if leg2.len() >= 2 { leg2[leg2.len() - 2] } else { *leg1.last().expect("jambe non vide") };
    let p_node = v.tgt(last2);
    let leg2_chains: FxHashSet<u32> = leg2.iter().map(|&f| f >> 1).collect();
    sc.next_gen();
    sc.stamp_leg1(t1, leg1, fx.tail_dm, false);
    if let Some(b2) = t2.node_best(p_node) {
        let mut ret = t2.path_to(b2);
        ret.reverse();
        let plain = Ban::plain(sc.ban, *sc.ban_gen);
        let free = ret.iter().all(|&f| plain.bits(f >> 1) & BAN == 0 && !leg2_chains.contains(&(f >> 1)));
        if free
            && turn_cost(v, cx.forb, last2, pred2, ret[0], None, None).is_some()
            && retro_ok(v, pred2, last2, ret[0])
            && junction_ok(cx, t1, t2, leg1, leg2, &ret)
        {
            fig.leg3_t2 += 1;
            return Ok(Leg::Found(ret));
        }
    }
    let gen = *sc.ban_gen;
    for &f in leg2 {
        sc.ban[(f >> 1) as usize] = (gen << 3) | BAN;
    }
    let w_m = f64::from(fx.o.tune.corridor_w_milli) / 1000.0 * fx.d;
    let use_cor = fx.o.tune.corridor_w_milli > 0;
    if use_cor {
        sc.cor.begin(fx.d, w_m);
        sc.cor.mark_leg(v, cx.disc, leg1, fx.d * fx.e.params.tail_reuse_share, 2.0 * w_m);
        sc.cor.mark_leg(v, cx.disc, leg2, 2.0 * w_m, 2.0 * w_m);
    }
    let ban =
        Ban { stamp: sc.ban, gen, cor: use_cor.then_some(&*sc.cor), kappa_q8: u32::from(fx.o.tune.kappa_q8) };
    let before = *budget;
    let mut exhausted = false;
    let path: Vec<u32> = if cx.org.is_some() { [leg1, leg2].concat() } else { Vec::new() };
    let lead = Lead { path: &path, dm: leg_dm(cx, t1, leg1, leg2), aux: Some(t2) };
    let found = astar(cx, sc.ta, t2, last2, pred2, &ban, &lead, budget, &mut exhausted, stop)
        .map_err(|_| halted(cx))?;
    fig.leg3_settled += before - *budget;
    if exhausted {
        return Ok(Leg::OutOfBudget);
    }
    Ok(match found {
        Some(f) => {
            fig.leg3_astar += 1;
            Leg::Found(f.fields)
        }
        None => {
            fig.leg3_failed += 1;
            Leg::None
        }
    })
}

/// Mètres (dm) d'un parcours de départ à arrivée : morceau partiel du départ, chaînes entières, morceau d'arrivée.
fn meters_dm(fx: &Fixed<'_, impl AsRef<[u8]>>, fields: &[u32]) -> f64 {
    let (cx, v) = (fx.cx, fx.cx.v);
    let n = fields.len();
    let mut m = f64::from(cx.s.out_len[(fields[0] & 1) as usize]);
    for &f in &fields[1..n.saturating_sub(1).max(1)] {
        m += f64::from(v.len_dm[(f >> 1) as usize]);
    }
    if n >= 2 {
        m += f64::from(cx.e.in_len[(fields[n - 1] & 1) as usize]);
    }
    m
}

/// La phase des triangles d'une passe (§15.5 et §15.7) : les paires admissibles à ce biais, dans l'ordre de leur rang,
/// jusqu'à `max_pairs` cherchées et `tri_keep` gardées sur toute la requête, ou la fin du budget de la passe.
#[allow(clippy::too_many_arguments)]
pub(crate) fn triangle_phase<B: AsRef<[u8]>>(
    fx: &Fixed<'_, B>,
    st: &mut TriState,
    t1: &Tree,
    t2: &Tree,
    sc: &mut Scratch<'_>,
    budget: &mut u64,
    new_forb: &mut Vec<(u32, u32)>,
    scored: &mut Vec<Scored>,
    fig: &mut FigureDiagnostics,
    stop: &mut dyn FnMut() -> bool,
) -> Result<(), Failure> {
    let (v, tune) = (fx.cx.v, &fx.o.tune);
    // Le facteur minimal de la passe, lu sur son arbre aller à la première recherche (§15.19 : ni la fenêtre, ni une
    // marge d'arrondi).
    let mut f_min_cell: Option<f64> = None;
    let (cands, attr_added) = candidates(fx, t1, t2);
    fig.candidates += cands.len() as u32;
    fig.attr_candidates += attr_added;
    let pairs = pairs_of(fx, t1, t2, &cands, st.draw.clockwise);
    fig.pairs += pairs.len() as u32;
    let est_tol = f64::from(tune.est_tol_milli) / 1000.0;
    let admissible = |st: &TriState, p: &Pair| (st.beta * p.est_m - fx.d).abs() <= est_tol * fx.d;
    fig.pairs_admissible += pairs.iter().filter(|p| admissible(st, p)).count() as u32;
    let mut rejected: BTreeMap<Violation, u32> = fig.tri_rejected.iter().copied().collect();
    // Reprises (écart au §15.7, §15.19) : une paire dont le triangle a été refusé pour un virage trop serré a ajouté des
    // transitions interdites ; si `sharp_retry`, la passe suivante la cherche une fois de plus, sans qu'elle prenne une
    // seconde place parmi les `max_pairs` paires de la requête (décision 2 du §9.7 : 4 paires par requête).
    let retry_now: Vec<(u32, u32)> = std::mem::take(&mut st.retry);
    let mut tried_now: Vec<(u32, u32)> = Vec::new();
    let mut sharp_now: Vec<(u32, u32)> = Vec::new();
    let mut out_of_budget = false;
    let res = (|| -> Result<(), Failure> {
        loop {
            if st.kept >= u32::from(tune.tri_keep) {
                return Ok(());
            }
            if *budget == 0 {
                out_of_budget = true;
                return Ok(());
            }
            if stop() {
                return Err(Failure::Stopped);
            }
            // Une paire neuve tant que la requête n'en a pas cherché `max_pairs`, ou une reprise de la passe d'avant.
            let fresh = st.searched.len() < usize::from(tune.max_pairs);
            // Le rang : l'écart du r / D de la paire au r / D visé (par pas de 0,02), l'agrément, puis les ids.
            let best = pairs
                .iter()
                .filter(|p| {
                    let k = (p.n1, p.n2);
                    !tried_now.contains(&k)
                        && ((fresh && !st.searched.contains(&k)) || retry_now.contains(&k))
                        && admissible(st, p)
                })
                .min_by(|a, b| pair_order(a, b, st.draw.rho, tune.attr_rank));
            let Some(&pair) = best else {
                return Ok(());
            };
            let key = (pair.n1, pair.n2);
            tried_now.push(key);
            if st.searched.contains(&key) {
                fig.retried += 1;
            } else {
                st.searched.push(key);
            }
            fig.searched += 1;
            if pair.wsum > 0 {
                fig.attr_pairs_searched += 1;
            }
            let f_min = *f_min_cell.get_or_insert_with(|| t1_min_factor(fx.cx, t1));
            let mut leg1 = t1.path_to(pair.b1);
            let mut leg2 = match run_leg2(fx, t1, t2, sc, &leg1, pair.n2, f_min, budget, fig, stop)? {
                Leg::Found(l) => l,
                Leg::None => continue,
                Leg::OutOfBudget => {
                    out_of_budget = true;
                    return Ok(());
                }
            };
            // Épi en P1 : une réparation, la jambe de départ refaite depuis P'. Une seule réparation par triangle (§15.5) :
            // un second épi, en P2, jette la paire.
            let mut repaired = false;
            if is_spike(fx, &leg1, &leg2) {
                fig.spikes += 1;
                let Some(i) = repair_prefix(fx, &leg1) else {
                    fig.spikes_dropped += 1;
                    continue;
                };
                leg1.truncate(i);
                leg2 = match run_leg2(fx, t1, t2, sc, &leg1, pair.n2, f_min, budget, fig, stop)? {
                    Leg::Found(l) => l,
                    Leg::None => {
                        fig.spikes_dropped += 1;
                        continue;
                    }
                    Leg::OutOfBudget => {
                        out_of_budget = true;
                        return Ok(());
                    }
                };
                if is_spike(fx, &leg1, &leg2) {
                    fig.spikes_dropped += 1;
                    continue;
                }
                fig.spikes_repaired += 1;
                repaired = true;
            }
            let mut leg3 = match run_leg3(fx, t1, t2, sc, &leg1, &leg2, budget, fig, stop)? {
                Leg::Found(l) => l,
                Leg::None => continue,
                Leg::OutOfBudget => {
                    out_of_budget = true;
                    return Ok(());
                }
            };
            // Épi en P2 : la jambe d'arrivée tronquée à P', la jambe 3 refaite depuis P'.
            if is_spike(fx, &leg2, &leg3) {
                fig.spikes += 1;
                if repaired {
                    fig.spikes_dropped += 1;
                    continue;
                }
                let Some(i) = repair_prefix(fx, &leg2) else {
                    fig.spikes_dropped += 1;
                    continue;
                };
                leg2.truncate(i);
                leg3 = match run_leg3(fx, t1, t2, sc, &leg1, &leg2, budget, fig, stop)? {
                    Leg::Found(l) => l,
                    Leg::None => {
                        fig.spikes_dropped += 1;
                        continue;
                    }
                    Leg::OutOfBudget => {
                        out_of_budget = true;
                        return Ok(());
                    }
                };
                if is_spike(fx, &leg2, &leg3) {
                    fig.spikes_dropped += 1;
                    continue;
                }
                fig.spikes_repaired += 1;
            }
            let mut fields = leg1.clone();
            fields.extend_from_slice(&leg2);
            fields.extend_from_slice(&leg3);
            let len_dm = meters_dm(fx, &fields);
            fig.complete += 1;
            if tune.secant {
                let ratio = (len_dm / 10.0) / (st.beta * pair.est_m).max(1.0);
                st.beta = (st.beta * ratio.sqrt()).clamp(0.90, 1.30);
            }
            let d_dm = fx.d * 10.0;
            if (len_dm - d_dm).abs() > fx.e.params.tolerance * d_dm {
                continue;
            }
            fig.within_tol += 1;
            let (steps, step_field) = steps_of(v, &fields, fx.s_snap, fx.e_snap);
            let route = Route {
                start: Position { edge: fx.s_snap.edge, offset_m: fx.s_snap.offset_m },
                end: Position { edge: fx.e_snap.edge, offset_m: fx.e_snap.offset_m },
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
                familiarity: fx.r.familiarity,
                family: Family::Triangle,
            };
            let chk = if tune.turns {
                check_route_aligned_in(fx.e, fx.win, fx.team.map(|t| t.data), fx.r, &route)
            } else {
                check_route_in(fx.e, fx.win, fx.team.map(|t| t.data), fx.r, &route)
            };
            if !chk.violations.is_empty() {
                for vcode in &chk.violations {
                    *rejected.entry(*vcode).or_insert(0) += 1;
                }
                let forb_before = new_forb.len();
                for &i in &chk.sharp_at {
                    if i + 1 < step_field.len() && step_field[i] != step_field[i + 1] {
                        new_forb.push((step_field[i], step_field[i + 1]));
                    }
                }
                if tune.sharp_retry && new_forb.len() > forb_before {
                    sharp_now.push(key);
                }
                continue;
            }
            let sc = scored_route(fx.e, fx.r, fx.d, route, &chk);
            if !scored.iter().any(|x| x.key == sc.key) {
                scored.push(sc);
                st.kept += 1;
                fig.tri_scored += 1;
            }
        }
    })();
    // Fin de la phase : assez de triangles, ou plus de paire (neuve ni reprise) : la requête a fini ses triangles, sauf
    // les reprises de la passe suivante. Budget épuisé ou arrêt : les reprises non jouées restent pour la passe suivante.
    if st.kept >= u32::from(tune.tri_keep) {
        st.done = true;
    } else {
        if out_of_budget || res.is_err() {
            sharp_now.extend(retry_now.iter().filter(|k| !tried_now.contains(k)));
        }
        st.done = !out_of_budget && res.is_ok() && sharp_now.is_empty();
        st.retry = sharp_now;
    }
    fig.tri_rejected = rejected.into_iter().collect();
    fig.beta_final = st.beta;
    res
}

/// Choix des trois parcours (§15.7), `scored` trié par note puis clé et déjà filtré par la part d'équipe : le premier
/// est le meilleur de la figure tirée (sinon le meilleur de toutes) ; les deux autres, à recouvrement < 0,5 avec chaque
/// choisi, préfèrent une figure encore absente à note voisine.
pub(crate) fn choose<B: AsRef<[u8]>>(
    e: &Engine<B>,
    pool: Vec<Scored>,
    tri_first: bool,
    near_score: f64,
) -> Vec<Route> {
    let want = if tri_first { Family::Triangle } else { Family::Turnaround };
    let mut pool: Vec<Option<Scored>> = pool.into_iter().map(Some).collect();
    let first = pool
        .iter()
        .position(|s| s.as_ref().is_some_and(|s| s.route.family == want))
        .or_else(|| pool.iter().position(Option::is_some));
    let mut chosen: Vec<Route> = Vec::new();
    let Some(first) = first else { return chosen };
    chosen.push(pool[first].take().expect("présent").route);
    while chosen.len() < 3 {
        let free: Vec<usize> = (0..pool.len())
            .filter(|&i| {
                pool[i].as_ref().is_some_and(|s| chosen.iter().all(|c| overlap_share(e, c, &s.route) < 0.5))
            })
            .collect();
        let Some(&c) = free.first() else { break };
        let fam_of = |i: usize| pool[i].as_ref().expect("présent").route.family;
        let seen: Vec<Family> = chosen.iter().map(|r| r.family).collect();
        let score_of = |i: usize| pool[i].as_ref().expect("présent").score;
        let alt = free.iter().copied().find(|&i| !seen.contains(&fam_of(i)));
        let pick = match alt {
            Some(a) if score_of(a) <= score_of(c) + near_score => a,
            _ => c,
        };
        chosen.push(pool[pick].take().expect("présent").route);
    }
    chosen
}
