//! `ord-loops-bench` : rejoue les requêtes du banc Java sur les équipes de la région, dans les trois modes du
//! moteur `ord-loops`, contrôle chaque parcours rendu avec le contrôleur indépendant `ord-loops-check`, et compare,
//! requête par requête, à l'issue du générateur Java. Note : `docs/design/technique/ord-loops-lot2.md` §8.
//!
//! **Privé** : les requêtes, les départs et les surcouches sont des données d'équipes réelles. Ils sont lus sous
//! `~/.cache/osm-route-details/graph/` et `~/code/perso/tribly-generator-bench/` ; les sorties (rapport JSON,
//! résumé, GeoJSON) sont écrites sous `~/.cache/osm-route-details/graph/lot2/` et nulle part ailleurs. Les équipes
//! y sont lettrées ; aucun départ, id de trace ou id d'équipe ne sort dans le résumé.

mod geojson;
mod http;
mod input;
mod record;
mod report;
mod teams;
mod windows;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context as _, Result};
use clap::Parser;
use ord_graph::format::GraphFile;
use ord_graph::overlay::Overlay;
use ord_graph::util::Poly;
use ord_loops::{Engine, LatLon, Mode, Params, Request, TeamData, Workspace};
use ord_loops_check::{Checker, Config, GraphIndex, TeamCheck};
use serde::Serialize;

use crate::input::{bearing_name, read_java_requests, read_letters, read_starts, ride_name, JavaRequest};
use crate::record::{mode_name, Context, Inputs, JavaSide, Rec};
use crate::report::{Autocheck, Meta, SummaryInput};
use crate::windows::{memory_mb, quantiles, AnonSampler, Kind, Windows};

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
    /// Valeurs du curseur du service (lot 3a), à la place de `--modes` : chacune donne le mode, la familiarité et λ
    /// que le service appliquerait (`ord_server::generator::cursor`), en processus comme en HTTP.
    #[arg(long, value_delimiter = ',')]
    cursors: Vec<f64>,
    /// Mode HTTP : adresse d'un service qui tourne (`http://127.0.0.1:28080`) ; les requêtes passent par
    /// `POST /v1/generator/routes` au lieu du moteur en processus. Exige `--cursors` et `--keys`.
    #[arg(long)]
    http: Option<String>,
    /// Fichier local (hors dépôt) `{ id d'équipe : { "key": …, "team": … } }` : la clé d'environnement et la
    /// référence d'équipe que le backend enverrait.
    #[arg(long)]
    keys: Option<PathBuf>,
    /// Moteur joué : `window` (le moteur fenêtré du lot 6, cache de fenêtres sous budget) ou `flat` (une fenêtre de
    /// toutes les tuiles par vue : le moteur à plat d'avant, la référence).
    #[arg(long, default_value = "window")]
    engine: String,
    /// Rejoue aussi chaque requête sur le moteur à plat et exige les mêmes résultats, exactement (étapes, mesures,
    /// diagnostics) ; la moindre différence fait échouer le banc.
    #[arg(long)]
    compare: bool,
    /// Budget de mémoire du cache de fenêtres, en Mo (moteur fenêtré).
    #[arg(long, default_value_t = 900)]
    budget_mb: usize,
    /// Pas d'arrondi du rayon de classe des fenêtres, m.
    #[arg(long, default_value_t = 10_000.0)]
    radius_step_m: f64,
    /// Compare les index globaux et la règle du degré admis à la force brute avant de jouer.
    #[arg(long)]
    self_check: bool,
    /// Écrit, par requête jouée, l'empreinte de la sortie complète du moteur (une ligne : lettre, i, mode, empreinte).
    /// Sert à comparer deux binaires, le moteur à plat d'avant le lot 6 et celui-ci.
    #[arg(long)]
    dump: Option<PathBuf>,
    /// Construit les données d'équipe et les tables du contrôleur de chaque surcouche du dossier (`--teams` filtre),
    /// écrit l'empreinte xxh3 de leur forme canonique (lettre, objet, empreinte, octets), puis s'arrête (finition 2
    /// du lot 6, §20.4).
    #[arg(long)]
    dump_teams: Option<PathBuf>,
    /// Avec `--dump-teams` : construit aussi chaque équipe à l'ancienne (construction de référence) et exige les
    /// mêmes octets.
    #[arg(long)]
    teams_reference: bool,
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
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

