//! Une requête rejouée dans un mode : ce qui est écrit au rapport JSON, et son exécution.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;

use ord_graph::format::GraphFile;
use ord_loops::{
    check_route, generate_with_diagnostics, route_geometry, Diagnostics, Engine, Failure, Generation, Mode,
    NoRouteDetail, Request, Route, TeamData, Workspace,
};
use ord_loops_check::Checker;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct JavaSide {
    pub outcome: String,
    pub ok: bool,
    pub ms: f64,
    /// Plus longue part recouverte des routes rendues, m.
    pub shared_m: Option<f64>,
    pub crossings: Option<u64>,
    pub km: Option<f64>,
    /// Issue du banc `head-alone`, s'il a joué cette requête.
    pub alone_outcome: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RouteRec {
    pub length_km: f64,
    /// Longueur / D.
    pub over_target: f64,
    pub team_share: f64,
    pub bridged_m: f64,
    pub shared_m: f64,
    pub crossings: u32,
    /// Longueur sur des arêtes d'équipe, telle que le moteur la compte, m.
    pub engine_team_m: f64,
    pub family: String,
    /// Violations du contrôleur indépendant.
    pub violations: Vec<String>,
    /// Refus du contrôle interne du moteur (vide pour un parcours rendu).
    pub internal: Vec<String>,
    pub checker_shared_m: f64,
    pub checker_crossings: u32,
    pub checker_team_m: f64,
    pub checker_bridged_m: f64,
    pub start_zone_m: f64,
    pub end_zone_m: f64,
    pub corridor_share: f64,
    /// Empreinte de la géométrie arrondie au millionième de degré (celle de la polyligne du service).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geom_xxh3: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Rec {
    pub team: String,
    pub i: u32,
    pub kind: String,
    pub start: usize,
    pub km: f64,
    pub bearing: String,
    pub ride: String,
    pub familiarity: f32,
    /// `interior`, `edge` (bord de la région).
    pub scope: String,
    pub mode: String,
    /// `OK`, `VIOLATION` (des parcours rendus, aucun accepté), un code d'échec ou `PANIC`.
    pub outcome: String,
    pub n_routes: usize,
    pub n_ok: usize,
    pub violations: BTreeMap<String, u32>,
    /// Parcours que le contrôleur refuse alors que le contrôle interne du moteur les avait laissé passer.
    pub disagreements: u32,
    /// Médiane des tours chauds, ms (le tour froid s'il n'y en a pas).
    pub ms: f64,
    pub cold_ms: f64,
    pub routes: Vec<RouteRec>,
    pub java: JavaSide,
    pub t1_settled: u64,
    pub astar_runs: u32,
    pub candidates: u32,
    pub within_tolerance: u32,
    pub start_snap_m: f64,
    /// Réseau atteint à moins de D/2, et part qui mène aussi à l'arrivée, m (échecs compris).
    pub reached_m: f64,
    pub way_back_m: f64,
    pub t2_settled: u64,
    /// Refus du contrôle interne, par code, sur tous les candidats (échecs compris).
    pub rejected: BTreeMap<String, u32>,
    /// L / D des candidats hors tolérance (diagnostic des échecs `NO_CANDIDATE`).
    pub off_tolerance: Vec<f32>,
    /// Valeur du curseur du service (lot 3a) quand le mode en vient.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<f64>,
    /// Mode HTTP : statut, parcours retirés par le contrôleur du service, mode et λ appliqués selon le service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dropped: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied: Option<String>,
}

impl Rec {
    pub fn success(&self) -> bool {
        self.n_ok > 0
    }
}

pub fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::Strict => "strict",
        Mode::TeamFirst => "team",
        Mode::Discovery => "discovery",
    }
}

pub fn failure_code(f: &Failure) -> String {
    match f {
        Failure::StartTooFar => "START_TOO_FAR".into(),
        Failure::EndTooFar => "END_TOO_FAR".into(),
        Failure::DistanceTooShort => "DISTANCE_TOO_SHORT".into(),
        Failure::NetworkTooSmall => "NETWORK_TOO_SMALL".into(),
        Failure::NoTeamData => "NO_TEAM_DATA".into(),
        Failure::Stopped => "STOPPED".into(),
        Failure::WindowEscape => "WINDOW_ESCAPE".into(),
        Failure::TooLong => "TOO_LONG".into(),
        Failure::NoRoute(d) => format!(
            "NO_ROUTE:{}",
            match d {
                NoRouteDetail::EndUnreachable => "END_UNREACHABLE",
                NoRouteDetail::NoWayBack => "NO_WAY_BACK",
                NoRouteDetail::TailTooLong => "TAIL_TOO_LONG",
                NoRouteDetail::NoCandidate => "NO_CANDIDATE",
                NoRouteDetail::Budget => "BUDGET",
            }
        ),
    }
}

/// Les noms des violations du contrôle interne, lus sur leur forme `Debug` (le banc ne dépend d'aucun de ses
/// champs : seule l'API publique du moteur sert, et seulement pour compter les désaccords).
fn internal_violations<B: AsRef<[u8]>>(
    e: &Engine<B>,
    team: &TeamData,
    req: &Request,
    r: &Route,
) -> Vec<String> {
    let c = check_route(e, Some(team), req, r);
    let s = format!("{:?}", c.violations);
    s.trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect()
}

