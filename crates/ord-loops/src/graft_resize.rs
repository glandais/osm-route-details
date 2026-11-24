//! Viser une distance : raccourcir par un arbre d'itinéraire, allonger par la lentille (note `ord-retouche-lot1.md` §6 et §7).
//!
//! - **Raccourcir** (`Δ = D′ − L < 0`) : un arbre aller depuis chaque couture `i` tirée, sur tout le parcours masqué. Pour
//!   chaque couture `j` en aval, l'économie `ℓ(i, j) − m(i, j)` se lit dans l'arbre, et son chemin **est** l'itinéraire de
//!   `i` à `j` aux poids du générateur, avec sa règle de virage : pas de lentille, pas de borne à 1,05 × le plus court
//!   chemin, pas de fenêtre tirée au vol d'oiseau. Un aller simple presque droit n'a pas d'économie à rendre
//!   ([`RetouchFailure::NoEconomy`]) : c'est une limite assumée.
//! - **Allonger** (`Δ > 0`) : la lentille de production entre deux coutures, avec le masque des chaînes gardées et les
//!   accroches de couture (cap gardé), arrêtée dès que trois propositions à `pick_tol` sont tenues.

use std::cell::Cell;
use std::time::Instant;

use ord_graph::util::SplitMix64;
use rustc_hash::FxHashMap;

use crate::engine::Engine;
use crate::generate::{generate_graft_in, LensGraft};
use crate::geo::LatLon;
use crate::graft::{
    link_snap, route_between_snaps, seam_anchor_end, seam_anchor_start, seam_table, KeepOut, Refused,
    Retouch, RetouchDiagnostics, RetouchFailure, RetouchKind, RetouchRequest, RetouchTune, Retouched,
    SeamState,
};
use crate::graft_move_end::{check_request, CONTEXT_M};
use crate::posed::PosedRoute;
use crate::search::{
    retro_ok, tree_forward, turn_cost, Ctx, Disc, Forbidden, Weighter, Workspace, INF, NONE,
};
use crate::team::{TeamCtx, TeamData, WindowTeam};
use crate::turns::{OrgSearch, TurnModel};
use crate::verify::{check_spliced_in, Violation};
use crate::window::{tiles_union, Window, WindowPlan};
use crate::{Failure, Figures, Request};

/// Un arbre de raccourcissement : la couture `i`, le rayon du disque et les coutures `j` lues dans l'arbre.
#[derive(Clone, Debug)]
struct TreeSpec {
    i: usize,
    radius_m: f64,
    js: Vec<usize>,
}

/// Une fenêtre `(i, j)` de la lentille et l'échelle `ℓ = s_j − s_i`.
#[derive(Clone, Copy, Debug)]
struct LensSpec {
    i: usize,
    j: usize,
    ell_m: f64,
}

/// Ce que `plan_resize` a dimensionné : la fenêtre et, dans l'ordre où `resize_in` les jouera, les arbres ou les fenêtres
/// de lentille tirés. `resize_in` joue exactement ce que le plan a dimensionné.
#[derive(Clone, Debug)]
pub struct ResizePlan {
    window: WindowPlan,
    target_m: f64,
    trees: Vec<TreeSpec>,
    lens: Vec<LensSpec>,
}

impl ResizePlan {
    /// Les tuiles à avoir en fenêtre.
    pub fn window(&self) -> &WindowPlan {
        &self.window
    }

    pub fn target_m(&self) -> f64 {
        self.target_m
    }

    /// Raccourcir (arbres) ou allonger (lentille).
    pub fn is_shortening(&self) -> bool {
        !self.trees.is_empty()
    }
}

fn union_plans(plans: Vec<WindowPlan>) -> WindowPlan {
    let mut it = plans.into_iter();
    let mut acc = it.next().expect("au moins un plan");
    for p in it {
        acc.need = tiles_union(&acc.need, &p.need);
        acc.class = tiles_union(&acc.class, &p.class);
        acc.snap_lmax_m = acc.snap_lmax_m.max(p.snap_lmax_m);
    }
    acc
}

