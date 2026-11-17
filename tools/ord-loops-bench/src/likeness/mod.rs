//! Banc de vraisemblance du lot 4 (4a) : les formes des parcours générés contre celles des traces réelles de
//! l'export, et la reconstruction d'une trace mise à l'écart (note : `docs/design/technique/ord-loops-lot4.md` §2 et
//! §11).
//!
//! **Privé** : l'export, les départs, les coordonnées et les ids de trace ou d'équipe sont des données d'équipes
//! réelles. Les sorties (rapport JSON, résumé) sont écrites sous `~/.cache/osm-route-details/graph/lot4/` et nulle
//! part ailleurs ; les équipes y sont lettrées, une boucle y est désignée par `(lettre, i)`, et seuls des agrégats et
//! des empreintes en sortent.

pub mod export;
#[cfg(test)]
mod fixture;
pub mod holdout;
pub mod measure;
pub mod out;
pub mod play;
pub mod shape;
pub mod walk;

use std::path::PathBuf;

use std::collections::BTreeMap;

use anyhow::{anyhow, Context as _, Result};
use clap::Args;
use ord_graph::mmap::MappedGraph;
use ord_graph::overlay::Overlay;
use ord_graph::util::Poly;
use ord_graph_match::SegIndex;
use ord_loops::{Params, TeamData, Workspace};
use ord_loops_check::{Checker, Config, GraphIndex, TeamCheck, TypeIndex};
use rayon::prelude::*;

use self::measure::Measures;
use self::play::Eng;
use crate::windows::AnonSampler;
use ord_loops::Engine;

/// Arguments de `likeness`. Les défauts sont sous `~/.cache/osm-route-details/graph/` (noté `G`).
#[derive(Debug, Args)]
pub struct LikenessArgs {
    /// Graphe de base (défaut : `G/lot6/france-20261115-z11.ordg`).
    #[arg(long)]
    pub graph: Option<PathBuf>,
    /// Vue `road` (défaut : `G/lot6/france-20261115-z11.road.ordv`).
    #[arg(long)]
    pub road: Option<PathBuf>,
    /// Vue `gravel` (défaut : `G/lot6/france-20261115-z11.gravel.ordv`).
    #[arg(long)]
    pub gravel: Option<PathBuf>,
    /// `.poly` de la région de `match_job` (défaut : `G/lot6/france.poly`).
    #[arg(long)]
    pub poly: Option<PathBuf>,
    /// Export privé (défaut : `~/code/perso/tribly-generator-bench/data/prod-network.csv.gz`).
    #[arg(long)]
    pub export: Option<PathBuf>,
    /// `{ id d'équipe : lettre }` (défaut : `G/lot2/team-letters.json`).
    #[arg(long)]
    pub letters: Option<PathBuf>,
    /// Dossier des surcouches du lot 6, pour le seul contrôle d'égalité des surcouches reconstruites (défaut :
    /// `G/lot6/overlays` ; absent : contrôle sauté).
    #[arg(long)]
    pub overlays: Option<PathBuf>,
    /// Sortie : `<label>-report.json` et `<label>-summary.md` (défaut : `G/lot4/`).
    #[arg(long)]
    pub out: Option<PathBuf>,
    #[arg(long, default_value = "likeness")]
    pub label: String,
    /// Valeurs du curseur jouées, par `ord_server::generator::cursor::{hundredths, apply}`.
    #[arg(long, value_delimiter = ',', default_values_t = [0.25, 1.0])]
    pub cursors: Vec<f64>,
    /// Équipes jouées (lettres, séparées par des virgules) ; toutes par défaut.
    #[arg(long, value_delimiter = ',')]
    pub teams: Vec<String>,
    /// Au plus ce nombre de boucles retenues par équipe (les premières dans l'ordre de l'export).
    #[arg(long)]
    pub limit: Option<usize>,
    /// Seulement les boucles de D au plus ce nombre de km.
    #[arg(long)]
    pub max_km: Option<f64>,
    /// D minimale d'une boucle retenue, km.
    #[arg(long, default_value_t = 5.0)]
    pub min_km: f64,
    /// L'arrivée est à moins de cette part de D du départ.
    #[arg(long, default_value_t = 0.05)]
    pub loop_gap: f64,
    /// Part appariée minimale d'une trace.
    #[arg(long, default_value_t = 0.95)]
    pub min_matched: f64,
    /// Plus long trou admis dans la suite appariée, m.
    #[arg(long, default_value_t = 1000.0)]
    pub max_hole_m: f64,
    /// Part des boucles mises à l'écart, par équipe.
    #[arg(long, default_value_t = 0.10)]
    pub holdout_share: f64,
    /// Nombre minimal de boucles retenues d'une équipe pour en mettre à l'écart.
    #[arg(long, default_value_t = 10)]
    pub holdout_min_loops: usize,
    /// Saute la reconstruction.
    #[arg(long)]
    pub no_holdout: bool,
    /// Graine de base : la graine d'une requête est `empreinte de la trace ^ graine`.
    #[arg(long, default_value_t = 20_261_117)]
    pub seed: u64,
    /// Fils de génération (défaut : `available_parallelism()`).
    #[arg(long)]
    pub threads: Option<usize>,
    /// Budget du cache de fenêtres partagé, en Mo.
    #[arg(long, default_value_t = 6000)]
    pub budget_mb: usize,
    /// Pas d'arrondi du rayon de classe des fenêtres, m.
    #[arg(long, default_value_t = 10_000.0)]
    pub radius_step_m: f64,
    /// Ne fait que l'étalonnage `shapes.py` (sans graphe) et s'arrête.
    #[arg(long)]
    pub calibrate_only: bool,
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| anyhow!("HOME n'est pas défini"))
}

