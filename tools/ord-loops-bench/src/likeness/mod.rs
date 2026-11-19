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
pub mod compare_attr;
pub mod compare_chain;
pub mod compare_org;
pub mod compare_svc;
pub mod export;
#[cfg(test)]
mod fixture;
pub mod holdout;
pub mod measure;
pub mod out;
pub mod p2p;
pub mod play;
pub mod shape;
pub mod svc;
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
use ord_graph_match::profile::collect::LearnSet;
use ord_graph_match::profile::learn::LearnParams;
use ord_graph_match::SegIndex;
use ord_loops::{Params, Taste, TeamData, Workspace};
use ord_loops_check::{Checker, Config, GraphIndex, TeamCheck, TypeIndex};
use rayon::prelude::*;

use self::chain::{ChainState, ClassMeters, StepOut};
use self::measure::Measures;
use self::play::Eng;
use self::taste::{ClassSums, TasteParams, TasteTable};
use self::variants::{ChainMeta, TasteSource, TeamTime, Variant, VariantMeta};
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
    /// Fichier d'attracteurs de la vue `road` (lot 4, 4d ; défaut : `G/lot6/france-20261115-z11.road.orda`, puis
    /// `G/lot4/4d/orda/`, s'il existe).
    #[arg(long)]
    pub attractors_road: Option<PathBuf>,
    /// Fichier d'attracteurs de la vue `gravel` (même défaut).
    #[arg(long)]
    pub attractors_gravel: Option<PathBuf>,
    /// Ne charge aucun fichier d'attracteurs (le contrôle « sans fichier » de I1 et I5) ; refuse une variante `attr`.
    #[arg(long)]
    pub no_attractors: bool,
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
/// Empreintes de `base` et de `svc` de la sonde point à point (lot 4, 4c, `ord-loops-lot4c-sonde.md` §1), à `--holdout-share
/// 0.25`, toutes équipes : le moteur d'aujourd'hui (`Classic`) doit les rejouer au bit près.
pub const SONDE_4C_BASE_FINGERPRINT: &str = "4686c904975302e7";
pub const SONDE_4C_SVC_FINGERPRINT: &str = "384f003c0bd19b7f";
/// I2′ (§16.6 de la note du lot 4) : `svc+org+turns:0+reuse:0` rend l'organique d'avant l'étape « virages » (`ea6ef63`), à
/// `--holdout-share 0.25` : toutes équipes (campagne 4 du 4c), et l'essai `--teams B,C --limit 20`.
pub const I2P_FULL_FINGERPRINT: &str = "c48118447315c118";
pub const I2P_TRIAL_FINGERPRINT: &str = "27952d4befd451f4";
/// I1 (§17.8 de la note du lot 4) : `common` à 25 % de mise à l'écart, toutes équipes (le `base` et le `svc` sont ceux de la sonde
/// 4c ci-dessus).
pub const SONDE_4C_COMMON_FINGERPRINT: &str = "07b62287e64fa258";
/// I5 (§17.8) : l'organique **sans attracteurs actifs** (`svc+org` sans fichier, `svc+org+attr:0` avec) rend 1.12 : toutes équipes
/// à 25 %, et l'essai `--teams B,C --limit 20`.
pub const I5_FULL_FINGERPRINT: &str = "6324bdfdabf62f38";
pub const I5_TRIAL_FINGERPRINT: &str = "50adca52ef713d18";

/// L'empreinte de I5 attendue pour la portée de la campagne ; `None` pour toute autre portée.
pub fn i5_expected(a: &LikenessArgs) -> Option<&'static str> {
    if !a.at_4a_defaults_but_scope() || (a.holdout_share - 0.25).abs() > 1e-9 {
        return None;
    }
    if a.limit.is_none() && a.teams.is_empty() {
        return Some(I5_FULL_FINGERPRINT);
    }
    (a.limit == Some(20) && a.teams == ["B", "C"]).then_some(I5_TRIAL_FINGERPRINT)
}

/// Le premier pas d'une chaîne `rc` à γ = 0,5, 72 cases, sans familiarité, doit redonner `rc05` de la première sonde
/// (relevé `ord-loops-lot4-sonde.md`, §2), bit pour bit, aux réglages du 4a : empreinte de résultat et `taste_xxh3`.
pub const RC05_FINGERPRINT: &str = "ebb20ed148f75185";
pub const RC05_TASTE_XXH3: &str = "4002e232ea272c83";

