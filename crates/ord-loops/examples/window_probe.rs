//! Taille et durée à froid des fenêtres autour de quatre centres-villes (lot 6, `ord-loops-lot6-mesures.md`) : pour
//! chaque centre, chaque distance et chaque vue, la classe de fenêtre d'une boucle (ou d'un aller simple de 300 km dont
//! le départ et l'arrivée sont proches), sa construction, une génération en découverte, les octets tenus.
//!
//! ```text
//! cargo run --release -p ord-loops --example window_probe -- <graphe.ordg> <road.ordv> <gravel.ordv>
//! ```
//!
//! Aucune donnée privée : quatre centres-villes publics. Pour des pages évincées, effacer le cache de pages des trois
//! fichiers avant (`posix_fadvise(DONTNEED)`).

use std::path::Path;
use std::time::Instant;

use ord_graph::format::GraphFile;
use ord_loops::{
    generate_in, Engine, LatLon, Mode, Params, Request, RideType, WindowTeam, Workspace,
    DEFAULT_OFF_TEAM_COST_Q8,
};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage : window_probe <graphe.ordg> <road.ordv> <gravel.ordv>");
        std::process::exit(2);
    }
    let open = |p: &str| GraphFile::open_mmap(Path::new(p)).expect("ouverture");
    let t = Instant::now();
    let engine = Engine::new(open(&a[1]), open(&a[2]), open(&a[3]), Params::default()).expect("moteur");
    println!("moteur chargé en {:.2} s", t.elapsed().as_secs_f64());
    println!("centre | type | D km | aller simple | tuiles | chaînes | construction ms | Mo fenêtre | génération ms | Mo espace de travail");
    let cities = [
        ("Paris", 48.8566, 2.3522),
        ("Lyon", 45.7640, 4.8357),
        ("Nantes", 47.2184, -1.5536),
        ("Clermont-Ferrand", 45.7772, 3.0870),
    ];
    for (name, lat, lon) in cities {
        for (d_km, one_way) in [(50.0, false), (100.0, false), (200.0, false), (300.0, true)] {
            for ride in [RideType::Road, RideType::Gravel] {
                let req = Request {
                    start: LatLon::new(lat, lon),
                    end: one_way.then(|| LatLon::new(lat + 0.01, lon + 0.01)),
                    bearing: None,
                    distance_m: d_km * 1000.0,
                    ride_type: ride,
                    mode: Mode::Discovery,
                    familiarity: 0.5,
                    off_team_cost_q8: DEFAULT_OFF_TEAM_COST_Q8,
                    seed: 1,
                    figures: ord_loops::Figures::Classic,
                };
                let plan = engine.plan(&req, 10_000.0);
                let t = Instant::now();
                let win = engine.build_window(plan.view, &plan.class, &mut || false).expect("construite");
                let build_ms = t.elapsed().as_secs_f64() * 1000.0;
                let mut ws = Workspace::new();
                let t = Instant::now();
                let res =
                    generate_in(&engine, &win, None::<ord_loops::TeamCtx<'_>>, &req, &mut ws, &mut || false);
                let gen_ms = t.elapsed().as_secs_f64() * 1000.0;
                let _ = WindowTeam::default();
                println!(
                    "{name} | {ride:?} | {d_km} | {one_way} | {} | {} | {build_ms:.0} | {:.0} | {gen_ms:.0} ({}) | {:.0}",
                    plan.class.len(),
                    win.n_chains(),
                    win.bytes() as f64 / 1e6,
                    match &res {
                        Ok(g) => format!("{} parcours", g.routes.len()),
                        Err(f) => format!("{f:?}"),
                    },
                    ws.bytes() as f64 / 1e6
                );
            }
        }
    }
}