/// Les chemins résolus.
#[derive(Debug)]
pub struct Paths {
    pub graph: PathBuf,
    pub road: PathBuf,
    pub gravel: PathBuf,
    pub poly: PathBuf,
    pub export: PathBuf,
    pub letters: PathBuf,
    pub overlays: Option<PathBuf>,
    pub out: PathBuf,
}

impl LikenessArgs {
    pub fn paths(&self) -> Result<Paths> {
        let home = home()?;
        let g = home.join(".cache/osm-route-details/graph");
        let lot6 = g.join("lot6");
        let pick = |a: &Option<PathBuf>, d: PathBuf| a.clone().unwrap_or(d);
        let overlays = match &self.overlays {
            Some(p) => Some(p.clone()),
            None => Some(lot6.join("overlays")).filter(|p| p.is_dir()),
        };
        Ok(Paths {
            graph: pick(&self.graph, lot6.join("france-20261115-z11.ordg")),
            road: pick(&self.road, lot6.join("france-20261115-z11.road.ordv")),
            gravel: pick(&self.gravel, lot6.join("france-20261115-z11.gravel.ordv")),
            poly: pick(&self.poly, lot6.join("france.poly")),
            export: pick(
                &self.export,
                home.join("code/perso/tribly-generator-bench/data/prod-network.csv.gz"),
            ),
            letters: pick(&self.letters, g.join("lot2/team-letters.json")),
            overlays,
            out: pick(&self.out, g.join("lot4")),
        })
    }
}

/// `{ id d'équipe : lettre }`.
pub fn read_letters(path: &std::path::Path) -> Result<std::collections::BTreeMap<u64, String>> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    let raw: std::collections::BTreeMap<String, String> = serde_json::from_str(&text)?;
    raw.into_iter()
        .map(|(k, v)| {
            Ok((
                k.parse::<u64>().map_err(|_| anyhow!("id d'équipe illisible dans le fichier des lettres"))?,
                v,
            ))
        })
        .collect()
}

/// Surcouche complète (avec ses kilomètres comptés) et variante « écart ».
type Built = Result<((Overlay, f64), Option<Overlay>)>;

/// Une phase : durée et pic de mémoire anonyme.
struct Phases {
    sampler: AnonSampler,
    started: std::time::Instant,
    list: Vec<out::Phase>,
}

