//! `route-play` : banc du calcul d'itinéraire de A à B sur des tronçons de traces réelles (lot 1 bis du routage, note
//! `docs/design/technique/ord-route-lot1bis-mesures.md`). Une **mesure** : aucun poids du moteur n'est changé.
//!
//! Chaque trace posée (`retouche::posed::pose`) donne jusqu'à trois tronçons, un par bande de vol d'oiseau ; chaque
//! tronçon est routé sans équipe, en découverte, avec des préférences de voie (`way_q8`) différentes, et l'itinéraire
//! est comparé à la sous-marche réelle (recouvrement en longueur, Jaccard, kilomètres par case).
//!
//! Trois phases : (1) neutre, `avoid_main_roads`, balayage case par case, crans du curseur ; (2) `best1` (le meilleur
//! multiplicateur de chaque case sur la moitié A) et ses voisins d'un cran ; (3) `best2`. La fenêtre d'un tronçon est
//! bâtie une fois par phase et rejouée pour toutes les variantes de la phase.
//!
//! **Privé** : l'export, les coordonnées, les ids de trace et d'équipe sont des données d'équipes réelles. Les sorties
//! vont sous `~/.cache/osm-route-details/graph/route-bench/` et nulle part ailleurs ; les équipes y sont lettrées, une
//! trace y est désignée par `(lettre, rang)`, aucune position ni aucun id n'y est écrit. Seuls les agrégats se recopient.

pub mod agg;
pub mod draw;
pub mod md;
pub mod overlap;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::{anyhow, Context as _, Result};
use clap::Args;
use ord_graph::attrs::HighwayClass;
use ord_graph::mmap::MappedGraph;
use ord_graph::util::Poly;
use ord_graph_match::geo::haversine;
use ord_graph_match::walk::{haversine_len, Join};
use ord_graph_match::SegIndex;
use ord_loops::{
    avoid_main_roads, route_in, way_factors, Engine, LatLon, Mode, Params, RideType, RouteFailure,
    RouteRequest, RouteTune, Routed, Window, WindowPlan, Workspace, WAY_NEUTRAL,
};
use rayon::prelude::*;
use serde::Serialize;
use xxhash_rust::xxh3::xxh3_64;

use self::agg::Obs;
use self::draw::{DrawRules, JoinKind, Pick, WalkView, BANDS};
use self::overlap::{Piece, Side};
use crate::likeness::export::{self, RetainParams};
use crate::likeness::play::Eng;
use crate::likeness::{read_letters, Phases};
use crate::retouche::continuity::Ctx;
use crate::retouche::posed::{self, Posed, SeamOpts, ViewInfo, ViewLookup};
use crate::retouche::PathOpts;

/// Une préférence de voie : un multiplicateur en 256es par case.
pub type WayQ8 = [u16; 7];

/// Les multiplicateurs du balayage (×0,5, ×0,71, ×1,41, ×2, ×3) et la liste des crans de la descente.
pub const SWEEP: [u16; 5] = [128, 182, 361, 512, 768];
pub const LADDER: [u16; 6] = [128, 182, 256, 361, 512, 768];
/// Les crans du curseur `use_roads`.
pub const CURSOR_U: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
/// Noms des cases (ordre de `way_q8`).
pub const CASES: [&str; 7] =
    ["grande route", "tertiary", "petite route", "rue", "cyclable ou verte", "chemin", "secondary"];

/// Arguments de `route-play`.
#[derive(Debug, Args)]
pub struct RoutePlayArgs {
    #[command(flatten)]
    pub paths: PathOpts,
    /// Préfixe des fichiers de sortie : `<label>-lines.jsonl`, `<label>-segments.jsonl`, `<label>-route.{json,md}`.
    #[arg(long, default_value = "route")]
    pub label: String,
    /// Tronçons gardés au plus par trace (parmi les trois bandes, au hasard de la graine).
    #[arg(long, default_value_t = 3)]
    pub per_track: usize,
    #[arg(long, default_value_t = 20_261_124)]
    pub seed: u64,
    /// Équipes (lettres) ; toutes par défaut.
    #[arg(long, value_delimiter = ',')]
    pub teams: Vec<String>,
    /// Au plus ce nombre de traces par équipe (les premières dans l'ordre de l'export).
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long)]
    pub threads: Option<usize>,
    /// Arrêt propre après ce nombre de minutes (les recherches restantes sont des échecs `stopped`).
    #[arg(long, default_value_t = 60.0)]
    pub max_minutes: f64,
    /// Phase 1 seulement (sans `best1`, sans descente, sans `best2`).
    #[arg(long)]
    pub no_tune: bool,
}

/// Nom d'un type de sortie.
pub fn ride_name(r: RideType) -> &'static str {
    match r {
        RideType::Road => "road",
        RideType::Gravel => "gravel",
        RideType::Mtb => "mtb",
    }
}

fn fail_name(f: RouteFailure) -> &'static str {
    match f {
        RouteFailure::StartTooFar => "start_too_far",
        RouteFailure::EndTooFar => "end_too_far",
        RouteFailure::TooLong => "too_long",
        RouteFailure::NoRoute => "no_route",
        RouteFailure::NoTeamData => "no_team_data",
        RouteFailure::Stopped => "stopped",
        RouteFailure::WindowEscape => "window_escape",
    }
}

