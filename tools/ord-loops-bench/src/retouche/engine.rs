//! `retouche-play --impl engine` : les mêmes essais que le lot 0, joués par l'API publique du moteur (lot 7a, note
//! `ord-retouche-lot1.md` §11) au lieu des prototypes.
//!
//! - `move_end` : le même `E′` (`draw.rs` inchangé), un appel `move_end_in` dans la fenêtre de `plan_move_end` prise au cache du
//!   banc.
//! - `resize` : les mêmes `δ` et le même échantillon de traces ; le banc ne tire plus de fenêtres, `plan_resize` et
//!   `resize_in` font le choix.
//! - les **contrôles** C1, C2 (plages entières) et C3 sont ceux du lot 0, sur `Retouched` (le recollé se fabrique depuis
//!   `from_seam`, `to_seam` et `new`) ; C3 est plus strict qu'au lot 0 : aucune arête gardée n'est libre, la zone des coutures
//!   non plus.
//!
//! Le temps de K4 est celui de l'appel du moteur (contrôles internes compris), fenêtre prise au cache ; le temps des
//! contrôles indépendants est à part (`indep_ms`). **Aucune position, aucun id** dans ce qui sort.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;

use ord_graph_match::walk::Walk;
use ord_loops::{
    LatLon, PosedRoute, Refused, Retouch, RetouchFailure, RetouchKind, RetouchRequest, Retouched, TeamCtx,
    Workspace,
};
use ord_loops_check::haversine_m;
use rustc_hash::FxHashSet;
use xxhash_rust::xxh3::xxh3_64;

use super::control::{self, node_latlon, OrigCache};
use super::draw::Trial;
use super::move_end::{MeLine, Variant};
use super::posed::Posed;
use super::resize::{Best, RsLine, WinLine};
use super::{Cursor, Env};
use crate::likeness::play::route_hash;
use crate::record::failure_code;

/// Ce que le moteur lit de la requête du curseur.
fn rreq(proto: &ord_loops::Request) -> RetouchRequest {
    RetouchRequest {
        ride_type: proto.ride_type,
        mode: proto.mode,
        familiarity: proto.familiarity,
        off_team_cost_q8: proto.off_team_cost_q8,
        seed: proto.seed,
    }
}

fn failure_name(f: &RetouchFailure) -> (String, Option<String>) {
    use RetouchFailure as F;
    match f {
        F::Pose(_) => ("pose".into(), None),
        F::NoSeam => ("no_seam".into(), None),
        F::EndTooFar => ("end_too_far".into(), None),
        F::NoSeamReached => ("no_seam_reached".into(), None),
        F::JunctionCut => ("junction_cut".into(), None),
        F::ChangeTooSmall => ("change_too_small".into(), None),
        F::ChangeTooLarge => ("change_too_large".into(), None),
        F::NoEconomy => ("no_economy".into(), None),
        F::NoWindow => ("no_window".into(), None),
        F::NoProposal { dominant } => ("engine_fail".into(), dominant.as_ref().map(failure_code)),
        F::AllRefused { dominant } => ("all_refused".into(), Some(dominant.code().to_string())),
        F::OffTarget { .. } => ("off_target".into(), None),
        F::Timeout => ("timeout".into(), None),
        F::NoTeamData => ("no_team_data".into(), None),
        F::WindowEscape => ("window_escape".into(), None),
        F::Stopped => ("stopped".into(), None),
        F::TooLong => ("too_long".into(), None),
    }
}

fn stratum(p: &Posed) -> (&'static str, &'static str) {
    (
        super::continuity::type_name(p.ty),
        match p.shape {
            super::continuity::Shape::Loop => "loop",
            super::continuity::Shape::OneWay => "one_way",
        },
    )
}

fn diag_codes(d: &ord_loops::RetouchDiagnostics) -> BTreeMap<String, u32> {
    d.spliced_refused.iter().map(|(v, n)| (v.code().to_string(), *n)).collect()
}