impl Phases {
    fn new() -> Self {
        let sampler = AnonSampler::start();
        let _ = sampler.take_max_mb();
        Self { sampler, started: std::time::Instant::now(), list: Vec::new() }
    }

    /// Clôt la phase en cours sous `name`.
    fn end(&mut self, name: &str) {
        let secs = self.started.elapsed().as_secs_f64();
        let peak = self.sampler.take_max_mb();
        eprintln!("phase {name} : {secs:.1} s, pic anonyme {peak:.0} Mo");
        self.list.push(out::Phase { name: name.to_string(), secs, peak_anon_mb: peak });
        self.started = std::time::Instant::now();
    }
}

fn open_graph(p: &std::path::Path) -> Result<MappedGraph> {
    MappedGraph::open_mmap(p).with_context(|| p.display().to_string())
}

pub fn run(a: &LikenessArgs) -> Result<()> {
    let paths = a.paths()?;
    if a.calibrate_only {
        return calibrate_only(a, &paths);
    }
    let t_all = std::time::Instant::now();
    let load0 = crate::load_avg();
    let letters = read_letters(&paths.letters)?;
    let wanted: Vec<(u64, String)> = letters
        .iter()
        .filter(|(_, l)| a.teams.is_empty() || a.teams.contains(l))
        .map(|(id, l)| (*id, l.clone()))
        .collect();
    anyhow::ensure!(!wanted.is_empty(), "aucune équipe ne correspond à --teams");
    let threads =
        a.threads.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get())).max(1);
    let mut ph = Phases::new();

    // 1. Le moteur fenêtré et l'index du contrôleur.
    let engine = Engine::new(
        open_graph(&paths.graph)?,
        open_graph(&paths.road)?,
        open_graph(&paths.gravel)?,
        Params::default(),
    )
    .map_err(|e| anyhow!("moteur : {e}"))?;
    let gf: &MappedGraph = engine.base();
    let graph_index = GraphIndex::build(gf);
    ph.end("moteur et index du contrôleur");

    // 2. L'export, et l'étalonnage `raw_legacy`.
    let wanted_ids: Vec<u64> = wanted.iter().map(|(id, _)| *id).collect();
    let mut ing = export::ingest(&paths.export, &wanted_ids)?;
    let raw = export::raw_legacy(&ing);
    let calibration = shape::legacy_lines(&raw.iter().flatten().copied().collect::<Vec<_>>());
    let letter_of: Vec<String> = ing
        .teams
        .iter()
        .map(|id| {
            letters.get(id).cloned().ok_or_else(|| anyhow!("équipe sans lettre dans le fichier des lettres"))
        })
        .collect::<Result<_>>()?;
    ph.end("lecture de l'export et étalonnage");

    // 3. L'appariement des clés distinctes.
    let poly = Poly::read(&paths.poly).with_context(|| paths.poly.display().to_string())?;
    let seg = SegIndex::build(gf);
    let jobs = export::match_keys(gf, &seg, &poly, &ing);
    ph.end("appariement");

    // 4. Les marches, la retenue.
    let rp = export::RetainParams {
        min_matched: a.min_matched,
        max_hole_m: a.max_hole_m,
        min_km: a.min_km,
        max_km: a.max_km,
        loop_gap: a.loop_gap,
    };
    let (loops, mut counts) = export::retain(gf, &ing, &jobs, &rp, a.limit);
    // La mise à l'écart : stable, par rang d'empreinte de l'id de trace, équipe par équipe.
    let held: std::collections::HashSet<usize> = if a.no_holdout {
        std::collections::HashSet::new()
    } else {
        holdout::select(&loops, a.holdout_share, a.holdout_min_loops)
    };
    for &i in &held {
        counts[loops[i].team].holdout += 1;
    }
    ph.end("marches et retenue");

    // 5. Les surcouches, par le code du dépôt.
    let team_ids: Vec<usize> = (0..ing.teams.len()).collect();
    // Complète, et « écart » (sans les boucles mises à l'écart) pour les équipes qui en ont.
    let skips: Vec<std::collections::HashSet<usize>> = team_ids
        .iter()
        .map(|&t| held.iter().filter(|&&i| loops[i].team == t).map(|&i| loops[i].track).collect())
        .collect();
    let built: Vec<Built> = team_ids
        .par_iter()
        .map(|&t| {
            let full =
                holdout::build_overlay(gf, &seg, &poly, &ing, &jobs, t, &std::collections::HashSet::new())?;
            let gap = if skips[t].is_empty() {
                None
            } else {
                Some(holdout::build_overlay(gf, &seg, &poly, &ing, &jobs, t, &skips[t])?.0)
            };
            Ok((full, gap))
        })
        .collect();
    let mut overlays: Vec<Overlay> = Vec::new();
    let mut gap_overlays: Vec<Option<Overlay>> = Vec::new();
    let mut overlay_checks: Vec<holdout::OverlayCheck> = Vec::new();
    for (t, b) in built.into_iter().enumerate() {
        let ((o, km), gap) = b?;
        let differing = paths
            .overlays
            .as_ref()
            .and_then(|d| holdout::overlay_file(d, ing.teams[t]))
            .and_then(|f| Overlay::read(&f).ok())
            .map(|file| holdout::differing_edges(&o, &file));
        overlay_checks.push(holdout::OverlayCheck {
            team: letter_of[t].clone(),
            edges: o.len(),
            km_counted: km,
            holdout_edges: gap.as_ref().map(Overlay::len),
            differing_edges: differing,
        });
        overlays.push(o);
        gap_overlays.push(gap);
    }
    ph.end("surcouches du dépôt");

    // 6. Le pic de mémoire est derrière : l'index, les appariements et les géométries sont lâchés.
    drop(jobs);
    drop(seg);
    ing.keys = Vec::new();
    ph.end("libération");

    // 7. Les mesures du réel : la forme, et le contrôleur sur les marches continues.
    let checker0 = Checker::with_tables(gf, None, Config::default(), &graph_index, None);
    let reals: Vec<(Measures, Option<Vec<String>>)> =
        loops.par_iter().map(|l| measure_real(gf, &checker0, l)).collect();
    let real_shape: Vec<[f64; 3]> =
        reals.iter().map(|(m, _)| [m.r_over_d, m.far_pos, m.compactness]).collect();
    let real_edges: Vec<std::collections::BTreeSet<u32>> =
        loops.par_iter().map(|l| measure::taken_edges(gf, &l.walk)).collect();
    let twins = holdout::twin_shares(gf, &loops, &held, &real_edges);
    let mut iqr = [[1.0; 3]; 3];
    for (ty, row) in iqr.iter_mut().enumerate() {
        for (k, cell) in row.iter_mut().enumerate() {
            let v: Vec<f64> = loops
                .iter()
                .zip(&real_shape)
                .filter(|(l, _)| usize::from(l.ty) == ty)
                .map(|(_, s)| s[k])
                .collect();
            *cell = shape::iqr(&v).filter(|q| *q > 0.0).unwrap_or(1.0);
        }
    }
    ph.end("mesures du réel");

    // 8. Les jeux, équipe par équipe dans l'ordre des lettres.
    let wins = play::SharedWindows::new(&engine, a.budget_mb * 1_000_000, a.radius_step_m);
    let ctx =
        play::PlayCtx { engine: &engine, wins: &wins, iqr, real_shape: &real_shape, real_edges: &real_edges };
    let mut workspaces: Vec<Workspace> = (0..threads).map(|_| Workspace::new()).collect();
    let mut plays: Vec<Vec<play::PlayRec>> = loops.iter().map(|_| Vec::new()).collect();
    let mut order: Vec<usize> = team_ids.clone();
    order.sort_by(|&x, &y| letter_of[x].cmp(&letter_of[y]));
    for t in order {
        let mine: Vec<usize> = (0..loops.len()).filter(|&i| loops[i].team == t).collect();
        if mine.is_empty() {
            continue;
        }
        let tt = std::time::Instant::now();
        let overlay = &overlays[t];
        let (team, team_check) = build_team(&engine, &graph_index, overlay, &letter_of[t])?;
        let checker =
            Checker::with_tables(gf, Some(overlay), Config::default(), &graph_index, Some(&team_check));
        let env = play::TeamEnv {
            key: 2 * t as u64,
            overlay,
            types: TypeIndex::of(overlay),
            team: &team,
            checker: &checker,
        };
        let mut tasks: Vec<play::Task> = Vec::new();
        for &i in &mine {
            for &c in &a.cursors {
                let req = play::request_of(gf, &loops[i], c, a.seed)?;
                tasks.push(play::Task { loop_idx: i, cursor: c, req, ride: play::ride_of(loops[i].ty) });
            }
        }
        tasks.sort_by_key(play::locality_key);
        for (k, rec) in play::run_tasks(&ctx, &env, &tasks, "full", &mut workspaces) {
            plays[tasks[k].loop_idx].push(rec);
        }
        wins.drop_team(env.key);
        // Les boucles mises à l'écart, rejouées sur la surcouche sans elles : même requête, même graine.
        if let Some(gap) = &gap_overlays[t] {
            let (team, team_check) = build_team(&engine, &graph_index, gap, &letter_of[t])?;
            let checker =
                Checker::with_tables(gf, Some(gap), Config::default(), &graph_index, Some(&team_check));
            let env = play::TeamEnv {
                key: 2 * t as u64 + 1,
                overlay: gap,
                types: TypeIndex::of(gap),
                team: &team,
                checker: &checker,
            };
            let mut tasks: Vec<play::Task> = Vec::new();
            for &i in mine.iter().filter(|i| held.contains(i)) {
                for &c in &a.cursors {
                    let req = play::request_of(gf, &loops[i], c, a.seed)?;
                    tasks.push(play::Task { loop_idx: i, cursor: c, req, ride: play::ride_of(loops[i].ty) });
                }
            }
            tasks.sort_by_key(play::locality_key);
            for (k, rec) in play::run_tasks(&ctx, &env, &tasks, "holdout", &mut workspaces) {
                plays[tasks[k].loop_idx].push(rec);
            }
            wins.drop_team(env.key);
        }
        eprintln!("équipe {} : {} boucles, {:.1} s", letter_of[t], mine.len(), tt.elapsed().as_secs_f64());
    }
    ph.end("jeux");

    // 9. Les sorties.
    let mut recs: Vec<out::LoopRec> = Vec::new();
    for (i, l) in loops.iter().enumerate() {
        let mut p = std::mem::take(&mut plays[i]);
        p.sort_by(|x, y| x.overlay.cmp(&y.overlay).then(x.cursor.total_cmp(&y.cursor)));
        recs.push(out::LoopRec {
            team: letter_of[l.team].clone(),
            i: l.i,
            ty: out::TYPE_NAMES[usize::from(l.ty).min(2)].to_string(),
            d_km: l.trace.d_graph / 1000.0,
            matched_share: l.matched_share,
            contiguous: l.walk.is_continuous(),
            holdout: held.contains(&i),
            twin_share: twins[i],
            poly_vs_graph: if l.trace.d_graph > 0.0 {
                (l.trace.l_poly - l.trace.d_graph).abs() / l.trace.d_graph
            } else {
                0.0
            },
            real: reals[i].0.clone(),
            real_check: reals[i].1.clone(),
            plays: p,
        });
    }
    recs.sort_by(|x, y| (&x.team, x.i).cmp(&(&y.team, y.i)));
    let mut retention: BTreeMap<String, export::RetentionCounts> = BTreeMap::new();
    for (t, c) in counts.into_iter().enumerate() {
        retention.insert(letter_of[t].clone(), c);
    }
    overlay_checks.sort_by(|x, y| x.team.cmp(&y.team));
    let vertex_histograms = vertex_histograms(&loops, &raw, &reals);
    ph.end("sorties");
    let fingerprint = out::fingerprint(&recs);
    let meta = meta_lines(a, &engine, &ing, threads, &load0, &letter_of, t_all.elapsed().as_secs_f64(), &ph);
    let report = out::Report {
        meta,
        retention,
        calibration,
        vertex_histograms,
        overlays: overlay_checks,
        phases: ph.list.clone(),
        windows: wins.counters(),
        cursors: a.cursors.clone(),
        fingerprint,
        loops: recs,
    };
    std::fs::create_dir_all(&paths.out)?;
    let json = paths.out.join(format!("{}-report.json", a.label));
    let md = paths.out.join(format!("{}-summary.md", a.label));
    std::fs::write(&json, serde_json::to_vec_pretty(&report)?)?;
    std::fs::write(&md, out::summary(&report))?;
    eprintln!(
        "rapport : {}\nrésumé : {}\nempreinte de résultat : {}",
        json.display(),
        md.display(),
        report.fingerprint
    );
    Ok(())
}

