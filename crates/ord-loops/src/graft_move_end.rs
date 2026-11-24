//! Déplacer l'arrivée d'un parcours posé (note `ord-retouche-lot1.md` §9.5) : un arbre retour depuis la nouvelle arrivée `E′`
//! sur le parcours masqué, et pour chaque couture la sortie qui minimise « longueur abandonnée + raccord ».
//!
//! Ce que le prototype du lot 0 (`probe_graft::move_end_in`) ne savait pas, et qui faisait 89 % de ses refus : la **jambe
//! gardée**. Il jugeait le raccord seul, la règle des grappes y étant levée sur les 200 premiers mètres — justement là où
//! est la couture —, et perdait le prédécesseur gardé en appelant `turn_cost(f_in, NONE, …)`. Ici le virage simple prend
//! l'état d'arrivée gardé **et son prédécesseur**, les grappes de 100 m de part et d'autre de la couture sont rejouées par
//! le modèle des virages avec les états gardés pour contexte (`OrgSearch::junction_ok`), puis `check_spliced_in` juge le
//! recollé sur 400 m de contexte.

use std::cell::Cell;
use std::time::Instant;

use rustc_hash::FxHashSet;

use crate::engine::Engine;
use crate::geo::{haversine_m, LatLon};
use crate::graft::{
    link_snap, route_between_snaps, seam_table, KeepOut, MoveEndVariant, Refused, Retouch,
    RetouchDiagnostics, RetouchFailure, RetouchKind, RetouchRequest, RetouchTune, Retouched, SeamState,
};
use crate::posed::PosedRoute;
use crate::search::{
    make_anchor, retro_ok, tree_backward, turn_cost, Ctx, Disc, Forbidden, Weighter, Workspace, INF,
};
use crate::team::{TeamCtx, TeamData, WindowTeam};
use crate::turns::{OrgSearch, TurnModel};
use crate::verify::check_spliced_in;
use crate::window::{Window, WindowPlan};
use crate::{Figures, Mode, Request, Route};

/// Contexte gardé que `check_spliced_in` lit de part et d'autre d'une couture (note §8).
pub(crate) const CONTEXT_M: f64 = 400.0;
/// Sorties essayées au plus par couture.
const EXITS_PER_SEAM: usize = 4;
/// Essais du choix d'une variante par passe.
const PICKS_PER_PASS: usize = 16;

/// La requête que `check_spliced_in` lit (il en remplace départ, arrivée et distance).
pub(crate) fn check_request(req: &RetouchRequest, at: LatLon) -> Request {
    Request {
        start: at,
        end: None,
        bearing: None,
        distance_m: 1.0,
        ride_type: req.ride_type,
        mode: req.mode,
        familiarity: req.familiarity,
        off_team_cost_q8: req.off_team_cost_q8,
        seed: req.seed,
        figures: Figures::Classic,
    }
}

/// Rayons `(ρ, ρ max)` de l'arbre retour pour une arrivée à `crow` mètres de celle d'origine.
fn radii(tune: &RetouchTune, crow: f64) -> (f64, f64) {
    let (factor, extra, min, max) = tune.move_end_radius;
    let rho = (factor * crow + extra).clamp(min, max);
    (rho, (2.0 * rho).min(max).max(rho))
}

impl<B: AsRef<[u8]>> Engine<B> {
    /// Fenêtre de `move_end` : le disque de rayon maximal autour de `E′` (celle du lot 0). `step_m` et `extra_m` comme
    /// [`Engine::plan_with`].
    pub fn plan_move_end(
        &self,
        posed: &PosedRoute,
        end: LatLon,
        tune: &RetouchTune,
        step_m: f64,
        extra_m: f64,
    ) -> WindowPlan {
        let crow = haversine_m(posed.end_point().to_e7(), end.to_e7());
        let (_, rho_max) = radii(tune, crow);
        let r = Request {
            start: end,
            end: None,
            bearing: None,
            distance_m: 2.0 * rho_max,
            ride_type: posed.ride_type(),
            mode: Mode::Discovery,
            familiarity: 0.0,
            off_team_cost_q8: crate::DEFAULT_OFF_TEAM_COST_Q8,
            seed: 0,
            figures: Figures::Classic,
        };
        self.plan_with(&r, step_m, extra_m)
    }
}

