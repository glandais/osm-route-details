//! Sonde du coût d'une recherche point à point (lot 4, 4c, §4.3 et §4.5 de `ord-loops-lot4.md` ; relevé
//! `ord-loops-lot4c-sonde.md`). **Code de mesure**, compilé seulement avec la fonctionnalité `probe` (le banc) : rien
//! de ce module n'est appelé par la génération.
//!
//! Pour une boucle : les deux arbres de la génération, puis, par secteur de 30° dans le cap, le nœud de la bande de
//! distance dont le chemin aller est le plus agréable (`coût T1 / mètres T1` minimal, §4.2) ; les paires de ces nœuds
//! écartées de 40° à 90° vues du départ, leur longueur estimée `mètres T1(P1) + φ × vol d'oiseau(P1, P2) + mètres
//! T2(P2)` ; pour les paires gardées (estimation à ±5 % de D), un A* de P1 à P2 sur le graphe privé de la jambe
//! départ → P1, borné au disque de la requête, avec l'heuristique du §4.3 (vol d'oiseau × plus petit facteur de coût,
//! et les deux bornes triangulaires des arbres), puis le même sans les bornes triangulaires.

use std::cell::Cell;
use std::time::Instant;

use rustc_hash::FxHashSet;

use crate::engine::Engine;
use crate::geo::{turn_angle_deg, Local};
use crate::search::{
    astar, astar_p2p, make_anchor, tree_backward, tree_forward, Anchor, Ban, Ctx, Disc, Forbidden, P2pEnd,
    P2pOut, P2pTarget, ViewWs, Weighter, Workspace, BAN, INF, NONE, TAIL,
};
use crate::team::TeamCtx;
use crate::window::Window;
use crate::{Failure, Mode, Request};

const SECTORS: usize = 12;

/// Réglages de la sonde.
#[derive(Clone, Copy, Debug)]
pub struct ProbeParams {
    /// Bande de distance des points lointains, en part de D, sur les mètres de T1.
    pub band: (f64, f64),
    /// Sous-bandes : 1 = un nœud par secteur (le §4.2 à la lettre) ; k = le plus agréable de chacune de k sous-bandes
    /// égales.
    pub sub_bands: usize,
    /// Écart P1 / P2 vu du départ, degrés.
    pub sep_deg: (f64, f64),
    /// Estimation gardée à ± cette part de D.
    pub tolerance: f64,
    /// Paires cherchées au plus par requête (les plus agréables d'abord).
    pub max_pairs: usize,
    /// Budget d'une recherche : ce multiple des états fixés de T1 (le budget global d'aujourd'hui vaut 6).
    pub budget_mult: u64,
}

impl Default for ProbeParams {
    fn default() -> Self {
        Self {
            band: (0.27, 0.39),
            sub_bands: 1,
            sep_deg: (40.0, 90.0),
            tolerance: 0.05,
            max_pairs: 4,
            budget_mult: 6,
        }
    }
}

/// Une recherche P1 → P2.
#[derive(Clone, Copy, Debug, Default)]
pub struct LegProbe {
    pub us: f64,
    pub settled: u64,
    pub found: bool,
    pub exhausted: bool,
    pub meters_m: f64,
    pub cost: u32,
}

/// Une paire cherchée.
#[derive(Clone, Copy, Debug, Default)]
pub struct PairProbe {
    /// Les deux sommets sont les plus agréables de leur secteur (le §4.2 à la lettre).
    pub literal: bool,
    pub sep_deg: f64,
    pub phi: f64,
    pub est_m: f64,
    pub leg1_m: f64,
    pub crow_m: f64,
    pub leg3_m: f64,
    /// Avec les bornes triangulaires T1 / T2.
    pub tri: LegProbe,
    /// Vol d'oiseau seul.
    pub crow: LegProbe,
    /// Mètres du chemin T2 depuis P2 qui reprennent la jambe 1 hors queue ou la jambe 2 (avec bornes).
    pub leg3_reuse_m: f64,
    /// Heuristique au départ (P1), en part du coût trouvé : vol d'oiseau seul, et avec les bornes.
    pub h0_crow: f64,
    pub h0_tri: f64,
    /// Si le chemin T2 depuis P2 reprend les jambes 1 ou 2 : l'A* de la jambe d'arrivée sur leur graphe privé.
    pub leg3_astar: Option<LegProbe>,
}

