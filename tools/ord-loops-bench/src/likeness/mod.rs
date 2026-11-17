//! Banc de vraisemblance du lot 4 (4a) : les formes des parcours générés contre celles des traces réelles de
//! l'export, et la reconstruction d'une trace mise à l'écart (note : `docs/design/technique/ord-loops-lot4.md` §2 et
//! §11).
//!
//! **Privé** : l'export, les départs, les coordonnées et les ids de trace ou d'équipe sont des données d'équipes
//! réelles. Les sorties (rapport JSON, résumé) sont écrites sous `~/.cache/osm-route-details/graph/lot4/` et nulle
//! part ailleurs ; les équipes y sont lettrées, une boucle y est désignée par `(lettre, i)`, et seuls des agrégats et
//! des empreintes en sortent.

pub mod chain;
pub mod compare;
pub mod export;
#[cfg(test)]
mod fixture;
pub mod holdout;
pub mod measure;
pub mod out;
pub mod play;
pub mod shape;
pub mod taste;
pub mod variants;
pub mod walk;

use std::path::PathBuf;

use std::collections::BTreeMap;

use anyhow::{anyhow, Context as _, Result};
use clap::Args;
use ord_graph::mmap::MappedGraph;
use ord_graph::overlay::Overlay;
use ord_graph::util::Poly;
use ord_graph_match::SegIndex;
use ord_loops::{Params, Taste, TeamData, Workspace};
use ord_loops_check::{Checker, Config, GraphIndex, TeamCheck, TypeIndex};
use rayon::prelude::*;

use self::measure::Measures;
use self::play::Eng;
use self::taste::{ClassSums, TasteParams, TasteTable};
use self::variants::{TasteSource, Variant, VariantMeta};
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
    /// Sonde du goût (§12 de la note) : variantes jouées l'une après l'autre, séparées par des virgules — `base`, ou
    /// des composants joints par `+` : `fam:<f>`, `famroad:<f>` (familiarité en ROAD, équipe d'abord, seulement), `ge:<γ>` (goût
    /// d'équipe), `gg:<γ>` (général), `rc:<γ>` (recalé), `rt:<γ>` (recalé par équipe), `n:<k>` (chaîne de `k` pas, avec
    /// `rc` ou `rt`), `dim:<72|me>` (forme de la table recalée).
    /// Absent : le banc du 4a à l'identique (une passe `base`, sorties `<label>-report.json` et `<label>-summary.md`).
    #[arg(long, value_delimiter = ',')]
    pub variants: Vec<String>,
    /// Goût : lissage `m`.
    #[arg(long, default_value_t = 0.5)]
    pub taste_m: f64,
    /// Goût : `g` est borné à `[1 / bound, bound]`.
    #[arg(long, default_value_t = 3.0)]
    pub taste_bound: f64,
    /// Goût : seuil de données (km-passages, ou km de traces pour `rc`) sous lequel la table est neutre.
    #[arg(long, default_value_t = 200.0)]
    pub taste_min_km: f64,
    /// Goût : côté de la trame du couloir, m.
    #[arg(long, default_value_t = 500.0)]
    pub taste_cell_m: f64,
    /// Goût : dilatation du couloir, en cases.
    #[arg(long, default_value_t = 2)]
    pub taste_corridor_cells: u32,
    /// Recalé par équipe (`rt`) : seuil de repli `n0`, une équipe pèse `N / (N + n0)` dans sa propre table.
    #[arg(long, default_value_t = 25.0)]
    pub taste_team_n0: f64,
    /// Recalé itéré : le multiplicateur cumulé d'une classe est borné à `[1 / b, b]`.
    #[arg(long, default_value_t = 4.0)]
    pub taste_mult_bound: f64,
}

/// L'empreinte de résultat de `base` au 4a (relevé `ord-loops-lot4-mesures.md`) : le crochet du goût est neutre si la
/// variante `base` la redonne, bit pour bit, aux défauts.
pub const BASE_4A_FINGERPRINT: &str = "55c3b065dc085f42";

