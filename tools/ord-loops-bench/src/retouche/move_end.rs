//! Les essais de `move_end` (note : `ord-retouche-lot0.md` §4) : déplacer l'arrivée d'une trace posée vers `E′`.
//!
//! Le moteur rend l'arbre retour par `probe_graft::move_end_in` ; le banc tire `E′`, bâtit le masque de toute la marche,
//! recolle (préfixe + raccord) et contrôle (C1 à C3). **Aucune position, aucun id** dans ce qui sort.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;

use ord_graph_match::walk::Walk;
use ord_loops::probe_graft::{
    move_end_in, KeepOut, MoveEndFail, MoveEndOut, MoveEndParams, MoveEndPick, SeamIn,
};
use ord_loops::{LatLon, Request, TeamCtx, Workspace};
use ord_loops_check::haversine_m;
use rustc_hash::FxHashSet;
use serde::Serialize;
use xxhash_rust::xxh3::xxh3_64;

use super::control::{self, node_latlon, OrigCache};
use super::draw::Trial;
use super::posed::Posed;
use super::{Cursor, Env};
use crate::likeness::play::route_hash;

/// Une sortie choisie, côté chiffres.
#[derive(Clone, Debug, Serialize)]
pub struct Variant {
    pub abandon_m: f64,
    pub raccord_m: f64,
    /// Raccord sur le vol d'oiseau de la couture à `E′`.
    pub raccord_over_crow: f64,
    /// `L′ / L`.
    pub length_ratio: f64,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct MeLine {
    pub team: String,
    pub rank: u32,
    pub ty: &'static str,
    pub shape: &'static str,
    pub cursor: f64,
    pub kind: String,
    /// `ok`, `truncated`, `end_too_far`, `end_on_route_no_seam`, `end_on_masked_chain`, `no_seam_reached`, `window_escape`, `no_team_data`,
    /// `panic`.
    pub outcome: String,
    /// Le recollé de la variante `best` passe C1, C2 et C3 (ou le parcours est tronqué à une couture).
    pub accepted: bool,
    pub crow_m: f64,
    pub rho_m: Option<f64>,
    pub widened: bool,
    /// Passes de `move_end_in` (reprises après un virage serré refusé par le contrôle interne).
    pub passes: u32,
    pub prefix_retry: bool,
    pub t2_settled: u64,
    pub seams_reached: u32,
    pub masked_chains: usize,
    pub masked_m: f64,
    pub best: Option<Variant>,
    pub keep_all: Option<Variant>,
    pub nearest: Option<Variant>,
    pub c1: Vec<String>,
    pub c2: Vec<String>,
    pub c3_outside_m: f64,
    pub c3_zone_m: f64,
    pub checked_m: f64,
    pub unchecked_m: f64,
    pub team_share: Option<f64>,
    pub angle_deg: Option<f64>,
    /// Temps de `move_end_in`, ms ; des contrôles, ms ; de la fenêtre (cache ou construction), ms.
    pub gen_ms: f64,
    pub ctl_ms: f64,
    pub win_ms: f64,
    /// Somme des temps de tous les appels à `move_end_in` de l'essai, échecs et reprises compris, ms.
    pub call_ms: f64,
    pub fp: String,
}

fn variant(p: &Posed, env: &Env<'_>, pk: &MoveEndPick, seam_global: usize, eprime: (f64, f64)) -> Variant {
    let s_k = p.seams[seam_global].s_m;
    let ll = node_latlon(env.ctl.gf, p.seams[seam_global].seam.node);
    let crow = haversine_m((ll.lat, ll.lon), eprime).max(1.0);
    Variant {
        abandon_m: pk.abandon_m,
        raccord_m: pk.raccord_m,
        raccord_over_crow: pk.raccord_m / crow,
        length_ratio: (s_k + pk.raccord_m) / p.l_m.max(1.0),
    }
}

/// Joue un essai.
pub fn play_trial(
    env: &Env<'_>,
    p: &Posed,
    cursor: &Cursor,
    trial: &Trial,
    ws: &mut Workspace,
    cache: &OrigCache,
) -> MeLine {
    let mut line = MeLine {
        team: p.letter.clone(),
        rank: p.rank,
        ty: super::continuity::type_name(p.ty),
        shape: match p.shape {
            super::continuity::Shape::Loop => "loop",
            super::continuity::Shape::OneWay => "one_way",
        },
        cursor: cursor.value,
        kind: trial.kind.label(),
        ..MeLine::default()
    };
    let valid = p.before(env.margin_m);
    if valid.is_empty() {
        line.outcome = "no_seam".into();
        return line;
    }
    let end = *p.line.pts.last().expect("polyligne non vide");
    let crow = haversine_m(end, trial.pt);
    line.crow_m = crow;
    let rho = (1.6 * crow + 3_000.0).clamp(5_000.0, 40_000.0);
    let prm = MoveEndParams {
        radius_m: rho,
        max_radius_m: (2.0 * rho).min(40_000.0).max(rho),
        min_radius_m: 5_000.0,
    };
    let proto = cursor.request(p, env.seed);
    let req = Request {
        start: LatLon::new(trial.pt.0, trial.pt.1),
        end: None,
        distance_m: 2.0 * prm.max_radius_m,
        ..proto.clone()
    };
    let seams: Vec<SeamIn> = valid
        .iter()
        .map(|&k| {
            let s = &p.seams[k];
            let leg = &p.walk.legs[s.seam.join];
            SeamIn {
                node: s.seam.node,
                in_edge: leg.edge,
                in_reverse: leg.reverse,
                abandon_m: (p.l_m - s.s_m).max(0.0),
            }
        })
        .collect();
    let all_edges = || p.edge_set.iter().copied();
    let t0 = Instant::now();
    let mut call_ms = 0.0;
    let (mut win, mut wt) = env.wins.acquire(env.team_key, env.team, &req, None);
    let mut ko = KeepOut::build(&win, all_edges(), []);
    let mut res = call(env, &win, &wt, &req, &seams, &ko, ws, &prm, &mut call_ms);
    if matches!(res, Ok(Err(MoveEndFail::WindowEscape))) {
        (win, wt) = env.wins.acquire_wide(env.team, &req, None);
        ko = KeepOut::build(&win, all_edges(), []);
        res = call(env, &win, &wt, &req, &seams, &ko, ws, &prm, &mut call_ms);
    }
    // Aucune couture atteinte : on rejoue une fois avec le masque du seul préfixe de la couture la plus tardive.
    if matches!(res, Ok(Err(MoveEndFail::NoSeamReached))) {
        let last = &p.seams[*valid.last().expect("non vide")];
        ko = KeepOut::build(&win, p.walk.legs[..=last.seam.join].iter().map(|l| l.edge), []);
        res = call(env, &win, &wt, &req, &seams, &ko, ws, &prm, &mut call_ms);
        line.prefix_retry = true;
    }
    line.win_ms = t0.elapsed().as_secs_f64() * 1000.0;
    line.call_ms = call_ms;
    line.masked_chains = ko.masked_chains();
    line.masked_m = ko.masked_m(&win);
    let res = match res {
        Err(_) => {
            line.outcome = "panic".into();
            return finish(line);
        }
        Ok(r) => r,
    };
    match res {
        Err(MoveEndFail::EndTooFar) => line.outcome = "end_too_far".into(),
        Err(MoveEndFail::NoSeamReached) => line.outcome = "no_seam_reached".into(),
        Err(MoveEndFail::WindowEscape) => line.outcome = "window_escape".into(),
        Err(MoveEndFail::NoTeamData) => line.outcome = "no_team_data".into(),
        Err(MoveEndFail::Stopped) => line.outcome = "stopped".into(),
        Err(MoveEndFail::EndOnRoute { edge }) => {
            // `E′` est sur le parcours : on le tronque à la première couture valide après son passage. Une arête
            // parcourue plusieurs fois (tige aller-retour, tour) est prise à son **dernier** passage, le cas prudent
            // pour K2. Une arête absente de la marche est une chaîne masquée plus longue que la marche (départ ou
            // arrivée au milieu d'une chaîne) : échec à part, jamais accepté, qui reste au dénominateur de K2.
            let Some(at) = last_pass(p.walk.legs.iter().map(|l| l.edge), edge) else {
                line.outcome = "end_on_masked_chain".into();
                return finish(line);
            };
            match valid.iter().find(|&&k| p.seams[k].seam.join >= at) {
                Some(&k) => {
                    line.outcome = "truncated".into();
                    line.accepted = true;
                    let abandon = (p.l_m - p.seams[k].s_m).max(0.0);
                    line.best = Some(Variant {
                        abandon_m: abandon,
                        raccord_m: 0.0,
                        raccord_over_crow: 0.0,
                        length_ratio: p.seams[k].s_m / p.l_m.max(1.0),
                    });
                }
                None => line.outcome = "end_on_route_no_seam".into(),
            }
        }
        Ok(out) => {
            line.outcome = "ok".into();
            record(env, p, &proto, trial, &valid, &out, cache, &mut line);
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

#[allow(clippy::too_many_arguments)]
fn call(
    env: &Env<'_>,
    win: &ord_loops::Window,
    wt: &ord_loops::WindowTeam,
    req: &Request,
    seams: &[SeamIn],
    ko: &KeepOut,
    ws: &mut Workspace,
    prm: &MoveEndParams,
    acc_ms: &mut f64,
) -> Result<Result<MoveEndOut, MoveEndFail>, ()> {
    let t = Instant::now();
    let r = catch_unwind(AssertUnwindSafe(|| {
        let tc = TeamCtx { data: env.team, win: wt };
        move_end_in(env.engine, win, Some(tc), req, seams, ko, ws, prm)
    }))
    .map_err(|_| ());
    *acc_ms += t.elapsed().as_secs_f64() * 1000.0;
    r
}

#[allow(clippy::too_many_arguments)]
fn record(
    env: &Env<'_>,
    p: &Posed,
    proto: &Request,
    trial: &Trial,
    valid: &[usize],
    out: &MoveEndOut,
    cache: &OrigCache,
    line: &mut MeLine,
) {
    line.rho_m = Some(out.radius_m);
    line.widened = out.widened;
    line.passes = out.passes;
    line.t2_settled = out.t2_settled;
    line.seams_reached = out.seams_reached;
    line.gen_ms = out.us / 1000.0;
    let mk = |pk: &MoveEndPick| variant(p, env, pk, valid[pk.seam], trial.pt);
    line.best = Some(mk(&out.best));
    line.keep_all = out.keep_all.as_ref().map(&mk);
    line.nearest = out.nearest.as_ref().map(&mk);

    // Les contrôles de la variante `best`.
    let t0 = Instant::now();
    let pick = &out.best;
    let seam = &p.seams[valid[pick.seam]];
    let gf = env.ctl.gf;
    let new = Walk::from_route(gf, &pick.raccord);
    let eprime = LatLon::new(trial.pt.0, trial.pt.1);
    let req_seg = control::seg_request(proto, node_latlon(gf, seam.seam.node), Some(eprime), pick.raccord_m);
    let rep = env.ctl.checker.check(&req_seg, &pick.raccord);
    line.c1 = control::codes_of(&rep).into_iter().map(String::from).collect();
    line.team_share = (rep.length_m > 0.0).then(|| rep.team_m / rep.length_m);
    let sp = control::splice(p, seam.seam.join, None, &new, seam.seam.node, None);
    let c2 = control::c2(&env.ctl, cache, p, &sp, seam.seam.join, None, proto);
    line.c2 = c2.new_codes.iter().map(|c| (*c).to_string()).collect();
    line.checked_m = c2.checked_m;
    line.unchecked_m = c2.unchecked_m;
    let zone: FxHashSet<u32> =
        control::zone_edges(p, seam.seam.join, env.margin_m, true).into_iter().collect();
    let kept: FxHashSet<u32> =
        p.walk.legs[..=seam.seam.join].iter().map(|l| l.edge).filter(|e| !zone.contains(e)).collect();
    let c3 = control::c3(gf, &pick.raccord.steps, &kept, &zone);
    line.c3_outside_m = c3.outside_m;
    line.c3_zone_m = c3.zone_m;
    line.angle_deg = control::seam_angles(gf, p, &sp).0;
    line.ctl_ms = t0.elapsed().as_secs_f64() * 1000.0;
    line.accepted = line.c1.is_empty() && line.c2.is_empty() && c3.outside_m <= 0.0;
    let mut fp = Vec::new();
    fp.extend_from_slice(&(valid[pick.seam] as u64).to_le_bytes());
    fp.extend_from_slice(&route_hash(&pick.raccord).to_le_bytes());
    fp.extend_from_slice(&u64::from(line.accepted).to_le_bytes());
    line.fp = format!("{:016x}", xxh3_64(&fp));
}

/// Indice de la jambe du dernier passage de `edge` sur la marche, `None` si la marche n'y passe pas.
fn last_pass(
    mut edges: impl DoubleEndedIterator<Item = u32> + ExactSizeIterator,
    edge: u32,
) -> Option<usize> {
    edges.rposition(|e| e == edge)
}

/// Le groupe d'un essai pour les agrégats, lu sur son étiquette (`r2_b0`, `team_end`, `close_loop`).
pub fn kind_group_of(label: &str) -> &'static str {
    match label.strip_prefix('r').and_then(|r| r.split('_').next()).and_then(|k| k.parse::<f64>().ok()) {
        Some(km) if km <= 2.0 => "ring_2",
        Some(km) if km <= 5.0 => "ring_5",
        Some(_) => "ring_15",
        None if label == "team_end" => "team_end",
        None => "close_loop",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_pass_takes_the_final_ride_and_reports_a_missing_edge() {
        let walk = [4u32, 7, 9, 7, 5];
        assert_eq!(last_pass(walk.iter().copied(), 7), Some(3));
        assert_eq!(last_pass(walk.iter().copied(), 4), Some(0));
        assert_eq!(last_pass(walk.iter().copied(), 99), None);
    }
}