/// La pose du moteur d'une trace du banc : la même marche et la même polyligne.
pub fn pose_of(
    engine: &crate::likeness::play::Eng,
    p: &Posed,
    tune: &ord_loops::PoseTune,
) -> Result<PosedRoute, ord_loops::PoseError> {
    use ord_graph_match::walk::Join;
    use ord_loops::{PosedInput, PosedJoin, PosedLeg};
    let legs: Vec<PosedLeg> = p
        .walk
        .legs
        .iter()
        .map(|l| PosedLeg { edge: l.edge, reverse: l.reverse, lo: l.lo, hi: l.hi })
        .collect();
    let joins: Vec<PosedJoin> = p
        .walk
        .joins
        .iter()
        .map(|j| match j {
            Join::Node(n) => PosedJoin::Node(*n),
            Join::Touch => PosedJoin::Touch,
            Join::Hole(d) => PosedJoin::Hole { chord_m: *d },
        })
        .collect();
    let line: Vec<LatLon> = p.line.pts.iter().map(|&(lat, lon)| LatLon::new(lat, lon)).collect();
    PosedRoute::pose(engine, &PosedInput { ride_type: p.ride, legs: &legs, joins: &joins, line: &line }, tune)
}

// --- move_end ----------------------------------------------------------------------------------------------------------

type Called = Result<Result<Retouch, Box<Refused>>, ()>;

/// Joue un essai de `move_end`.
pub fn play_trial(
    env: &Env<'_>,
    p: &Posed,
    pr: &PosedRoute,
    cursor: &Cursor,
    trial: &Trial,
    ws: &mut Workspace,
    cache: &OrigCache,
) -> MeLine {
    let (ty, shape) = stratum(p);
    let mut line = MeLine {
        team: p.letter.clone(),
        rank: p.rank,
        ty,
        shape,
        cursor: cursor.value,
        kind: trial.kind.label(),
        ..MeLine::default()
    };
    if (0..pr.seams().len()).all(|k| !pr.valid_before(k)) {
        line.outcome = "no_seam".into();
        return line;
    }
    let end = *p.line.pts.last().expect("polyligne non vide");
    line.crow_m = haversine_m(end, trial.pt);
    let proto = cursor.request(p, env.seed);
    let rq = rreq(&proto);
    let eprime = LatLon::new(trial.pt.0, trial.pt.1);
    let step = env.wins.step_m();
    let t0 = Instant::now();
    let plan = env.engine.plan_move_end(pr, eprime, &env.tune, step, 0.0);
    let (mut win, mut wt) = env.wins.acquire_plan(env.team_key, env.team, &plan, None);
    line.win_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let call =
        |win: &ord_loops::Window, wt: &ord_loops::WindowTeam, ws: &mut Workspace, acc: &mut f64| -> Called {
            let t = Instant::now();
            let r = catch_unwind(AssertUnwindSafe(|| {
                let tc = TeamCtx { data: env.team, win: wt };
                ord_loops::move_end_in(env.engine, win, Some(tc), pr, eprime, &rq, &env.tune, ws, &mut || {
                    false
                })
            }))
            .map_err(|_| ());
            *acc += t.elapsed().as_secs_f64() * 1000.0;
            r
        };
    let mut call_ms = 0.0;
    let mut res = call(&win, &wt, ws, &mut call_ms);
    if matches!(&res, Ok(Err(r)) if r.failure == RetouchFailure::WindowEscape) {
        let extra = env.engine.longest_chain_m(pr.ride_type().view_index()) + 10_000.0;
        let wide = env.engine.plan_move_end(pr, eprime, &env.tune, step, extra);
        (win, wt) = env.wins.acquire_plan_wide(env.team, wide.view, &wide, None);
        res = call(&win, &wt, ws, &mut call_ms);
    }
    line.call_ms = call_ms;
    let res = match res {
        Err(()) => {
            line.outcome = "panic".into();
            return finish(line);
        }
        Ok(r) => r,
    };
    match res {
        Err(f) => {
            let d = &f.diagnostics;
            line.outcome = failure_name(&f.failure).0;
            line.junction_cut = d.junction_cut;
            line.spliced_refused = diag_codes(d);
            line.t2_settled = d.settled;
            line.passes = d.passes;
            line.rho_m = d.radius_m;
            line.widened = d.widened;
            line.seams_refused_window = d.seams_refused_window;
            line.short_leads = d.short_leads;
        }
        Ok(out) => {
            let d = &out.diagnostics;
            line.rho_m = d.radius_m;
            line.widened = d.widened;
            line.passes = d.passes;
            line.t2_settled = d.settled;
            line.gen_ms = d.elapsed_us as f64 / 1000.0;
            line.junction_cut = d.junction_cut;
            line.spliced_refused = diag_codes(d);
            line.seams_refused_window = d.seams_refused_window;
            line.short_leads = d.short_leads;
            let best = &out.proposals[0];
            if best.kind == RetouchKind::Truncate {
                line.outcome = "truncated".into();
                line.accepted = true;
                line.best = Some(Variant {
                    abandon_m: best.abandon_m.unwrap_or(0.0),
                    raccord_m: 0.0,
                    raccord_over_crow: 0.0,
                    length_ratio: best.length_m / p.l_m.max(1.0),
                });
            } else {
                line.outcome = "ok".into();
                record(env, p, pr, &proto, trial, &out.proposals, cache, &mut line);
            }
        }
    }
    finish(line)
}