impl LikenessArgs {
    pub fn taste_params(&self) -> TasteParams {
        TasteParams {
            m: self.taste_m,
            bound: self.taste_bound,
            min_km: self.taste_min_km,
            cell_m: self.taste_cell_m,
            corridor_cells: self.taste_corridor_cells,
            team_n0: self.taste_team_n0,
            mult_bound: self.taste_mult_bound,
        }
    }

    /// Les réglages du 4a : le seul cas où l'empreinte de `base` est comparable à celle du relevé.
    pub fn at_4a_defaults(&self) -> bool {
        self.limit.is_none()
            && self.max_km.is_none()
            && self.teams.is_empty()
            && self.cursors == [0.25, 1.0]
            && !self.no_holdout
            && self.min_km == 5.0
            && self.loop_gap == 0.05
            && self.min_matched == 0.95
            && self.max_hole_m == 1000.0
            && self.holdout_share == 0.10
            && self.holdout_min_loops == 10
            && self.seed == 20_261_117
            && self.radius_step_m == 10_000.0
    }
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
    // La sonde du goût : refus avant toute phase lourde. Sans `--variants`, une passe `base` et les sorties du 4a.
    let listed = variants::parse_variants(&a.variants)?;
    anyhow::ensure!(
        listed.iter().all(|v| !matches!(v.taste, Some((TasteSource::RecalTeam, _)))),
        "`rt` n'est pas encore branché (commit suivant)"
    );
    let legacy = listed.is_empty();
    let variants = if legacy { vec![Variant::base()] } else { listed };
    let tp = a.taste_params();
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
    let real_class_m: Vec<[f64; ord_loops::TASTE_CLASSES]> =
        loops.par_iter().map(|l| measure::class_lengths(gf, &l.walk, &l.trace)).collect();
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

    // 7 bis. Goût : les sommes par classe de chaque surcouche, complète et « écart » (si une variante en a besoin).
    let mut order: Vec<usize> =
        team_ids.iter().copied().filter(|&t| loops.iter().any(|l| l.team == t)).collect();
    order.sort_by(|&x, &y| letter_of[x].cmp(&letter_of[y]));
    let mut sums_full: Vec<ClassSums> = Vec::new();
    let mut sums_gap: Vec<Option<ClassSums>> = Vec::new();
    if variants.iter().any(|v| matches!(v.taste, Some((TasteSource::Team | TasteSource::General, _)))) {
        let (road, gravel) = (open_graph(&paths.road)?, open_graph(&paths.gravel)?);
        let ve = taste::ViewEdges::build([&road, &gravel], gf.header().edges as usize);
        for t in 0..overlays.len() {
            sums_full.push(taste::class_sums(gf, &ve, &overlays[t], &tp));
            sums_gap.push(gap_overlays[t].as_ref().map(|g| taste::class_sums(gf, &ve, g, &tp)));
        }
        drop(ve);
        ph.end("goût : sommes par classe");
    }