/// Une sortie atteinte : la couture (indice dans la table), l'état de sortie, le coût et les mètres du raccord.
#[derive(Clone, Copy, Debug)]
struct Pick {
    si: usize,
    fo: u32,
    raccord_m: f64,
}

/// Déplacer l'arrivée du parcours posé vers `end`, dans une fenêtre qui contient les tuiles de
/// [`Engine::plan_move_end`]. Rend jusqu'à trois variantes (`Best` d'abord, puis « tout garder » et « quitter au plus près »
/// quand elles diffèrent et passent le contrôle), ou `Truncate` si `end` est sur le parcours. Un refus rend ses
/// diagnostics avec sa cause ([`Refused`]).
#[allow(clippy::too_many_arguments)]
pub fn move_end_in<B: AsRef<[u8]>>(
    e: &Engine<B>,
    win: &Window,
    team: Option<TeamCtx<'_>>,
    posed: &PosedRoute,
    end: LatLon,
    req: &RetouchRequest,
    tune: &RetouchTune,
    ws: &mut Workspace,
    stop: &mut dyn FnMut() -> bool,
) -> Result<Retouch, Box<Refused>> {
    let t0 = Instant::now();
    let mut diag = RetouchDiagnostics::default();
    let res = move_end_run(e, win, team, posed, end, req, tune, ws, stop, &mut diag);
    diag.elapsed_us = t0.elapsed().as_micros() as u64;
    match res {
        Ok(proposals) => Ok(Retouch { proposals, diagnostics: diag }),
        Err(failure) => Err(Box::new(Refused { failure, diagnostics: diag })),
    }
}