/// Case de voie d'une arête de base : celle du moteur (`way_index` de `route.rs`, privé) : `secondary` à part.
fn way_of(gf: &MappedGraph, e: u32) -> usize {
    let a = gf.edge(e).attrs;
    if a.class() == Some(HighwayClass::Secondary) {
        6
    } else {
        ord_loops::taste_way(a)
    }
}

fn edge_len_m(gf: &MappedGraph, e: u32) -> f64 {
    f64::from(gf.edge(e).len_dm) / 10.0
}

/// Un tronçon et sa référence.
#[derive(Debug, Clone, Serialize)]
pub struct Seg {
    pub team: String,
    pub rank: u32,
    pub band: &'static str,
    pub half: char,
    pub ride: &'static str,
    pub crow_m: f64,
    /// Longueur parcourue de la sous-marche (jambes), m, et longueur de sa réunion par arête.
    pub l_ref_m: f64,
    pub ref_union_m: f64,
    pub hole_m: f64,
    pub ref_km: [f64; 7],
    pub ref_offroad_km: f64,
    /// Kilomètres de la référence sur des arêtes hors de la vue du type (que le moteur ne peut pas prendre).
    pub ref_off_view_km: f64,
    /// Zone de la recherche (0 ou 1), et l'échec commun à toutes les variantes s'il y en a un (accroche, aucun chemin).
    pub zone: u8,
    pub fail: Option<&'static str>,
    #[serde(skip)]
    pub band_i: usize,
    #[serde(skip)]
    pub cluster: usize,
    #[serde(skip)]
    pub ride_t: Option<RideType>,
    #[serde(skip)]
    pub start: (i32, i32),
    #[serde(skip)]
    pub end: (i32, i32),
    #[serde(skip)]
    pub reference: Side,
}

/// Ce qu'une ligne a de reproductible (l'empreinte du jsonl ne lit que cela).
#[derive(Debug, Clone, Serialize)]
pub struct Core {
    pub seg: usize,
    pub team: String,
    pub rank: u32,
    pub band: &'static str,
    pub half: char,
    pub ride: &'static str,
    pub phase: u8,
    pub variant: String,
    pub q8: WayQ8,
    pub ok: bool,
    pub fail: Option<&'static str>,
    pub l_m: f64,
    pub route_union_m: f64,
    pub common_m: f64,
    pub jaccard: f64,
    pub precision: f64,
    pub recall: f64,
    pub detour: f64,
    pub km: [f64; 7],
    pub offroad_km: f64,
    pub sharp_relaxed: bool,
    pub enlarged: bool,
    pub settled: u64,
    /// Empreinte xxh3 des pas de l'itinéraire.
    pub fp: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Line {
    #[serde(flatten)]
    pub core: Core,
    /// Temps de la recherche, à chaud (la fenêtre a déjà servi à une recherche neutre), ms.
    pub ms: f64,
}

/// Une variante : nom et préférence.
#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
    pub q8: WayQ8,
}

fn var(name: impl Into<String>, q8: WayQ8) -> Variant {
    Variant { name: name.into(), q8 }
}

fn with_case(base: WayQ8, k: usize, q: u16) -> WayQ8 {
    let mut v = base;
    v[k] = q;
    v
}

/// Les variantes de la phase 1 pour un type.
pub fn phase1_variants(ride: RideType) -> Vec<Variant> {
    let mut v = vec![var("neutral", WAY_NEUTRAL), var("avoid", avoid_main_roads(WAY_NEUTRAL))];
    for k in 0..7 {
        for &q in &SWEEP {
            v.push(var(format!("sweep:c{k}:{q}"), with_case(WAY_NEUTRAL, k, q)));
        }
    }
    for &u in &CURSOR_U {
        v.push(var(format!("cursor:u{u:.2}:d0.25"), way_factors(ride, u, 0.25)));
    }
    if ride == RideType::Road {
        for &u in &CURSOR_U {
            v.push(var(format!("cursor:u{u:.2}:d1.00"), way_factors(ride, u, 1.0)));
        }
    }
    v
}

/// Les voisins d'un cran de `best`, case par case, dans [`LADDER`].
pub fn neighbours(best: WayQ8) -> Vec<Variant> {
    let mut v = Vec::new();
    for k in 0..7 {
        let Some(i) = LADDER.iter().position(|&q| q == best[k]) else { continue };
        if i + 1 < LADDER.len() {
            v.push(var(format!("desc:c{k}:{}", LADDER[i + 1]), with_case(best, k, LADDER[i + 1])));
        }
        if i > 0 {
            v.push(var(format!("desc:c{k}:{}", LADDER[i - 1]), with_case(best, k, LADDER[i - 1])));
        }
    }
    v
}

