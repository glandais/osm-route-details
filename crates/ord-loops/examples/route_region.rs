//! Calcul d'itinéraire de bout en bout sur la vraie région.
//!
//! ```text
//! cargo run --release -p ord-loops --example route_region -- <graphe.ordg> <road.ordv> <gravel.ordv> \
//!     <lat> <lon> <lat> <lon> [road|gravel|mtb [discovery|team surcouche.ordo]]
//! ```
//!
//! Écrit, par zone essayée : le temps de la fenêtre, celui de la recherche (à froid puis deux tours chauds), la
//! longueur, le détour sur le vol d'oiseau, les états fixés, les parts d'équipe et de non-revêtu. Aucune coordonnée
//! n'est écrite.

use std::path::Path;
use std::time::Instant;

use ord_graph::format::GraphFile;
use ord_graph::overlay::Overlay;
use ord_loops::{
    route_in, Engine, LatLon, Mode, Params, RideType, RouteFailure, RouteRequest, RouteTune, TeamCtx,
    TeamData, WindowTeam, Workspace,
};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 8 {
        eprintln!("usage : route_region <graphe.ordg> <road.ordv> <gravel.ordv> <lat> <lon> <lat> <lon> [type [mode surcouche]]");
        std::process::exit(2);
    }
    let open = |p: &str| GraphFile::open_mmap(Path::new(p)).expect("ouverture");
    let engine = Engine::new(open(&a[1]), open(&a[2]), open(&a[3]), Params::default()).expect("moteur");
    let num = |i: usize| -> f64 { a[i].parse().expect("nombre") };
    let ride = match a.get(8).map(String::as_str) {
        Some("gravel") => RideType::Gravel,
        Some("mtb") => RideType::Mtb,
        _ => RideType::Road,
    };
    let mode = match a.get(9).map(String::as_str) {
        Some("team") => Mode::TeamFirst,
        _ => Mode::Discovery,
    };
    // `ORD_USE_ROADS=<réglage>,<neutre du profil>` (0 : pistes, 1 : routes) : la préférence de voie ; absent : aucune.
    let way = std::env::var("ORD_USE_ROADS").ok().map_or(ord_loops::WAY_NEUTRAL, |s| {
        let (u, d) = s.split_once(',').expect("réglage,neutre");
        ord_loops::way_factors(ride, u.parse().expect("réglage"), d.parse().expect("neutre"))
    });
    // `ORD_AVOID_MAIN=1` : éviter les grandes routes, par-dessus la préférence.
    let way = if std::env::var("ORD_AVOID_MAIN").is_ok() { ord_loops::avoid_main_roads(way) } else { way };
    let overlay = a.get(10).map(|p| Overlay::read(Path::new(p)).expect("surcouche"));
    let team = overlay.as_ref().map(|o| TeamData::build(&engine, o).expect("équipe"));
    let r = RouteRequest {
        start: LatLon::new(num(4), num(5)),
        end: LatLon::new(num(6), num(7)),
        ride_type: ride,
        mode,
        familiarity: 0.75,
        off_team_cost_q8: ord_loops::DEFAULT_OFF_TEAM_COST_Q8,
        way_q8: way,
    };
    let tune = RouteTune::default();
    let crow = r.crow_m();
    let mut ws = Workspace::new();
    for step in 0..tune.reach.len() {
        let reach = r.reach_m(&tune, step).expect("pas");
        let t = Instant::now();
        let plan = engine.plan_route(&r, reach, 0.0, 0.0);
        let win = engine.build_window(plan.view, &plan.need, &mut || false).expect("fenêtre");
        let wt = team.as_ref().map(|t| WindowTeam::build(&engine, &win, t));
        let ctx = team.as_ref().zip(wt.as_ref()).map(|(data, win)| TeamCtx { data, win });
        let win_ms = t.elapsed().as_secs_f64() * 1000.0;
        let mut last = None;
        let mut times = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            last = Some(route_in(&engine, &win, ctx, &r, &tune, reach, &mut ws, &mut || false));
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        print!(
            "zone {step} ({:.0} km, {} chaînes) : fenêtre {win_ms:.0} ms, recherche {:.0} / {:.0} / {:.0} ms : ",
            reach / 1000.0,
            win.n_chains(),
            times[0],
            times[1],
            times[2]
        );
        match last.expect("trois tours") {
            Ok(o) => {
                let l = o.route.length_m;
                println!(
                    "{:.1} km (vol d'oiseau {:.1} km, × {:.2}), {} états, équipe {:.0} %, non revêtu {:.0} %, virage serré {}",
                    l / 1000.0,
                    crow / 1000.0,
                    l / crow.max(1.0),
                    o.settled,
                    100.0 * (o.route.team_m + o.route.bridged_m) / l.max(1.0),
                    100.0 * o.route.offroad_m / l.max(1.0),
                    o.sharp_relaxed
                );
                return;
            }
            Err(RouteFailure::NoRoute) => println!("aucun chemin dans la zone"),
            Err(f) => {
                println!("échec {f:?}");
                return;
            }
        }
    }
}