/// Ce que la sonde rend pour une requête.
#[derive(Clone, Debug, Default)]
pub struct Probe {
    pub t1_settled: u64,
    pub t2_settled: u64,
    pub trees_us: f64,
    pub min_factor_us: f64,
    /// Plus petit coût par décimètre des arêtes de la fenêtre, et le médian (pour situer).
    pub min_factor: f64,
    pub median_factor: f64,
    /// Nœuds candidats (un par secteur et sous-bande), paires écartées de 40° à 90°, paires à ± la tolérance.
    pub candidates: u32,
    pub pairs: u32,
    pub pairs_in_tol: u32,
    /// Les mêmes, restreints au §4.2 à la lettre : le plus agréable de chaque secteur, toutes sous-bandes réunies.
    pub pairs_lit: u32,
    pub pairs_in_tol_lit: u32,
    /// Estimation / D de toutes les paires écartées (pour lire le rendement de la bande).
    pub est_over_d: Vec<f32>,
    pub probed: Vec<PairProbe>,
}

/// La sonde sur une boucle (`r.end` vide). Mêmes accroche, poids, disque et arbres que `generate_inner`.
pub fn probe_triangle_in<B: AsRef<[u8]>>(
    e: &Engine<B>,
    win: &Window,
    team: Option<TeamCtx<'_>>,
    r: &Request,
    ws: &mut Workspace,
    p: &ProbeParams,
) -> Result<Probe, Failure> {
    if r.end.is_some() {
        return Err(Failure::NoRoute(crate::NoRouteDetail::NoCandidate));
    }
    if r.mode != Mode::Discovery && team.is_none() {
        return Err(Failure::NoTeamData);
    }
    let d = r.distance_m;
    let ride = r.ride_type;
    let vi = ride.view_index();
    let v = &win.v;
    let w = Weighter::new(v, team, ride, r.mode, r.familiarity, r.off_team_cost_q8);
    let min_dm = (e.params.min_component_share * d * 10.0) as u64;
    let strict_tt = if w.strict() { w.tt() } else { None };
    let ok = |c: u32| -> bool {
        w.chain(c) != INF && strict_tt.is_none_or(|tt| tt.strict_len_of(v.gid[c as usize]) >= min_dm)
    };
    let s_snap =
        e.snap(win, r.start, &ok, min_dm).map_err(|()| Failure::WindowEscape)?.ok_or(Failure::StartTooFar)?;
    let esc = Cell::new(false);
    let sa: Anchor = make_anchor(e, v, &w, s_snap);
    let s_e7 = r.start.to_e7();
    let disc = Disc::new(s_e7, s_e7, d, true);
    let forb = Forbidden::default();
    let cx = Ctx { v, w: &w, disc: &disc, s: &sa, e: &sa, forb: &forb, esc: &esc };
    let mut out = Probe::default();
    let vws = &mut ws.views[vi];
    if vws.ban.len() < v.n_chains || vws.ban.len() > (2 * v.n_chains).max(64) {
        vws.ban = vec![0; v.n_chains];
        vws.ban_gen = 0;
    }
    let t0 = Instant::now();
    let halted = |_| if esc.get() { Failure::WindowEscape } else { Failure::Stopped };
    tree_forward(&cx, &mut vws.t1, &mut || false).map_err(halted)?;
    tree_backward(&cx, &mut vws.t2, &mut || false).map_err(halted)?;
    out.trees_us = t0.elapsed().as_secs_f64() * 1e6;
    let ViewWs { t1, t2, ta, ban, ban_gen, .. } = vws;
    let (t1, t2) = (&*t1, &*t2);
    out.t1_settled = t1.settled.len() as u64;
    out.t2_settled = t2.settled.len() as u64;

    // Le plus petit facteur de coût (par décimètre) des arêtes de la fenêtre : un balayage.
    let t0 = Instant::now();
    let mut minf = f64::INFINITY;
    for (c, &l) in v.len_dm.iter().enumerate().take(v.n_chains) {
        let wc = w.chain(c as u32);
        if wc != INF && l > 0 {
            minf = minf.min(f64::from(wc) / f64::from(l));
        }
    }
    out.min_factor_us = t0.elapsed().as_secs_f64() * 1e6;
    if !minf.is_finite() {
        minf = 0.0;
    }
    out.min_factor = minf;
    {
        // Le facteur médian des états fixés de T1, pour situer le plus petit (hors du temps mesuré).
        let mut fs: Vec<f64> = t1
            .settled
            .iter()
            .filter_map(|&f| {
                let c = f >> 1;
                let l = v.len_dm[c as usize];
                let wc = w.chain(c);
                (wc != INF && l > 0).then(|| f64::from(wc) / f64::from(l))
            })
            .collect();
        if !fs.is_empty() {
            let k = fs.len() / 2;
            fs.select_nth_unstable_by(k, f64::total_cmp);
            out.median_factor = fs[k];
        }
    }

    // Les points lointains : par secteur dans le cap et par sous-bande, le plus agréable.
    let loc = Local::new(s_e7);
    let d_dm = d * 10.0;
    let nb = p.sub_bands.max(1);
    let (lo, hi) = p.band;
    let in_bearing = |k: usize| match r.bearing {
        Some(b) => turn_angle_deg(b.degrees(), k as f64 * 30.0 + 15.0) <= 50.0,
        None => true,
    };
    // (agrément, nœud, état T1, état T2)
    let mut best: Vec<Option<(f64, u32, u32, u32)>> = vec![None; SECTORS * nb];
    for &node in &t1.reached {
        let Some(b2) = t2.node_best(node) else { continue };
        let b1 = t1.node_best(node).expect("atteint");
        let m1 = f64::from(t1.meters(b1));
        let s = m1 / d_dm;
        if s < lo || s >= hi {
            continue;
        }
        let ang = disc.bearing_of(v.pos[node as usize]);
        let sec = ((ang / 30.0) as usize) % SECTORS;
        if !in_bearing(sec) {
            continue;
        }
        let sb = (((s - lo) / (hi - lo) * nb as f64) as usize).min(nb - 1);
        let ratio = f64::from(t1.get(b1)) / m1.max(1.0);
        let slot = &mut best[sec * nb + sb];
        if slot.is_none_or(|(r0, n0, ..)| ratio < r0 || (ratio == r0 && node < n0)) {
            *slot = Some((ratio, node, b1, b2));
        }
    }
    // Le plus agréable de chaque secteur, toutes sous-bandes réunies : le candidat du §4.2 à la lettre.
    let mut lit_slot = [usize::MAX; SECTORS];
    for (sec, slot) in lit_slot.iter_mut().enumerate() {
        let mut bestr = f64::INFINITY;
        for sb in 0..nb {
            if let Some((r0, ..)) = best[sec * nb + sb] {
                if r0 < bestr {
                    bestr = r0;
                    *slot = sec * nb + sb;
                }
            }
        }
    }
    let cands: Vec<(f64, u32, u32, u32, bool)> = best
        .iter()
        .enumerate()
        .filter_map(|(k, x)| x.map(|(a, b, c, dd)| (a, b, c, dd, lit_slot[k / nb] == k)))
        .collect();
    out.candidates = cands.len() as u32;
    let xy = |n: u32| loc.xy(v.pos[n as usize]);
    let ang_of = |n: u32| disc.bearing_of(v.pos[n as usize]);
    // (pas à la lettre, agrément moyen, i P1, j P2, écart, φ, estimation m, vol d'oiseau P1-P2 m)
    let mut pairs: Vec<PairKey> = Vec::new();
    for i in 0..cands.len() {
        for j in (i + 1)..cands.len() {
            let (ai, aj) = (ang_of(cands[i].1), ang_of(cands[j].1));
            let sep = turn_angle_deg(ai, aj);
            if sep < p.sep_deg.0 || sep > p.sep_deg.1 {
                continue;
            }
            // Sens de marche : P2 dans le sens des aiguilles d'une montre après P1.
            let (i1, i2) = if (aj - ai).rem_euclid(360.0) <= 180.0 { (i, j) } else { (j, i) };
            let (n1, n2) = (cands[i1].1, cands[i2].1);
            let (p1, p2) = (xy(n1), xy(n2));
            let (r1, r2) = (p1.0.hypot(p1.1).max(1.0), p2.0.hypot(p2.1).max(1.0));
            let (m1, m2b) =
                (f64::from(t1.meters(cands[i1].2)) / 10.0, f64::from(t1.meters(cands[i2].2)) / 10.0);
            let phi = (m1 / r1 + m2b / r2) / 2.0;
            let crow = (p1.0 - p2.0).hypot(p1.1 - p2.1);
            let back = f64::from(t2.meters(cands[i2].3)) / 10.0;
            let est = m1 + phi * crow + back;
            let lit = cands[i1].4 && cands[i2].4;
            let in_tol = (est - d).abs() <= p.tolerance * d;
            out.pairs += 1;
            out.pairs_lit += u32::from(lit);
            out.est_over_d.push((est / d) as f32);
            if in_tol {
                out.pairs_in_tol += 1;
                out.pairs_in_tol_lit += u32::from(lit);
                pairs.push((!lit, (cands[i1].0 + cands[i2].0) / 2.0, i1, i2, sep, phi, est, crow));
            }
        }
    }
    // Les paires à la lettre d'abord, puis les plus agréables.
    pairs.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)).then(a.3.cmp(&b.3)));
    let tail_dm = (e.params.tail_reuse_share * d * 10.0) as u32;
    for &(not_lit, _, i1, i2, sep, phi, est, crow) in pairs.iter().take(p.max_pairs) {
        let (_, n1, b1, _, _) = cands[i1];
        let (_, n2, _, b2_2, _) = cands[i2];
        let leg1 = t1.path_to(b1);
        let last = *leg1.last().expect("chemin non vide");
        let last_pred = if leg1.len() >= 2 { leg1[leg1.len() - 2] } else { NONE };
        *ban_gen += 1;
        if *ban_gen >= (1 << 28) {
            ban.iter_mut().for_each(|x| *x = 0);
            *ban_gen = 1;
        }
        for &f in &leg1 {
            let c = (f >> 1) as usize;
            let bits = if t1.meters(f) <= tail_dm { TAIL } else { BAN };
            let cur = if ban[c] >> 3 == *ban_gen { ban[c] & 7 } else { 0 };
            let bits = if cur & BAN != 0 { BAN } else { bits };
            ban[c] = (*ban_gen << 3) | bits;
        }
        let b = Ban::plain(ban, *ban_gen);
        let budget = p.budget_mult * t1.settled.len() as u64 + 20_000;
        let tgt = P2pTarget {
            node: n2,
            xy: xy(n2),
            t1: t1.node_best(n2).map_or(0, |x| t1.get(x)),
            t2: t2.node_best(n2).map_or(0, |x| t2.get(x)),
            f_min: minf,
            slack: 0,
        };
        let halted = |_| if esc.get() { Failure::WindowEscape } else { Failure::Stopped };
        let run = |ta: &mut crate::search::Tree,
                   tri: bool|
         -> Result<(LegProbe, Option<Vec<u32>>), Failure> {
            let t0 = Instant::now();
            let mut left = u64::MAX / 2;
            let r =
                astar_p2p(&cx, ta, t1, t2, last, last_pred, &tgt, &b, tri, budget, &mut left, &mut || false)
                    .map_err(halted)?;
            Ok(leg_of(&r, t0.elapsed().as_secs_f64() * 1e6))
        };
        // Heuristique au départ de la jambe 2 (nœud P1), pour situer.
        let h_at = |tri: bool| -> f64 { f64::from(tgt.h(&cx, t1, t2, n1, tri)) };
        let (tri_leg, tri_path) = run(ta, true)?;
        let (crow_leg, _) = run(ta, false)?;
        if esc.get() {
            return Err(Failure::WindowEscape);
        }
        let mut pp = PairProbe {
            literal: !not_lit,
            sep_deg: sep,
            phi,
            est_m: est,
            leg1_m: f64::from(t1.meters(last)) / 10.0,
            crow_m: crow,
            leg3_m: f64::from(t2.meters(b2_2)) / 10.0,
            tri: tri_leg,
            crow: crow_leg,
            ..PairProbe::default()
        };
        let found_cost = if tri_leg.found { f64::from(tri_leg.cost).max(1.0) } else { 0.0 };
        if found_cost > 0.0 {
            pp.h0_crow = h_at(false) / found_cost;
            pp.h0_tri = h_at(true) / found_cost;
        }
        if let Some(leg2) = tri_path {
            let leg2_chains: FxHashSet<u32> = leg2.iter().map(|&f| f >> 1).collect();
            let mut ret = t2.path_to(b2_2);
            ret.reverse();
            let mut reuse = 0u64;
            for &f in &ret {
                let c = f >> 1;
                if b.bits(c) & BAN != 0 || leg2_chains.contains(&c) {
                    reuse += u64::from(v.len_dm[c as usize]);
                }
            }
            pp.leg3_reuse_m = reuse as f64 / 10.0;
            if reuse > 0 {
                // La jambe d'arrivée du §4.3 : l'A* d'heuristique T2 (celui des retours d'aujourd'hui), sur le graphe
                // privé des jambes 1 (hors queue) et 2, avec son propre budget.
                *ban_gen += 1;
                if *ban_gen >= (1 << 28) {
                    ban.iter_mut().for_each(|x| *x = 0);
                    *ban_gen = 1;
                }
                for &f in &leg1 {
                    let c = (f >> 1) as usize;
                    let bits = if t1.meters(f) <= tail_dm { TAIL } else { BAN };
                    let cur = if ban[c] >> 3 == *ban_gen { ban[c] & 7 } else { 0 };
                    let bits = if cur & BAN != 0 { BAN } else { bits };
                    ban[c] = (*ban_gen << 3) | bits;
                }
                for &f in &leg2 {
                    ban[(f >> 1) as usize] = (*ban_gen << 3) | BAN;
                }
                let b3 = Ban::plain(ban, *ban_gen);
                let last2 = *leg2.last().expect("jambe non vide");
                let pred2 = if leg2.len() >= 2 { leg2[leg2.len() - 2] } else { last };
                let mut budget3 = budget;
                let mut exhausted3 = false;
                let t0 = Instant::now();
                let found =
                    astar(&cx, ta, t2, last2, pred2, &b3, &mut budget3, &mut exhausted3, &mut || false)
                        .map_err(|_| if esc.get() { Failure::WindowEscape } else { Failure::Stopped })?;
                pp.leg3_astar = Some(LegProbe {
                    us: t0.elapsed().as_secs_f64() * 1e6,
                    settled: budget - budget3,
                    found: found.is_some(),
                    exhausted: exhausted3,
                    meters_m: found.as_ref().map_or(0.0, |f| f64::from(f.meters) / 10.0),
                    cost: 0,
                });
            }
        }
        out.probed.push(pp);
    }
    Ok(out)
}

/// Une paire gardée : (pas à la lettre, agrément moyen, i P1, j P2, écart, φ, estimation m, vol d'oiseau P1-P2 m).
type PairKey = (bool, f64, usize, usize, f64, f64, f64, f64);

/// La mesure d'une recherche point à point.
fn leg_of(r: &P2pOut, us: f64) -> (LegProbe, Option<Vec<u32>>) {
    let found = r.end == P2pEnd::Found;
    let leg = LegProbe {
        us,
        settled: r.settled,
        found,
        exhausted: matches!(r.end, P2pEnd::Capped | P2pEnd::BudgetOut),
        meters_m: if found { f64::from(r.meters) / 10.0 } else { 0.0 },
        cost: if found { r.cost } else { 0 },
    };
    (leg, r.fields.clone())
}