/// La campagne 2 (`recal-equipe`, seconde sonde, 25 % mis à l'écart, `rc:0.5+n:3`) : empreinte de résultat et
/// `taste_xxh3` de `base`, `rc05-p1`, `rc05-p2` et `rc05-p3`. Le rejeu de la chaîne commune du 3b (§14.3.1) doit les
/// redonner, bit pour bit : sinon le banc a changé et la table commune exportée ne serait pas celle qu'on croit.
pub const RECAL_EQUIPE_CHECKS: [(&str, &str, &str); 4] = [
    ("base", "4686c904975302e7", "2d06800538d394c2"),
    ("rc05-p1", "a123cb82e6b36fc1", "40c8278314911d91"),
    ("rc05-p2", "ad57882c3ed75c10", "2f57dc10b00d143d"),
    ("rc05-p3", "2c563b8bbe8059ae", "66ee08d3eb9c910c"),
];

/// L'empreinte de I2′ attendue pour la portée de la campagne : toutes équipes à 25 % de mise à l'écart, ou l'essai
/// `--teams B,C --limit 20` ; `None` pour toute autre portée (aucun contrôle).
pub fn i2p_expected(a: &LikenessArgs) -> Option<&'static str> {
    if !a.at_4a_defaults_but_scope() || (a.holdout_share - 0.25).abs() > 1e-9 {
        return None;
    }
    if a.limit.is_none() && a.teams.is_empty() {
        return Some(I2P_FULL_FINGERPRINT);
    }
    (a.limit == Some(20) && a.teams == ["B", "C"]).then_some(I2P_TRIAL_FINGERPRINT)
}