/// Ce qui est joué pour chaque requête : un mode du lot 2, ou une valeur du curseur du service (lot 3a).
#[derive(Clone, Copy, Debug)]
enum Play {
    Mode(Mode),
    Cursor(f64),
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
    let plays: Vec<Play> = if a.cursors.is_empty() {
        modes.iter().map(|&m| Play::Mode(m)).collect()
    } else {
        a.cursors.iter().map(|&c| Play::Cursor(c)).collect()
    };
    let client = match &a.http {
        Some(url) => {
            anyhow::ensure!(!a.cursors.is_empty(), "--http exige --cursors");
            Some(http::Client::new(url)?)
        }
        None => None,
    };
    let keys = match (&a.http, &a.keys) {
        (Some(_), Some(p)) => http::read_keys(p)?,
        (Some(_), None) => return Err(anyhow!("--http exige --keys")),
        (None, _) => BTreeMap::new(),
    };
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
    let load_s = t0.elapsed().as_secs_f64();
    let sampler = AnonSampler::start();
    let load_anon_mb = sampler.take_max_mb();
    let mut team_build_anon: Vec<(String, f64, f64)> = Vec::new();
    // Par équipe : octets tenus une fois construite (surcouche, données du moteur, tables du contrôleur), en Mo.
    let mut resident_mb: Vec<(String, f64, f64, f64)> = Vec::new();
    let mut plan_estimates: Vec<(String, f64)> = Vec::new();
    let graph_index = GraphIndex::build(engine.base());
    let tm = engine.index_timings_ms();
    eprintln!(
        "moteur chargé en {load_s:.1} s (index : road {} + {} ms, gravel {} + {} ms ; {:.1} Mo)",
        tm[0].0,
        tm[0].1,
        tm[1].0,
        tm[1].1,
        engine.index_bytes() as f64 / 1e6
    );
    if let Some(path) = &a.dump_teams {
        let cfg = Config {
            hole_m: a.hole_m.unwrap_or(100.0),
            connector_m: a.connector_m.unwrap_or(30.0),
            ..Config::default()
        };
        return teams::dump(
            &engine,
            &graph_index,
            &overlays,
            &letters,
            &a.teams,
            &cfg,
            a.teams_reference,
            path,
        );
    }
    let mut self_check_line = None;
    if a.self_check {
        let t = std::time::Instant::now();
        let c = engine.self_check().map_err(|e| anyhow!("auto-contrôle des index : {e}"))?;
        let line = format!(
            "{} + {} chaînes étrangères, {} + {} chaînes sous le plafond, {} + {} nœuds de base, {} arêtes, {:.1} s : identiques à la force brute",
            c.foreign[0], c.foreign[1], c.components[0], c.components[1], c.degrees[0], c.degrees[1], c.base_edges, t.elapsed().as_secs_f64()
        );
        eprintln!("auto-contrôle : {line}");
        self_check_line = Some(line);
    }
    let kind = match a.engine.as_str() {
        "window" => Kind::Window,
        "flat" => Kind::Flat,
        other => return Err(anyhow!("moteur inconnu : {other} (window, flat)")),
    };
    let mut wins = Windows::new(&engine, kind, a.budget_mb * 1_000_000, a.radius_step_m);
    let mut flat_wins = Windows::new(&engine, Kind::Flat, usize::MAX / 2, a.radius_step_m);
    let mut dump: Vec<String> = Vec::new();
    let mut mismatches: Vec<String> = Vec::new();
    let mut compared = 0usize;
    let mut window_ms_by_request: Vec<f64> = Vec::new();

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
    for (team_n, (letter, id)) in team_ids.iter().enumerate() {
        if !a.teams.is_empty() && !a.teams.contains(letter) {
            continue;
        }
        let team_key = team_n as u64;
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
        let check_cfg = Config {
            hairpin_single_node: a.hairpin_single_node,
            hole_m: a.hole_m.unwrap_or(100.0),
            connector_m: a.connector_m.unwrap_or(30.0),
            ..Config::default()
        };
        let t = std::time::Instant::now();
        let _ = sampler.take_max_mb();
        let anon_before = memory_mb().2;
        let plan = TeamData::plan_build(&engine, &overlay).map_err(|e| anyhow!("équipe {letter} : {e}"))?;
        let check_plan = TeamCheck::plan(engine.base(), &overlay);
        // Comme le service : les données d'équipe et les tables du contrôleur se construisent **en parallèle** et
        // tiennent, ensemble, leur mémoire transitoire dans le budget des fenêtres (lot 6, finition).
        let estimated = plan.estimated_bytes + check_plan.estimated_bytes;
        wins.reserve_team_build(estimated)?;
        let (team, team_check) = std::thread::scope(|sc| {
            let check = sc.spawn(|| {
                TeamCheck::build_planned(engine.base(), &graph_index, &overlay, &check_cfg, &check_plan)
            });
            let team = TeamData::build_planned(&engine, &overlay, &plan);
            (team, check.join().expect("tables du contrôleur"))
        });
        let team = team.map_err(|e| anyhow!("équipe {letter} : {e}"))?;
        let peak = sampler.take_max_mb();
        eprintln!(
            "équipe {letter} : construction {:.1} s, pic anonyme {peak:.0} Mo (+{:.0} Mo), estimation {:.0} Mo ({:.0} + {:.0})",
            t.elapsed().as_secs_f64(),
            peak - anon_before,
            estimated as f64 / 1e6,
            plan.estimated_bytes as f64 / 1e6,
            check_plan.estimated_bytes as f64 / 1e6
        );
        team_build_anon.push((letter.clone(), t.elapsed().as_secs_f64(), peak));
        plan_estimates.push((letter.clone(), estimated as f64 / 1e6));
        resident_mb.push((
            letter.clone(),
            overlay.bytes() as f64 / 1e6,
            team.bytes() as f64 / 1e6,
            team_check.bytes() as f64 / 1e6,
        ));
        wins.release_team_build(estimated);
        for r in [ord_loops::RideType::Road, ord_loops::RideType::Gravel] {
            let s = team.stats(r);
            eprintln!(
                "équipe {letter} {r:?} : équipe {:.0} km, comblé {:.1} km, hors vue {:.1} km (du type) / {:.1} km (tous types)",
                s.team_km, s.bridged_km, s.outside_typed_km, s.outside_any_km
            );
        }
        let checker =
            Checker::with_tables(engine.base(), Some(&overlay), check_cfg, &graph_index, Some(&team_check));
        let ctx = Context { engine: &engine, team: &team, checker: &checker };
        eprintln!(
            "équipe {letter} : {} requêtes dans le périmètre, données en {:.2} s",
            inside.len(),
            t.elapsed().as_secs_f64()
        );

        let api_key = keys.get(id);
        if client.is_some() && api_key.is_none() {
            return Err(anyhow!("pas de clé pour l'équipe {letter} dans le fichier des clés"));
        }
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
            for play in &plays {
                // Un mode du lot 2 (familiarité de la requête Java, λ = 3) ou une valeur du curseur du service.
                let (mode, familiarity, q8, cursor) = match *play {
                    Play::Mode(m) => (m, jr.familiarity, ord_loops::DEFAULT_OFF_TEAM_COST_Q8, None),
                    Play::Cursor(c) => {
                        let applied = ord_server::generator::cursor::apply(
                            ord_server::generator::cursor::hundredths(c)
                                .ok_or_else(|| anyhow!("curseur hors de [0, 1] : {c}"))?,
                        );
                        (applied.mode, applied.familiarity, applied.off_team_cost_q8, Some(c))
                    }
                };
                let req = Request {
                    start: LatLon::new(p.0, p.1),
                    end,
                    bearing: jr.bearing,
                    distance_m: jr.km * 1000.0,
                    ride_type: jr.ride,
                    mode,
                    familiarity,
                    off_team_cost_q8: q8,
                    seed: u64::from(jr.i),
                };
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
                    mode: match cursor {
                        Some(c) => format!("c{c}"),
                        None => mode_name(mode).into(),
                    },
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
                    cursor,
                    http_status: None,
                    dropped: None,
                    window_build_ms: None,
                    applied: cursor.map(|_| {
                        let l = f64::from(q8) / 256.0;
                        format!("{} q8={q8} λ={l:.3} familiarité={familiarity}", mode_name(mode))
                    }),
                };
                if let (Some(client), Some(c)) = (&client, cursor) {
                    let ask = http::Ask {
                        start: req.start,
                        end: req.end,
                        bearing: req.bearing,
                        distance_m: req.distance_m,
                        ride: req.ride_type,
                        cursor: c,
                        seed: req.seed,
                        key: api_key,
                    };
                    let (answer, ms) = client.run(&ask, a.rounds)?;
                    http::fill(&mut rec, &answer, ms, req.distance_m);
                    recs.push(rec);
                    continue;
                }
                let acq = wins.acquire(team_key, &team, &req)?;
                if acq.built {
                    rec.window_build_ms = Some(acq.build_ms);
                    window_ms_by_request.push(acq.build_ms);
                }
                let inp = Inputs { win: &acq.win, team: &team, wt: &acq.wt };
                let outcome = record::run_request(&engine, inp, &req, &mut ws, a.rounds);
                if matches!(outcome.result, Err(ord_loops::Failure::WindowEscape)) {
                    wins.escapes += 1;
                }
                let sig = format!("{:?}", (&outcome.result, &outcome.diag));
                if a.dump.is_some() {
                    dump.push(format!("{letter} {} {} {:016x}", jr.i, rec.mode, fnv(&sig)));
                }
                if a.compare {
                    let facq = flat_wins.acquire(team_key, &team, &req)?;
                    let finp = Inputs { win: &facq.win, team: &team, wt: &facq.wt };
                    let flat = record::run_request(&engine, finp, &req, &mut ws, 0);
                    compared += 1;
                    if sig != format!("{:?}", (&flat.result, &flat.diag)) {
                        mismatches.push(format!("équipe {letter} requête {} mode {}", jr.i, rec.mode));
                    }
                }
                ctx.fill(&mut rec, &req, &outcome, &acq.win);
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
        wins.forget_team(team_key);
        flat_wins.forget_team(team_key);
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
        (
            "modes".into(),
            if a.cursors.is_empty() {
                a.modes.join(", ")
            } else {
                format!(
                    "curseurs {} ({})",
                    a.cursors.iter().map(f64::to_string).collect::<Vec<_>>().join(", "),
                    if a.http.is_some() { "HTTP" } else { "en processus" }
                )
            },
        ),
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
    let (hwm, rss, anon, file) = memory_mb();
    lines.push((
        "moteur".into(),
        format!("{} (budget {} Mo, pas de rayon {} m)", a.engine, a.budget_mb, a.radius_step_m),
    ));
    lines.push((
        "chargement".into(),
        format!(
            "{load_s:.1} s, index road {} + {} ms, gravel {} + {} ms, {:.1} Mo",
            tm[0].0,
            tm[0].1,
            tm[1].0,
            tm[1].1,
            engine.index_bytes() as f64 / 1e6
        ),
    ));
    lines.push((
        "mémoire anonyme (RssAnon, échantillonnée)".into(),
        format!(
            "ouverture {load_anon_mb:.0} Mo ; pic au total {:.0} Mo ; construction d'équipe, données et contrôleur en parallèle (s, pic Mo) : {} ; estimations réservées au budget (Mo) : {} ; octets résidents par équipe (surcouche + données + contrôleur, Mo) : {} ; total {:.0} Mo",
            sampler.max_mb(),
            team_build_anon
                .iter()
                .map(|(l, s, m)| format!("{l} {s:.1} s {m:.0}"))
                .collect::<Vec<_>>()
                .join(", "),
            plan_estimates.iter().map(|(l, m)| format!("{l} {m:.0}")).collect::<Vec<_>>().join(", "),
            resident_mb
                .iter()
                .map(|(l, o, t, c)| format!("{l} {o:.1} + {t:.1} + {c:.1}"))
                .collect::<Vec<_>>()
                .join(", "),
            resident_mb.iter().map(|(_, o, t, c)| o + t + c).sum::<f64>()
        ),
    ));
    lines.push((
        "mémoire du processus".into(),
        format!("VmHWM {hwm:.0} Mo, VmRSS {rss:.0} Mo (RssAnon {anon:.0} Mo, RssFile {file:.0} Mo) à la fin"),
    ));
    let cs = wins.cache_stats();
    let q = |v: Vec<f64>| {
        quantiles(v).map_or_else(|| "-".into(), |(m, p, x)| format!("{m:.0} / {p:.0} / {x:.0} ms"))
    };
    lines.push((
        "fenêtres".into(),
        format!(
            "{} construites, {} servies du cache, {} évincées ; construction à froid (médiane / p90 / max) {} ; agrégats d'équipe {} ; mémoire comptée au plus {:.0} Mo ; {} échappements",
            wins.built.len(),
            cs.hits,
            cs.evicted_windows,
            q(wins.built.iter().map(|b| b.0).collect()),
            q(wins.team_builds_ms.clone()),
            wins.peak_counted as f64 / 1e6,
            wins.escapes
        ),
    ));
    if let Some((_, _, c)) = quantiles(wins.built.iter().map(|b| b.1 as f64).collect()) {
        let med = quantiles(wins.built.iter().map(|b| b.1 as f64).collect()).map_or(0.0, |q| q.0);
        lines.push(("taille des fenêtres".into(), format!("{med:.0} chaînes en médiane, {c:.0} au plus")));
    }
    if let Some(l) = &self_check_line {
        lines.push(("auto-contrôle".into(), l.clone()));
    }
    if a.compare {
        lines.push((
            "comparaison au moteur à plat".into(),
            format!("{compared} requêtes comparées, {} différences", mismatches.len()),
        ));
    }
    if let Some(l) = a.limit {
        lines.push(("limite".into(), format!("{l} requêtes par équipe (banc partiel)")));
    }
    if let Some(k) = a.max_km {
        lines.push(("limite".into(), format!("D ≤ {k} km (banc partiel)")));
    }
    std::fs::create_dir_all(&out)?;
    let json_path = out.join(format!("{}-report.json", a.label));
    std::fs::write(&json_path, serde_json::to_vec_pretty(&Report { meta: &lines, records: &recs })?)?;
    let mode_labels: Vec<String> = if a.cursors.is_empty() {
        a.modes.clone()
    } else {
        a.cursors.iter().map(|c| format!("c{c}")).collect()
    };
    let md = report::summary(&SummaryInput {
        meta: &Meta { lines: lines.clone() },
        recs: &recs,
        java_total: &java_total,
        autocheck: &autocheck,
        modes: &mode_labels,
        rounds: a.rounds,
    });
    let md_path = out.join(format!("{}-summary.md", a.label));
    std::fs::write(&md_path, md)?;
    eprintln!("rapport : {}\nrésumé : {}", json_path.display(), md_path.display());
    if let Some(p) = &a.dump {
        std::fs::write(p, dump.join("\n") + "\n")?;
        eprintln!("empreintes : {}", p.display());
    }
    if wins.escapes > 0 {
        return Err(anyhow!("{} échappements de fenêtre (garde-fou levé)", wins.escapes));
    }
    if !mismatches.is_empty() {
        for m in mismatches.iter().take(20) {
            eprintln!("DIFFÉRENCE : {m}");
        }
        return Err(anyhow!("{} requêtes dont le résultat diffère du moteur à plat", mismatches.len()));
    }
    Ok(())
}
