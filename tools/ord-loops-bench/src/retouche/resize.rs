//! Les demandes de `resize` (note : `ord-retouche-lot0.md` §5) : viser une distance par une greffe locale.
//!
//! La recherche est la lentille d'aller simple de production, par `generate_with_figures_in`, du nœud de la couture `i`
//! au nœud de la couture `j`, avec `D = ℓ + Δ` et le masque des arêtes gardées posé par `Workspace::set_keep_out`. Le banc
//! tire les fenêtres, choisit parmi tous les candidats, recolle et contrôle (C1 à C3). **Aucune position, aucun id.**

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;

use ord_graph_match::walk::Walk;
use ord_loops::probe_graft::KeepOut;
use ord_loops::{generate_with_figures_in, Failure, Request, Route, TeamCtx, Workspace};
use rustc_hash::FxHashSet;
use serde::Serialize;
use xxhash_rust::xxh3::xxh3_64;

use super::control::{self, node_latlon, normalize_between, OrigCache};
use super::draw::{self, Win};
use super::posed::Posed;
use super::{Cursor, Env};
use crate::likeness::play::route_hash;
use crate::record::failure_code;

/// Une fenêtre jouée.
#[derive(Clone, Debug, Serialize, Default)]
pub struct WinLine {
    pub ell_m: f64,
    pub family: u8,
    /// `routes`, ou un code d'échec du moteur, ou `seam_snap_off`, ou `panic`.
    pub outcome: String,
    pub routes: usize,
    pub gen_ms: f64,
    pub masked_chains: usize,
}