/// `best1` : pour chaque case, le multiplicateur du balayage (256 compris) de plus grand score ; à égalité, 256.
pub fn pick_best1(score: &dyn Fn(usize, u16) -> f64) -> WayQ8 {
    let mut b = WAY_NEUTRAL;
    for (k, slot) in b.iter_mut().enumerate() {
        let mut best = (256u16, score(k, 256));
        for &q in &SWEEP {
            let s = score(k, q);
            if s > best.1 {
                best = (q, s);
            }
        }
        *slot = best.0;
    }
    b
}

/// `best2` : pour chaque case, le cran voisin de `best1` qui améliore le score de `best1`, s'il y en a un.
pub fn pick_best2(best1: WayQ8, s_best1: f64, score: &dyn Fn(usize, u16) -> Option<f64>) -> WayQ8 {
    let mut b = best1;
    for (k, slot) in b.iter_mut().enumerate() {
        let mut best = (best1[k], s_best1);
        for &q in &LADDER {
            if let Some(s) = score(k, q) {
                if s > best.1 {
                    best = (q, s);
                }
            }
        }
        *slot = best.0;
    }
    b
}

/// Les fenêtres d'un tronçon pour une phase : celle de la zone, et l'élargie (bâtie au premier échappement).
struct SegWin<'e> {
    engine: &'e Eng,
    r: RouteRequest,
    reach: f64,
    plan: WindowPlan,
    win: Window,
    enlarged: Option<Window>,
}

impl<'e> SegWin<'e> {
    fn build(engine: &'e Eng, r: RouteRequest, reach: f64, stop: &mut dyn FnMut() -> bool) -> Option<Self> {
        let plan = engine.plan_route(&r, reach, 0.0, 0.0);
        let win = engine.build_window(plan.view, &plan.need, stop)?;
        Some(Self { engine, r, reach, plan, win, enlarged: None })
    }

    /// Route dans la fenêtre ; sur un échappement, rejoue une fois dans la fenêtre élargie (comme `ord_loops::route`).
    fn run(
        &mut self,
        q8: WayQ8,
        ws: &mut Workspace,
        stop: &mut dyn FnMut() -> bool,
    ) -> (Result<Routed, RouteFailure>, bool) {
        let r = RouteRequest { way_q8: q8, ..self.r };
        let tune = RouteTune::default();
        let first = route_in(self.engine, &self.win, None, &r, &tune, self.reach, ws, stop);
        if !matches!(first, Err(RouteFailure::WindowEscape)) {
            return (first, false);
        }
        if self.enlarged.is_none() {
            let plan = self.engine.plan_route(&r, self.reach, 0.0, self.plan.snap_lmax_m + 10_000.0);
            match self.engine.build_window(plan.view, &plan.need, stop) {
                Some(w) => self.enlarged = Some(w),
                None => return (Err(RouteFailure::Stopped), true),
            }
        }
        let w = self.enlarged.as_ref().expect("fenêtre élargie bâtie");
        (route_in(self.engine, w, None, &r, &tune, self.reach, ws, stop), true)
    }
}

/// Ce qu'une phase rend pour un tronçon : ses lignes, sa zone et l'échec commun.
type SegOut = (Vec<Line>, u8, Option<&'static str>);

/// Ce que les phases lisent.
struct Env<'a> {
    engine: &'a Eng,
    gf: &'a MappedGraph,
    deadline: Instant,
    cut: &'a AtomicBool,
}