    // 8. Les jeux : les variantes l'une après l'autre ; équipe par équipe dans l'ordre des lettres.
    let wins = play::SharedWindows::new(&engine, a.budget_mb * 1_000_000, a.radius_step_m);
    let ctx =
        play::PlayCtx { engine: &engine, wins: &wins, iqr, real_shape: &real_shape, real_edges: &real_edges };
    let mut workspaces: Vec<Workspace> = (0..threads).map(|_| Workspace::new()).collect();
    let bench = Bench {
        args: a,
        gf,
        engine: &engine,
        graph_index: &graph_index,
        loops: &loops,
        held: &held,
        overlays: &overlays,
        gap_overlays: &gap_overlays,
        letter_of: &letter_of,
        order: &order,
        ctx: &ctx,
        params: &tp,
        sums_full: &sums_full,
        sums_gap: &sums_gap,
    };
    let mut retention: BTreeMap<String, export::RetentionCounts> = BTreeMap::new();
    for (t, c) in counts.into_iter().enumerate() {
        retention.insert(letter_of[t].clone(), c);
    }
    overlay_checks.sort_by(|x, y| x.team.cmp(&y.team));
    let vertex_histograms = vertex_histograms(&loops, &raw, &reals);
    let mut rc_inputs: Option<RcInputs> = None;
    let mut done: Vec<compare::Done> = Vec::new();
    std::fs::create_dir_all(&paths.out)?;
    for v in &variants {
        wins.take_team_counters();
        let w0 = wins.counters();
        let run = bench.play_variant(v, rc_inputs.as_ref(), &mut workspaces)?;
        ph.end(&if legacy { "jeux".to_string() } else { format!("variante {}", v.name) });
        let team_windows = wins.take_team_counters();
        let w1 = wins.counters();
        let windows = play::WindowCounters {
            built: w1.built - w0.built,
            hits: w1.hits - w0.hits,
            overflow: w1.overflow - w0.overflow,
            escapes: w1.escapes - w0.escapes,
        };
        if v.is_base() && rc_inputs.is_none() {
            rc_inputs = Some(RcInputs::of(&loops, &held, &real_class_m, &run.plays));
        }

        // 9. Les sorties de la variante.
        let mut plays = run.plays;
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
        let fingerprint = out::fingerprint(&recs);
        let mut tables = run.tables;
        tables.sort_by(|x, y| (&x.source, &x.overlay).cmp(&(&y.source, &y.overlay)));
        let last = ph.list.last().cloned();
        let variant = (!legacy).then(|| VariantMeta {
            name: v.name.clone(),
            fam: v.fam,
            fam_road_only: v.fam_road_only,
            taste: v.taste.map(|(s, _)| s.code().to_string()),
            gamma: v.taste.map(|(_, g)| g),
            params: tp.clone(),
            taste_xxh3: taste::taste_xxh3(&tables),
            tables,
            team_windows,
            secs: last.as_ref().map_or(0.0, |p| p.secs),
            peak_anon_mb: last.as_ref().map_or(0.0, |p| p.peak_anon_mb),
        });
        let mut meta =
            meta_lines(a, &engine, &ing, threads, &load0, &letter_of, t_all.elapsed().as_secs_f64(), &ph);
        if !legacy {
            meta.push(("variante".into(), v.name.clone()));
            meta.push(("goût, paramètres".into(), format!("{tp:?}")));
        }
        let report = out::Report {
            meta,
            retention: retention.clone(),
            calibration: calibration.clone(),
            vertex_histograms: vertex_histograms.clone(),
            overlays: overlay_checks.clone(),
            phases: ph.list.clone(),
            windows,
            variant,
            cursors: a.cursors.clone(),
            fingerprint,
            loops: recs,
        };
        let stem = if legacy { a.label.clone() } else { format!("{}-{}", a.label, v.name) };
        let json = paths.out.join(format!("{stem}-report.json"));
        let md = paths.out.join(format!("{stem}-summary.md"));
        std::fs::write(&json, serde_json::to_vec_pretty(&report)?)?;
        std::fs::write(&md, out::summary(&report))?;
        eprintln!(
            "rapport : {}\nrésumé : {}\nempreinte de résultat de {} : {}",
            json.display(),
            md.display(),
            v.name,
            report.fingerprint
        );
        let same = report.fingerprint == BASE_4A_FINGERPRINT;
        if let Some(meta) = &report.variant {
            // Gardé pour la comparaison (et pour `rc`, déjà tiré).
            done.push(compare::Done {
                variant: v.clone(),
                meta: meta.clone(),
                fingerprint: report.fingerprint.clone(),
                loops: report.loops,
            });
        }
        if v.is_base() && a.at_4a_defaults() {
            eprintln!("« `base` = 4a : {BASE_4A_FINGERPRINT} » : {}", if same { "oui" } else { "NON" });
            anyhow::ensure!(
                same || legacy,
                "l'empreinte de `base` n'est pas celle du 4a ({BASE_4A_FINGERPRINT}) : le crochet du goût n'est pas \
                 neutre, ou le banc a changé ; chercher, ne pas lire la suite"
            );
        }
    }
    if !legacy {
        let dirty = crate::git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
        let conds = compare::Conditions {
            commit: format!(
                "{}{}",
                crate::git(&["rev-parse", "--short", "HEAD"]).unwrap_or_default(),
                if dirty { " (arbre modifié)" } else { "" }
            ),
            cursors: a.cursors.clone(),
            params: tp.clone(),
            at_4a_defaults: a.at_4a_defaults(),
            base_4a_fingerprint: BASE_4A_FINGERPRINT.to_string(),
        };
        compare::write(&paths.out, &a.label, &conds, &done)?;
    }
    Ok(())
}