impl<B: AsRef<[u8]>> Engine<B> {
    /// Fenêtre de `resize` : l'union (`tiles_union`) des zones de ses recherches — disques de raccourcissement, ellipses des
    /// fenêtres d'allongement. Le choix des coutures se fait sans fenêtre (règles 1, 3 et 4) ; la règle 2 se juge ensuite.
    /// `step_m` et `extra_m` comme [`Engine::plan_with`].
    #[allow(clippy::too_many_arguments)]
    pub fn plan_resize(
        &self,
        posed: &PosedRoute,
        target_m: f64,
        req: &RetouchRequest,
        tune: &RetouchTune,
        step_m: f64,
        extra_m: f64,
    ) -> Result<ResizePlan, RetouchFailure> {
        let l = posed.length_m();
        let big = target_m - l;
        if big.abs() < tune.min_change_m {
            return Err(RetouchFailure::ChangeTooSmall);
        }
        if big.abs() > tune.max_change * l {
            return Err(RetouchFailure::ChangeTooLarge);
        }
        let seams = posed.seams();
        let both: Vec<usize> = (0..seams.len()).filter(|&k| posed.valid_both(k)).collect();
        let inner: Vec<usize> = both
            .iter()
            .copied()
            .filter(|&k| seams[k].s_m >= tune.ends_share * l && seams[k].s_m <= (1.0 - tune.ends_share) * l)
            .collect();
        if inner.len() < 2 {
            return Err(RetouchFailure::NoSeam);
        }
        let s = |k: usize| seams[k].s_m;
        let ll = |k: usize| LatLon::from_e7(self.pos(seams[k].node));
        let mut rng = SplitMix64(req.seed ^ posed.fingerprint());
        rng.next_u64();
        let mk_req = |start: LatLon, end: Option<LatLon>, d: f64| Request {
            start,
            end,
            bearing: None,
            distance_m: d,
            ride_type: req.ride_type,
            mode: req.mode,
            familiarity: req.familiarity,
            off_team_cost_q8: req.off_team_cost_q8,
            seed: req.seed,
            figures: Figures::Classic,
        };
        let (mut trees, mut lens): (Vec<TreeSpec>, Vec<LensSpec>) = (Vec::new(), Vec::new());
        let mut plans: Vec<WindowPlan> = Vec::new();
        if big < 0.0 {
            let abs = -big;
            let tau = tune.pick_tol;
            let (lo, hi) = (tune.ends_share * l, (1.0 - tune.ends_share) * l - abs);
            if hi > lo {
                let n = tune.shorten_trees.max(1);
                for b in 0..n {
                    let (b0, b1) =
                        (lo + (hi - lo) * b as f64 / n as f64, lo + (hi - lo) * (b + 1) as f64 / n as f64);
                    let in_box: Vec<usize> = inner
                        .iter()
                        .copied()
                        .filter(|&k| s(k) >= b0 && (s(k) < b1 || (b + 1 == n && s(k) <= b1)))
                        .collect();
                    if in_box.is_empty() {
                        continue;
                    }
                    let i = in_box[rng.below(in_box.len())];
                    let js: Vec<usize> = inner
                        .iter()
                        .copied()
                        .filter(|&j| s(j) > s(i) && s(j) - s(i) >= abs + tune.min_segment_m)
                        .collect();
                    let Some(far) = js.iter().map(|&j| s(j) - s(i) - (1.0 - tau) * abs).reduce(f64::max)
                    else {
                        continue;
                    };
                    let radius_m = far.clamp(2_000.0, tune.shorten_radius_max_m.max(2_000.0));
                    trees.push(TreeSpec { i, radius_m, js });
                }
            }
            if trees.is_empty() {
                return Err(RetouchFailure::NoEconomy);
            }
            // Les cases sont jouées dans un ordre tiré : l'arrêt anticipé ne favorise pas le début du parcours.
            for k in (1..trees.len()).rev() {
                trees.swap(k, rng.below(k + 1));
            }
            for t in &trees {
                plans.push(self.plan_with(&mk_req(ll(t.i), None, 2.0 * t.radius_m), step_m, extra_m));
            }
        } else {
            for mult in [1.5, 3.0] {
                let ell_t = mult * big;
                let (lo, hi) = (tune.ends_share * l, (1.0 - tune.ends_share) * l - ell_t);
                if hi <= lo {
                    continue;
                }
                let mut family: Vec<LensSpec> = Vec::new();
                for b in 0..6 {
                    let (b0, b1) = (lo + (hi - lo) * b as f64 / 6.0, lo + (hi - lo) * (b + 1) as f64 / 6.0);
                    let in_box: Vec<usize> =
                        inner.iter().copied().filter(|&k| s(k) >= b0 && s(k) < b1).collect();
                    if in_box.is_empty() {
                        continue;
                    }
                    let i = in_box[rng.below(in_box.len())];
                    let j = inner
                        .iter()
                        .copied()
                        .filter(|&j| s(j) > s(i) && (0.8 * ell_t..=1.25 * ell_t).contains(&(s(j) - s(i))))
                        .min_by(|&a, &b| (s(a) - s(i) - ell_t).abs().total_cmp(&(s(b) - s(i) - ell_t).abs()));
                    if let Some(j) = j {
                        family.push(LensSpec { i, j, ell_m: s(j) - s(i) });
                    }
                }
                // Les cases d'une famille sont jouées dans un ordre tiré, la famille 1,5 |Δ| d'abord (76,3 % des meilleures
                // propositions au lot 0).
                for k in (1..family.len()).rev() {
                    family.swap(k, rng.below(k + 1));
                }
                lens.extend(family);
            }
            lens.truncate(tune.lengthen_windows);
            if lens.is_empty() {
                return Err(RetouchFailure::NoWindow);
            }
            for w in &lens {
                plans.push(self.plan_with(&mk_req(ll(w.i), Some(ll(w.j)), w.ell_m + big), step_m, extra_m));
            }
        }
        Ok(ResizePlan { window: union_plans(plans), target_m, trees, lens })
    }
}

