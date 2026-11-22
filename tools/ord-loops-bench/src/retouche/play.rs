//! `retouche-play` (note : `ord-retouche-lot0.md` §7) : jouer `move_end` et `resize` sur les traces de l'export, avec
//! les surcouches complètes du lot 6, et contrôler chaque recollé.
//!
//! **Privé** : les sorties vont sous `~/.cache/osm-route-details/graph/retouche/`, les équipes y sont lettrées, une trace
//! y est désignée par `(lettre, rang)`, aucune position ni aucun id n'y est écrit. Seuls les agrégats du `.md` se recopient.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::{anyhow, Context as _, Result};
use clap::{Args, ValueEnum};
use ord_graph::overlay::Overlay;
use ord_graph::util::Poly;
use ord_graph_match::SegIndex;
use ord_loops::{Engine, Params, Taste, Workspace};
use ord_loops_check::{Checker, Config, GraphIndex};
use rayon::prelude::*;
use xxhash_rust::xxh3::xxh3_64;

use super::continuity::{Ctx, Shape};
use super::control::{Ctl, OrigCache};
use super::draw::{self, MoveEndDraw};
use super::move_end::{self, MeLine};
use super::posed::{self, Posed, SeamOpts, ViewInfo, ViewLookup};
use super::resize::{self, RsLine};
use super::{open, out_play, Cursor, Env, PathOpts};
use crate::likeness::export::{self, RetainParams};
use crate::likeness::holdout;
use crate::likeness::play::SharedWindows;
use crate::likeness::{build_team, read_letters, Phases};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Op {
    MoveEnd,
    Resize,
    Both,
}

/// Arguments de `retouche-play`. Les défauts sont ceux de la note du lot 0 (§7.1).
#[derive(Debug, Args)]
pub struct RetouchePlayArgs {
    #[command(flatten)]
    pub paths: PathOpts,
    /// Préfixe des fichiers de sortie : `<label>-move-end.jsonl`, `<label>-resize.jsonl`, `<label>-play.{json,md}`.
    #[arg(long, default_value = "play")]
    pub label: String,
    #[arg(long, value_enum, default_value = "both")]
    pub op: Op,
    /// Valeurs du curseur (0,25 : équipe d'abord ; 1 : découverte).
    #[arg(long, value_delimiter = ',', default_values_t = [0.25, 1.0])]
    pub cursors: Vec<f64>,
    /// Écarts `δ` de `resize`, en pour cent.
    #[arg(long, value_delimiter = ',', default_values_t = [-35.0, -25.0, -10.0, 10.0, 25.0, 35.0], allow_hyphen_values = true)]
    pub deltas: Vec<f64>,
    /// Fenêtres jouées au plus par demande de `resize`.
    #[arg(long, default_value_t = 12)]
    pub windows: usize,
    /// Rayons des points `E′` autour de l'arrivée, km.
    #[arg(long, value_delimiter = ',', default_values_t = [2.0, 5.0, 15.0])]
    pub radii_km: Vec<f64>,
    /// Caps tirés par rayon.
    #[arg(long, default_value_t = 4)]
    pub bearings: u8,
    #[arg(long)]
    pub no_team_ends: bool,
    #[arg(long)]
    pub no_close_loop: bool,
    /// Traces de `resize` au plus par équipe × type × forme.
    #[arg(long, default_value_t = 60)]
    pub sample_per_stratum: usize,
    /// `move_end` sur toutes les traces éligibles ; `false` : le même échantillon que `resize`.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub move_end_all: bool,
    /// Rejoue `resize` sans masque (à l'essai seulement : ce que le masque apporte).
    #[arg(long)]
    pub no_keep_out: bool,
    #[arg(long, default_value_t = 20_261_122)]
    pub seed: u64,
    /// Budget du cache de fenêtres partagé, en Mo.
    #[arg(long, default_value_t = 6000)]
    pub budget_mb: usize,
    /// Pas d'arrondi du rayon de classe des fenêtres, m.
    #[arg(long, default_value_t = 10_000.0)]
    pub radius_step_m: f64,
    #[arg(long, value_delimiter = ',')]
    pub teams: Vec<String>,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long)]
    pub threads: Option<usize>,
    /// Marge continue exigée de chaque côté d'une couture, m.
    #[arg(long, default_value_t = 200.0)]
    pub seam_margin_m: f64,
    #[arg(long, default_value_t = 15.0)]
    pub proj_tol_m: f64,
    /// Demi-largeur de la fenêtre de C2 autour d'une couture, m de graphe.
    #[arg(long, default_value_t = 5000.0)]
    pub c2_window_m: f64,
    /// Candidats contrôlés au plus par demande de `resize`.
    #[arg(long, default_value_t = 12)]
    pub max_controls: usize,
    /// Arrêt propre après ce nombre de minutes (les lignes déjà écrites restent).
    #[arg(long, default_value_t = 60.0)]
    pub max_minutes: f64,
}