pub struct Outcome {
    pub result: Result<Generation, Failure>,
    pub diag: Diagnostics,
    pub panicked: Option<String>,
    pub cold_ms: f64,
    pub ms: f64,
}

fn one_call<B: AsRef<[u8]>>(
    e: &Engine<B>,
    team: &TeamData,
    req: &Request,
    ws: &mut Workspace,
) -> (Result<Generation, Failure>, Diagnostics, Option<String>, f64) {
    let t = Instant::now();
    let r =
        catch_unwind(AssertUnwindSafe(|| generate_with_diagnostics(e, Some(team), req, ws, &mut || false)));
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    match r {
        Ok((res, diag)) => (res, diag, None, ms),
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panique".into());
            (Err(Failure::Stopped), Diagnostics::default(), Some(msg), ms)
        }
    }
}

/// Un tour froid (son résultat est celui du rapport), puis `rounds` tours chauds dont on garde la médiane.
pub fn run_request<B: AsRef<[u8]>>(
    e: &Engine<B>,
    team: &TeamData,
    req: &Request,
    ws: &mut Workspace,
    rounds: usize,
) -> Outcome {
    let (result, diag, panicked, cold_ms) = one_call(e, team, req, ws);
    let mut warm: Vec<f64> = Vec::new();
    if panicked.is_none() {
        for _ in 0..rounds {
            warm.push(one_call(e, team, req, ws).3);
        }
    }
    warm.sort_by(f64::total_cmp);
    let ms = if warm.is_empty() { cold_ms } else { warm[warm.len() / 2] };
    Outcome { result, diag, panicked, cold_ms, ms }
}

pub struct Context<'a, B> {
    pub engine: &'a Engine<B>,
    pub team: &'a TeamData,
    pub checker: &'a Checker<'a, B>,
}

impl<B: AsRef<[u8]>> Context<'_, B> {
    pub fn graph(&self) -> &GraphFile<B> {
        self.engine.base()
    }

    /// Contrôle chaque parcours rendu et remplit ce qui concerne le moteur dans `rec`.
    pub fn fill(&self, rec: &mut Rec, req: &Request, out: &Outcome) {
        rec.ms = out.ms;
        rec.cold_ms = out.cold_ms;
        if let Some(p) = &out.panicked {
            rec.outcome = "PANIC".into();
            rec.violations.insert(format!("PANIC: {p}"), 1);
            return;
        }
        let dg = &out.diag;
        rec.t1_settled = dg.t1_settled;
        rec.t2_settled = dg.t2_settled;
        rec.astar_runs = dg.astar_runs;
        rec.candidates = dg.candidates;
        rec.within_tolerance = dg.within_tolerance;
        rec.start_snap_m = dg.start_snap_m;
        rec.reached_m = dg.reached_m;
        rec.way_back_m = dg.way_back_m;
        rec.off_tolerance = dg.off_tolerance.clone();
        for (c, n) in &dg.rejected {
            rec.rejected.insert(format!("{c:?}"), *n);
        }
        match &out.result {
            Err(f) => rec.outcome = failure_code(f),
            Ok(g) => {
                rec.n_routes = g.routes.len();
                for r in &g.routes {
                    let rep = self.checker.check(req, r);
                    let internal = internal_violations(self.engine, self.team, req, r);
                    let codes: Vec<String> = rep.violations.iter().map(|v| v.code().to_string()).collect();
                    if codes.is_empty() {
                        rec.n_ok += 1;
                    } else if internal.is_empty() {
                        rec.disagreements += 1;
                    }
                    for c in &codes {
                        *rec.violations.entry(c.clone()).or_default() += 1;
                    }
                    rec.routes.push(RouteRec {
                        length_km: r.length_m / 1000.0,
                        over_target: rep.length_m / req.distance_m,
                        team_share: if rep.length_m > 0.0 { rep.team_m / rep.length_m } else { 0.0 },
                        bridged_m: rep.bridged_m,
                        shared_m: r.shared_m,
                        crossings: r.crossings,
                        engine_team_m: r.team_m,
                        family: format!("{:?}", r.family),
                        violations: codes,
                        internal,
                        checker_shared_m: rep.shared_m,
                        checker_crossings: rep.crossings,
                        checker_team_m: rep.team_m,
                        checker_bridged_m: rep.bridged_m,
                        start_zone_m: rep.start_zone_m,
                        end_zone_m: rep.end_zone_m,
                        corridor_share: rep.corridor_share,
                        geom_xxh3: Some(crate::http::geom_xxh3(crate::http::rounded(&route_geometry(
                            self.engine.base(),
                            r,
                        )))),
                    });
                }
                rec.outcome = if g.routes.is_empty() {
                    "NO_ROUTES".into()
                } else if rec.n_ok > 0 {
                    "OK".into()
                } else {
                    "VIOLATION".into()
                };
            }
        }
    }
}