impl Env<'_> {
    fn request(&self, s: &Seg) -> RouteRequest {
        RouteRequest {
            start: LatLon::from_e7(s.start),
            end: LatLon::from_e7(s.end),
            ride_type: s.ride_t.expect("type du tronçon"),
            mode: Mode::Discovery,
            familiarity: 0.0,
            off_team_cost_q8: ord_loops::DEFAULT_OFF_TEAM_COST_Q8,
            way_q8: WAY_NEUTRAL,
        }
    }

    fn line(&self, si: usize, s: &Seg, phase: u8, v: &Variant) -> Line {
        Line {
            core: Core {
                seg: si,
                team: s.team.clone(),
                rank: s.rank,
                band: s.band,
                half: s.half,
                ride: s.ride,
                phase,
                variant: v.name.clone(),
                q8: v.q8,
                ok: false,
                fail: s.fail,
                l_m: 0.0,
                route_union_m: 0.0,
                common_m: 0.0,
                jaccard: 0.0,
                precision: 0.0,
                recall: 0.0,
                detour: 0.0,
                km: [0.0; 7],
                offroad_km: 0.0,
                sharp_relaxed: false,
                enlarged: false,
                settled: 0,
                fp: String::new(),
            },
            ms: 0.0,
        }
    }

    fn measure(&self, line: &mut Line, s: &Seg, out: &Result<Routed, RouteFailure>) {
        let c = &mut line.core;
        match out {
            Err(f) => {
                c.ok = false;
                c.fail = Some(fail_name(*f));
            }
            Ok(o) => {
                let gf = self.gf;
                let rt = &o.route;
                let n = rt.steps.len();
                let pieces: Vec<Piece> = rt
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(i, st)| {
                        let g = haversine_len(gf, st.base_edge);
                        let (a, b) =
                            overlap::step_fractions(i, n, st.reverse, rt.start.offset_m, rt.end.offset_m, g);
                        Piece::new(st.base_edge, a, b)
                    })
                    .collect();
                let len = |e: u32| edge_len_m(gf, e);
                let ov = overlap::overlap(&Side::of(&pieces), &s.reference, &len);
                let (km, off) =
                    overlap::km_by_way(&pieces, &len, &|e| way_of(gf, e), &|e| gf.edge(e).attrs.offroad());
                let mut h = Vec::with_capacity(n * 5);
                for st in &rt.steps {
                    h.extend_from_slice(&st.base_edge.to_le_bytes());
                    h.push(u8::from(st.reverse));
                }
                c.ok = true;
                c.fail = None;
                c.l_m = rt.length_m;
                c.route_union_m = ov.route_m;
                c.common_m = ov.common_m;
                c.jaccard = ov.jaccard();
                c.precision = ov.precision();
                c.recall = ov.recall();
                c.detour = if s.l_ref_m > 0.0 { rt.length_m / s.l_ref_m } else { f64::NAN };
                c.km = km;
                c.offroad_km = off;
                c.sharp_relaxed = o.sharp_relaxed;
                c.settled = o.settled;
                c.fp = format!("{:016x}", xxh3_64(&h));
            }
        }
    }

    /// Joue une phase sur un tronçon. Phase 1 : fixe la zone et l'échec commun (renvoyés).
    fn play_seg(&self, si: usize, s: &Seg, phase: u8, variants: &[Variant], ws: &mut Workspace) -> SegOut {
        let deadline = self.deadline;
        let cut = self.cut;
        let mut stop = || {
            let late = Instant::now() > deadline;
            if late {
                cut.store(true, Ordering::Relaxed);
            }
            late
        };
        let fail_all = |f: &'static str| -> Vec<Line> {
            variants
                .iter()
                .map(|v| {
                    let mut l = self.line(si, s, phase, v);
                    l.core.fail = Some(f);
                    l
                })
                .collect()
        };
        if phase > 1 {
            if let Some(f) = s.fail {
                return (fail_all(f), s.zone, s.fail);
            }
        }
        let r = self.request(s);
        let tune = RouteTune::default();
        let zones: Vec<u8> = if phase == 1 { vec![0, 1] } else { vec![s.zone] };
        let mut chosen: Option<SegWin<'_>> = None;
        let mut zone = 0u8;
        for z in zones {
            let reach = r.reach_m(&tune, usize::from(z)).expect("pas de zone");
            let Some(mut sw) = SegWin::build(self.engine, r, reach, &mut stop) else {
                return (fail_all("stopped"), z, Some("stopped"));
            };
            // Recherche neutre de chauffe : elle fixe aussi la zone (l'existence d'un chemin et l'accroche ne
            // dépendent pas de la préférence de voie, qui ne rend aucune chaîne interdite).
            let (warm, _) = sw.run(WAY_NEUTRAL, ws, &mut stop);
            match warm {
                Ok(_) => {
                    chosen = Some(sw);
                    zone = z;
                    break;
                }
                Err(RouteFailure::NoRoute) if phase == 1 && z == 0 => continue,
                Err(f) if phase == 1 => {
                    let n = fail_name(f);
                    return (fail_all(n), z, Some(n));
                }
                Err(_) => {
                    // Hors phase 1 (ne devrait pas arriver) : on joue quand même, chaque variante dira son échec.
                    chosen = Some(sw);
                    zone = z;
                    break;
                }
            }
        }
        let Some(mut sw) = chosen else { return (fail_all("no_route"), 1, Some("no_route")) };
        let mut lines = Vec::with_capacity(variants.len());
        for v in variants {
            let t = Instant::now();
            let (out, enlarged) = sw.run(v.q8, ws, &mut stop);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            let mut l = self.line(si, s, phase, v);
            l.ms = ms;
            l.core.enlarged = enlarged;
            self.measure(&mut l, s, &out);
            lines.push(l);
        }
        (lines, zone, None)
    }
}

fn open(p: &std::path::Path) -> Result<MappedGraph> {
    MappedGraph::open_mmap(p).with_context(|| p.display().to_string())
}