fn finish(mut line: MeLine) -> MeLine {
    if line.fp.is_empty() {
        line.fp = format!("{:016x}", xxh3_64(line.outcome.as_bytes()));
    }
    line
}

fn variant_of(p: &Posed, env: &Env<'_>, pr: &PosedRoute, r: &Retouched, eprime: (f64, f64)) -> Variant {
    let seam = pr.seams()[r.from_seam.expect("move_end a une couture")];
    let ll = node_latlon(env.ctl.gf, seam.node);
    let crow = haversine_m((ll.lat, ll.lon), eprime).max(1.0);
    Variant {
        abandon_m: r.abandon_m.unwrap_or(0.0),
        raccord_m: r.new.length_m,
        raccord_over_crow: r.new.length_m / crow,
        length_ratio: r.length_m / p.l_m.max(1.0),
    }
}

#[allow(clippy::too_many_arguments)]
fn record(
    env: &Env<'_>,
    p: &Posed,
    pr: &PosedRoute,
    proto: &ord_loops::Request,
    trial: &Trial,
    proposals: &[Retouched],
    cache: &OrigCache,
    line: &mut MeLine,
) {
    let pick = |v: ord_loops::MoveEndVariant| {
        proposals
            .iter()
            .find(|r| r.kind == RetouchKind::MoveEnd(v))
            .map(|r| variant_of(p, env, pr, r, trial.pt))
    };
    let best = &proposals[0];
    line.best = pick(ord_loops::MoveEndVariant::Best);
    line.keep_all = pick(ord_loops::MoveEndVariant::KeepAll);
    line.nearest = pick(ord_loops::MoveEndVariant::Nearest);
    // Les contrôles indépendants de la variante `best`.
    let t0 = Instant::now();
    let seam = pr.seams()[best.from_seam.expect("couture")];
    let gf = env.ctl.gf;
    let new = Walk::from_route(gf, &best.new);
    let eprime = LatLon::new(trial.pt.0, trial.pt.1);
    let req_seg = control::seg_request(proto, node_latlon(gf, seam.node), Some(eprime), best.new.length_m);
    let rep = env.ctl.checker.check(&req_seg, &best.new);
    line.c1 = control::codes_of(&rep).into_iter().map(String::from).collect();
    line.team_share = (rep.length_m > 0.0).then(|| rep.team_m / rep.length_m);
    let sp = control::splice(p, seam.join, None, &new, seam.node, None);
    let c2 = control::c2(&env.ctl, cache, p, &sp, seam.join, None, proto);
    line.c2 = c2.new_codes.iter().map(|c| (*c).to_string()).collect();
    line.checked_m = c2.checked_m;
    line.unchecked_m = c2.unchecked_m;
    // Aucune arête gardée n'est libre : la zone des coutures non plus.
    let kept: FxHashSet<u32> = p.walk.legs[..=seam.join].iter().map(|l| l.edge).collect();
    let c3 = control::c3(gf, &best.new.steps, &kept, &FxHashSet::default());
    line.c3_outside_m = c3.outside_m;
    line.c3_zone_m = 0.0;
    line.checked_m = c2.checked_m;
    line.angle_deg = control::seam_angles(gf, p, &sp).0;
    line.indep_ms = t0.elapsed().as_secs_f64() * 1000.0;
    line.accepted = line.c1.is_empty() && line.c2.is_empty() && c3.outside_m <= 0.0;
    let mut fp = Vec::new();
    fp.extend_from_slice(&(best.from_seam.unwrap_or(0) as u64).to_le_bytes());
    fp.extend_from_slice(&route_hash(&best.new).to_le_bytes());
    fp.extend_from_slice(&u64::from(line.accepted).to_le_bytes());
    line.fp = format!("{:016x}", xxh3_64(&fp));
}