/// Le contrôle de la campagne 2 pour une variante, s'il s'applique : `Some((empreinte, taste_xxh3))` attendus.
pub fn recal_equipe_expected(a: &LikenessArgs, name: &str) -> Option<(&'static str, &'static str)> {
    (a.at_4a_defaults_but_holdout_share() && a.holdout_share == 0.25 && a.taste_at_4a_defaults())
        .then(|| RECAL_EQUIPE_CHECKS.iter().find(|c| c.0 == name).map(|c| (c.1, c.2)))
        .flatten()
}

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

    /// Les réglages du goût aux défauts du 4a (`m`, `bound`, `min_km`) : ceux du relevé de la première sonde.
    pub fn taste_at_4a_defaults(&self) -> bool {
        self.taste_m == 0.5 && self.taste_bound == 3.0 && self.taste_min_km == 200.0
    }

    /// Les réglages du 4a : le seul cas où l'empreinte de `base` est comparable à celle du relevé.
    pub fn at_4a_defaults(&self) -> bool {
        self.at_4a_defaults_but_holdout_share() && self.holdout_share == 0.10 && self.holdout_min_loops == 10
    }

    /// Les réglages du 4a hormis la part mise à l'écart : la lecture « toutes » de `base` (surcouche complète, toutes
    /// les boucles) n'en dépend pas, ses `S_base` se comparent alors à ceux du 4a (contrôle « toutes », §13.6).
    pub fn at_4a_defaults_but_holdout_share(&self) -> bool {
        self.limit.is_none() && self.teams.is_empty() && self.at_4a_defaults_but_scope()
    }

    /// Les réglages du 4a hormis la part mise à l'écart, les équipes et la limite par équipe (l'essai de fumée du §16.10 :
    /// `--teams B,C --limit 20`).
    pub fn at_4a_defaults_but_scope(&self) -> bool {
        self.max_km.is_none()
            && self.cursors == [0.25, 1.0]
            && !self.no_holdout
            && self.min_km == 5.0
            && self.loop_gap == 0.05
            && self.min_matched == 0.95
            && self.max_hole_m == 1000.0
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
    /// Fichiers d'attracteurs `[road, gravel]` : donnés, ou ceux du défaut s'ils existent ; `None` avec `--no-attractors`.
    pub attractors: [Option<PathBuf>; 2],
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
        let attr_default = |view: &str| {
            [lot6.clone(), g.join("lot4/4d/orda")]
                .into_iter()
                .map(|d| d.join(format!("france-20261115-z11.{view}.orda")))
                .find(|p| p.is_file())
        };
        let attractors = if self.no_attractors {
            [None, None]
        } else {
            [
                self.attractors_road.clone().or_else(|| attr_default("road")),
                self.attractors_gravel.clone().or_else(|| attr_default("gravel")),
            ]
        };
        Ok(Paths {
            attractors,
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
type Built = Result<((Overlay, f64), Option<Overlay>, Option<LearnSet>)>;

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
    let legacy = listed.is_empty();
    let variants = if legacy { vec![Variant::base()] } else { listed };
    // Les attracteurs (4d) : une variante `attr` sans les deux fichiers est refusée avant toute phase lourde.
    if variants.iter().any(|v| v.org.is_some_and(|o| o.attr.is_some())) {
        anyhow::ensure!(
            !a.no_attractors && paths.attractors.iter().all(Option::is_some),
            "une variante `attr` demande les deux fichiers d'attracteurs (`--attractors-road`, `--attractors-gravel`) et pas \
             `--no-attractors`"
        );
    }
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
    let mut engine = Engine::new(
        open_graph(&paths.graph)?,
        open_graph(&paths.road)?,
        open_graph(&paths.gravel)?,
        // Toujours le poids de base de 1.9.2 : les sondes du goût passent leur table au crochet (qui remplace le poids
        // de base) et les relevés passés se rejouent tels quels (§14.3.3).
        Params { base_taste: Taste::neutral(), ..Params::default() },
    )
    .map_err(|e| anyhow!("moteur : {e}"))?;
    // Les fichiers d'attracteurs (4d), chargés dans le moteur quel que soit le jeu de variantes (I1 et I5 se lisent fichier
    // chargé) et en grilles d'ancrages pour les mesures.
    let mut grids = measure::AttrGrids::default();
    for (vi, path) in paths.attractors.iter().enumerate() {
        let Some(path) = path else { continue };
        let file =
            ord_graph::attract::AttrFile::open_mmap(path).map_err(|e| anyhow!("{} : {e}", path.display()))?;
        engine.set_attractors(vi, file).map_err(|e| anyhow!("{} : {e}", path.display()))?;
        let again =
            ord_graph::attract::AttrFile::open_mmap(path).map_err(|e| anyhow!("{} : {e}", path.display()))?;
        let view = open_graph(if vi == 0 { &paths.road } else { &paths.gravel })?;
        let grid = ord_graph::attract::AnchorGrid::new(&again, &view);
        if vi == 0 {
            grids.road = Some(grid);
        } else {
            grids.gravel = Some(grid);
        }
        eprintln!(
            "attracteurs {} : {} ({} ancrages, zone {})",
            ["road", "gravel"][vi],
            path.display(),
            again.n_anchors(),
            again.n_zone()
        );
    }
    let engine = engine;
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
    // Le 3b : les boucles d'apprentissage de `svc`, collectées au passage du dépôt sur les traces non mises à l'écart (la
    // surcouche « écart » si l'équipe en a une, sinon la complète), sans plafond : les plafonds se tirent ensuite.
    let want_learn = variants.iter().any(|v| v.svc.is_some_and(|s| s.kind == variants::SvcKind::Svc));
    let retain = ord_graph_match::profile::collect::RetainParams {
        min_matched: a.min_matched,
        max_hole_m: a.max_hole_m,
        min_km: a.min_km,
        max_km: a.max_km.or(LearnParams::default().retain.max_km),
        loop_gap: a.loop_gap,
    };
    let collector = |on: bool| {
        on.then(|| {
            ord_graph_match::profile::collect::LoopCollector::new(
                retain,
                ord_graph_match::profile::collect::Cap::None,
            )
        })
    };
    let built: Vec<Built> = team_ids
        .par_iter()
        .map(|&t| {
            let none = std::collections::HashSet::new();
            let has_gap = !skips[t].is_empty();
            let (full, km, l_full) = holdout::build_overlay_learn(
                gf,
                &seg,
                &poly,
                &ing,
                &jobs,
                t,
                &none,
                collector(want_learn && !has_gap),
            )?;
            let (gap, l_gap) = if has_gap {
                let (g, _, l) = holdout::build_overlay_learn(
                    gf,
                    &seg,
                    &poly,
                    &ing,
                    &jobs,
                    t,
                    &skips[t],
                    collector(want_learn),
                )?;
                (Some(g), l)
            } else {
                (None, None)
            };
            Ok(((full, km), gap, l_gap.or(l_full)))
        })
        .collect();
    let mut overlays: Vec<Overlay> = Vec::new();
    let mut gap_overlays: Vec<Option<Overlay>> = Vec::new();
    let mut learn_sets: Vec<Option<LearnSet>> = Vec::new();
    let mut overlay_checks: Vec<holdout::OverlayCheck> = Vec::new();
    for (t, b) in built.into_iter().enumerate() {
        let ((o, km), gap, learn_set) = b?;
        learn_sets.push(learn_set);
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
        loops.par_iter().map(|l| measure_real(gf, &checker0, &grids, l)).collect();
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
    let ctx = play::PlayCtx {
        engine: &engine,
        wins: &wins,
        iqr,
        real_shape: &real_shape,
        real_edges: &real_edges,
        grids: &grids,
    };
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
        learn_sets: &learn_sets,
        retain,
        threads,
    };
    let mut retention: BTreeMap<String, export::RetentionCounts> = BTreeMap::new();
    for (t, c) in counts.into_iter().enumerate() {
        retention.insert(letter_of[t].clone(), c);
    }
    overlay_checks.sort_by(|x, y| x.team.cmp(&y.team));
    let vertex_histograms = vertex_histograms(&loops, &raw, &reals);
    // Les chaînes de recalé (§13.6) : les mètres de `base`, ceux du pas précédent, l'état de la chaîne en cours.
    let mut base_meters: Option<ClassMeters> = None;
    let mut prev_meters: Option<ClassMeters> = None;
    let mut chain: Option<ChainState> = None;
    let mut last_name = String::new();
    let mut step1_check: Option<bool> = None;
    let mut done: Vec<compare::Done> = Vec::new();
    std::fs::create_dir_all(&paths.out)?;
    for v in &variants {
        wins.take_team_counters();
        let w0 = wins.counters();
        // Un pas de chaîne : la table se corrige d'après les jeux de `base` (pas 1) ou du pas précédent.
        let mut step: Option<StepOut> = None;
        let mut learned_from = String::new();
        if let (Some(c), Some((src, gamma))) = (&v.chain, v.taste) {
            if c.step == 1 {
                chain = Some(ChainState::new(c.dims, gamma));
                learned_from = "base".to_string();
            } else {
                learned_from = last_name.clone();
            }
            let inputs = if c.step == 1 { base_meters.as_ref() } else { prev_meters.as_ref() }
                .ok_or_else(|| anyhow!("`{}` a besoin des jeux de `{learned_from}`", v.name))?;
            let st = chain.as_mut().ok_or_else(|| anyhow!("chaîne `{}` sans état", c.stem))?;
            step = Some(st.step(src, inputs, &order, &letter_of, &tp));
            prev_meters = None;
        }
        *p2p::CURRENT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = v.name.clone();
        let run = bench.play_variant(v, step.as_ref(), &mut workspaces)?;
        ph.end(&if legacy { "jeux".to_string() } else { format!("variante {}", v.name) });
        let team_windows = wins.take_team_counters();
        let w1 = wins.counters();
        let windows = play::WindowCounters {
            built: w1.built - w0.built,
            hits: w1.hits - w0.hits,
            overflow: w1.overflow - w0.overflow,
            escapes: w1.escapes - w0.escapes,
        };
        // Les mètres de cette variante : ce que le pas suivant apprend, et l'écart que ses parcours laissent.
        let meters = ClassMeters::of(&loops, &held, &real_class_m, &run.plays, ing.teams.len());
        let pooled_name =
            if matches!(v.taste, Some((TasteSource::RecalTeam, _))) { "commun" } else { "général" };
        let l1_after = chain::l1_gaps(&meters, &order, &letter_of, pooled_name);
        let class_meters = chain::pooled_out(&meters);
        let chain_diag = step.map(|s| s.diag).unwrap_or_default();
        let team_times = run.team_times;
        let run_svc_teams = run.svc_teams;
        match &v.chain {
            Some(c) if c.step < c.steps => prev_meters = Some(meters),
            Some(_) => chain = None,
            None if v.is_base() && base_meters.is_none() => base_meters = Some(meters),
            None => {}
        }
        last_name = v.name.clone();

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
            chain: v.chain.as_ref().map(|c| ChainMeta {
                stem: c.stem.clone(),
                step: c.step,
                steps: c.steps,
                learned_from: learned_from.clone(),
                dims: c.dims.code().to_string(),
                team_n0: tp.team_n0,
                mult_bound: tp.mult_bound,
                diag: chain_diag,
            }),
            l1_after,
            team_times,
            class_meters,
            svc: v.svc.map(|sv| svc_meta(&sv, v.org, a, retain, run_svc_teams)),
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
        // « Pas 1 = `rc05` » : la chaîne commune à 72 cases, γ = 0,5, sans familiarité, aux réglages du 4a.
        if matches!(v.name.as_str(), "rc05" | "rc05-p1") && a.at_4a_defaults() && a.taste_at_4a_defaults() {
            let taste = report.variant.as_ref().map_or("", |m| m.taste_xxh3.as_str());
            let ok = report.fingerprint == RC05_FINGERPRINT && taste == RC05_TASTE_XXH3;
            eprintln!(
                "« pas 1 = `rc05` ({RC05_FINGERPRINT}, {RC05_TASTE_XXH3}) » : {}",
                if ok { "oui" } else { "NON" }
            );
            step1_check = Some(ok);
            anyhow::ensure!(
                ok,
                "le pas 1 n'est pas `rc05` ({} · {taste}) : chercher, ne pas lire la suite",
                report.fingerprint
            );
        }
        if let Some((fp, tx)) = recal_equipe_expected(a, &v.name) {
            let taste = report.variant.as_ref().map_or("", |m| m.taste_xxh3.as_str());
            let ok = report.fingerprint == fp && taste == tx;
            eprintln!("« `{}` = campagne 2 ({fp}, {tx}) » : {}", v.name, if ok { "oui" } else { "NON" });
            anyhow::ensure!(
                ok,
                "`{}` n'est pas celle de la campagne 2 ({} · {taste}, attendu {fp} · {tx}) : le banc a changé, \
                 chercher, ne pas lire la suite",
                v.name,
                report.fingerprint
            );
        }
        // 4c : à 25 % de mise à l'écart, toutes équipes, `base` et `svc` sont ceux de la sonde point à point.
        let at_sonde = a.at_4a_defaults_but_holdout_share() && (a.holdout_share - 0.25).abs() < 1e-9;
        if at_sonde && v.is_base() {
            let ok = report.fingerprint == SONDE_4C_BASE_FINGERPRINT;
            eprintln!(
                "« `base` = sonde 4c : {SONDE_4C_BASE_FINGERPRINT} » : {}",
                if ok { "oui" } else { "NON" }
            );
            anyhow::ensure!(
                ok,
                "l'empreinte de `base` n'est pas celle de la sonde 4c ({SONDE_4C_BASE_FINGERPRINT}) : le moteur `Classic` n'est plus celui de 1.10 ; chercher, ne pas lire la suite"
            );
        }
        if at_sonde && v.name == "svc" {
            let ok = report.fingerprint == SONDE_4C_SVC_FINGERPRINT;
            eprintln!(
                "« `svc` = sonde 4c : {SONDE_4C_SVC_FINGERPRINT} » : {}",
                if ok { "oui" } else { "NON" }
            );
            anyhow::ensure!(
                ok,
                "l'empreinte de `svc` n'est pas celle de la sonde 4c ({SONDE_4C_SVC_FINGERPRINT}) : I1 manque ; chercher, ne pas lire la suite"
            );
        }
        if at_sonde && v.name == "common" {
            let ok = report.fingerprint == SONDE_4C_COMMON_FINGERPRINT;
            eprintln!(
                "« `common` = sonde 4c : {SONDE_4C_COMMON_FINGERPRINT} » : {}",
                if ok { "oui" } else { "NON" }
            );
            anyhow::ensure!(
                ok,
                "l'empreinte de `common` n'est pas celle de la sonde 4c ({SONDE_4C_COMMON_FINGERPRINT}) : I1 manque ; chercher, ne pas lire la suite"
            );
        }
        // 4d (§17.8) : I5, l'organique sans attracteurs actifs rend 1.12 (fichier chargé ou non), arrêt si elle manque.
        if v.name == "svc-org" || v.name == "svc-org-attr0" {
            if let Some(want) = i5_expected(a) {
                let ok = report.fingerprint == want;
                eprintln!("« `{}` = 1.12 (I5 : {want}) » : {}", v.name, if ok { "oui" } else { "NON" });
                anyhow::ensure!(
                    ok,
                    "`{}` ({}) n'est pas l'organique de 1.12 ({want}) : I5 manque ; chercher, ne pas lire la suite",
                    v.name,
                    report.fingerprint
                );
            }
        }
        // 4c, étape « virages » (§16.8) : I2 et I2′, arrêt si l'un manque.
        if let Some(o) = v.org {
            if o.tri0 && o.w_milli == Some(0) && o.turns0 && o.reuse0 {
                if let Some(svc) = done.iter().find(|d| d.variant.name == "svc") {
                    let ok = report.fingerprint == svc.fingerprint;
                    eprintln!(
                        "« `{}` = `svc` (I2 : {}) » : {}",
                        v.name,
                        svc.fingerprint,
                        if ok { "oui" } else { "NON" }
                    );
                    anyhow::ensure!(
                        ok,
                        "`{}` ({}) n'est pas `svc` ({}) : I2 manque, l'organique coupé n'est plus le `Classic` ; chercher, ne pas lire la suite",
                        v.name,
                        report.fingerprint,
                        svc.fingerprint
                    );
                }
            }
            let is_svc = v.svc.is_some_and(|s| s.kind == variants::SvcKind::Svc);
            if is_svc && o.turns0 && o.reuse0 && !o.tri0 && o.w_milli.is_none() && !o.lc {
                if let Some(want) = i2p_expected(a) {
                    let ok = report.fingerprint == want;
                    eprintln!(
                        "« `{}` = organique d'avant l'étape (I2′ : {want}) » : {}",
                        v.name,
                        if ok { "oui" } else { "NON" }
                    );
                    anyhow::ensure!(
                        ok,
                        "`{}` ({}) n'est pas l'organique d'avant l'étape ({want}) : I2′ manque ; chercher, ne pas lire la suite",
                        v.name,
                        report.fingerprint
                    );
                }
            }
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
            holdout_share: a.holdout_share,
            step1_check,
            // Hors des réglages du 4a, mais seulement par la part mise à l'écart : un autre `--seed`, `--min-km`…
            // changerait les boucles ou les requêtes, et le contrôle échouerait à tort.
            all_check_applies: !a.at_4a_defaults() && a.at_4a_defaults_but_holdout_share(),
        };
        let stop = compare::write(&paths.out, &a.label, &conds, &done)?;
        anyhow::ensure!(
            stop.is_empty(),
            "un contrôle du 4d manque : {} ; chercher, ne pas lire la suite",
            stop.join(" ; ")
        );
    }
    Ok(())
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
    /// 3b : les boucles d'apprentissage de chaque équipe (indice de `Ingest::teams`), sans plafond.
    learn_sets: &'a [Option<LearnSet>],
    retain: ord_graph_match::profile::collect::RetainParams,
    threads: usize,
}

/// Ce qu'une variante rend : ses jeux (par boucle, dans l'ordre des boucles) et les tables de goût jouées.
struct VariantRun {
    plays: Vec<Vec<play::PlayRec>>,
    tables: Vec<TasteTable>,
    team_times: Vec<TeamTime>,
    svc_teams: Vec<svc::SvcTeam>,
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
        step: Option<&StepOut>,
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
            // Recalé (`rc`) : la table commune du pas, la même pour toutes les équipes et les deux surcouches.
            Some((TasteSource::Recal, _)) => {
                let out = step.ok_or_else(|| anyhow!("`rc` a besoin d'un pas de chaîne"))?;
                tables.extend(out.tables.iter().cloned());
                fixed = (out.general.clone(), out.general.clone());
            }
            // Recalé par équipe (`rt`) : une table par équipe, les mêmes pour les deux surcouches.
            Some((TasteSource::RecalTeam, _)) => {
                let out = step.ok_or_else(|| anyhow!("`rt` a besoin d'un pas de chaîne"))?;
                tables.extend(out.tables.iter().cloned());
            }
            _ => {}
        }
        let mut plays: Vec<Vec<play::PlayRec>> = self.loops.iter().map(|_| Vec::new()).collect();
        let mut team_times: Vec<TeamTime> = Vec::new();
        let mut svc_teams: Vec<svc::SvcTeam> = Vec::new();
        // Le chemin du service (3b) : `common` passe la même table à toutes les équipes.
        let common_taste: Option<Taste> = match v.svc {
            Some(sv) if sv.kind == variants::SvcKind::Common => {
                let c = svc::common_of(sv.norm).taste();
                tables.extend(taste_tables("commun", "full", &c));
                Some(c)
            }
            _ => None,
        };
        for &t in self.order {
            // `svc` : le profil de l'équipe, appris une fois (sur la surcouche « écart »), servi aux deux surcouches.
            let learned_taste: Option<Taste> = match v.svc {
                Some(sv) if sv.kind == variants::SvcKind::Svc => {
                    let (taste, meta) = self.learn_team(t, &sv, v.org)?;
                    tables.extend(taste_tables(&self.letter_of[t], "full", &taste));
                    svc_teams.push(meta);
                    Some(taste)
                }
                _ => None,
            };
            let mine: Vec<usize> = (0..self.loops.len()).filter(|&i| self.loops[i].team == t).collect();
            let tt = std::time::Instant::now();
            // Temps de l'équipe (§13.6) : constructions des deux surcouches, et jeux `full` (cumul des `PlayRec::ms`).
            let mut build_s = 0.0;
            let mut full_ms: Vec<(f64, f64)> = a.cursors.iter().map(|&c| (c, 0.0)).collect();
            let mut plays_full = 0usize;
            let tasks_of = |ids: &[usize]| -> Result<Vec<play::Task>> {
                let mut tasks: Vec<play::Task> = Vec::new();
                for &i in ids {
                    for &c in &a.cursors {
                        let mut req = variants::adapt_request(
                            play::request_of(gf, &self.loops[i], c, a.seed)?,
                            v.fam,
                            v.fam_road_only,
                        );
                        // 4c : le mélange de l'équipe (ses formes au dépôt) avec `svc`, le général avec `common`.
                        if let Some(o) = &v.org {
                            let shapes = v
                                .svc
                                .filter(|s| s.kind == variants::SvcKind::Svc)
                                .and_then(|_| self.learn_sets[t].as_ref())
                                .map(|s| &s.shapes);
                            req.figures = o.figures(
                                ord_graph_match::profile::shape::mix_for(shapes, req.ride_type),
                                req.ride_type,
                            );
                        }
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
                    _ if v.svc.is_some() => learned_taste.clone().or_else(|| common_taste.clone()),
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
                    Some((TasteSource::RecalTeam, _)) => step.and_then(|o| o.team.get(&t)).cloned().flatten(),
                    Some(_) => {
                        if label == "full" {
                            fixed.0.clone()
                        } else {
                            fixed.1.clone()
                        }
                    }
                    None => None,
                };
                let tb = std::time::Instant::now();
                let (team, team_check) =
                    build_team(self.engine, self.graph_index, overlay, &self.letter_of[t])?;
                build_s += tb.elapsed().as_secs_f64();
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
                    if label == "full" {
                        plays_full += 1;
                        if let Some(slot) = full_ms.iter_mut().find(|(c, _)| *c == rec.cursor) {
                            slot.1 += rec.ms;
                        }
                    }
                    plays[tasks[k].loop_idx].push(rec);
                }
                self.ctx.wins.drop_team(env.key);
            }
            let wall_s = tt.elapsed().as_secs_f64();
            eprintln!("{} : équipe {} : {} boucles, {wall_s:.1} s", v.name, self.letter_of[t], mine.len());
            let by_cursor: Vec<(f64, f64)> = full_ms.iter().map(|&(c, ms)| (c, ms / 1_000.0)).collect();
            team_times.push(TeamTime {
                team: self.letter_of[t].clone(),
                loops: mine.len(),
                plays_full,
                cpu_s_full: by_cursor.iter().map(|x| x.1).sum(),
                cpu_s_full_by_cursor: by_cursor,
                build_s,
                wall_s,
            });
        }
        Ok(VariantRun { plays, tables, team_times, svc_teams })
    }
}