/// Ce que `rc` lit de `base` : par type, les mètres par classe du réel et ceux du **premier** parcours de `base` (mêmes
/// boucles non mises à l'écart, surcouche complète, tous les curseurs, jeux `OK`).
struct RcInputs {
    num: [Vec<f64>; 3],
    den: [Vec<f64>; 3],
}

impl RcInputs {
    fn of(
        loops: &[export::Loop],
        held: &std::collections::HashSet<usize>,
        real_class_m: &[[f64; ord_loops::TASTE_CLASSES]],
        plays: &[Vec<play::PlayRec>],
    ) -> Self {
        let z = || vec![0.0; ord_loops::TASTE_CLASSES];
        let (mut num, mut den) = ([z(), z(), z()], [z(), z(), z()]);
        for (i, l) in loops.iter().enumerate().filter(|(i, _)| !held.contains(i)) {
            let ty = usize::from(l.ty).min(2);
            for p in plays[i].iter().filter(|p| p.overlay == "full" && p.outcome == "OK") {
                let Some(fc) = &p.first_class_m else { continue };
                for c in 0..ord_loops::TASTE_CLASSES {
                    num[ty][c] += real_class_m[i][c];
                    den[ty][c] += fc[c];
                }
            }
        }
        Self { num, den }
    }
}

/// Tout ce que le jeu d'une variante lit.
struct Bench<'a> {
    args: &'a LikenessArgs,
    gf: &'a MappedGraph,
    engine: &'a Eng,
    graph_index: &'a GraphIndex,
    loops: &'a [export::Loop],
    held: &'a std::collections::HashSet<usize>,
    overlays: &'a [Overlay],
    gap_overlays: &'a [Option<Overlay>],
    letter_of: &'a [String],
    /// Les équipes jouées (qui ont des boucles), dans l'ordre des lettres.
    order: &'a [usize],
    ctx: &'a play::PlayCtx<'a>,
    params: &'a TasteParams,
    sums_full: &'a [ClassSums],
    sums_gap: &'a [Option<ClassSums>],
}

/// Ce qu'une variante rend : ses jeux (par boucle, dans l'ordre des boucles) et les tables de goût jouées.
struct VariantRun {
    plays: Vec<Vec<play::PlayRec>>,
    tables: Vec<TasteTable>,
}