/// Une proposition et la case dont elle vient.
struct Prop {
    case: usize,
    order: usize,
    r: Retouched,
}

/// Viser `plan.target_m()` : raccourcir par arbre ou allonger par lentille, dans une fenêtre qui contient les tuiles de
/// [`Engine::plan_resize`]. Rend jusqu'à `tune.proposals` propositions de cases distinctes, la meilleure d'abord.
///
/// Un refus rend ses diagnostics avec sa cause ([`Refused`]) : arbres joués, lectures, coupes de la règle des grappes.
#[allow(clippy::too_many_arguments)]
pub fn resize_in<B: AsRef<[u8]>>(
    e: &Engine<B>,
    win: &Window,
    team: Option<TeamCtx<'_>>,
    posed: &PosedRoute,
    plan: &ResizePlan,
    req: &RetouchRequest,
    tune: &RetouchTune,
    ws: &mut Workspace,
    stop: &mut dyn FnMut() -> bool,
) -> Result<Retouch, Box<Refused>> {
    let t0 = Instant::now();
    let mut diag = RetouchDiagnostics::default();
    let res = resize_run(e, win, team, posed, plan, req, tune, ws, stop, &mut diag);
    diag.elapsed_us = t0.elapsed().as_micros() as u64;
    match res {
        Ok(proposals) => Ok(Retouch { proposals, diagnostics: diag }),
        Err(failure) => Err(Box::new(Refused { failure, diagnostics: diag })),
    }
}