impl Bench<'_> {
    /// L'apprentissage d'une équipe par le code du dépôt (§14.12.2) : ses boucles non mises à l'écart (plafonnées comme le
    /// service), le joueur du banc sur la surcouche « écart » (la complète sans mise à l'écart), `learn`.
    fn learn_team(
        &self,
        t: usize,
        sv: &variants::Svc,
        org: Option<variants::Org>,
    ) -> Result<(Taste, svc::SvcTeam)> {
        use ord_server::generator::cursor;
        let a = self.args;
        let hs: Vec<u32> = a
            .cursors
            .iter()
            .map(|&c| cursor::hundredths(c).ok_or_else(|| anyhow!("curseur hors de [0, 1] : {c}")))
            .collect::<Result<_>>()?;
        let hs: [u32; 2] =
            hs.try_into().map_err(|_| anyhow!("`svc` joue exactement deux curseurs (`--cursors`)"))?;
        let learn_organic = org.is_some_and(|o| !o.lc);
        let learn_attractors = org.is_some_and(|o| !o.lc && o.attr.is_some_and(|x| x.la));
        let lp =
            svc::learn_params(sv, self.retain, hs, a.seed, learn_figures(learn_organic), learn_attractors);
        let cursors: Vec<(ord_loops::Mode, f32, u32)> = lp
            .cursors
            .iter()
            .map(|&h| {
                let c = cursor::apply(h);
                (c.mode, c.familiarity, c.off_team_cost_q8)
            })
            .collect();
        let set = self.learn_sets[t].as_ref().map(|s| s.capped(svc::cap_of(sv))).unwrap_or_default();
        let mut by_type = [0u32; 3];
        for l in &set.loops {
            by_type[usize::from(l.ty).min(2)] += 1;
        }
        let overlay = self.gap_overlays[t].as_ref().unwrap_or(&self.overlays[t]);
        let t0 = std::time::Instant::now();
        let (team, team_check) = build_team(self.engine, self.graph_index, overlay, &self.letter_of[t])?;
        let checker = Checker::with_tables(
            self.gf,
            Some(overlay),
            Config::default(),
            self.graph_index,
            Some(&team_check),
        );
        let (keys, cpu) = (svc::BenchPlayer::fresh_keys(), std::sync::atomic::AtomicU64::new(0));
        let player = svc::BenchPlayer {
            engine: self.engine,
            gf: self.gf,
            wins: self.ctx.wins,
            team: &team,
            checker: &checker,
            threads: self.threads,
            next_key: &keys,
            cpu_ns: &cpu,
            org: org.filter(|o| !o.lc),
        };
        let learned =
            ord_graph_match::profile::learn::learn(&set, &cursors, &player, &svc::common_of(sv.norm), &lp);
        let wall = t0.elapsed().as_secs_f64();
        let cpu_s = cpu.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e9;
        eprintln!(
            "  profil de l'équipe {} : {} boucles, {} pas, {} jeux, {wall:.1} s",
            self.letter_of[t],
            set.loops.len(),
            learned.steps_done,
            learned.plays
        );
        let meta = svc::team_meta(&self.letter_of[t], by_type, &learned, wall, cpu_s);
        Ok((learned.taste, meta))
    }
}