/// Données d'équipe et tables du contrôleur d'une surcouche, construites en parallèle comme le fait le service.
fn build_team(
    engine: &Eng,
    gi: &GraphIndex,
    overlay: &Overlay,
    letter: &str,
) -> Result<(TeamData, TeamCheck)> {
    let cfg = Config::default();
    let plan = TeamData::plan_build(engine, overlay).map_err(|e| anyhow!("équipe {letter} : {e}"))?;
    let check_plan = TeamCheck::plan(engine.base(), overlay, &cfg);
    let (team, check) = std::thread::scope(|sc| {
        let check = sc.spawn(|| {
            TeamCheck::build_planned(engine.base(), gi, overlay, &cfg, &check_plan)
                .map_err(|e| anyhow!("équipe {letter} : {e}"))
        });
        let team =
            TeamData::build_planned(engine, overlay, &plan).map_err(|e| anyhow!("équipe {letter} : {e}"));
        (team, check.join().expect("tables du contrôleur"))
    });
    Ok((team?, check?))
}

/// Mesures d'une boucle réelle, et le contrôleur sur sa marche si elle est continue (découverte : aucune règle
/// d'équipe).
fn measure_real(
    gf: &MappedGraph,
    checker: &Checker<'_, memmap2::Mmap>,
    l: &export::Loop,
) -> (Measures, Option<Vec<String>>) {
    if !l.walk.is_continuous() {
        return (measure::measure(gf, &l.walk, &l.trace, None), None);
    }
    let route = l.walk.to_route(gf);
    let Some((lat, lon)) = walk::start_latlon(gf, &l.walk) else {
        return (measure::measure(gf, &l.walk, &l.trace, None), None);
    };
    let req = ord_loops::Request {
        start: ord_loops::LatLon::new(lat, lon),
        end: None,
        bearing: None,
        distance_m: l.trace.d_graph,
        ride_type: play::ride_of(l.ty),
        mode: ord_loops::Mode::Discovery,
        familiarity: 0.5,
        off_team_cost_q8: 256,
        seed: 0,
    };
    let rep = checker.check(&req, &route);
    let codes: Vec<String> = rep.violations.iter().map(|v| v.code().to_string()).collect();
    let structural =
        codes.iter().any(|c| matches!(c.as_str(), "EMPTY" | "NOT_ON_GRAPH" | "DISCONTINUOUS" | "BAD_OFFSET"));
    let crossings = (!structural).then_some(rep.crossings);
    (measure::measure(gf, &l.walk, &l.trace, crossings), Some(codes))
}

