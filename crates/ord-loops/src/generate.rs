//! Génération (note lot 2, §5) : accroche, deux arbres, faisabilité, famille « point de demi-tour » (aller pris dans
//! l'arbre aller, retour par un A* sur le graphe privé de l'aller), contrôle de chaque candidat, choix de trois
//! parcours.

use std::collections::{BTreeMap, BTreeSet};

use ord_graph::format::GraphFile;
use ord_graph::util::SplitMix64;
use rustc_hash::FxHashSet;

use crate::engine::{Engine, Snap, ViewData};
use crate::geo::{heading_deg, turn_angle_deg, LatLon, Local};
use crate::reverse::astar_back;
use crate::search::{
    astar, make_anchor, tree_backward, tree_forward, Anchor, Ban, Ctx, Disc, Forbidden, Tree, ViewWs,
    Weighter, Workspace, BAN, INF, NONE, TAIL,
};
use crate::team::TeamData;
use crate::verify::{check_route, trim_piece, Violation};
use crate::{Diagnostics, Failure, Family, Generation, Mode, NoRouteDetail, Position, Request, Route, Step};

const SECTORS: usize = 12;
/// Équipe d'abord : à partir de cette familiarité, une proposition garde au moins cette part de la meilleure part d'équipe.
const MIN_FAMILIARITY_FOR_SHARE: f32 = 0.25;
const MIN_SHARE_RATIO: f64 = 0.5;
/// Passes : la première, puis jusqu'à deux autres après avoir interdit les virages que le contrôle a refusés.
const PASSES: usize = 6;