/// Les tronçons d'une trace posée.
fn segments_of(
    gf: &MappedGraph,
    view: &dyn ViewLookup,
    p: &Posed,
    cluster: usize,
    per_track: usize,
    seed: u64,
) -> Vec<Seg> {
    let legs = &p.walk.legs;
    let leg_m: Vec<f64> = legs.iter().map(|l| posed::leg_len_m(gf, l)).collect();
    let joins: Vec<JoinKind> = p
        .walk
        .joins
        .iter()
        .enumerate()
        .map(|(j, k)| match k {
            Join::Node(_) if view.has_edge(legs[j].edge) && view.has_edge(legs[j + 1].edge) => JoinKind::Node,
            Join::Node(_) => JoinKind::NodeOffView,
            Join::Touch => JoinKind::Touch,
            Join::Hole(d) => JoinKind::Hole(*d),
        })
        .collect();
    let node = |j: usize| match p.walk.joins[j] {
        Join::Node(n) => gf.node_pos(n),
        _ => unreachable!("un bout de tronçon est un nœud"),
    };
    let deg = |q: (i32, i32)| [f64::from(q.0) * 1e-7, f64::from(q.1) * 1e-7];
    let crow = |i: usize, k: usize| haversine(deg(node(i)), deg(node(k)));
    let w = WalkView { leg_m: &leg_m, joins: &joins };
    let mut picks: Vec<Pick> = draw::draw(&w, &crow, &BANDS, &DrawRules::default(), seed, p.id_hash);
    if picks.len() > per_track {
        // Un sous-ensemble reproductible, dans l'ordre des bandes.
        picks.sort_by_key(|pk| draw::mix(seed ^ 0x6b65_6570, p.id_hash, pk.band as u64, 0));
        picks.truncate(per_track);
        picks.sort_by_key(|pk| pk.band);
    }
    let half = draw::half_of(p.id_hash);
    picks
        .into_iter()
        .map(|pk| {
            let pieces: Vec<Piece> =
                legs[pk.j_start + 1..=pk.j_end].iter().map(|l| Piece::new(l.edge, l.lo, l.hi)).collect();
            let len = |e: u32| edge_len_m(gf, e);
            let (ref_km, ref_off) =
                overlap::km_by_way(&pieces, &len, &|e| way_of(gf, e), &|e| gf.edge(e).attrs.offroad());
            let off_view: f64 = pieces
                .iter()
                .filter(|pc| !view.has_edge(pc.edge))
                .map(|pc| (pc.hi - pc.lo) * len(pc.edge) / 1000.0)
                .sum();
            let reference = Side::of(&pieces);
            let ref_union_m = reference.len_m(&len);
            Seg {
                team: p.letter.clone(),
                rank: p.rank,
                band: draw::band_name(pk.band),
                half,
                ride: ride_name(p.ride),
                crow_m: pk.crow_m,
                l_ref_m: pk.l_ref_m,
                ref_union_m,
                hole_m: pk.hole_m,
                ref_km,
                ref_offroad_km: ref_off,
                ref_off_view_km: off_view,
                zone: 0,
                fail: None,
                band_i: pk.band,
                cluster,
                ride_t: Some(p.ride),
                start: node(pk.j_start),
                end: node(pk.j_end),
                reference,
            }
        })
        .collect()
}

/// Les lignes d'une variante, indexées par tronçon.
type ByVariant<'a> = BTreeMap<&'a str, Vec<Option<&'a Line>>>;

fn index_lines<'a>(n_segs: usize, lines: &'a [Line]) -> ByVariant<'a> {
    let mut m: ByVariant<'a> = BTreeMap::new();
    for l in lines {
        m.entry(l.core.variant.as_str()).or_insert_with(|| vec![None; n_segs])[l.core.seg] = Some(l);
    }
    m
}

/// Jaccard moyen (échecs à 0) d'une variante sur les tronçons d'un type et d'une moitié.
fn mean_j(segs: &[Seg], by: &ByVariant<'_>, ride: &str, half: char, variant: &str) -> Option<f64> {
    let v = by.get(variant)?;
    let (mut s, mut n) = (0.0, 0usize);
    for (i, sg) in segs.iter().enumerate() {
        if sg.ride == ride && sg.half == half {
            if let Some(l) = v[i] {
                s += if l.core.ok { l.core.jaccard } else { 0.0 };
                n += 1;
            }
        }
    }
    (n > 0).then(|| s / n as f64)
}

/// Un groupe d'agrégat : type, bande (`None` : toutes), moitié, variante.
#[derive(Debug, Clone, Serialize)]
pub struct GroupKey {
    pub ride: &'static str,
    pub band: &'static str,
    pub half: char,
    pub variant: String,
}

#[derive(Debug, Serialize)]
pub struct Group {
    #[serde(flatten)]
    pub key: GroupKey,
    #[serde(flatten)]
    pub stats: agg::Stats,
}

/// La meilleure table d'un type et les scores qui l'ont choisie.
#[derive(Debug, Clone, Serialize)]
pub struct Tuned {
    pub ride: &'static str,
    pub best1: WayQ8,
    pub best1_score_a: f64,
    pub best2: WayQ8,
    /// Scores (Jaccard moyen sur A) du balayage : `[case][multiplicateur de LADDER]`.
    pub sweep_a: Vec<Vec<Option<f64>>>,
    /// Scores sur A des voisins de `best1` : `[case][multiplicateur de LADDER]`.
    pub desc_a: Vec<Vec<Option<f64>>>,
}

const MAIN: [&str; 4] = ["neutral", "avoid", "best1", "best2"];