/// Histogrammes de sommets sur les boucles retenues, par type : `raw_legacy`, `vertices_legacy`, `vertices`,
/// `vertices_10` (plafonnés à 6).
fn vertex_histograms(
    loops: &[export::Loop],
    raw: &[Option<shape::RawRow>],
    reals: &[(Measures, Option<Vec<String>>)],
) -> BTreeMap<String, BTreeMap<String, BTreeMap<u32, usize>>> {
    let mut out: BTreeMap<String, BTreeMap<String, BTreeMap<u32, usize>>> = BTreeMap::new();
    for (k, ty) in out::TYPE_NAMES.iter().enumerate() {
        let sel: Vec<usize> = (0..loops.len()).filter(|&i| usize::from(loops[i].ty) == k).collect();
        if sel.is_empty() {
            continue;
        }
        let e = out.entry((*ty).to_string()).or_default();
        e.insert(
            "raw_legacy".into(),
            out::histogram(sel.iter().filter_map(|&i| raw[loops[i].track]).map(|r| r.corners.max(0) as u32)),
        );
        e.insert("vertices_legacy".into(), out::histogram(sel.iter().map(|&i| reals[i].0.vertices_legacy)));
        e.insert("vertices".into(), out::histogram(sel.iter().map(|&i| reals[i].0.vertices)));
        e.insert("vertices_10".into(), out::histogram(sel.iter().map(|&i| reals[i].0.vertices_10)));
    }
    out
}