impl Bench<'_> {
    /// Les jeux d'une variante : équipe par équipe dans l'ordre des lettres, exactement le §11.9 (8) du 4a — `build_team`,
    /// jeux sur la surcouche complète, puis jeux des boucles mises à l'écart sur la surcouche « écart » — avec la
    /// `Taste` de la variante pour cette équipe et cette surcouche (`None` si neutre ou sans goût) et les requêtes
    /// adaptées par `fam`. `drop_team` après chaque (équipe, surcouche) : aucun agrégat d'une variante n'est lu par la
    /// suivante ; les `Window` restent en cache d'une variante à l'autre.
    fn play_variant(
        &self,
        v: &Variant,
        rc: Option<&RcInputs>,
        workspaces: &mut [Workspace],
    ) -> Result<VariantRun> {
        let (a, gf) = (self.args, self.gf);
        let mut tables: Vec<TasteTable> = Vec::new();
        // Les goûts qui ne dépendent pas de l'équipe : général (sommes de toutes les équipes jouées) et recalé.
        let mut fixed: (Option<Taste>, Option<Taste>) = (None, None);
        match v.taste {
            Some((TasteSource::General, gamma)) => {
                let (mut full, mut gap) = (ClassSums::new(), ClassSums::new());
                for &t in self.order {
                    full.add(&self.sums_full[t]);
                    gap.add(self.sums_gap[t].as_ref().unwrap_or(&self.sums_full[t]));
                }
                let (tf, tbl) = taste::learn_from_sums("général", "full", &full, gamma, self.params);
                tables.extend(tbl);
                let (tg, tbl) = taste::learn_from_sums("général", "holdout", &gap, gamma, self.params);
                tables.extend(tbl);
                fixed = (tf, tg);
            }
            Some((TasteSource::Recal, gamma)) => {
                let rc = rc.ok_or_else(|| anyhow!("`rc` a besoin des jeux de `base`"))?;
                let (t, tbl) = taste::learn("recalé", "full", &rc.num, &rc.den, 1_000.0, gamma, self.params);
                tables.extend(tbl);
                fixed = (t.clone(), t);
            }
            _ => {}
        }
        let mut plays: Vec<Vec<play::PlayRec>> = self.loops.iter().map(|_| Vec::new()).collect();
        for &t in self.order {
            let mine: Vec<usize> = (0..self.loops.len()).filter(|&i| self.loops[i].team == t).collect();
            let tt = std::time::Instant::now();
            let tasks_of = |ids: &[usize]| -> Result<Vec<play::Task>> {
                let mut tasks: Vec<play::Task> = Vec::new();
                for &i in ids {
                    for &c in &a.cursors {
                        let req = variants::adapt_request(
                            play::request_of(gf, &self.loops[i], c, a.seed)?,
                            v.fam,
                            v.fam_road_only,
                        );
                        tasks.push(play::Task {
                            loop_idx: i,
                            cursor: c,
                            req,
                            ride: play::ride_of(self.loops[i].ty),
                        });
                    }
                }
                tasks.sort_by_key(play::locality_key);
                Ok(tasks)
            };
            // (surcouche, étiquette, clé du cache d'équipe, boucles jouées)
            let held_mine: Vec<usize> = mine.iter().copied().filter(|i| self.held.contains(i)).collect();
            let mut jobs: Vec<(&Overlay, &str, u64, &[usize])> =
                vec![(&self.overlays[t], "full", 2 * t as u64, &mine)];
            if let Some(gap) = &self.gap_overlays[t] {
                jobs.push((gap, "holdout", 2 * t as u64 + 1, &held_mine));
            }
            for (overlay, label, key, ids) in jobs {
                let taste: Option<Taste> = match v.taste {
                    Some((TasteSource::Team, gamma)) => {
                        let sums = if label == "full" {
                            &self.sums_full[t]
                        } else {
                            self.sums_gap[t].as_ref().expect("somme « écart » de l'équipe")
                        };
                        let (taste, tbl) =
                            taste::learn_from_sums(&self.letter_of[t], label, sums, gamma, self.params);
                        tables.extend(tbl);
                        taste
                    }
                    Some(_) => {
                        if label == "full" {
                            fixed.0.clone()
                        } else {
                            fixed.1.clone()
                        }
                    }
                    None => None,
                };
                let (team, team_check) =
                    build_team(self.engine, self.graph_index, overlay, &self.letter_of[t])?;
                let checker = Checker::with_tables(
                    gf,
                    Some(overlay),
                    Config::default(),
                    self.graph_index,
                    Some(&team_check),
                );
                let env = play::TeamEnv {
                    key,
                    overlay,
                    types: TypeIndex::of(overlay),
                    team: &team,
                    checker: &checker,
                    taste: taste.as_ref(),
                };
                let tasks = tasks_of(ids)?;
                for (k, rec) in play::run_tasks(self.ctx, &env, &tasks, label, workspaces) {
                    plays[tasks[k].loop_idx].push(rec);
                }
                self.ctx.wins.drop_team(env.key);
            }
            eprintln!(
                "{} : équipe {} : {} boucles, {:.1} s",
                v.name,
                self.letter_of[t],
                mine.len(),
                tt.elapsed().as_secs_f64()
            );
        }
        Ok(VariantRun { plays, tables })
    }
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