#[allow(clippy::too_many_arguments)]
fn resize_run<B: AsRef<[u8]>>(
    e: &Engine<B>,
    win: &Window,
    team: Option<TeamCtx<'_>>,
    posed: &PosedRoute,
    plan: &ResizePlan,
    req: &RetouchRequest,
    tune: &RetouchTune,
    ws: &mut Workspace,
    stop: &mut dyn FnMut() -> bool,
    diag: &mut RetouchDiagnostics,
) -> Result<Vec<Retouched>, RetouchFailure> {
    let t0 = Instant::now();
    if req.mode != crate::Mode::Discovery && team.is_none() {
        return Err(RetouchFailure::NoTeamData);
    }
    let ride = req.ride_type;
    assert_eq!(posed.ride_type(), ride, "le parcours posé n'est pas du type demandé");
    let vi = ride.view_index();
    assert_eq!(win.vi, vi, "la fenêtre n'est pas celle de la vue du type demandé");
    let v = &win.v;
    let l_m = posed.length_m();
    let big = plan.target_m - l_m;
    let d_target = plan.target_m;
    let team_data: Option<&TeamData> = team.map(|t| t.data);
    let all_edges = || posed.legs().iter().map(|l| l.edge);
    let ko_all = KeepOut::build(win, all_edges(), []);
    let w_all = Weighter::new(v, team, ride, req.mode, req.familiarity, req.off_team_cost_q8)
        .with_keep_out(Some(ko_all.bits()));
    // Les coutures dont le plan a besoin, jugées dans la fenêtre.
    let mut needed: Vec<usize> = Vec::new();
    for t in &plan.trees {
        needed.push(t.i);
        needed.extend(&t.js);
    }
    for w in &plan.lens {
        needed.push(w.i);
        needed.push(w.j);
    }
    needed.sort_unstable();
    needed.dedup();
    let table: FxHashMap<usize, SeamState> =
        seam_table(e, win, posed, &needed, true, diag).into_iter().flatten().map(|s| (s.seam, s)).collect();
    // L'échéance : celle de l'appelant et `budget_ms`.
    let dead = Cell::new(false);
    let budget = std::time::Duration::from_millis(u64::from(tune.budget_ms));
    let mut stopper = || {
        if t0.elapsed() >= budget {
            dead.set(true);
            return true;
        }
        stop()
    };
    let chk_req = check_request(req, posed.end_point());
    let esc = Cell::new(false);
    let mut props: Vec<Prop> = Vec::new();
    let mut order = 0usize;
    let mut any_econ = false;
    let mut ran = false;
    let mut fails: Vec<(Failure, u32)> = Vec::new();
    let seams = posed.seams();
    let tol = tune.tolerance;
    let err_of = |len: f64| (len - d_target).abs() / d_target;
    // Arrêt anticipé (§6.4) : trois propositions de cases distinctes à `pick_tol`, ou l'échéance douce passée avec une
    // proposition à `tolerance`.
    let early = |props: &[Prop], diag: &mut RetouchDiagnostics| -> bool {
        if t0.elapsed() >= budget {
            dead.set(true);
            return true;
        }
        let mut tight: Vec<usize> =
            props.iter().filter(|p| p.r.err.is_some_and(|x| x <= tune.pick_tol)).map(|p| p.case).collect();
        tight.sort_unstable();
        tight.dedup();
        if tight.len() >= tune.proposals {
            diag.stopped_early = true;
            return true;
        }
        if t0.elapsed().as_millis() as u32 >= tune.soft_ms
            && props.iter().any(|p| p.r.err.is_some_and(|x| x <= tol))
        {
            diag.stopped_early = true;
            return true;
        }
        false
    };
    let mut dominant_viol: FxHashMap<Violation, u32> = FxHashMap::default();

    if plan.is_shortening() {
        let org = OrgSearch::new(Some(TurnModel::new(e.params.max_turn_deg, tune.turn_margin_deg)), false);
        'trees: for spec in &plan.trees {
            if early(&props, &mut *diag) || dead.get() {
                break;
            }
            let Some(st_i) = table.get(&spec.i) else { continue };
            ran = true;
            // Le masque de l'arbre : tout le parcours, ou seulement les jambes gardées (avant `i`, après `j_max`) quand la
            // partie remplacée est libre.
            let ko_tree;
            let w_tree;
            let w_arbre: &Weighter = if tune.free_shorten {
                let jmax = spec.js.iter().filter_map(|j| seams.get(*j)).map(|s| s.join).max();
                let legs = posed.legs();
                let after = jmax.map_or(legs.len(), |j| (j + 1).min(legs.len()));
                let edges = legs[..=seams[spec.i].join].iter().chain(&legs[after..]).map(|l| l.edge);
                ko_tree = KeepOut::build(win, edges, []);
                w_tree = Weighter::new(v, team, ride, req.mode, req.familiarity, req.off_team_cost_q8)
                    .with_keep_out(Some(ko_tree.bits()));
                &w_tree
            } else {
                &w_all
            };
            let anchor = seam_anchor_start(e, v, st_i);
            let pos_i = v.pos[st_i.nl as usize];
            let disc = Disc::new(pos_i, pos_i, 2.0 * spec.radius_m, true);
            let mut forb = Forbidden::default();
            let s_i = seams[spec.i].s_m;
            for attempt in 0..2 {
                let cx = Ctx {
                    v,
                    w: w_arbre,
                    disc: &disc,
                    s: &anchor,
                    e: &anchor,
                    forb: &forb,
                    esc: &esc,
                    org: Some(&org),
                };
                if tree_forward(&cx, &mut ws.views[vi].t1, &mut stopper).is_err() {
                    if esc.get() {
                        return Err(RetouchFailure::WindowEscape);
                    }
                    if dead.get() {
                        break 'trees;
                    }
                    return Err(RetouchFailure::Stopped);
                }
                let t1 = &ws.views[vi].t1;
                diag.trees += 1;
                diag.settled += t1.settled.len() as u64;
                // Lecture des `j` : l'arrivée la moins chère à la couture, avec le virage d'entrée dans la jambe gardée.
                struct Read {
                    j: usize,
                    fe: u32,
                    err0: f64,
                    kept: f64,
                }
                let mut reads: Vec<Read> = Vec::new();
                let mut best_err0: Option<f64> = None;
                for &j in &spec.js {
                    let Some(st_j) = table.get(&j) else { continue };
                    let mut best: Option<(u64, u32)> = None;
                    let mut arrived = false;
                    for &fe in v.inn(st_j.nl) {
                        let g = t1.get(fe);
                        if g == INF {
                            continue;
                        }
                        arrived = true;
                        let pred = t1.pred(fe);
                        let hin_over = (pred == NONE).then(|| anchor.out_head[(fe & 1) as usize]);
                        let Some(tc) = turn_cost(v, &forb, fe, pred, st_j.f_out, hin_over, None) else {
                            continue;
                        };
                        if !retro_ok(v, pred, fe, st_j.f_out)
                            || (st_j.succ_out != NONE && !retro_ok(v, fe, st_j.f_out, st_j.succ_out))
                        {
                            continue;
                        }
                        let total = u64::from(g) + u64::from(tc);
                        if best.is_none_or(|(b, bf)| total < b || (total == b && fe < bf)) {
                            best = Some((total, fe));
                        }
                    }
                    let Some((_, fe)) = best else {
                        if arrived && attempt == 0 {
                            diag.shorten_turn_refused += 1;
                        }
                        continue;
                    };
                    let m = f64::from(t1.meters(fe)) / 10.0;
                    let l_new = s_i + m + (l_m - seams[j].s_m);
                    let err0 = err_of(l_new);
                    if attempt == 0 {
                        diag.shorten_reached += 1;
                        best_err0 = Some(best_err0.map_or(err0, |b| b.min(err0)));
                    }
                    if err0 <= tune.shorten_read_tol {
                        reads.push(Read { j, fe, err0, kept: s_i + l_m - seams[j].s_m });
                    }
                }
                if attempt == 0 {
                    diag.tree_best_err0.push(best_err0);
                }
                any_econ |= !reads.is_empty();
                // Comme le tri final : l'écart arrondi à 0,5 %, puis la plus grande part gardée, puis la couture.
                let bucket = |x: f64| (x / 0.005).round() as i64;
                reads.sort_by(|a, b| {
                    bucket(a.err0)
                        .cmp(&bucket(b.err0))
                        .then(b.kept.total_cmp(&a.kept))
                        .then(a.err0.total_cmp(&b.err0))
                        .then(a.j.cmp(&b.j))
                });
                reads.truncate(tune.shorten_j_per_i);
                let mut pending: Vec<(u32, u32)> = Vec::new();
                let mut got = false;
                for rd in reads {
                    let st_j = &table[&rd.j];
                    let mut fields = t1.path_to(rd.fe);
                    if fields.first() != Some(&st_i.f_in) || fields.len() < 2 {
                        diag.shorten_head_mismatch += 1;
                        continue;
                    }
                    fields.remove(0);
                    let start = link_snap(e, v, fields[0], false);
                    let end = link_snap(e, v, rd.fe, true);
                    let (mut route, step_field) =
                        route_between_snaps(v, &fields, &start, &end, req.familiarity);
                    crate::route::measure(e, team_data, ride, &mut route);
                    // Partie remplacée libre : aucune jambe gardée d'après `j` ne doit être reprise (C3 local).
                    if tune.free_shorten {
                        let used: rustc_hash::FxHashSet<u32> =
                            route.steps.iter().map(|s| s.base_edge).collect();
                        let after = &posed.legs()[(seams[rd.j].join + 1).min(posed.legs().len())..];
                        if after.iter().any(|l| used.contains(&l.edge)) {
                            diag.shorten_reuse_refused += 1;
                            *dominant_viol.entry(Violation::ReuseOutsideTail).or_insert(0) += 1;
                            continue;
                        }
                    }
                    // La règle des grappes de part et d'autre de chaque couture, avec les états gardés pour contexte.
                    if !org.junction_ok(&cx, &st_i.lead_before, st_i.dm, &fields, None) {
                        diag.junction_cut += 1;
                        continue;
                    }
                    let mut ret: Vec<u32> = vec![st_j.f_out];
                    ret.extend_from_slice(&st_j.lead_after);
                    if !org.junction_ok(&cx, &fields, st_i.dm.saturating_add(t1.meters(rd.fe)), &ret, None) {
                        diag.junction_cut += 1;
                        continue;
                    }
                    let (si, sj) = (seams[spec.i], seams[rd.j]);
                    let before = posed.context_before(si.join, CONTEXT_M);
                    let after = posed.context_after(sj.join, CONTEXT_M);
                    let chk = check_spliced_in(e, win, team_data, &chk_req, &before, &route, &after);
                    if !chk.violations.is_empty() {
                        for vv in &chk.violations {
                            *diag.spliced_refused.entry(*vv).or_insert(0) += 1;
                            *dominant_viol.entry(*vv).or_insert(0) += 1;
                        }
                        for &i in &chk.sharp_at_new {
                            if i + 1 < step_field.len() && step_field[i] != step_field[i + 1] {
                                pending.push((step_field[i], step_field[i + 1]));
                            }
                        }
                        continue;
                    }
                    let length_m = si.s_m + route.length_m + (l_m - sj.s_m);
                    got = true;
                    props.push(Prop {
                        case: spec.i,
                        order,
                        r: Retouched {
                            kind: RetouchKind::Shorten,
                            from_seam: Some(spec.i),
                            to_seam: Some(rd.j),
                            from_s_m: si.s_m,
                            to_s_m: sj.s_m,
                            length_m,
                            kept_m: si.s_m + l_m - sj.s_m,
                            err: Some(err_of(length_m)),
                            abandon_m: None,
                            new: route,
                        },
                    });
                    order += 1;
                }
                // Tous les `j` de ce `i` refusés pour un virage : une passe de plus avec les transitions refusées.
                if got || pending.is_empty() || attempt == 1 {
                    break;
                }
                forb.extend(pending);
            }
        }
    } else {
        for (ci, spec) in plan.lens.iter().enumerate() {
            if early(&props, &mut *diag) || dead.get() {
                break;
            }
            let (Some(st_i), Some(st_j)) = (table.get(&spec.i), table.get(&spec.j)) else { continue };
            ran = true;
            let (si, sj) = (seams[spec.i], seams[spec.j]);
            let ko_free;
            let ko = if tune.free_section {
                // La partie remplacée est libre : ses arêtes ne sont pas masquées (variante du lot 0).
                let edges =
                    posed.legs()[..=si.join].iter().chain(&posed.legs()[sj.join + 1..]).map(|l| l.edge);
                ko_free = KeepOut::build(win, edges, []);
                &ko_free
            } else {
                &ko_all
            };
            let graft = LensGraft {
                keep_out: ko,
                start: seam_anchor_start(e, v, st_i),
                end: seam_anchor_end(e, v, st_j),
                node_i: si.node,
                node_j: sj.node,
            };
            let rq = Request {
                start: LatLon::from_e7(e.pos(si.node)),
                end: Some(LatLon::from_e7(e.pos(sj.node))),
                bearing: None,
                distance_m: spec.ell_m + big,
                ride_type: ride,
                mode: req.mode,
                familiarity: req.familiarity,
                off_team_cost_q8: req.off_team_cost_q8,
                seed: req.seed,
                figures: Figures::Classic,
            };
            diag.windows_tried += 1;
            let (res, dg) = generate_graft_in(e, win, team, &rq, &graft, ws, &mut stopper);
            diag.settled += dg.t1_settled + dg.t2_settled + dg.astar_settled;
            for (vv, n) in &dg.rejected {
                *dominant_viol.entry(*vv).or_insert(0) += n;
            }
            match res {
                Err(Failure::WindowEscape) => return Err(RetouchFailure::WindowEscape),
                Err(Failure::Stopped) => {
                    if dead.get() {
                        break;
                    }
                    return Err(RetouchFailure::Stopped);
                }
                Err(Failure::NoTeamData) => return Err(RetouchFailure::NoTeamData),
                Err(Failure::TooLong) => return Err(RetouchFailure::TooLong),
                Err(f) => match fails.iter_mut().find(|x| x.0 == f) {
                    Some(x) => x.1 += 1,
                    None => fails.push((f, 1)),
                },
                Ok(mut list) => {
                    list.sort_by(|a, b| {
                        let (ea, eb) = (
                            err_of(si.s_m + a.route.length_m + (l_m - sj.s_m)),
                            err_of(si.s_m + b.route.length_m + (l_m - sj.s_m)),
                        );
                        ea.total_cmp(&eb)
                    });
                    let before = posed.context_before(si.join, CONTEXT_M);
                    let after = posed.context_after(sj.join, CONTEXT_M);
                    for sc in list {
                        let route = sc.route;
                        let chk = check_spliced_in(e, win, team_data, &chk_req, &before, &route, &after);
                        if !chk.violations.is_empty() {
                            for vv in &chk.violations {
                                *diag.spliced_refused.entry(*vv).or_insert(0) += 1;
                                *dominant_viol.entry(*vv).or_insert(0) += 1;
                            }
                            continue;
                        }
                        let length_m = si.s_m + route.length_m + (l_m - sj.s_m);
                        props.push(Prop {
                            case: ci,
                            order,
                            r: Retouched {
                                kind: RetouchKind::Lengthen,
                                from_seam: Some(spec.i),
                                to_seam: Some(spec.j),
                                from_s_m: si.s_m,
                                to_s_m: sj.s_m,
                                length_m,
                                kept_m: si.s_m + l_m - sj.s_m,
                                err: Some(err_of(length_m)),
                                abandon_m: None,
                                new: route,
                            },
                        });
                        order += 1;
                        break;
                    }
                }
            }
        }
    }
    diag.deadline_hit = dead.get();
    // Le tri de la note du lot 0 : d'abord à `tolerance` ou moins, puis l'écart arrondi à 0,5 %, puis la part gardée, puis
    // l'ordre de tirage ; une proposition par case.
    props.sort_by(|a, b| {
        let (ea, eb) = (a.r.err.unwrap_or(1.0), b.r.err.unwrap_or(1.0));
        (ea > tol)
            .cmp(&(eb > tol))
            .then(((ea / 0.005).round() as i64).cmp(&((eb / 0.005).round() as i64)))
            .then(b.r.kept_m.total_cmp(&a.r.kept_m))
            .then(a.order.cmp(&b.order))
    });
    let mut seen: Vec<usize> = Vec::new();
    let mut out: Vec<Retouched> = Vec::new();
    for p in props {
        if seen.contains(&p.case) {
            continue;
        }
        seen.push(p.case);
        out.push(p.r);
        if out.len() >= tune.proposals {
            break;
        }
    }
    diag.proposals_tight = out.iter().filter(|r| r.err.is_some_and(|x| x <= tune.pick_tol)).count() as u32;
    if let Some(first) = out.first() {
        let best_err = first.err.unwrap_or(1.0);
        if best_err > tol {
            return Err(RetouchFailure::OffTarget { best_err });
        }
        return Ok(out);
    }
    if dead.get() {
        return Err(RetouchFailure::Timeout);
    }
    if !ran {
        return Err(RetouchFailure::NoSeam);
    }
    if plan.is_shortening() {
        if !any_econ {
            return Err(RetouchFailure::NoEconomy);
        }
    } else if !fails.is_empty() && dominant_viol.is_empty() {
        let dominant = fails.iter().max_by_key(|(f, n)| (*n, format!("{f:?}"))).map(|(f, _)| *f);
        return Err(RetouchFailure::NoProposal { dominant });
    }
    match dominant_viol.iter().max_by_key(|(v, n)| (**n, **v)).map(|(v, _)| *v) {
        Some(dominant) => Err(RetouchFailure::AllRefused { dominant }),
        // Des chemins lus, tous coupés par la règle des grappes : aucun n'a pu être contrôlé.
        None if diag.junction_cut > 0 => Err(RetouchFailure::JunctionCut),
        None => Err(RetouchFailure::NoProposal { dominant: None }),
    }
}