/// Les lignes `meta` du rapport.
#[allow(clippy::too_many_arguments)]
fn meta_lines(
    a: &LikenessArgs,
    engine: &Eng,
    ing: &export::Ingest,
    threads: usize,
    load0: &str,
    letters: &[String],
    secs: f64,
    ph: &Phases,
) -> Vec<(String, String)> {
    let epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let h = engine.base().header();
    let dirty = crate::git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    let mut v: Vec<(String, String)> = vec![
        ("date".into(), crate::iso(epoch)),
        (
            "commit".into(),
            format!(
                "{}{}",
                crate::git(&["rev-parse", "--short", "HEAD"]).unwrap_or_default(),
                if dirty { " (arbre modifié)" } else { "" }
            ),
        ),
        ("machine".into(), crate::read_first_line("/proc/cpuinfo", "model name").unwrap_or_default()),
        ("fils de génération".into(), threads.to_string()),
        ("charge (1, 5, 15 min) au début / à la fin".into(), format!("{load0} / {}", crate::load_avg())),
        ("profil".into(), if cfg!(debug_assertions) { "debug".into() } else { "release".into() }),
        (
            "graphe".into(),
            format!("{} nœuds, {} arêtes, outil {}, réplication {}", h.nodes, h.edges, h.tool_version, h.replication_timestamp),
        ),
        ("export".into(), format!("{} lignes lues, empreinte xxh3 {:016x}", ing.rows, ing.export_xxh3)),
        ("équipes".into(), letters.join(", ")),
        (
            "paramètres".into(),
            format!(
                "curseurs {:?}, D min {} km, boucle < {} de D, part appariée ≥ {}, trou ≤ {} m, mise à l'écart {} (≥ {} boucles), graine {}, budget {} Mo, pas de rayon {} m",
                a.cursors, a.min_km, a.loop_gap, a.min_matched, a.max_hole_m, a.holdout_share, a.holdout_min_loops, a.seed, a.budget_mb, a.radius_step_m
            ),
        ),
        ("durée du banc".into(), format!("{secs:.0} s")),
        ("pic de mémoire anonyme, toutes phases".into(), format!("{:.0} Mo", ph.sampler.max_mb())),
    ];
    if let Some(l) = a.limit {
        v.push(("limite".into(), format!("{l} boucles retenues par équipe (banc partiel)")));
    }
    if let Some(k) = a.max_km {
        v.push(("limite".into(), format!("D ≤ {k} km (banc partiel)")));
    }
    v
}

/// `--calibrate-only` : `shapes.py` en entier sur l'export brut, sans graphe, au format de `shapes.py`.
fn calibrate_only(a: &LikenessArgs, p: &Paths) -> Result<()> {
    let t = std::time::Instant::now();
    let ing = export::ingest(&p.export, &[])?;
    let rows: Vec<shape::RawRow> = export::raw_legacy(&ing).into_iter().flatten().collect();
    let lines = shape::legacy_lines(&rows);
    let text = lines.join("\n") + "\n";
    std::fs::create_dir_all(&p.out)?;
    let path = p.out.join(format!("{}-calibration.txt", a.label));
    std::fs::write(&path, &text)?;
    print!("{text}");
    eprintln!("étalonnage : {} ({:.1} s)", path.display(), t.elapsed().as_secs_f64());
    Ok(())
}