/// La proposition retenue.
#[derive(Clone, Debug, Serialize, Default)]
pub struct Best {
    /// Écart relatif de `L′` à `D′`.
    pub err: f64,
    pub kept_share: f64,
    /// Mètres du tronçon neuf hors des arêtes de l'original.
    pub changed_m: f64,
    pub new_len_m: f64,
    pub team_share: Option<f64>,
    pub angle_in_deg: Option<f64>,
    pub angle_out_deg: Option<f64>,
    pub family: u8,
    pub c3_zone_m: f64,
    pub checked_m: f64,
    pub unchecked_m: f64,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct RsLine {
    pub team: String,
    pub rank: u32,
    pub ty: &'static str,
    pub shape: &'static str,
    pub cursor: f64,
    pub delta: f64,
    pub beyond_limit: bool,
    /// Longueur de la trace et distance visée, m.
    pub l_m: f64,
    pub target_m: f64,
    /// Poids de la strate (population / échantillon) pour les agrégats.
    pub weight: f64,
    /// `ok`, `off_target`, `no_window`, `all_refused`, `engine_fail`, `panic`.
    pub outcome: String,
    /// L'échec dominant des fenêtres quand `engine_fail`.
    pub fail: Option<String>,
    pub windows: Vec<WinLine>,
    pub candidates: usize,
    pub proposals: usize,
    pub refused: usize,
    /// Codes de refus de C1 et C2, comptés sur les candidats refusés.
    pub refused_codes: BTreeMap<String, u32>,
    pub reuse_refused: usize,
    pub best: Option<Best>,
    /// Somme des temps de génération des fenêtres, ms (le temps qu'attend la personne).
    pub total_ms: f64,
    /// Temps de génération cumulé jusqu'à la première fenêtre qui rend un candidat à 3 % ou moins.
    pub first_ok_ms: Option<f64>,
    pub ctl_ms: f64,
    /// Contrôles réellement joués (C1 à C3), et le plafond `max_controls` a été atteint avec des candidats restants.
    pub controls_run: usize,
    pub capped: bool,
    pub fp: String,
}

struct Cand {
    win: usize,
    order: usize,
    route: Route,
    err: f64,
    kept_share: f64,
    new_len: f64,
}

/// Les arêtes interdites d'une fenêtre : préfixe et suffixe gardés, moins la zone des coutures.
fn masks(p: &Posed, margin: f64, ji: usize, jj: usize) -> (Vec<u32>, Vec<u32>) {
    let free: Vec<u32> = control::zone_edges(p, ji, margin, true)
        .into_iter()
        .chain(control::zone_edges(p, jj, margin, false))
        .collect();
    let freeset: FxHashSet<u32> = free.iter().copied().collect();
    let mask: Vec<u32> = p.walk.legs[..=ji]
        .iter()
        .chain(&p.walk.legs[jj + 1..])
        .map(|l| l.edge)
        .filter(|e| !freeset.contains(e))
        .collect();
    (mask, free)
}

/// Joue une demande : une trace, un curseur, un `δ`.
pub fn play_request(
    env: &Env<'_>,
    p: &Posed,
    cursor: &Cursor,
    delta: f64,
    weight: f64,
    ws: &mut Workspace,
    cache: &OrigCache,
) -> RsLine {
    let mut line = RsLine {
        team: p.letter.clone(),
        rank: p.rank,
        ty: super::continuity::type_name(p.ty),
        shape: match p.shape {
            super::continuity::Shape::Loop => "loop",
            super::continuity::Shape::OneWay => "one_way",
        },
        cursor: cursor.value,
        delta,
        beyond_limit: delta.abs() > 0.30,
        l_m: p.l_m,
        target_m: p.l_m * (1.0 + delta),
        weight,
        ..RsLine::default()
    };
    let gf = env.ctl.gf;
    let cand_seams = p.both(env.margin_m);
    let crow = |i: usize, j: usize| {
        let (a, b) = (node_latlon(gf, p.seams[i].seam.node), node_latlon(gf, p.seams[j].seam.node));
        ord_loops_check::haversine_m((a.lat, a.lon), (b.lat, b.lon))
    };
    let wins: Vec<Win> = draw::windows(p, &cand_seams, delta, env.max_windows, env.seed, &crow);
    if wins.is_empty() {
        line.outcome = "no_window".into();
        return line;
    }
    let proto = cursor.request(p, env.seed);
    let d_target = p.l_m * (1.0 + delta);
    let big_delta = delta * p.l_m;
    let mut cands: Vec<Cand> = Vec::new();
    let mut fails: BTreeMap<String, u32> = BTreeMap::new();
    let mut cum_ms = 0.0;
    let mut first_ok: Option<f64> = None;
    for (wi, w) in wins.iter().enumerate() {
        let (si, sj) = (&p.seams[w.i], &p.seams[w.j]);
        let req = Request {
            start: node_latlon(gf, si.seam.node),
            end: Some(node_latlon(gf, sj.seam.node)),
            distance_m: w.ell_m + big_delta,
            ..proto.clone()
        };
        let mut wl = WinLine { ell_m: w.ell_m, family: w.family, ..WinLine::default() };
        let (mask, free) = masks(p, env.margin_m, si.seam.join, sj.seam.join);
        let t0 = Instant::now();
        let (mut win, mut wt) = env.wins.acquire(env.team_key, env.team, &req, None);
        let mut run = |win: &ord_loops::Window, wt: &ord_loops::WindowTeam, ws: &mut Workspace| {
            if !env.no_keep_out {
                let ko = KeepOut::build(win, mask.iter().copied(), free.iter().copied());
                wl.masked_chains = ko.masked_chains();
                ws.set_keep_out(Some(ko));
            }
            let t = Instant::now();
            let r = catch_unwind(AssertUnwindSafe(|| {
                let tc = TeamCtx { data: env.team, win: wt };
                generate_with_figures_in(env.engine, win, Some(tc), &req, ws, &mut || false)
            }));
            (r, t.elapsed().as_secs_f64() * 1000.0)
        };
        let (mut res, mut gen_ms) = run(&win, &wt, ws);
        if matches!(&res, Ok((Err(Failure::WindowEscape), ..))) {
            (win, wt) = env.wins.acquire_wide(env.team, &req, None);
            let first_ms = gen_ms;
            (res, gen_ms) = run(&win, &wt, ws);
            gen_ms += first_ms;
        }
        let _ = t0;
        wl.gen_ms = gen_ms;
        cum_ms += gen_ms;
        match res {
            Err(_) => {
                wl.outcome = "panic".into();
                *fails.entry("panic".into()).or_default() += 1;
            }
            Ok((Err(f), ..)) => {
                wl.outcome = failure_code(&f);
                *fails.entry(wl.outcome.clone()).or_default() += 1;
            }
            Ok((Ok(g), diag, _)) => {
                if diag.start_snap_m > 0.5 || diag.end_snap_m > 0.5 {
                    wl.outcome = "seam_snap_off".into();
                    *fails.entry(wl.outcome.clone()).or_default() += 1;
                } else {
                    wl.outcome = "routes".into();
                    for (k, r) in g.routes.iter().enumerate() {
                        let Some(route) = normalize_between(gf, r, si.seam.node, sj.seam.node) else {
                            *fails.entry("seam_snap_off".into()).or_default() += 1;
                            continue;
                        };
                        wl.routes += 1;
                        let new_len = r.length_m;
                        let l_new = si.s_m + new_len + (p.l_m - sj.s_m);
                        let err = (l_new - d_target).abs() / d_target;
                        if err <= 0.03 && first_ok.is_none() {
                            first_ok = Some(cum_ms);
                        }
                        cands.push(Cand {
                            win: wi,
                            order: k,
                            route,
                            err,
                            kept_share: (si.s_m + p.l_m - sj.s_m) / l_new.max(1.0),
                            new_len,
                        });
                    }
                    if wl.routes == 0 {
                        wl.outcome = "seam_snap_off".into();
                    }
                }
            }
        }
        line.windows.push(wl);
    }
    line.total_ms = cum_ms;
    line.first_ok_ms = first_ok;
    line.candidates = cands.len();
    if cands.is_empty() {
        if line.windows.iter().all(|w| w.outcome == "routes") {
            line.outcome = "all_refused".into();
        } else {
            line.outcome = "engine_fail".into();
            line.fail = fails.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))).map(|x| x.0.clone());
        }
        return line;
    }
    // Le tri de la note : d'abord à 3 % ou moins, puis l'écart arrondi à 0,5 %, puis la part gardée, puis le rang.
    cands.sort_by(|a, b| {
        (a.err > 0.03)
            .cmp(&(b.err > 0.03))
            .then(((a.err / 0.005).round() as i64).cmp(&((b.err / 0.005).round() as i64)))
            .then(b.kept_share.total_cmp(&a.kept_share))
            .then(a.win.cmp(&b.win))
            .then(a.order.cmp(&b.order))
    });
    // Les contrôles : jusqu'à trois propositions de fenêtres distinctes ; une refusée est remplacée par la suivante.
    let t0 = Instant::now();
    let mut proposals: Vec<(usize, Best, Route)> = Vec::new();
    let mut used_wins: FxHashSet<usize> = FxHashSet::default();
    // Le plafond compte les contrôles réellement joués : un candidat sauté (fenêtre déjà proposée) n'use pas le budget.
    for c in &cands {
        if proposals.len() >= 3 {
            break;
        }
        if used_wins.contains(&c.win) {
            continue;
        }
        if line.controls_run >= env.max_controls {
            line.capped = true;
            break;
        }
        line.controls_run += 1;
        let w = &wins[c.win];
        let (si, sj) = (&p.seams[w.i], &p.seams[w.j]);
        let new = Walk::from_route(gf, &c.route);
        let req_seg = control::seg_request(
            &proto,
            node_latlon(gf, si.seam.node),
            Some(node_latlon(gf, sj.seam.node)),
            w.ell_m + big_delta,
        );
        let rep = env.ctl.checker.check(&req_seg, &c.route);
        let mut refused: Vec<String> =
            control::codes_of(&rep).into_iter().map(|c| format!("C1:{c}")).collect();
        let sp = control::splice(p, si.seam.join, Some(sj.seam.join), &new, si.seam.node, Some(sj.seam.node));
        let c2 = control::c2(&env.ctl, cache, p, &sp, si.seam.join, Some(sj.seam.join), &proto);
        refused.extend(c2.new_codes.iter().map(|c| format!("C2:{c}")));
        let (mask, free) = masks(p, env.margin_m, si.seam.join, sj.seam.join);
        let free: FxHashSet<u32> = free.into_iter().collect();
        let kept: FxHashSet<u32> = mask.into_iter().collect();
        let c3 = control::c3(gf, &c.route.steps, &kept, &free);
        if c3.outside_m > 0.0 {
            line.reuse_refused += 1;
            refused.push("C3:REUSE".into());
        }
        if !refused.is_empty() {
            line.refused += 1;
            for r in refused {
                *line.refused_codes.entry(r).or_default() += 1;
            }
            continue;
        }
        let changed: f64 = c
            .route
            .steps
            .iter()
            .filter(|s| !p.edge_set.contains(&s.base_edge))
            .map(|s| f64::from(gf.edge(s.base_edge).len_dm) / 10.0)
            .sum();
        let (angle_in, angle_out) = control::seam_angles(gf, p, &sp);
        used_wins.insert(c.win);
        proposals.push((
            c.win,
            Best {
                err: c.err,
                kept_share: c.kept_share,
                changed_m: changed,
                new_len_m: c.new_len,
                team_share: (rep.length_m > 0.0).then(|| rep.team_m / rep.length_m),
                angle_in_deg: angle_in,
                angle_out_deg: angle_out,
                family: w.family,
                c3_zone_m: c3.zone_m,
                checked_m: c2.checked_m,
                unchecked_m: c2.unchecked_m,
            },
            c.route.clone(),
        ));
    }
    line.ctl_ms = t0.elapsed().as_secs_f64() * 1000.0;
    line.proposals = proposals.len();
    match proposals.first() {
        None => line.outcome = "all_refused".into(),
        Some((wi, b, route)) => {
            line.outcome = if b.err <= 0.03 { "ok" } else { "off_target" }.into();
            let mut fp = Vec::new();
            fp.extend_from_slice(&(*wi as u64).to_le_bytes());
            fp.extend_from_slice(&route_hash(route).to_le_bytes());
            line.fp = format!("{:016x}", xxh3_64(&fp));
            line.best = Some(b.clone());
        }
    }
    line
}