/// Les figures que joue l'apprentissage d'une variante : organiques avec `org` (hors `lc`), sinon classiques.
fn learn_figures(organic: bool) -> ord_graph_match::profile::learn::LearnFigures {
    if organic {
        ord_graph_match::profile::learn::LearnFigures::Organic
    } else {
        ord_graph_match::profile::learn::LearnFigures::Classic
    }
}

/// Ce que `common` et `svc` écrivent sur eux-mêmes (§14.12.2).
fn svc_meta(
    sv: &variants::Svc,
    org: Option<variants::Org>,
    a: &LikenessArgs,
    retain: ord_graph_match::profile::collect::RetainParams,
    teams: Vec<svc::SvcTeam>,
) -> svc::SvcMeta {
    let hs: Vec<u32> =
        a.cursors.iter().filter_map(|&c| ord_server::generator::cursor::hundredths(c)).collect();
    let cursors = [hs.first().copied().unwrap_or(25), hs.get(1).copied().unwrap_or(100)];
    let learn_attractors = org.is_some_and(|o| !o.lc && o.attr.is_some_and(|x| x.la));
    let lp = svc::learn_params(
        sv,
        retain,
        cursors,
        a.seed,
        learn_figures(org.is_some_and(|o| !o.lc)),
        learn_attractors,
    );
    svc::SvcMeta {
        common: ord_graph_match::profile::format::hex16(svc::common_of(sv.norm).hash_for(lp.mix)),
        learn: ord_graph_match::profile::format::hex16(lp.hash()),
        cap: if sv.kind == variants::SvcKind::Svc { format!("{:?}", lp.cap) } else { String::new() },
        normalize: sv.norm,
        mix: lp.mix.label().to_string(),
        teams,
    }
}

