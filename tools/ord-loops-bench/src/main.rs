//! `ord-loops-bench` : rejoue les requêtes du banc Java sur les équipes de la région, dans les trois modes du
//! moteur `ord-loops`, contrôle chaque parcours rendu avec le contrôleur indépendant `ord-loops-check`, et compare,
//! requête par requête, à l'issue du générateur Java. Note : `docs/design/technique/ord-loops-lot2.md` §8.
//!
//! **Privé** : les requêtes, les départs et les surcouches sont des données d'équipes réelles. Ils sont lus sous
//! `~/.cache/osm-route-details/graph/` et `~/code/perso/tribly-generator-bench/` ; les sorties (rapport JSON,
//! résumé, GeoJSON) sont écrites sous `~/.cache/osm-route-details/graph/lot2/` et nulle part ailleurs. Les équipes
//! y sont lettrées ; aucun départ, id de trace ou id d'équipe ne sort dans le résumé.

mod geojson;
mod input;
mod record;
mod report;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context as _, Result};
use clap::Parser;
use ord_graph::format::GraphFile;
use ord_graph::overlay::Overlay;
use ord_graph::util::Poly;
use ord_loops::{Engine, LatLon, Mode, Params, Request, TeamData, Workspace};
use ord_loops_check::{Checker, Config};
use serde::Serialize;

use crate::input::{bearing_name, read_java_requests, read_letters, read_starts, ride_name, JavaRequest};
use crate::record::{mode_name, Context, JavaSide, Rec};
use crate::report::{Autocheck, Meta, SummaryInput};

#[derive(Debug, Parser)]
#[command(
    about = "Banc du moteur ord-loops : rejeu des requêtes Java, contrôle indépendant, rapport (lot 2)"
)]
struct Args {
    /// Graphe de base (défaut : le graphe de la région sous `~/.cache/osm-route-details/graph/`).
    #[arg(long)]
    graph: Option<PathBuf>,
    /// Vues `road` et `gravel` (défaut : `lot2/views/`).
    #[arg(long)]
    road: Option<PathBuf>,
    #[arg(long)]
    gravel: Option<PathBuf>,
    /// Dossier des surcouches `<id d'équipe>-….ordo` (défaut : `overlays/`).
    #[arg(long)]
    overlays: Option<PathBuf>,
    /// Départs reconstruits de l'export (défaut : `lot2/bench-starts.json`).
    #[arg(long)]
    starts: Option<PathBuf>,
    /// `{ id d'équipe : lettre }` (défaut : `lot2/team-letters.json`).
    #[arg(long)]
    letters: Option<PathBuf>,
    /// Résultats du banc Java `head` et `head-alone`.
    #[arg(long, default_value = "~/code/perso/tribly-generator-bench/results/bench-1115-head")]
    java: String,
    #[arg(long, default_value = "~/code/perso/tribly-generator-bench/results/bench-1115-head-alone")]
    java_alone: String,
    /// `.poly` de la région (défaut : celui du graphe).
    #[arg(long)]
    poly: Option<PathBuf>,
    /// Sortie (défaut : `lot2/`).
    #[arg(long)]
    out: Option<PathBuf>,
    /// Préfixe des fichiers de sortie.
    #[arg(long, default_value = "bench")]
    label: String,
    /// Équipes jouées (lettres, séparées par des virgules) ; toutes par défaut.
    #[arg(long, value_delimiter = ',')]
    teams: Vec<String>,
    #[arg(long, value_delimiter = ',', default_value = "strict,team,discovery")]
    modes: Vec<String>,
    /// Tours chauds par requête, en plus du tour froid ; la médiane des tours chauds est le temps rapporté.
    #[arg(long, default_value_t = 3)]
    rounds: usize,
    /// Au plus ce nombre de requêtes par équipe (les premières du périmètre).
    #[arg(long)]
    limit: Option<usize>,
    /// Seulement les requêtes de au plus ce nombre de km.
    #[arg(long)]
    max_km: Option<f64>,
    /// GeoJSON de requêtes précises : `<lettre>:<i>`, répétable.
    #[arg(long = "geojson")]
    geojson: Vec<String>,
    /// GeoJSON de toute requête ayant au moins une violation.
    #[arg(long)]
    geojson_violations: bool,
    /// GeoJSON de toute requête réussie par Java et pas par Rust (même mode).
    #[arg(long)]
    geojson_lost: bool,
    /// L'exemption « épingle d'une même way » vaut aussi pour un nœud seul (voir `ord_loops_check::Config`).
    #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
    hairpin_single_node: bool,
    /// Trou comblé en strict, m (défaut du moteur : 100) ; sert à classer les requêtes perdues.
    #[arg(long)]
    hole_m: Option<f64>,
    /// Jonction courte comblée en strict, m (défaut du moteur : 30).
    #[arg(long)]
    connector_m: Option<f64>,
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| anyhow!("HOME n'est pas défini"))
}