fn fingerprint(r: &Request) -> u64 {
    let (la, lo) = r.start.to_e7();
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for x in [
        la as u64,
        lo as u64,
        r.distance_m.to_bits(),
        r.ride_type.index() as u64,
        r.mode as u64,
        r.end.map_or(0, |p| p.to_e7().0 as u64),
        r.bearing.map_or(99, |b| b.degrees() as u64),
    ] {
        h = (h ^ x).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Les étapes d'une suite d'états : morceau partiel du départ, arêtes entières, morceau partiel de l'arrivée.
fn steps_of(v: &ViewData, fields: &[u32], s: &Snap, en: &Snap) -> (Vec<Step>, Vec<u32>) {
    let mut steps = Vec::new();
    let mut step_field = Vec::new();
    let n = fields.len();
    for (i, &f) in fields.iter().enumerate() {
        let links = v.oriented_links(f);
        let len = links.len();
        let a = if i == 0 {
            if f & 1 == 0 {
                s.link
            } else {
                len - 1 - s.link
            }
        } else {
            0
        };
        let b = if i == n - 1 {
            if f & 1 == 0 {
                en.link
            } else {
                len - 1 - en.link
            }
        } else {
            len - 1
        };
        if a > b {
            continue;
        }
        for &(e, rev) in &links[a..=b] {
            steps.push(Step { base_edge: e, reverse: rev });
            step_field.push(f);
        }
    }
    (steps, step_field)
}

/// Géométrie d'un parcours (points de la polyligne, du départ à l'arrivée).
pub fn route_geometry<B: AsRef<[u8]>>(base: &GraphFile<B>, r: &Route) -> Vec<LatLon> {
    let mut out: Vec<(i32, i32)> = Vec::new();
    for (i, st) in r.steps.iter().enumerate() {
        let (a, b) = base.edge_ends(st.base_edge);
        let mut nat = vec![base.node_pos(a)];
        nat.extend(base.geometry(st.base_edge));
        nat.push(base.node_pos(b));
        let (pts, ..) = trim_piece(r, i, nat);
        for p in pts {
            if out.last() != Some(&p) {
                out.push(p);
            }
        }
    }
    out.into_iter().map(LatLon::from_e7).collect()
}

struct Scored {
    route: Route,
    score: f64,
    key: Vec<Step>,
}

fn overlap_share<B: AsRef<[u8]>>(e: &Engine<B>, a: &Route, b: &Route) -> f64 {
    let sa: BTreeSet<u32> = a.steps.iter().map(|s| s.base_edge).collect();
    let mut common = 0u64;
    let mut seen = BTreeSet::new();
    for s in &b.steps {
        if sa.contains(&s.base_edge) && seen.insert(s.base_edge) {
            common += u64::from(e.bd.len_dm[s.base_edge as usize]);
        }
    }
    let la: u64 = sa.iter().map(|&x| u64::from(e.bd.len_dm[x as usize])).sum();
    let lb: u64 = b
        .steps
        .iter()
        .map(|s| s.base_edge)
        .collect::<BTreeSet<_>>()
        .iter()
        .map(|&x| u64::from(e.bd.len_dm[x as usize]))
        .sum();
    common as f64 / la.min(lb).max(1) as f64
}

fn bearing_deviation<B: AsRef<[u8]>>(e: &Engine<B>, r: &Request, route: &Route) -> Option<f64> {
    let b = r.bearing?;
    let s = r.start.to_e7();
    let loc = Local::new(s);
    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
    for st in &route.steps {
        let (a, bb) = e.bd.ends[st.base_edge as usize];
        let p = loc.xy(e.bd.pos[(if st.reverse { a } else { bb }) as usize]);
        sx += p.0;
        sy += p.1;
        n += 1.0;
    }
    if n == 0.0 {
        return None;
    }
    Some(turn_angle_deg(b.degrees(), heading_deg(sx / n, sy / n)))
}

/// Génère jusqu'à trois parcours.
pub fn generate<B: AsRef<[u8]>>(
    e: &Engine<B>,
    team: Option<&TeamData>,
    r: &Request,
    ws: &mut Workspace,
    stop: &mut dyn FnMut() -> bool,
) -> Result<Generation, Failure> {
    generate_with_diagnostics(e, team, r, ws, stop).0
}

/// Comme [`generate`], mais rend aussi les diagnostics quand rien n'est rendu (réseau atteint, chemin de retour,
/// refus du contrôle) : sans eux un échec ne se classe pas.
pub fn generate_with_diagnostics<B: AsRef<[u8]>>(
    e: &Engine<B>,
    team: Option<&TeamData>,
    r: &Request,
    ws: &mut Workspace,
    stop: &mut dyn FnMut() -> bool,
) -> (Result<Generation, Failure>, Diagnostics) {
    let mut diag = Diagnostics::default();
    let res = generate_inner(e, team, r, ws, stop, &mut diag);
    match res {
        Ok(mut g) => {
            g.diagnostics = diag.clone();
            (Ok(g), diag)
        }
        Err(f) => (Err(f), diag),
    }
}

fn generate_inner<B: AsRef<[u8]>>(
    e: &Engine<B>,
    team: Option<&TeamData>,
    r: &Request,
    ws: &mut Workspace,
    stop: &mut dyn FnMut() -> bool,
    diag: &mut Diagnostics,
) -> Result<Generation, Failure> {
    if r.mode != Mode::Discovery && team.is_none() {
        return Err(Failure::NoTeamData);
    }
    let d = r.distance_m;
    let ride = r.ride_type;
    let vi = ride.view_index();
    let v = e.view(ride);
    let w = Weighter::new(e, team, ride, r.mode, r.familiarity);
    let min_dm = (e.params.min_component_share * d * 10.0) as u64;
    let strict_tt = if w.strict() { w.tt() } else { None };
    let ok = |c: u32| -> bool {
        w.chain(c) != INF && strict_tt.is_none_or(|tt| tt.strict_comp_len_of_chain(v, c) >= min_dm)
    };
    let s_snap = e.snap(ride, r.start, &ok, min_dm).ok_or(Failure::StartTooFar)?;
    let is_loop = r.end.is_none();
    let e_snap = match r.end {
        None => s_snap,
        Some(p) => e.snap(ride, p, &ok, min_dm).ok_or(Failure::EndTooFar)?,
    };
    let sa: Anchor = make_anchor(e, v, &w, s_snap);
    let ea: Anchor = if is_loop { sa } else { make_anchor(e, v, &w, e_snap) };
    let s_e7 = r.start.to_e7();
    let e_e7 = r.end.unwrap_or(r.start).to_e7();
    let disc = Disc::new(s_e7, e_e7, d, is_loop);
    diag.start_snap_m = s_snap.dist_m;
    diag.end_snap_m = e_snap.dist_m;
    let mut rng = SplitMix64(r.seed ^ fingerprint(r));
    let mut scored: Vec<Scored> = Vec::new();
    let mut rejected: BTreeMap<Violation, u32> = BTreeMap::new();
    let mut exhausted = false;
    let mut forb = Forbidden::default();
    let tail_dm = (e.params.tail_reuse_share * d * 10.0) as u32;
    let d_dm = d * 10.0;

    for pass in 0..PASSES {
        let cx = Ctx { v, w: &w, disc: &disc, s: &sa, e: &ea, forb: &forb };
        let vws = &mut ws.views[vi];
        if vws.ban.len() != v.n_chains {
            vws.ban = vec![0; v.n_chains];
            vws.ban_gen = 0;
        }
        tree_forward(&cx, &mut vws.t1, stop).map_err(|_| Failure::Stopped)?;
        tree_backward(&cx, &mut vws.t2, stop).map_err(|_| Failure::Stopped)?;
        let ViewWs { t1, t2, ta, ban, ban_gen } = vws;
        let (t1, t2) = (&*t1, &*t2);
        if pass == 0 {
            diag.t1_settled = t1.settled.len() as u64;
            diag.t2_settled = t2.settled.len() as u64;
            feasibility(v, &sa, &s_snap, &e_snap, t1, t2, is_loop, d, diag)?;
        }

        // Candidats « point de demi-tour » : un secteur de 30° après l'autre.
        let mut sectors: Vec<Vec<(u32, u32, u32)>> = vec![Vec::new(); SECTORS];
        for &node in &t1.reached {
            let Some(b2) = t2.node_best(node) else { continue };
            let b1 = t1.node_best(node).expect("atteint");
            let ang = disc.bearing_of(v.pos[node as usize]);
            sectors[((ang / 30.0) as usize) % SECTORS].push((node, t1.meters(b1), t2.meters(b2)));
        }
        let mut order: Vec<usize> = (0..SECTORS)
            .filter(|&k| match r.bearing {
                Some(b) if is_loop => turn_angle_deg(b.degrees(), k as f64 * 30.0 + 15.0) <= 50.0,
                _ => true,
            })
            .collect();
        for i in (1..order.len()).rev() {
            let j = rng.below(i + 1);
            order.swap(i, j);
        }
        let mut budget = u64::from(e.params.astar_budget) * t1.settled.len() as u64 + 20_000;
        // L'ordre inverse a son propre budget : il ne doit pas épuiser celui de l'ordre de base.
        let mut rev_budget = u64::from(e.params.astar_budget) * t1.settled.len() as u64 / 3 + 10_000;
        let mut new_forb: Vec<(u32, u32)> = Vec::new();
        for &sector in &order {
            if stop() {
                return Err(Failure::Stopped);
            }
            let list = &sectors[sector];
            if list.is_empty() {
                continue;
            }
            let mut rho = e.params.rho0 * (1.0 + (rng.below(61) as f64 - 30.0) / 1000.0);
            // Nœuds écartés du secteur : les demi-tours essayés, et tous les nœuds d'un candidat rendu mais écarté
            // (hors tolérance ou refusé) : un demi-tour pris sur ce même parcours redonnerait le même cycle, de même
            // longueur, quel que soit ρ (campagne du lot 2 : 5 à 18 candidats identiques sur les réseaux stricts).
            let mut tried: FxHashSet<u32> = FxHashSet::default();
            for _ in 0..e.params.tries_per_sector {
                let pick = list
                    .iter()
                    .filter(|x| !tried.contains(&x.0))
                    .map(|&(n, o, b)| ((f64::from(o) + rho * f64::from(b) - d_dm).abs(), n, o, b))
                    .min_by(|x, y| x.0.partial_cmp(&y.0).expect("fini").then(x.1.cmp(&y.1)));
                let Some((_, tnode, _, back_dm)) = pick else { break };
                tried.insert(tnode);
                let out_fields = t1.path_to(t1.node_best(tnode).expect("atteint"));
                let last = *out_fields.last().expect("chemin non vide");
                let last_pred = if out_fields.len() >= 2 { out_fields[out_fields.len() - 2] } else { NONE };
                // Interdits : l'aller (hors queue) ; queue : le début de l'aller, repris au coût x 2.
                *ban_gen += 1;
                if *ban_gen >= (1 << 28) {
                    ban.iter_mut().for_each(|x| *x = 0);
                    *ban_gen = 1;
                }
                for &f in &out_fields {
                    let c = (f >> 1) as usize;
                    let bits = if t1.meters(f) <= tail_dm { TAIL } else { BAN };
                    let cur = if ban[c] >> 3 == *ban_gen { ban[c] & 7 } else { 0 };
                    let bits = if cur & BAN != 0 { BAN } else { bits };
                    ban[c] = (*ban_gen << 3) | bits;
                }
                let b = Ban { stamp: ban, gen: *ban_gen };
                diag.astar_runs += 1;
                let before = budget;
                let found = astar(&cx, ta, t2, last, last_pred, &b, &mut budget, &mut exhausted, stop)
                    .map_err(|_| Failure::Stopped)?;
                diag.astar_settled += before - budget;
                // (états du parcours, mètres (dm) du parcours, mètres (dm) du retour)
                let mut got: Option<(Vec<u32>, f64, f64)> = found.map(|f| {
                    let mut fields = out_fields.clone();
                    fields.extend_from_slice(&f.fields);
                    (fields, f64::from(t1.meters(last)) + f64::from(f.meters), f64::from(f.meters))
                });
                if got.is_none() && !exhausted {
                    // Ordre inverse : retour = chemin de l'arbre retour, aller cherché à rebours.
                    let b2 = t2.node_best(tnode).expect("atteint par T2");
                    let mut ret = t2.path_to(b2);
                    ret.reverse();
                    *ban_gen += 1;
                    if *ban_gen >= (1 << 28) {
                        ban.iter_mut().for_each(|x| *x = 0);
                        *ban_gen = 1;
                    }
                    for &f in &ret {
                        let c = (f >> 1) as usize;
                        let bits = if t2.meters(f) <= tail_dm { TAIL } else { BAN };
                        let cur = if ban[c] >> 3 == *ban_gen { ban[c] & 7 } else { 0 };
                        let bits = if cur & BAN != 0 { BAN } else { bits };
                        ban[c] = (*ban_gen << 3) | bits;
                    }
                    let b = Ban { stamp: ban, gen: *ban_gen };
                    diag.astar_runs += 1;
                    let before = rev_budget;
                    let mut rev_exhausted = false;
                    let back =
                        astar_back(&cx, ta, t1, tnode, ret[0], &b, &mut rev_budget, &mut rev_exhausted, stop)
                            .map_err(|_| Failure::Stopped)?;
                    diag.astar_settled += before - rev_budget;
                    got = back.map(|f| {
                        let ret_m = f64::from(t2.meters(ret[0]));
                        let mut fields = f.fields;
                        fields.extend_from_slice(&ret);
                        (fields, f64::from(f.meters) + ret_m, ret_m)
                    });
                    if got.is_some() {
                        diag.reverse_found += 1;
                    }
                }
                let Some((fields, total_dm, ret_dm)) = got else {
                    if exhausted {
                        break;
                    }
                    continue;
                };
                diag.candidates += 1;
                let rho_seen = ret_dm / f64::from(back_dm.max(1));
                tried.extend(fields.iter().map(|&f| v.tgt(f)));
                if (total_dm - d_dm).abs() > e.params.tolerance * d_dm {
                    diag.off_tolerance.push((total_dm / d_dm) as f32);
                    rho = rho_seen;
                    continue;
                }
                diag.within_tolerance += 1;
                let (steps, step_field) = steps_of(v, &fields, &s_snap, &e_snap);
                let mut route = Route {
                    start: Position { edge: s_snap.edge, offset_m: s_snap.offset_m },
                    end: Position { edge: e_snap.edge, offset_m: e_snap.offset_m },
                    steps,
                    length_m: 0.0,
                    team_m: 0.0,
                    bridged_m: 0.0,
                    shared_m: 0.0,
                    crossings: 0,
                    unpaved_m: 0.0,
                    presumed_surface_m: 0.0,
                    bearing_deviation_deg: None,
                    familiarity: r.familiarity,
                    family: Family::Turnaround,
                };
                let chk = check_route(e, team, r, &route);
                if !chk.violations.is_empty() {
                    for vcode in &chk.violations {
                        *rejected.entry(*vcode).or_insert(0) += 1;
                    }
                    for &i in &chk.sharp_at {
                        if let Some(st) = route.steps.get(i) {
                            let (a, b) = e.bd.ends[st.base_edge as usize];
                            diag.sharp_nodes.push(if st.reverse { a } else { b });
                        }
                        if i + 1 < step_field.len() && step_field[i] != step_field[i + 1] {
                            new_forb.push((step_field[i], step_field[i + 1]));
                        }
                    }
                    rho = rho_seen;
                    continue;
                }
                route.length_m = chk.length_m;
                route.team_m = chk.team_m;
                route.bridged_m = chk.bridged_m;
                route.shared_m = chk.shared_m;
                route.crossings = chk.crossings;
                route.unpaved_m = chk.unpaved_m;
                route.presumed_surface_m = chk.presumed_surface_m;
                route.bearing_deviation_deg = bearing_deviation(e, r, &route);
                let dev = route.bearing_deviation_deg.unwrap_or(0.0) / 45.0;
                let team_term = if r.mode == Mode::Discovery {
                    0.0
                } else {
                    3.0 * (1.0 - (route.team_m + route.bridged_m) / route.length_m.max(1.0))
                };
                let score = (route.length_m - d).abs() / d * 20.0
                    + route.shared_m / d * 10.0
                    + f64::from(route.crossings) * 0.5
                    + dev
                    + team_term;
                let key = route.steps.clone();
                if !scored.iter().any(|x| x.key == key) {
                    scored.push(Scored { route, score, key });
                }
                break;
            }
            if exhausted {
                break;
            }
        }
        let wanted = scored.len() >= 3;
        if exhausted || wanted || new_forb.is_empty() {
            break;
        }
        forb.extend(new_forb);
    }
    diag.rejected = rejected.into_iter().collect();
    if scored.is_empty() {
        let only_tail = !diag.rejected.is_empty()
            && diag
                .rejected
                .iter()
                .all(|(c, _)| matches!(c, Violation::TailTooLong | Violation::ReuseOutsideTail));
        let detail = if exhausted {
            NoRouteDetail::Budget
        } else if only_tail {
            NoRouteDetail::TailTooLong
        } else {
            NoRouteDetail::NoCandidate
        };
        return Err(Failure::NoRoute(detail));
    }
    scored.sort_by(|a, b| a.score.partial_cmp(&b.score).expect("fini").then_with(|| a.key.cmp_key(&b.key)));
    // Équipe d'abord, familiarité demandée : une proposition qui n'emprunte presque pas la route de l'équipe n'est pas
    // gardée à côté d'une autre qui s'y tient (moins de la moitié de la meilleure part d'équipe).
    let share = |x: &Route| (x.team_m + x.bridged_m) / x.length_m.max(1.0);
    let min_share = if r.mode == Mode::TeamFirst && r.familiarity >= MIN_FAMILIARITY_FOR_SHARE {
        MIN_SHARE_RATIO * scored.iter().map(|s| share(&s.route)).fold(0.0, f64::max)
    } else {
        0.0
    };
    let mut chosen: Vec<Route> = Vec::new();
    for s in scored {
        if chosen.len() == 3 {
            break;
        }
        if share(&s.route) < min_share {
            continue;
        }
        if chosen.iter().all(|c| overlap_share(e, c, &s.route) < 0.5) {
            chosen.push(s.route);
        }
    }
    Ok(Generation {
        routes: chosen,
        network_m: diag.reached_m,
        familiarity_fallback: false,
        diagnostics: Diagnostics::default(),
    })
}

/// Faisabilité sur les deux arbres, avant tout candidat (corrections Java §13.10 et §13.11).
#[allow(clippy::too_many_arguments)]
fn feasibility(
    v: &ViewData,
    sa: &Anchor,
    s_snap: &Snap,
    e_snap: &Snap,
    t1: &Tree,
    t2: &Tree,
    is_loop: bool,
    d: f64,
    diag: &mut Diagnostics,
) -> Result<(), Failure> {
    let mut seen = vec![false; v.n_chains];
    let mut reached_dm = 0u64;
    for &f in &t1.settled {
        let c = (f >> 1) as usize;
        if !seen[c] {
            seen[c] = true;
            reached_dm += u64::from(v.len_dm[c]);
        }
    }
    diag.reached_m = reached_dm as f64 / 10.0;
    let mut way_back_dm = 0u64;
    for (c, &s) in seen.iter().enumerate() {
        if s && (t2.get((c as u32) << 1) != INF || t2.get(((c as u32) << 1) | 1) != INF) {
            way_back_dm += u64::from(v.len_dm[c]);
        }
    }
    diag.way_back_m = way_back_dm as f64 / 10.0;
    if !is_loop {
        // L'arrivée est atteinte si un état de départ mène à un nœud de l'arbre retour.
        let mut shortest_dm: Option<u32> = None;
        for dir in 0..2u32 {
            if sa.out_cost[dir as usize] == INF {
                continue;
            }
            let f = (s_snap.chain << 1) | dir;
            if let Some(b) = t2.node_best(v.tgt(f)) {
                let m = t1.meters(f) + t2.meters(b);
                shortest_dm = Some(shortest_dm.map_or(m, |x| x.min(m)));
            }
        }
        if s_snap.chain == e_snap.chain {
            shortest_dm = shortest_dm.or(Some(0));
        }
        let Some(sh) = shortest_dm else { return Err(Failure::NoRoute(NoRouteDetail::EndUnreachable)) };
        if d < 1.05 * f64::from(sh) / 10.0 {
            return Err(Failure::DistanceTooShort);
        }
    }
    if diag.reached_m < 0.9 * d {
        return Err(Failure::NetworkTooSmall);
    }
    if is_loop && diag.way_back_m < 0.5 * d {
        return Err(Failure::NoRoute(NoRouteDetail::NoWayBack));
    }
    Ok(())
}

trait CmpKey {
    fn cmp_key(&self, other: &Self) -> std::cmp::Ordering;
}

impl CmpKey for Vec<Step> {
    fn cmp_key(&self, other: &Self) -> std::cmp::Ordering {
        let k = |v: &Vec<Step>| v.iter().map(|s| (s.base_edge, s.reverse)).collect::<Vec<_>>();
        k(self).cmp(&k(other))
    }
}