#[allow(clippy::too_many_arguments)]
fn move_end_run<B: AsRef<[u8]>>(
    e: &Engine<B>,
    win: &Window,
    team: Option<TeamCtx<'_>>,
    posed: &PosedRoute,
    end: LatLon,
    req: &RetouchRequest,
    tune: &RetouchTune,
    ws: &mut Workspace,
    stop: &mut dyn FnMut() -> bool,
    diag: &mut RetouchDiagnostics,
) -> Result<Vec<Retouched>, RetouchFailure> {
    if req.mode != Mode::Discovery && team.is_none() {
        return Err(RetouchFailure::NoTeamData);
    }
    let ride = req.ride_type;
    assert_eq!(posed.ride_type(), ride, "le parcours posé n'est pas du type demandé");
    let vi = ride.view_index();
    assert_eq!(win.vi, vi, "la fenêtre n'est pas celle de la vue du type demandé");
    let v = &win.v;
    let l_m = posed.length_m();
    let ko = KeepOut::build(win, posed.legs().iter().map(|l| l.edge), []);
    let free = Weighter::new(v, team, ride, req.mode, req.familiarity, req.off_team_cost_q8);
    let w = Weighter::new(v, team, ride, req.mode, req.familiarity, req.off_team_cost_q8)
        .with_keep_out(Some(ko.bits()));
    let crow = haversine_m(posed.end_point().to_e7(), end.to_e7());
    let (rho0, rho_max) = radii(tune, crow);
    let min_dm = (e.params.min_component_share * tune.move_end_radius.2 * 10.0) as u64;
    let strict_tt = if free.strict() { free.tt() } else { None };
    let ok_free = |c: u32| -> bool {
        free.chain(c) != INF && strict_tt.is_none_or(|tt| tt.strict_len_of(v.gid[c as usize]) >= min_dm)
    };
    let snapped = e.snap(win, end, &ok_free, min_dm).map_err(|()| RetouchFailure::WindowEscape)?;
    let mut s_snap = snapped.filter(|s| s.dist_m <= tune.end_snap_m).ok_or(RetouchFailure::EndTooFar)?;
    if ko.is_masked(s_snap.chain) {
        // `E′` est sur le parcours : coupé au dernier passage de l'arête accrochée.
        if let Some(k) = posed.legs().iter().rposition(|l| l.edge == s_snap.edge) {
            return Ok(truncate(e, posed, &s_snap, k));
        }
        // Une chaîne masquée plus longue que la marche (départ ou arrivée au milieu d'une chaîne) : `E′` est réaccroché
        // avec le poids masqué, et l'opération suit son cours.
        let ok_masked = |c: u32| -> bool {
            w.chain(c) != INF && strict_tt.is_none_or(|tt| tt.strict_len_of(v.gid[c as usize]) >= min_dm)
        };
        s_snap = e
            .snap(win, end, &ok_masked, min_dm)
            .map_err(|()| RetouchFailure::WindowEscape)?
            .filter(|s| s.dist_m <= tune.end_snap_m)
            .ok_or(RetouchFailure::EndTooFar)?;
    }
    let ea = make_anchor(e, v, &w, s_snap);

    let wanted: Vec<usize> = (0..posed.seams().len()).filter(|&k| posed.valid_before(k)).collect();
    if wanted.is_empty() {
        return Err(RetouchFailure::NoSeam);
    }
    let states: Vec<SeamState> =
        seam_table(e, win, posed, &wanted, false, diag).into_iter().flatten().collect();
    if states.is_empty() {
        return Err(RetouchFailure::NoSeam);
    }
    let org = OrgSearch::new(Some(TurnModel::new(e.params.max_turn_deg, tune.turn_margin_deg)), false);
    let s_e7 = end.to_e7();
    let esc = Cell::new(false);
    let mut forb = Forbidden::default();
    let team_data: Option<&TeamData> = team.map(|t| t.data);
    let chk_req = check_request(req, end);
    let halted = |esc: &Cell<bool>| {
        if esc.get() {
            RetouchFailure::WindowEscape
        } else {
            RetouchFailure::Stopped
        }
    };
    let mut refused_any = false;

    for pass in 0..tune.passes.max(1) {
        diag.passes += 1;
        let mut rho = rho0;
        // Sorties de chaque couture atteinte : `(coût, état)`, les moins chères d'abord.
        let mut exits: Vec<Vec<(u64, u32)>>;
        loop {
            let disc = Disc::new(s_e7, s_e7, 2.0 * rho, true);
            let cx = Ctx { v, w: &w, disc: &disc, s: &ea, e: &ea, forb: &forb, esc: &esc, org: Some(&org) };
            let t2 = &mut ws.views[vi].t2;
            tree_backward(&cx, t2, stop).map_err(|_| halted(&esc))?;
            diag.settled += t2.settled.len() as u64;
            diag.trees += 1;
            exits = states
                .iter()
                .map(|st| {
                    let mut list: Vec<(u64, u32)> = Vec::new();
                    for &fo in v.out(st.nl) {
                        let g = t2.get(fo);
                        if g == INF {
                            continue;
                        }
                        // Le virage du moteur à la couture, avec le prédécesseur gardé (cap d'une chaîne courte).
                        let Some(tc) = turn_cost(v, &forb, st.f_in, st.pred_in, fo, None, None) else {
                            continue;
                        };
                        if !retro_ok(v, st.pred_in, st.f_in, fo) {
                            continue;
                        }
                        list.push((u64::from(g) + u64::from(tc), fo));
                    }
                    list.sort_unstable();
                    list.truncate(EXITS_PER_SEAM);
                    list
                })
                .collect();
            diag.radius_m = Some(rho);
            if exits.iter().any(|x| !x.is_empty()) || rho >= rho_max {
                break;
            }
            rho = (2.0 * rho).min(rho_max);
            diag.widened = true;
        }
        if exits.iter().all(Vec::is_empty) {
            return Err(RetouchFailure::NoSeamReached);
        }
        let t2 = &ws.views[vi].t2;
        // Seul `cx.v` sert à `junction_ok` ; les transitions à interdire sont notées et posées après les essais.
        let disc = Disc::new(s_e7, s_e7, 2.0 * rho, true);
        let cx = Ctx { v, w: &w, disc: &disc, s: &ea, e: &ea, forb: &forb, esc: &esc, org: Some(&org) };
        let mut pending: Vec<(u32, u32)> = Vec::new();
        let fields_of = |fo: u32| -> Vec<u32> {
            let mut f = t2.path_to(fo);
            f.reverse();
            f
        };
        // Les sorties retirées (couture, état) : une grappe ou un virage à la couture que le contrôle refuse.
        let mut banned: FxHashSet<(usize, u32)> = FxHashSet::default();
        let mut dead: FxHashSet<usize> = FxHashSet::default();
        let mut replay = false;
        let mut result: Option<Vec<Retouched>> = None;
        for _ in 0..PICKS_PER_PASS {
            // La première sortie de chaque couture que la règle des grappes laisse passer.
            let mut reached: Vec<Pick> = Vec::new();
            for (si, st) in states.iter().enumerate() {
                if dead.contains(&si) {
                    continue;
                }
                for &(_, fo) in &exits[si] {
                    if banned.contains(&(si, fo)) {
                        continue;
                    }
                    let fields = fields_of(fo);
                    if org.junction_ok(&cx, &st.lead_before, st.dm, &fields, Some(t2)) {
                        reached.push(Pick { si, fo, raccord_m: f64::from(t2.meters(fo)) / 10.0 });
                        break;
                    }
                    diag.junction_cut += 1;
                    banned.insert((si, fo));
                }
            }
            if reached.is_empty() {
                break;
            }
            let abandon = |p: &Pick| (l_m - posed.seams()[states[p.si].seam].s_m).max(0.0);
            let seam_of = |p: &Pick| states[p.si].seam;
            let best = *reached
                .iter()
                .min_by(|a, b| {
                    (abandon(a) + a.raccord_m)
                        .total_cmp(&(abandon(b) + b.raccord_m))
                        .then(seam_of(b).cmp(&seam_of(a)))
                })
                .expect("non vide");
            let build = |p: &Pick| -> (Route, Vec<u32>) {
                let fields = fields_of(p.fo);
                let start = link_snap(e, v, p.fo, false);
                let (mut route, step_field) =
                    route_between_snaps(v, &fields, &start, &s_snap, req.familiarity);
                crate::route::measure(e, team_data, ride, &mut route);
                (route, step_field)
            };
            let verdict = |p: &Pick, route: &Route| {
                let seam = posed.seams()[states[p.si].seam];
                let before = posed.context_before(seam.join, CONTEXT_M);
                check_spliced_in(e, win, team_data, &chk_req, &before, route, &[])
            };
            let (route, step_field) = build(&best);
            let chk = verdict(&best, &route);
            if chk.violations.is_empty() {
                let mk = |p: &Pick, route: Route, variant: MoveEndVariant| {
                    let seam = &posed.seams()[states[p.si].seam];
                    let kept = seam.s_m;
                    Retouched {
                        kind: RetouchKind::MoveEnd(variant),
                        from_seam: Some(states[p.si].seam),
                        to_seam: None,
                        from_s_m: seam.s_m,
                        to_s_m: l_m,
                        length_m: kept + route.length_m,
                        kept_m: kept,
                        err: None,
                        abandon_m: Some((l_m - seam.s_m).max(0.0)),
                        new: route,
                    }
                };
                let mut out = vec![mk(&best, route, MoveEndVariant::Best)];
                let last = reached.iter().max_by_key(|p| seam_of(p)).copied().expect("non vide");
                let near = reached
                    .iter()
                    .min_by(|a, b| a.raccord_m.total_cmp(&b.raccord_m).then(seam_of(b).cmp(&seam_of(a))))
                    .copied()
                    .expect("non vide");
                for (p, variant) in [(last, MoveEndVariant::KeepAll), (near, MoveEndVariant::Nearest)] {
                    if p.si == best.si && p.fo == best.fo {
                        continue;
                    }
                    let (r, _) = build(&p);
                    if verdict(&p, &r).violations.is_empty() {
                        out.push(mk(&p, r, variant));
                    }
                }
                result = Some(out);
                break;
            }
            refused_any = true;
            for v in &chk.violations {
                *diag.spliced_refused.entry(*v).or_insert(0) += 1;
            }
            if chk.sharp_entry || chk.seam_u_turn {
                // Le virage ou la grappe à la couture : cette sortie est écartée, la suivante ou la couture suivante est prise.
                banned.insert((best.si, best.fo));
            } else if !chk.sharp_at_new.is_empty() && pass + 1 < tune.passes {
                // Un virage du raccord : il est interdit et l'arbre est rejoué.
                let before = pending.len();
                for &i in &chk.sharp_at_new {
                    if i + 1 < step_field.len()
                        && step_field[i] != step_field[i + 1]
                        && !forb.contains(&(step_field[i], step_field[i + 1]))
                    {
                        pending.push((step_field[i], step_field[i + 1]));
                    }
                }
                if pending.len() > before {
                    replay = true;
                    break;
                }
                dead.insert(best.si);
            } else {
                dead.insert(best.si);
            }
        }
        forb.extend(pending);
        if let Some(proposals) = result {
            return Ok(proposals);
        }
        if !replay {
            break;
        }
    }
    let _ = refused_any;
    match diag.spliced_refused.iter().max_by_key(|(_, n)| **n).map(|(v, _)| *v) {
        Some(dominant) => Err(RetouchFailure::AllRefused { dominant }),
        // Des sorties atteintes, toutes coupées par la règle des grappes : pas un arbre qui n'atteint rien.
        None if diag.junction_cut > 0 => Err(RetouchFailure::JunctionCut),
        None => Err(RetouchFailure::NoSeamReached),
    }
}