/// Comme [`resize_in`], en bâtissant la fenêtre (et les agrégats de l'équipe), rejouée une fois élargie sur
/// [`RetouchFailure::WindowEscape`] : pour les tests et les outils ; le service garde ses fenêtres en cache.
#[allow(clippy::too_many_arguments)]
pub fn resize<B: AsRef<[u8]>>(
    e: &Engine<B>,
    team: Option<&TeamData>,
    posed: &PosedRoute,
    target_m: f64,
    req: &RetouchRequest,
    tune: &RetouchTune,
    ws: &mut Workspace,
    stop: &mut dyn FnMut() -> bool,
) -> Result<Retouch, RetouchFailure> {
    let mut extra_m = 0.0;
    let mut last = RetouchFailure::WindowEscape;
    for _ in 0..2 {
        let plan = e.plan_resize(posed, target_m, req, tune, 0.0, extra_m)?;
        let Some(win) = e.build_window(plan.window.view, &plan.window.need, stop) else {
            return Err(RetouchFailure::Stopped);
        };
        let wt = team.map(|t| WindowTeam::build(e, &win, t));
        let ctx = team.zip(wt.as_ref()).map(|(data, win)| TeamCtx { data, win });
        match resize_in(e, &win, ctx, posed, &plan, req, tune, ws, stop).map_err(|r| r.failure) {
            Err(RetouchFailure::WindowEscape) => {
                last = RetouchFailure::WindowEscape;
                extra_m = plan.window.snap_lmax_m + 10_000.0;
            }
            other => return other,
        }
    }
    Err(last)
}