fn expand(p: &str) -> Result<PathBuf> {
    Ok(match p.strip_prefix("~/") {
        Some(rest) => home()?.join(rest),
        None => PathBuf::from(p),
    })
}

fn parse_mode(s: &str) -> Result<Mode> {
    match s {
        "strict" => Ok(Mode::Strict),
        "team" => Ok(Mode::TeamFirst),
        "discovery" => Ok(Mode::Discovery),
        other => Err(anyhow!("mode inconnu : {other} (strict, team, discovery)")),
    }
}

fn read_first_line(path: &str, key: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .find(|l| l.starts_with(key))?
        .split_once(':')
        .map(|(_, v)| v.trim().to_string())
}

fn git(args: &[&str]) -> Option<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::process::Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Date ISO (UTC) d'un nombre de secondes depuis 1970.
fn iso(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // jours depuis 1970-01-01 -> date civile (algorithme de H. Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn load_avg() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .map(|s| s.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

#[derive(Serialize)]
struct Report<'a> {
    meta: &'a [(String, String)],
    records: &'a [Rec],
}

struct GeoWant {
    specs: BTreeSet<(String, u32)>,
    violations: bool,
    lost: bool,
}

impl GeoWant {
    fn wants(&self, rec: &Rec) -> bool {
        self.specs.contains(&(rec.team.clone(), rec.i))
            || (self.violations && !rec.violations.is_empty())
            || (self.lost && rec.java.ok && !rec.success())
    }
}