/// `E′` est sur le parcours : coupé au dernier passage de l'arête accrochée, à la projection du point accroché sur la
/// polyligne d'origine, sans couture ni tronçon neuf.
fn truncate<B: AsRef<[u8]>>(
    e: &Engine<B>,
    posed: &PosedRoute,
    snap: &crate::engine::Snap,
    leg: usize,
) -> Vec<Retouched> {
    let l_m = posed.length_m();
    let pts = e.edge_polyline(snap.edge);
    let at = crate::verify::slice(&pts, snap.offset_m, snap.offset_m)[0];
    // Abscisse d'origine estimée de la jambe : sa part de la marche, rapportée à la polyligne.
    let walk_m = posed.leg_end_m(posed.legs().len() - 1).max(1.0);
    let s_est = posed.leg_end_m(leg) / walk_m * l_m;
    let span = 2_000.0 + 0.02 * l_m;
    let cut = posed
        .project_last(at, s_est - span, s_est + span, posed.proj_tol_m())
        .map_or(s_est.clamp(0.0, l_m), |(_, s)| s);
    let route = Route {
        start: crate::Position { edge: snap.edge, offset_m: snap.offset_m },
        end: crate::Position { edge: snap.edge, offset_m: snap.offset_m },
        steps: Vec::new(),
        length_m: 0.0,
        team_m: 0.0,
        bridged_m: 0.0,
        shared_m: 0.0,
        crossings: 0,
        unpaved_m: 0.0,
        offroad_m: 0.0,
        presumed_surface_m: 0.0,
        bearing_deviation_deg: None,
        familiarity: 0.0,
        family: crate::Family::Turnaround,
    };
    vec![Retouched {
        kind: RetouchKind::Truncate,
        from_seam: None,
        to_seam: None,
        from_s_m: cut,
        to_s_m: l_m,
        new: route,
        length_m: cut,
        kept_m: cut,
        err: None,
        abandon_m: Some((l_m - cut).max(0.0)),
    }]
}