// --- resize ------------------------------------------------------------------------------------------------------------

/// Joue une demande de `resize`.
#[allow(clippy::too_many_arguments)]
pub fn play_request(
    env: &Env<'_>,
    p: &Posed,
    pr: &PosedRoute,
    cursor: &Cursor,
    delta: f64,
    weight: f64,
    ws: &mut Workspace,
    cache: &OrigCache,
) -> RsLine {
    let (ty, shape) = stratum(p);
    let mut line = RsLine {
        team: p.letter.clone(),
        rank: p.rank,
        ty,
        shape,
        cursor: cursor.value,
        delta,
        beyond_limit: delta.abs() > 0.30,
        l_m: p.l_m,
        target_m: p.l_m * (1.0 + delta),
        weight,
        ..RsLine::default()
    };
    let gf = env.ctl.gf;
    let proto = cursor.request(p, env.seed);
    let rq = rreq(&proto);
    let target = pr.length_m() * (1.0 + delta);
    let big_delta = target - pr.length_m();
    let step = env.wins.step_m();
    let plan = match env.engine.plan_resize(pr, target, &rq, &env.tune, step, 0.0) {
        Ok(plan) => plan,
        Err(f) => {
            line.outcome = failure_name(&f).0;
            return line;
        }
    };
    let (mut win, mut wt) = env.wins.acquire_plan(env.team_key, env.team, plan.window(), None);
    let call = |win: &ord_loops::Window,
                wt: &ord_loops::WindowTeam,
                plan: &ord_loops::ResizePlan,
                ws: &mut Workspace,
                acc: &mut f64|
     -> Called {
        let t = Instant::now();
        let r = catch_unwind(AssertUnwindSafe(|| {
            let tc = TeamCtx { data: env.team, win: wt };
            ord_loops::resize_in(env.engine, win, Some(tc), pr, plan, &rq, &env.tune, ws, &mut || false)
        }))
        .map_err(|_| ());
        *acc += t.elapsed().as_secs_f64() * 1000.0;
        r
    };
    let mut call_ms = 0.0;
    let mut res = call(&win, &wt, &plan, ws, &mut call_ms);
    if matches!(&res, Ok(Err(r)) if r.failure == RetouchFailure::WindowEscape) {
        let extra = env.engine.longest_chain_m(pr.ride_type().view_index()) + 10_000.0;
        if let Ok(wide) = env.engine.plan_resize(pr, target, &rq, &env.tune, step, extra) {
            (win, wt) = env.wins.acquire_plan_wide(env.team, wide.window().view, wide.window(), None);
            res = call(&win, &wt, &wide, ws, &mut call_ms);
        }
    }
    line.total_ms = call_ms;
    let mut wl = WinLine { outcome: "routes".into(), gen_ms: call_ms, ..WinLine::default() };
    let res = match res {
        Err(()) => {
            line.outcome = "panic".into();
            return line;
        }
        Ok(r) => r,
    };
    // Les compteurs du moteur sont lus aussi sur un refus : ils disent ce que la recherche a vu avant de refuser.
    let (retouch, failure) = match res {
        Ok(r) => (Some(r), None),
        Err(f) => (None, Some(f)),
    };
    if let Some(r) = &retouch {
        absorb(&mut line, &r.diagnostics);
    }
    if let Some(f) = &failure {
        absorb(&mut line, &f.diagnostics);
        let (name, dom) = failure_name(&f.failure);
        wl.outcome = name.clone();
        line.outcome = name;
        line.fail = dom;
        line.windows.push(wl);
        return line;
    }
    let out = retouch.expect("succès");
    wl.routes = out.proposals.len();
    line.candidates = out.proposals.len();
    // Les contrôles indépendants : chaque proposition, la première acceptée est retenue.
    let t0 = Instant::now();
    let mut accepted: Vec<(Best, &Retouched)> = Vec::new();
    for r in &out.proposals {
        let (si, sj) = (pr.seams()[r.from_seam.expect("couture")], pr.seams()[r.to_seam.expect("couture")]);
        line.controls_run += 1;
        let new = Walk::from_route(gf, &r.new);
        let sect = (r.to_s_m - r.from_s_m) + big_delta;
        let req_seg = control::seg_request(
            &proto,
            node_latlon(gf, si.node),
            Some(node_latlon(gf, sj.node)),
            sect.max(1.0),
        );
        let rep = env.ctl.checker.check(&req_seg, &r.new);
        let mut refused: Vec<String> =
            control::codes_of(&rep).into_iter().map(|c| format!("C1:{c}")).collect();
        let sp = control::splice(p, si.join, Some(sj.join), &new, si.node, Some(sj.node));
        let c2 = control::c2(&env.ctl, cache, p, &sp, si.join, Some(sj.join), &proto);
        refused.extend(c2.new_codes.iter().map(|c| format!("C2:{c}")));
        // Les jambes gardées avant la couture `i` et après la couture `j` : aucune n'est reprise, pas même sur la zone.
        let kept: FxHashSet<u32> =
            p.walk.legs[..=si.join].iter().chain(&p.walk.legs[sj.join + 1..]).map(|l| l.edge).collect();
        let c3 = control::c3(gf, &r.new.steps, &kept, &FxHashSet::default());
        if c3.outside_m > 0.0 {
            line.reuse_refused += 1;
            refused.push("C3:REUSE".into());
        }
        if !refused.is_empty() {
            line.refused += 1;
            for c in refused {
                *line.refused_codes.entry(c).or_default() += 1;
            }
            continue;
        }
        let changed: f64 = r
            .new
            .steps
            .iter()
            .filter(|s| !p.edge_set.contains(&s.base_edge))
            .map(|s| f64::from(gf.edge(s.base_edge).len_dm) / 10.0)
            .sum();
        let (angle_in, angle_out) = control::seam_angles(gf, p, &sp);
        accepted.push((
            Best {
                err: r.err.unwrap_or(1.0),
                kept_share: r.kept_m / r.length_m.max(1.0),
                changed_m: changed,
                new_len_m: r.new.length_m,
                team_share: (rep.length_m > 0.0).then(|| rep.team_m / rep.length_m),
                angle_in_deg: angle_in,
                angle_out_deg: angle_out,
                family: 0,
                c3_zone_m: 0.0,
                checked_m: c2.checked_m,
                unchecked_m: c2.unchecked_m,
            },
            r,
        ));
    }
    line.indep_ms = t0.elapsed().as_secs_f64() * 1000.0;
    line.ctl_ms = 0.0;
    line.proposals = accepted.len();
    line.windows.push(wl);
    match accepted.first() {
        None => line.outcome = "all_refused".into(),
        Some((b, r)) => {
            line.outcome = if b.err <= env.tune.tolerance { "ok" } else { "off_target" }.into();
            if b.err <= env.tune.tolerance {
                line.first_ok_ms = Some(call_ms);
            }
            let mut fp = Vec::new();
            fp.extend_from_slice(&(r.from_seam.unwrap_or(0) as u64).to_le_bytes());
            fp.extend_from_slice(&route_hash(&r.new).to_le_bytes());
            line.fp = format!("{:016x}", xxh3_64(&fp));
            line.best = Some(b.clone());
        }
    }
    line
}

/// Les compteurs du moteur d'une demande.
fn absorb(line: &mut RsLine, d: &ord_loops::RetouchDiagnostics) {
    line.trees = d.trees;
    line.windows_tried = d.windows_tried;
    line.stopped_early = d.stopped_early;
    line.deadline_hit = d.deadline_hit;
    line.junction_cut = d.junction_cut;
    line.spliced_refused = diag_codes(d);
    line.seams_refused_window = d.seams_refused_window;
    line.short_leads = d.short_leads;
    line.shorten_reached = d.shorten_reached;
    line.shorten_turn_refused = d.shorten_turn_refused;
    line.shorten_head_mismatch = d.shorten_head_mismatch;
    line.shorten_reuse_refused = d.shorten_reuse_refused;
    line.best_err0 = d.tree_best_err0.iter().flatten().copied().reduce(f64::min);
}