/// Les trois tables d'une `Taste` pour la sortie (source, surcouche) : seulement les `q8`, que `taste_xxh3` hache.
fn taste_tables(source: &str, overlay: &str, t: &Taste) -> Vec<TasteTable> {
    taste::TYPES
        .iter()
        .enumerate()
        .map(|(i, (name, _))| TasteTable {
            source: source.to_string(),
            overlay: overlay.to_string(),
            ride: (*name).to_string(),
            num_km: 0.0,
            neutral: t.q8[i].iter().all(|&q| q == 256),
            // Pas de parts apprises à montrer (la table vient du dépôt) : des vecteurs de la bonne taille, sans effet.
            pn: vec![0.0; ord_loops::TASTE_CLASSES],
            pd: vec![0.0; ord_loops::TASTE_CLASSES],
            g: vec![1.0; ord_loops::TASTE_CLASSES],
            q8: t.q8[i].to_vec(),
        })
        .collect()
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
    grids: &measure::AttrGrids,
    l: &export::Loop,
) -> (Measures, Option<Vec<String>>) {
    let (mut m, codes) = measure_real_plain(gf, checker, l);
    m.attr = measure::attr_masks(gf, grids, play::ride_of(l.ty), &l.walk, &l.trace);
    (m, codes)
}

fn measure_real_plain(
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
        figures: ord_loops::Figures::Classic,
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