/// Comme [`move_end_in`], en bâtissant la fenêtre (et les agrégats de l'équipe), rejouée une fois élargie sur
/// [`RetouchFailure::WindowEscape`] : pour les tests et les outils ; le service garde ses fenêtres en cache.
#[allow(clippy::too_many_arguments)]
pub fn move_end<B: AsRef<[u8]>>(
    e: &Engine<B>,
    team: Option<&TeamData>,
    posed: &PosedRoute,
    end: LatLon,
    req: &RetouchRequest,
    tune: &RetouchTune,
    ws: &mut Workspace,
    stop: &mut dyn FnMut() -> bool,
) -> Result<Retouch, RetouchFailure> {
    let mut extra_m = 0.0;
    let mut last = RetouchFailure::WindowEscape;
    for _ in 0..2 {
        let plan = e.plan_move_end(posed, end, tune, 0.0, extra_m);
        let Some(win) = e.build_window(plan.view, &plan.need, stop) else {
            return Err(RetouchFailure::Stopped);
        };
        let wt = team.map(|t| WindowTeam::build(e, &win, t));
        let ctx = team.zip(wt.as_ref()).map(|(data, win)| TeamCtx { data, win });
        match move_end_in(e, &win, ctx, posed, end, req, tune, ws, stop).map_err(|r| r.failure) {
            Err(RetouchFailure::WindowEscape) => {
                last = RetouchFailure::WindowEscape;
                extra_m = plan.snap_lmax_m + 10_000.0;
            }
            other => return other,
        }
    }
    Err(last)
}
