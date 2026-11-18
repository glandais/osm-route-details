//! Sonde du coût d'une recherche point à point (lot 4, 4c ; relevé `ord-loops-lot4c-sonde.md`) : branchée sur les jeux
//! de la surcouche complète quand `ORD_P2P_PROBE` nomme un fichier. Une ligne JSON par jeu de D dans
//! `ORD_P2P_KM` (défaut `30-150`) : type, curseur, D, temps et issue du jeu du moteur, et ce que rend
//! `ord_loops::probe::probe_triangle_in` (agrégats et compteurs seulement, ni position ni identifiant).
//! Seule la variante `ORD_P2P_VARIANT` (défaut `svc`, le chemin du service 1.10) est sondée.
//! `ORD_P2P_SUB_BANDS` (défaut 1) et `ORD_P2P_BAND` (défaut `0.27-0.39`) règlent le tirage des points lointains.

use std::io::Write as _;
use std::sync::{Mutex, OnceLock};

use ord_loops::probe::{probe_triangle_in, LegProbe, ProbeParams};
use ord_loops::{Request, TeamCtx, TeamData, Window, WindowTeam, Workspace};
use serde_json::json;

use super::play::Eng;

/// La variante en cours de jeu (posée par `run`).
pub static CURRENT: Mutex<String> = Mutex::new(String::new());

struct Sink {
    variant: String,
    file: Mutex<std::io::BufWriter<std::fs::File>>,
    km: (f64, f64),
    params: ProbeParams,
}

fn pair_of(s: &str) -> Option<(f64, f64)> {
    let (a, b) = s.split_once('-')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn sink() -> Option<&'static Sink> {
    static S: OnceLock<Option<Sink>> = OnceLock::new();
    S.get_or_init(|| {
        let path = std::env::var_os("ORD_P2P_PROBE")?;
        let file = std::fs::File::create(&path).expect("fichier de la sonde point à point");
        let km = std::env::var("ORD_P2P_KM").ok().and_then(|s| pair_of(&s)).unwrap_or((30.0, 150.0));
        let mut params = ProbeParams::default();
        if let Some(n) = std::env::var("ORD_P2P_SUB_BANDS").ok().and_then(|s| s.parse().ok()) {
            params.sub_bands = n;
        }
        if let Some(b) = std::env::var("ORD_P2P_BAND").ok().and_then(|s| pair_of(&s)) {
            params.band = b;
        }
        eprintln!("sonde point à point : D dans {km:?} km, {params:?}");
        let variant = std::env::var("ORD_P2P_VARIANT").unwrap_or_else(|_| "svc".into());
        Some(Sink { variant, file: Mutex::new(std::io::BufWriter::new(file)), km, params })
    })
    .as_ref()
}

/// La sonde est branchée et la requête est dans la plage de D.
pub fn wanted(req: &Request, overlay: &str) -> bool {
    overlay == "full"
        && req.end.is_none()
        && sink().is_some_and(|s| {
            (s.km.0..=s.km.1).contains(&(req.distance_m / 1000.0))
                && *CURRENT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) == s.variant
        })
}

fn leg(l: &LegProbe) -> serde_json::Value {
    json!({"us": l.us, "settled": l.settled, "found": l.found, "exhausted": l.exhausted, "m": l.meters_m, "cost": l.cost})
}

/// Joue la sonde sur la fenêtre du jeu et écrit sa ligne.
#[allow(clippy::too_many_arguments)]
pub fn probe(
    engine: &Eng,
    win: &Window,
    team: &TeamData,
    wt: &WindowTeam,
    req: &Request,
    ws: &mut Workspace,
    team_key: u64,
    cursor: f64,
    gen_ms: f64,
    outcome: &str,
    diag: Option<&ord_loops::Diagnostics>,
) {
    let Some(s) = sink() else { return };
    let tc = TeamCtx { data: team, win: wt };
    let t0 = std::time::Instant::now();
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        probe_triangle_in(engine, win, Some(tc), req, ws, &s.params)
    }));
    let probe_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let mut line = json!({
        "team": team_key / 2,
        "ride": format!("{:?}", req.ride_type),
        "cursor": cursor,
        "d_km": req.distance_m / 1000.0,
        "gen_ms": gen_ms,
        "outcome": outcome,
        "probe_ms": probe_ms,
        "diag": diag.map(|g| json!({
            "t1": g.t1_settled, "t2": g.t2_settled, "astar": g.astar_settled, "runs": g.astar_runs,
            "cands": g.candidates, "within": g.within_tolerance,
        })),
    });
    match res {
        Err(_) => line["probe"] = json!("PANIC"),
        Ok(Err(f)) => line["probe"] = json!(format!("{f:?}")),
        Ok(Ok(p)) => {
            line["probe"] = json!({
                "t1": p.t1_settled, "t2": p.t2_settled, "trees_us": p.trees_us, "minf_us": p.min_factor_us,
                "minf": p.min_factor, "medf": p.median_factor, "cands": p.candidates, "pairs": p.pairs,
                "in_tol": p.pairs_in_tol, "pairs_lit": p.pairs_lit, "in_tol_lit": p.pairs_in_tol_lit,
                "est_over_d": p.est_over_d,
                "probed": p.probed.iter().map(|x| json!({
                    "lit": x.literal, "sep": x.sep_deg, "phi": x.phi, "est": x.est_m, "leg1": x.leg1_m, "crow_m": x.crow_m,
                    "leg3": x.leg3_m, "reuse3": x.leg3_reuse_m, "h0c": x.h0_crow, "h0t": x.h0_tri,
                    "tri": leg(&x.tri), "crow": leg(&x.crow), "leg3a": x.leg3_astar.as_ref().map(leg),
                })).collect::<Vec<_>>(),
            });
        }
    }
    let mut f = s.file.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = writeln!(f, "{line}");
    let _ = f.flush();
}