fn main() -> Result<()> {
    let a = Args::parse();
    let cache = home()?.join(".cache/osm-route-details/graph");
    let out = a.out.clone().unwrap_or_else(|| cache.join("lot2"));
    let graph_path = a.graph.clone().unwrap_or_else(|| cache.join("pays-de-la-loire-20261114-z11.ordg"));
    let road_path =
        a.road.clone().unwrap_or_else(|| out.join("views/pays-de-la-loire-20261114-z11.road.ordv"));
    let gravel_path =
        a.gravel.clone().unwrap_or_else(|| out.join("views/pays-de-la-loire-20261114-z11.gravel.ordv"));
    let overlays = a.overlays.clone().unwrap_or_else(|| cache.join("overlays"));
    let starts = read_starts(&a.starts.clone().unwrap_or_else(|| out.join("bench-starts.json")))?;
    let letters = read_letters(&a.letters.clone().unwrap_or_else(|| out.join("team-letters.json")))?;
    let poly = Poly::read(&a.poly.clone().unwrap_or_else(|| cache.join("pays-de-la-loire.poly")))?;
    let java_dir = expand(&a.java)?;
    let alone_dir = expand(&a.java_alone)?;
    let modes: Vec<Mode> = a.modes.iter().map(|m| parse_mode(m)).collect::<Result<_>>()?;
    let geo = GeoWant {
        specs: a
            .geojson
            .iter()
            .map(|s| {
                let (t, i) =
                    s.split_once(':').ok_or_else(|| anyhow!("--geojson attend <lettre>:<i>, pas `{s}`"))?;
                Ok((t.to_string(), i.parse::<u32>()?))
            })
            .collect::<Result<_>>()?,
        violations: a.geojson_violations,
        lost: a.geojson_lost,
    };
    let geo_dir = out.join("geojson");

    let load0 = load_avg();
    let t0 = std::time::Instant::now();
    let open = |p: &Path| GraphFile::open_mmap(p).with_context(|| p.display().to_string());
    let mut params = Params::default();
    if let Some(h) = a.hole_m {
        params.hole_m = h;
    }
    if let Some(h) = a.connector_m {
        params.connector_m = h;
    }
    let engine = Engine::new(open(&graph_path)?, open(&road_path)?, open(&gravel_path)?, params)
        .map_err(|e| anyhow!("moteur : {e}"))?;
    engine.warm();
    eprintln!("moteur chargé en {:.1} s", t0.elapsed().as_secs_f64());

    // équipes : un `requests-<id>.jsonl` du banc Java par équipe
    let mut team_ids: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir(&java_dir).with_context(|| java_dir.display().to_string())? {
        let name = entry?.file_name().to_string_lossy().to_string();
        if let Some(id) = name.strip_prefix("requests-").and_then(|n| n.strip_suffix(".jsonl")) {
            let letter = letters
                .get(id)
                .cloned()
                .ok_or_else(|| anyhow!("équipe sans lettre dans le fichier des lettres"))?;
            team_ids.push((letter, id.to_string()));
        }
    }
    team_ids.sort();

    let mut recs: Vec<Rec> = Vec::new();
    let mut java_total: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut autocheck: Vec<Autocheck> = Vec::new();
    for (letter, id) in &team_ids {
        if !a.teams.is_empty() && !a.teams.contains(letter) {
            continue;
        }
        let Some(pts) = starts.get(id) else { continue };
        let reqs = read_java_requests(&java_dir.join(format!("requests-{id}.jsonl")))?;
        let alone: BTreeMap<u32, String> =
            read_java_requests(&alone_dir.join(format!("requests-{id}.jsonl")))
                .map(|v| v.into_iter().map(|r| (r.i, r.outcome)).collect())
                .unwrap_or_default();
        // autocontrôle de la reconstruction des départs : `km` d'un aller simple = ⌈1,4 × vol d'oiseau⌉
        let (mut ok1, mut bad1) = (0, 0);
        for r in reqs.iter().filter(|r| r.kind == "oneway") {
            let exp = input::oneway_end(pts, r.start)
                .map(|j| (input::crow_m(pts[r.start], pts[j]) * 1.4 / 1000.0).ceil());
            if exp == Some(r.km.round()) {
                ok1 += 1;
            } else {
                bad1 += 1;
            }
        }
        autocheck.push(Autocheck {
            team: letter.clone(),
            starts: pts.len(),
            oneway_ok: ok1,
            oneway_bad: bad1,
        });

        let inside: Vec<(&JavaRequest, (f64, f64))> = reqs
            .iter()
            .filter_map(|r| pts.get(r.start).map(|p| (r, *p)))
            .filter(|(_, p)| poly.contains(p.1, p.0))
            .collect();
        java_total.insert(letter.clone(), (reqs.len(), inside.len()));
        let overlay_path = std::fs::read_dir(&overlays)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&format!("{id}-")) && n.ends_with(".ordo"))
            })
            .ok_or_else(|| anyhow!("pas de surcouche pour l'équipe {letter}"))?;
        let overlay =
            Overlay::read(&overlay_path).map_err(|e| anyhow!("surcouche de l'équipe {letter} : {e}"))?;
        let t = std::time::Instant::now();
        let team = TeamData::build(&engine, &overlay).map_err(|e| anyhow!("équipe {letter} : {e}"))?;
        for r in [ord_loops::RideType::Road, ord_loops::RideType::Gravel] {
            let s = team.stats(r);
            eprintln!(
                "équipe {letter} {r:?} : équipe {:.0} km, comblé {:.1} km, hors vue {:.1} km (du type) / {:.1} km (tous types)",
                s.team_km, s.bridged_km, s.outside_typed_km, s.outside_any_km
            );
        }
        let checker = Checker::new(
            engine.base(),
            Some(&overlay),
            Config {
                hairpin_single_node: a.hairpin_single_node,
                hole_m: a.hole_m.unwrap_or(100.0),
                connector_m: a.connector_m.unwrap_or(30.0),
                ..Config::default()
            },
        );
        let ctx = Context { engine: &engine, team: &team, checker: &checker };
        eprintln!(
            "équipe {letter} : {} requêtes dans le périmètre, données en {:.2} s",
            inside.len(),
            t.elapsed().as_secs_f64()
        );

        let mut ws = Workspace::new();
        let mut played = 0usize;
        for (jr, p) in inside {
            if a.max_km.is_some_and(|m| jr.km > m) {
                continue;
            }
            if a.limit.is_some_and(|l| played >= l) {
                break;
            }
            let end = if jr.kind == "oneway" {
                match input::oneway_end(pts, jr.start) {
                    Some(j) => Some(LatLon::new(pts[j].0, pts[j].1)),
                    None => continue,
                }
            } else {
                None
            };
            played += 1;
            let scope = if poly.contains_disc(p.0, p.1, jr.km * 500.0) { "interior" } else { "edge" };
            for &mode in &modes {
                let req = Request {
                    start: LatLon::new(p.0, p.1),
                    end,
                    bearing: jr.bearing,
                    distance_m: jr.km * 1000.0,
                    ride_type: jr.ride,
                    mode,
                    familiarity: jr.familiarity,
                    seed: u64::from(jr.i),
                };
                let outcome = record::run_request(&engine, &team, &req, &mut ws, a.rounds);
                let mut rec = Rec {
                    team: letter.clone(),
                    i: jr.i,
                    kind: jr.kind.clone(),
                    start: jr.start,
                    km: jr.km,
                    bearing: bearing_name(jr.bearing),
                    ride: ride_name(jr.ride).into(),
                    familiarity: jr.familiarity,
                    scope: scope.into(),
                    mode: mode_name(mode).into(),
                    outcome: String::new(),
                    n_routes: 0,
                    n_ok: 0,
                    violations: BTreeMap::new(),
                    disagreements: 0,
                    ms: 0.0,
                    cold_ms: 0.0,
                    routes: Vec::new(),
                    java: JavaSide {
                        outcome: jr.outcome.clone(),
                        ok: jr.ok(),
                        ms: jr.ms,
                        shared_m: jr.routes.iter().map(|r| r.shared_m).reduce(f64::max),
                        crossings: jr.routes.iter().map(|r| r.crossings).max(),
                        km: jr.routes.first().map(|r| r.km),
                        alone_outcome: alone.get(&jr.i).cloned(),
                    },
                    t1_settled: 0,
                    astar_runs: 0,
                    candidates: 0,
                    within_tolerance: 0,
                    start_snap_m: 0.0,
                    reached_m: 0.0,
                    way_back_m: 0.0,
                    t2_settled: 0,
                    rejected: Default::default(),
                    off_tolerance: Vec::new(),
                };
                ctx.fill(&mut rec, &req, &outcome);
                if geo.wants(&rec) {
                    geojson::write(
                        &geo_dir,
                        ctx.graph(),
                        &overlay,
                        &req,
                        &rec,
                        outcome.result.as_ref().ok(),
                    )?;
                }
                recs.push(rec);
            }
        }
    }

    let load1 = load_avg();
    let epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let h = engine.base().header();
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    let mut lines: Vec<(String, String)> = vec![
        ("date".into(), iso(epoch)),
        (
            "commit".into(),
            format!(
                "{}{}",
                git(&["rev-parse", "--short", "HEAD"]).unwrap_or_default(),
                if dirty { " (arbre modifié)" } else { "" }
            ),
        ),
        ("machine".into(), read_first_line("/proc/cpuinfo", "model name").unwrap_or_default()),
        (
            "fils disponibles".into(),
            std::thread::available_parallelism().map_or_else(|_| "?".into(), |n| n.get().to_string()),
        ),
        ("charge (1, 5, 15 min) au début / à la fin".into(), format!("{load0} / {load1}")),
        ("profil".into(), if cfg!(debug_assertions) { "debug".into() } else { "release".into() }),
        (
            "graphe".into(),
            format!(
                "{} nœuds, {} arêtes, outil {}, réplication {}",
                h.nodes, h.edges, h.tool_version, h.replication_timestamp
            ),
        ),
        ("modes".into(), a.modes.join(", ")),
        (
            "équipes jouées".into(),
            team_ids
                .iter()
                .filter(|(l, _)| a.teams.is_empty() || a.teams.contains(l))
                .map(|(l, _)| l.clone())
                .collect::<Vec<_>>()
                .join(", "),
        ),
        ("requêtes × modes enregistrées".into(), recs.len().to_string()),
        ("durée du banc".into(), format!("{:.0} s", t0.elapsed().as_secs_f64())),
    ];
    if let Some(l) = a.limit {
        lines.push(("limite".into(), format!("{l} requêtes par équipe (banc partiel)")));
    }
    if let Some(k) = a.max_km {
        lines.push(("limite".into(), format!("D ≤ {k} km (banc partiel)")));
    }
    std::fs::create_dir_all(&out)?;
    let json_path = out.join(format!("{}-report.json", a.label));
    std::fs::write(&json_path, serde_json::to_vec_pretty(&Report { meta: &lines, records: &recs })?)?;
    let md = report::summary(&SummaryInput {
        meta: &Meta { lines: lines.clone() },
        recs: &recs,
        java_total: &java_total,
        autocheck: &autocheck,
        modes: &a.modes,
        rounds: a.rounds,
    });
    let md_path = out.join(format!("{}-summary.md", a.label));
    std::fs::write(&md_path, md)?;
    eprintln!("rapport : {}\nrésumé : {}", json_path.display(), md_path.display());
    Ok(())
}
