//! Génération de bout en bout sur la vraie région.
//!
//! ```text
//! cargo run --release -p ord-loops --example region -- <graphe.ordg> <road.ordv> <gravel.ordv> \
//!     [lat lon km road|gravel|mtb [discovery|team|strict surcouche.ordo]]
//! ```
//!
//! Sans coordonnées : un point au milieu de la région (centre d'Angers), 50 km à vélo de route, mode découverte
//! (aucune donnée d'équipe nécessaire). Avec une surcouche et `lat` = `auto`, le départ est le milieu d'une arête
//! du réseau de l'équipe (aucune coordonnée n'est écrite nulle part).

use std::path::Path;
use std::time::Instant;

use ord_graph::format::GraphFile;
use ord_graph::overlay::Overlay;
use ord_loops::{
    check_route, generate, Engine, LatLon, Mode, Params, Request, RideType, TeamData, Workspace,
};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage : region <graphe.ordg> <road.ordv> <gravel.ordv> [lat lon km road|gravel|mtb [mode surcouche]]");
        std::process::exit(2);
    }
    let open = |p: &str| GraphFile::open_mmap(Path::new(p)).expect("ouverture");
    let t = Instant::now();
    let engine = Engine::new(open(&a[1]), open(&a[2]), open(&a[3]), Params::default()).expect("moteur");
    eprintln!("moteur chargé en {:.2} s", t.elapsed().as_secs_f64());
    let t = Instant::now();
    engine.warm();
    eprintln!("index d'accroche en {:.2} s", t.elapsed().as_secs_f64());
    let ride = match a.get(7).map(String::as_str) {
        Some("gravel") => RideType::Gravel,
        Some("mtb") => RideType::Mtb,
        _ => RideType::Road,
    };
    let mode = match a.get(8).map(String::as_str) {
        Some("strict") => Mode::Strict,
        Some("team") => Mode::TeamFirst,
        _ => Mode::Discovery,
    };
    let overlay = a.get(9).map(|p| Overlay::read(Path::new(p)).expect("surcouche"));
    let team = overlay.as_ref().map(|o| {
        let t = Instant::now();
        let td = TeamData::build(&engine, o).expect("équipe");
        eprintln!("équipe construite en {:.2} s : {:?}", t.elapsed().as_secs_f64(), td.stats(ride));
        td
    });
    let start = if a.get(4).map(String::as_str) == Some("auto") {
        let o = overlay.as_ref().expect("`auto` demande une surcouche");
        let (e, _) = o.iter().nth(o.len() / 2).expect("surcouche vide");
        let g = engine.base();
        let (la, lo) = g.node_pos(g.edge_ends(e).0);
        LatLon::from_e7((la, lo))
    } else {
        let num = |i: usize, d: f64| a.get(i).map_or(d, |s| s.parse().expect("nombre"));
        LatLon::new(num(4, 47.4784), num(5, -0.5632))
    };
    let km: f64 = a.get(6).map_or(50.0, |s| s.parse().expect("km"));
    let req = Request {
        start,
        end: None,
        bearing: None,
        distance_m: km * 1000.0,
        ride_type: ride,
        mode,
        familiarity: 0.5,
        seed: 1,
    };
    let mut ws = Workspace::new();
    for round in 0..3 {
        let t = Instant::now();
        let res = generate(&engine, team.as_ref(), &req, &mut ws, &mut || false);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        match res {
            Ok(g) => {
                println!("tour {round} : {} parcours en {ms:.0} ms ({:?})", g.routes.len(), g.diagnostics);
                for r in &g.routes {
                    let c = check_route(&engine, team.as_ref(), &req, r);
                    println!(
                        "  {:.1} km (D {:.1}), équipe {:.0} %, comblé {:.0} m, partagé {:.0} m, croisements {}, violations {:?}",
                        r.length_m / 1000.0,
                        req.distance_m / 1000.0,
                        100.0 * r.team_m / r.length_m,
                        r.bridged_m,
                        r.shared_m,
                        r.crossings,
                        c.violations
                    );
                }
            }
            Err(f) => println!("tour {round} : échec {f:?} en {ms:.0} ms"),
        }
    }
}