pub fn run_route_play(a: &RoutePlayArgs) -> Result<()> {
    let mut paths = a.paths.resolve()?;
    if a.paths.out.is_none() {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from).ok_or_else(|| anyhow!("HOME"))?;
        paths.out = home.join(".cache/osm-route-details/graph/route-bench");
    }
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
    let deadline = started + std::time::Duration::from_secs_f64(a.max_minutes * 60.0);
    let mut ph = Phases::new();

    // 1. Le moteur : les paramètres du service (goût commun, roulabilité), sans équipe.
    let engine: Eng =
        Engine::new(open(&paths.graph)?, open(&paths.road)?, open(&paths.gravel)?, Params::default())
            .map_err(|e| anyhow!("moteur : {e}"))?;
    let gf = engine.base();
    let (road, gravel) = (open(&paths.road)?, open(&paths.gravel)?);
    let n_edges = gf.header().edges as usize;
    let views = [ViewInfo::build(&road, n_edges), ViewInfo::build(&gravel, n_edges)];
    ph.end("moteur et masques des vues");

    // 2. L'export, l'appariement, la pose.
    let mut ing = export::ingest(&paths.export, &wanted)?;
    let letter_of: Vec<String> = ing
        .teams
        .iter()
        .map(|id| letters.get(id).cloned().ok_or_else(|| anyhow!("équipe sans lettre")))
        .collect::<Result<_>>()?;
    let poly = Poly::read(&paths.poly).with_context(|| paths.poly.display().to_string())?;
    let seg_index = SegIndex::build(gf);
    let jobs = export::match_keys(gf, &seg_index, &poly, &ing);
    drop(seg_index);
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
            opts: SeamOpts { margin_m: 200.0, touch_continues: false },
            proj_tol_m: 15.0,
            retain: RetainParams::default(),
        };
        pool.install(|| {
            todo.par_iter()
                .map(|&(idx, rank)| posed::pose(&ctx, idx, rank, &letter_of[ing.tracks[idx].team]))
                .collect()
        })
    };
    let export_xxh3 = ing.export_xxh3;
    let rows_read = todo.len();
    ing.keys = Vec::new();
    drop(jobs);
    ph.end("export, appariement et pose");

    // 3. Les tronçons.
    let n_posed = posed_all.iter().flatten().count();
    let kept: Vec<&Posed> = posed_all.iter().flatten().filter(|p| p.order_anomalies == 0).collect();
    let n_anomaly = n_posed - kept.len();
    let per_track: Vec<Vec<Seg>> = pool.install(|| {
        kept.par_iter()
            .enumerate()
            .map(|(ci, p)| {
                let view = &views[p.ride.view_index()] as &dyn ViewLookup;
                segments_of(gf, view, p, ci, a.per_track, a.seed)
            })
            .collect()
    });
    let tracks_with_seg = per_track.iter().filter(|v| !v.is_empty()).count();
    let mut segs: Vec<Seg> = per_track.into_iter().flatten().collect();
    eprintln!(
        "traces posées : {n_posed} sur {rows_read} lignes ; écartées (anomalie d'ordre) : {n_anomaly} ; \
         tronçons : {} sur {tracks_with_seg} traces",
        segs.len()
    );
    ph.end("tirage des tronçons");

    // 4. Les phases.
    let cut = AtomicBool::new(false);
    let env = Env { engine: &engine, gf, deadline, cut: &cut };
    // Les plus longs d'abord ; les sorties restent dans l'ordre des tronçons.
    let mut order: Vec<usize> = (0..segs.len()).collect();
    order.sort_by(|&x, &y| segs[y].crow_m.total_cmp(&segs[x].crow_m).then(x.cmp(&y)));
    let run_phase = |segs: &[Seg], phase: u8, vars: &(dyn Fn(&Seg) -> Vec<Variant> + Sync)| {
        let done = AtomicUsize::new(0);
        let total = segs.len();
        let t0 = Instant::now();
        let mut res: Vec<(usize, SegOut)> = pool.install(|| {
            order
                .par_iter()
                .map_init(Workspace::new, |ws, &si| {
                    let out = env.play_seg(si, &segs[si], phase, &vars(&segs[si]), ws);
                    ws.shrink_to(1_500_000_000);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_multiple_of((total / 20).max(1)) {
                        let el = t0.elapsed().as_secs_f64();
                        eprintln!(
                            "  phase {phase} : {n}/{total} tronçons, {el:.0} s, reste ~{:.0} s",
                            el / n as f64 * (total - n) as f64
                        );
                    }
                    (si, out)
                })
                .collect()
        });
        res.sort_by_key(|(si, _)| *si);
        res.into_iter().map(|(_, o)| o).collect::<Vec<_>>()
    };
    let p1 = run_phase(&segs, 1, &|s: &Seg| phase1_variants(s.ride_t.expect("type")));
    let mut lines: Vec<Line> = Vec::new();
    for (s, (ls, zone, fail)) in segs.iter_mut().zip(p1) {
        s.zone = zone;
        s.fail = fail;
        lines.extend(ls);
    }
    ph.end("phase 1 (neutre, avoid, balayage, curseur)");

    let rides: Vec<RideType> = [RideType::Road, RideType::Gravel, RideType::Mtb]
        .into_iter()
        .filter(|r| segs.iter().any(|s| s.ride_t == Some(*r)))
        .collect();
    let mut tuned: Vec<Tuned> = Vec::new();
    if !a.no_tune {
        // best1, sur la moitié A.
        let by = index_lines(segs.len(), &lines);
        let mut best1: BTreeMap<&'static str, WayQ8> = BTreeMap::new();
        for &r in &rides {
            let rn = ride_name(r);
            let score = |k: usize, q: u16| -> Option<f64> {
                if q == 256 {
                    mean_j(&segs, &by, rn, 'A', "neutral")
                } else {
                    mean_j(&segs, &by, rn, 'A', &format!("sweep:c{k}:{q}"))
                }
            };
            let b1 = pick_best1(&|k, q| score(k, q).unwrap_or(f64::NEG_INFINITY));
            best1.insert(rn, b1);
            tuned.push(Tuned {
                ride: rn,
                best1: b1,
                best1_score_a: f64::NAN,
                best2: b1,
                sweep_a: (0..7).map(|k| LADDER.iter().map(|&q| score(k, q)).collect()).collect(),
                desc_a: Vec::new(),
            });
        }
        drop(by);
        let p2 = run_phase(&segs, 2, &|s: &Seg| {
            let b1 = best1[s.ride];
            let mut v = vec![var("best1", b1)];
            v.extend(neighbours(b1));
            v
        });
        for (ls, _, _) in p2 {
            lines.extend(ls);
        }
        ph.end("phase 2 (best1 et voisins)");
        let by = index_lines(segs.len(), &lines);
        let mut best2: BTreeMap<&'static str, WayQ8> = BTreeMap::new();
        for t in &mut tuned {
            let rn = t.ride;
            let s1 = mean_j(&segs, &by, rn, 'A', "best1").unwrap_or(f64::NEG_INFINITY);
            let b1 = t.best1;
            let score = |k: usize, q: u16| -> Option<f64> {
                if q == b1[k] {
                    None
                } else {
                    mean_j(&segs, &by, rn, 'A', &format!("desc:c{k}:{q}"))
                }
            };
            t.best1_score_a = s1;
            t.best2 = pick_best2(b1, s1, &score);
            t.desc_a = (0..7).map(|k| LADDER.iter().map(|&q| score(k, q)).collect()).collect();
            best2.insert(t.ride, t.best2);
        }
        drop(by);
        let p3 = run_phase(&segs, 3, &|s: &Seg| vec![var("best2", best2[s.ride])]);
        for (ls, _, _) in p3 {
            lines.extend(ls);
        }
        ph.end("phase 3 (best2)");
    }

    // 5. Les agrégats.
    let by = index_lines(segs.len(), &lines);
    let neutral = by.get("neutral").cloned().unwrap_or_else(|| vec![None; segs.len()]);
    let mut variants: Vec<&str> = Vec::new();
    for l in &lines {
        if !variants.contains(&l.core.variant.as_str()) {
            variants.push(l.core.variant.as_str());
        }
    }
    let mut keys: Vec<GroupKey> = Vec::new();
    let mut groups: Vec<(Vec<Obs>, u64, bool)> = Vec::new();
    for &r in &rides {
        let rn = ride_name(r);
        for band in [None, Some(0usize), Some(1), Some(2)] {
            for half in ['A', 'B'] {
                for &v in &variants {
                    let Some(col) = by.get(v) else { continue };
                    let obs: Vec<Obs> = segs
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| s.ride == rn && s.half == half && band.is_none_or(|b| s.band_i == b))
                        .filter_map(|(i, s)| {
                            let l = col[i]?;
                            let jn = neutral[i].map_or(0.0, |n| if n.core.ok { n.core.jaccard } else { 0.0 });
                            Some(Obs {
                                cluster: s.cluster,
                                ok: l.core.ok,
                                jaccard: if l.core.ok { l.core.jaccard } else { 0.0 },
                                jaccard_neutral: jn,
                                precision: l.core.precision,
                                recall: l.core.recall,
                                detour: l.core.detour,
                                ms: l.ms,
                                km: l.core.km,
                                offroad_km: l.core.offroad_km,
                                ref_km: s.ref_km,
                                ref_offroad_km: s.ref_offroad_km,
                            })
                        })
                        .collect();
                    if obs.is_empty() {
                        continue;
                    }
                    let gseed = xxh3_64(format!("{rn}/{band:?}/{half}/{v}/{}", a.seed).as_bytes());
                    keys.push(GroupKey {
                        ride: rn,
                        band: band.map_or("all", draw::band_name),
                        half,
                        variant: v.to_string(),
                    });
                    groups.push((obs, gseed, MAIN.contains(&v)));
                }
            }
        }
    }
    let stats = pool.install(|| agg::stats_many(groups));
    let groups: Vec<Group> = keys.into_iter().zip(stats).map(|(key, stats)| Group { key, stats }).collect();
    ph.end("agrégats");

    // 6. Les sorties.
    std::fs::create_dir_all(&paths.out)?;
    let mut jsonl = String::new();
    let mut core_bytes: Vec<u8> = Vec::new();
    for l in &lines {
        jsonl.push_str(&serde_json::to_string(l)?);
        jsonl.push('\n');
        core_bytes.extend_from_slice(serde_json::to_string(&l.core)?.as_bytes());
        core_bytes.push(b'\n');
    }
    let mut seg_jsonl = String::new();
    for s in &segs {
        seg_jsonl.push_str(&serde_json::to_string(s)?);
        seg_jsonl.push('\n');
    }
    let fp_lines = format!("{:016x}", xxh3_64(&core_bytes));
    let fp_segs = format!("{:016x}", xxh3_64(seg_jsonl.as_bytes()));
    let count = |f: &dyn Fn(&Seg) -> bool| segs.iter().filter(|s| f(s)).count();
    let pop = md::Population {
        rows_read,
        posed: n_posed,
        anomaly: n_anomaly,
        kept: kept.len(),
        tracks_with_seg,
        segs: segs.len(),
        zone1: count(&|s| s.zone == 1 && s.fail.is_none()),
        common_fail: {
            let mut m: BTreeMap<&str, usize> = BTreeMap::new();
            for s in &segs {
                if let Some(f) = s.fail {
                    *m.entry(f).or_default() += 1;
                }
            }
            m
        },
        by_ride_band_half: {
            let mut m: BTreeMap<String, usize> = BTreeMap::new();
            for s in &segs {
                *m.entry(format!("{}/{}/{}", s.ride, s.band, s.half)).or_default() += 1;
            }
            m
        },
        ref_off_view_share: {
            let (o, t): (f64, f64) =
                segs.iter().fold((0.0, 0.0), |(o, t), s| (o + s.ref_off_view_km, t + s.l_ref_m / 1000.0));
            o / t.max(1e-9)
        },
        enlarged: lines.iter().filter(|l| l.core.enlarged).count(),
        lines: lines.len(),
    };
    let meta: Vec<(String, String)> = vec![
        ("empreinte xxh3 de l'export".into(), format!("{export_xxh3:016x}")),
        ("empreinte xxh3 du jsonl des lignes (sans les temps)".into(), fp_lines.clone()),
        ("empreinte xxh3 du jsonl des tronçons".into(), fp_segs.clone()),
        ("graine".into(), a.seed.to_string()),
        ("tronçons par trace au plus".into(), a.per_track.to_string()),
        ("fils".into(), threads.to_string()),
        ("durée".into(), format!("{:.0} s", started.elapsed().as_secs_f64())),
        ("coupé par --max-minutes".into(), cut.load(Ordering::Relaxed).to_string()),
        (
            "portée".into(),
            format!(
                "{} ; {}",
                if a.teams.is_empty() {
                    "toutes les équipes".into()
                } else {
                    format!("équipes {}", a.teams.join(","))
                },
                a.limit.map_or("toutes les traces".into(), |n| format!("{n} traces par équipe au plus"))
            ),
        ),
    ];
    let json = serde_json::json!({
        "meta": meta.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect::<serde_json::Map<_, _>>(),
        "population": &pop,
        "tuned": &tuned,
        "groups": &groups,
    });
    let base = |ext: &str| paths.out.join(format!("{}-{ext}", a.label));
    std::fs::write(base("lines.jsonl"), &jsonl)?;
    std::fs::write(base("segments.jsonl"), &seg_jsonl)?;
    std::fs::write(base("route.json"), serde_json::to_string_pretty(&json)?)?;
    std::fs::write(base("route.md"), md::markdown(&meta, &pop, &tuned, &groups))?;
    ph.end("sorties");
    eprintln!("empreintes : lignes {fp_lines}, tronçons {fp_segs}");
    eprintln!("sorties : {}", paths.out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase1_has_the_asked_variants() {
        let road = phase1_variants(RideType::Road);
        let gravel = phase1_variants(RideType::Gravel);
        assert_eq!(road.len(), 2 + 35 + 5 + 5);
        assert_eq!(gravel.len(), 2 + 35 + 5);
        assert_eq!(road[1].q8, [384, 256, 256, 256, 256, 256, 512]);
        let s = road.iter().find(|v| v.name == "sweep:c6:768").unwrap();
        assert_eq!(s.q8, [256, 256, 256, 256, 256, 256, 768]);
        // Le cran 0,25 au neutre 0,25 est le neutre.
        assert_eq!(road.iter().find(|v| v.name == "cursor:u0.25:d0.25").unwrap().q8, WAY_NEUTRAL);
    }

    #[test]
    fn best1_and_best2_pick_per_case() {
        // Case 2 : 512 est le meilleur ; case 5 : 128 ; ailleurs 256.
        let score = |k: usize, q: u16| match (k, q) {
            (2, 512) => 0.6,
            (5, 128) => 0.55,
            _ => 0.5,
        };
        let b1 = pick_best1(&score);
        assert_eq!(b1, [256, 256, 512, 256, 256, 128, 256]);
        let n = neighbours(b1);
        // 128 n'a pas de cran en dessous : 13 voisins.
        assert_eq!(n.len(), 13);
        assert!(n.iter().any(|v| v.name == "desc:c2:768") && !n.iter().any(|v| v.name == "desc:c5:64"));
        let b2 = pick_best2(b1, 0.6, &|k, q| match (k, q) {
            (2, 768) => Some(0.61),
            (0, 182) => Some(0.59),
            _ => Some(0.6),
        });
        assert_eq!(b2, [256, 256, 768, 256, 256, 128, 256]);
    }
}