#[derive(Default)]
struct Out {
    me: Vec<MeLine>,
    rs: Vec<RsLine>,
}

fn type_ok(t: u8) -> bool {
    t <= 2
}

pub fn run_play(a: &RetouchePlayArgs) -> Result<()> {
    let paths = a.paths.resolve()?;
    let cursors: Vec<Cursor> = a.cursors.iter().map(|&c| Cursor::new(c)).collect::<Result<_>>()?;
    let letters = read_letters(&paths.letters)?;
    let wanted: Vec<u64> = letters
        .iter()
        .filter(|(_, l)| a.teams.is_empty() || a.teams.contains(l))
        .map(|(id, _)| *id)
        .collect();
    anyhow::ensure!(!wanted.is_empty(), "aucune équipe ne correspond à --teams");
    let threads =
        a.threads.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get())).max(1);
    let started = Instant::now();
    let mut ph = Phases::new();

    // 1. Le moteur fenêtré et l'index du contrôleur.
    let engine = Engine::new(
        open(&paths.graph)?,
        open(&paths.road)?,
        open(&paths.gravel)?,
        // Toujours le poids de base de 1.9.2, comme `likeness` : les relevés passés se rejouent tels quels.
        Params { base_taste: Taste::neutral(), ..Params::default() },
    )
    .map_err(|e| anyhow!("moteur : {e}"))?;
    let gf = engine.base();
    let graph_index = GraphIndex::build(gf);
    let (road, gravel) = (open(&paths.road)?, open(&paths.gravel)?);
    let n_edges = gf.header().edges as usize;
    let views = [ViewInfo::build(&road, n_edges), ViewInfo::build(&gravel, n_edges)];
    ph.end("moteur, index du contrôleur et masques des vues");

    // 2. L'export, l'appariement et la pose de chaque trace.
    let mut ing = export::ingest(&paths.export, &wanted)?;
    let letter_of: Vec<String> = ing
        .teams
        .iter()
        .map(|id| letters.get(id).cloned().ok_or_else(|| anyhow!("équipe sans lettre")))
        .collect::<Result<_>>()?;
    ph.end("lecture de l'export");
    let poly = Poly::read(&paths.poly).with_context(|| paths.poly.display().to_string())?;
    let seg = SegIndex::build(gf);
    let jobs = export::match_keys(gf, &seg, &poly, &ing);
    drop(seg);
    ph.end("appariement");
    let mut next = vec![0u32; ing.teams.len()];
    let mut todo: Vec<(usize, u32)> = Vec::new();
    for (idx, t) in ing.tracks.iter().enumerate() {
        let rank = next[t.team];
        next[t.team] += 1;
        if a.limit.is_none_or(|n| (rank as usize) < n) {
            todo.push((idx, rank));
        }
    }
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
    let posed_all: Vec<Option<Posed>> = {
        let ctx = Ctx {
            gf,
            ing: &ing,
            jobs: &jobs,
            views: [&views[0] as &dyn ViewLookup, &views[1] as &dyn ViewLookup],
            opts: SeamOpts { margin_m: a.seam_margin_m, touch_continues: false },
            proj_tol_m: a.proj_tol_m,
            retain: RetainParams::default(),
        };
        pool.install(|| {
            todo.par_iter()
                .map(|&(idx, rank)| posed::pose(&ctx, idx, rank, &letter_of[ing.tracks[idx].team]))
                .collect()
        })
    };
    let export_xxh3 = ing.export_xxh3;
    let team_ids = ing.teams.clone();
    let rows_read = todo.len();
    ing.keys = Vec::new();
    drop(jobs);
    ph.end("pose des traces");

    // 3. Les populations.
    let mut by_team: BTreeMap<usize, Vec<&Posed>> = BTreeMap::new();
    for p in posed_all.iter().flatten() {
        by_team.entry(p.team).or_default().push(p);
    }
    let draw_cfg = MoveEndDraw {
        radii_km: a.radii_km.clone(),
        bearings: a.bearings,
        team_ends: !a.no_team_ends,
        close_loop: !a.no_close_loop,
    };
    let do_me = matches!(a.op, Op::MoveEnd | Op::Both);
    let do_rs = matches!(a.op, Op::Resize | Op::Both);
    // Échantillon de `resize` : par équipe × type × forme, au rang de `xxh3(id_hash ^ seed)`.
    let eligible_rs = |p: &Posed| {
        type_ok(p.ty) && p.l_m >= 5000.0 && {
            let n = p
                .both(a.seam_margin_m)
                .into_iter()
                .filter(|&k| p.seams[k].s_m >= 0.05 * p.l_m && p.seams[k].s_m <= 0.95 * p.l_m)
                .count();
            n >= 2
        }
    };
    let mut weight_of: BTreeMap<(usize, usize), f64> = BTreeMap::new(); // (team, rank) -> poids
    let mut strata: BTreeMap<(usize, u8, bool), Vec<&Posed>> = BTreeMap::new();
    for ps in by_team.values() {
        for p in ps {
            if eligible_rs(p) {
                strata.entry((p.team, p.ty, p.shape == Shape::Loop)).or_default().push(p);
            }
        }
    }
    let mut strata_report: Vec<(String, usize, usize)> = Vec::new();
    for ((_, ty, is_loop), mut v) in strata {
        let pop = v.len();
        v.sort_by_key(|p| draw::sample_rank(p.id_hash, a.seed));
        v.truncate(a.sample_per_stratum);
        let w = pop as f64 / v.len() as f64;
        strata_report.push((
            format!(
                "{}/{}/{}",
                v[0].letter,
                super::continuity::type_name(ty),
                if is_loop { "loop" } else { "one_way" }
            ),
            pop,
            v.len(),
        ));
        for p in v {
            weight_of.insert((p.team, p.rank as usize), w);
        }
    }
    let n_sample = weight_of.len();
    let n_me_eligible =
        posed_all.iter().flatten().filter(|p| type_ok(p.ty) && !p.before(a.seam_margin_m).is_empty()).count();
    eprintln!(
        "traces posées : {} sur {} lignes ; éligibles à move_end : {} ; échantillon de resize : {}",
        posed_all.iter().flatten().count(),
        rows_read,
        n_me_eligible,
        n_sample
    );

    // 4. Les jeux, équipe par équipe dans l'ordre des lettres.
    let wins = SharedWindows::new(&engine, a.budget_mb * 1_000_000, a.radius_step_m);
    let deadline = started + std::time::Duration::from_secs_f64(a.max_minutes * 60.0);
    let cut = AtomicBool::new(false);
    let mut order: Vec<usize> = by_team.keys().copied().collect();
    order.sort_by(|&x, &y| letter_of[x].cmp(&letter_of[y]));
    let total_tracks: usize = by_team.values().map(Vec::len).sum();
    let done = AtomicUsize::new(0);
    let mut all_me: Vec<MeLine> = Vec::new();
    let mut all_rs: Vec<RsLine> = Vec::new();
    let mut team_secs: Vec<(String, usize, f64)> = Vec::new();
    for t in order {
        let tt = Instant::now();
        let letter = &letter_of[t];
        let Some(file) = holdout::overlay_file(&paths.overlays, team_ids[t]) else {
            eprintln!("équipe {letter} : pas de surcouche, sautée");
            continue;
        };
        let overlay =
            Overlay::read(&file).map_err(|e| anyhow!("équipe {letter} : surcouche illisible : {e}"))?;
        let (team, team_check) = build_team(&engine, &graph_index, &overlay, letter)?;
        let checker =
            Checker::with_tables(gf, Some(&overlay), Config::default(), &graph_index, Some(&team_check));
        let env = Env {
            engine: &engine,
            wins: &wins,
            team_key: t as u64,
            team: &team,
            ctl: Ctl { gf, checker: &checker, c2_window_m: a.c2_window_m },
            seed: a.seed,
            margin_m: a.seam_margin_m,
            max_windows: a.windows,
            max_controls: a.max_controls,
            no_keep_out: a.no_keep_out,
        };
        // Les plus longues d'abord : une trace de 400 km occupe un fil des minutes, et seule la fin de la file la ferait
        // attendre. L'ordre des sorties reste celui des rangs.
        let mut tracks: Vec<&Posed> = by_team[&t].clone();
        tracks.sort_by(|x, y| y.l_m.total_cmp(&x.l_m).then(x.rank.cmp(&y.rank)));
        // Les arrivées des traces de l'équipe : le lieu réel de l'essai `team_end`.
        let ends: Vec<(u32, (f64, f64))> =
            tracks.iter().filter_map(|p| p.line.pts.last().map(|&e| (p.rank, e))).collect();
        let outs: Vec<Out> = pool.install(|| {
            tracks
                .par_iter()
                .map_init(Workspace::new, |ws, p| {
                    if Instant::now() > deadline {
                        cut.store(true, Ordering::Relaxed);
                        return Out::default();
                    }
                    let cache = OrigCache::default();
                    let mut out = Out::default();
                    let weight = weight_of.get(&(p.team, p.rank as usize)).copied();
                    if do_me
                        && (a.move_end_all || weight.is_some())
                        && type_ok(p.ty)
                        && !p.before(a.seam_margin_m).is_empty()
                    {
                        let trials =
                            draw::move_end_trials(p, &ends, &draw_cfg, a.seed, p.shape == Shape::OneWay);
                        for cur in &cursors {
                            for tr in &trials {
                                out.me.push(move_end::play_trial(&env, p, cur, tr, ws, &cache));
                            }
                        }
                    }
                    if let (true, Some(w)) = (do_rs, weight) {
                        for cur in &cursors {
                            for &d in &a.deltas {
                                out.rs.push(resize::play_request(&env, p, cur, d / 100.0, w, ws, &cache));
                            }
                        }
                    }
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_multiple_of((total_tracks / 20).max(1)) || n == (total_tracks / 50).max(1) {
                        let el = started.elapsed().as_secs_f64();
                        eprintln!(
                            "  {n}/{total_tracks} traces, {:.0} s, reste ~{:.0} s",
                            el,
                            el / n as f64 * (total_tracks - n) as f64
                        );
                    }
                    ws.shrink_to(1_500_000_000);
                    out
                })
                .collect()
        });
        let mut outs: Vec<(u32, Out)> = tracks.iter().map(|p| p.rank).zip(outs).collect();
        outs.sort_by_key(|(rank, _)| *rank);
        for (_, o) in outs {
            all_me.extend(o.me);
            all_rs.extend(o.rs);
        }
        wins.drop_team(env.team_key);
        let secs = tt.elapsed().as_secs_f64();
        eprintln!("équipe {letter} : {} traces, {secs:.1} s", tracks.len());
        team_secs.push((letter.clone(), tracks.len(), secs));
    }
    ph.end("jeux");

    // 5. Les sorties.
    std::fs::create_dir_all(&paths.out)?;
    let me_jsonl = out_play::jsonl(&all_me)?;
    let rs_jsonl = out_play::jsonl(&all_rs)?;
    let fp = |lines: &str| {
        let mut h = Vec::new();
        for l in lines.lines() {
            if let Some(i) = l.find("\"fp\":\"") {
                h.extend_from_slice(l[i + 6..].split('"').next().unwrap_or("").as_bytes());
            }
        }
        format!("{:016x}", xxh3_64(&h))
    };
    let wc = wins.counters();
    let meta = vec![
        ("empreinte xxh3 de l'export".to_string(), format!("{export_xxh3:016x}")),
        ("empreinte des recollés `move_end`".to_string(), fp(&me_jsonl)),
        ("empreinte des recollés `resize`".to_string(), fp(&rs_jsonl)),
        ("lignes de l'export lues".to_string(), rows_read.to_string()),
        ("traces éligibles à move_end".to_string(), n_me_eligible.to_string()),
        ("échantillon de resize".to_string(), n_sample.to_string()),
        ("essais move_end".to_string(), all_me.len().to_string()),
        ("demandes resize".to_string(), all_rs.len().to_string()),
        ("curseurs".to_string(), format!("{:?}", a.cursors)),
        ("δ (%)".to_string(), format!("{:?}", a.deltas)),
        ("masque des arêtes gardées".to_string(), (!a.no_keep_out).to_string()),
        ("marge de couture".to_string(), format!("{} m", a.seam_margin_m)),
        ("fenêtre de C2".to_string(), format!("{} m", a.c2_window_m)),
        ("fils".to_string(), threads.to_string()),
        ("durée".to_string(), format!("{:.0} s", started.elapsed().as_secs_f64())),
        ("fenêtres bâties / lues au cache".to_string(), format!("{} / {}", wc.built, wc.hits)),
        (
            "portée".to_string(),
            format!(
                "{} ; {}",
                if a.teams.is_empty() {
                    "toutes les équipes".to_string()
                } else {
                    format!("équipes {}", a.teams.join(","))
                },
                a.limit.map_or("toutes les traces".to_string(), |n| format!("{n} traces par équipe au plus"))
            ),
        ),
        (
            "strates de resize (population / échantillon, par lettre, type et forme)".to_string(),
            strata_report.iter().map(|(k, p, n)| format!("{k}:{p}/{n}")).collect::<Vec<_>>().join(" "),
        ),
    ];
    let summary = out_play::summarise(meta, &all_me, &all_rs, cut.load(Ordering::Relaxed));
    let base = |ext: &str| paths.out.join(format!("{}-{ext}", a.label));
    std::fs::write(base("move-end.jsonl"), &me_jsonl)?;
    std::fs::write(base("resize.jsonl"), &rs_jsonl)?;
    std::fs::write(base("play.json"), serde_json::to_string_pretty(&summary.json)?)?;
    std::fs::write(base("play.md"), &summary.md)?;
    ph.end("sorties");
    eprintln!("sorties : {}", paths.out.display());
    Ok(())
}
