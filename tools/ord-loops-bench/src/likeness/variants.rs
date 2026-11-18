//! Variantes de la sonde du goût (note lot 4, §12.3) : la syntaxe de `--variants`, la requête que `fam` adapte, et ce
//! que chaque variante écrit sur elle-même.

use anyhow::{anyhow, bail, ensure, Result};
use ord_loops::{Mode, Request, RideType};
use serde::Serialize;

use super::chain::ChainOwnerStep;
use super::taste::{TasteParams, TasteTable};

/// D'où vient la table de goût d'une variante.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TasteSource {
    /// Par équipe et par surcouche (`ge`) : le 4b complet.
    Team,
    /// Parts mises en commun par type, toutes équipes (`gg`) : un réglage de `K` appris.
    General,
    /// Recalée sur l'écart entre le réel et ce que `base` génère (`rc`) : la borne du levier.
    Recal,
    /// Recalée **par équipe** (`rt`, §13.2.4) : les longueurs de l'équipe seule, avec repli vers la table commune.
    RecalTeam,
}

impl TasteSource {
    pub fn code(self) -> &'static str {
        match self {
            Self::Team => "ge",
            Self::General => "gg",
            Self::Recal => "rc",
            Self::RecalTeam => "rt",
        }
    }
}

/// Forme de la table recalée (§13.2) : 72 classes, ou effets principaux (voie, revêtement, balisé : 11 paramètres).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Dims {
    D72,
    Me,
}

impl Dims {
    pub fn code(self) -> &'static str {
        match self {
            Self::D72 => "72",
            Self::Me => "me",
        }
    }
}

/// La place d'une variante dans une chaîne de recalé itéré (§13.5) : `rc:0.5+n:3` donne trois variantes
/// consécutives de même `stem`, `step` valant 1, 2, 3 ; `rc:0.5` seul est une chaîne d'un pas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chain {
    /// Nom de la chaîne, sans le suffixe de pas : `rc05`, `famroad100-rt05-me`.
    pub stem: String,
    /// De 1 à `steps`.
    pub step: u8,
    pub steps: u8,
    pub dims: Dims,
}

/// Ce que la variante passe à toutes les équipes (3b, §14.12.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SvcKind {
    /// `common` : la table commune du moteur, la même pour toutes les équipes et les deux surcouches.
    Common,
    /// `svc` : par équipe, le profil appris par le code du dépôt, joué sur les deux surcouches.
    Svc,
}

/// Plafond des boucles d'apprentissage de `svc` : `cap:<n>` par équipe (0 : aucun), `capt:<n>` par type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SvcCap {
    Team(usize),
    PerType(usize),
}

/// Variante du chemin du service (3b) : `common`, `svc`, avec `norm`, `cap:<n>` ou `capt:<n>`, et `mix:sonde`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Svc {
    pub kind: SvcKind,
    /// La table commune normalisée, et des pas normalisés.
    pub norm: bool,
    /// `None` : le plafond du service (500 par équipe).
    pub cap: Option<SvcCap>,
    /// `mix:sonde` : le mélange de la seconde sonde (départ neutre, cible commune de chaque pas) au lieu du plafonné.
    pub sonde: bool,
}

/// Les figures organiques d'une variante (lot 4, 4c, §15.11.2) : `org` et ses options.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Org {
    /// `w:<x>` : demi-largeur du couloir en millièmes de D (0 : sans couloir).
    pub w_milli: Option<u16>,
    /// `k:<x>` : κ en 256es.
    pub kappa_q8: Option<u16>,
    /// `tri:0` : aucun triangle (la lentille au couloir seule).
    pub tri0: bool,
    /// `pairs:<n>`.
    pub pairs: Option<u8>,
    /// `wide` : bande [0,25 ; 0,43], écart [25° ; 90°], 4 sous-bandes.
    pub wide: bool,
    /// `boost:<x>` : multiplicateur de la part de triangles, en 256es.
    pub boost_q8: Option<u16>,
    /// `lc` : apprentissage `Classic`, service organique.
    pub lc: bool,
    /// `noretry` : une paire refusée pour un virage serré n'est pas reprise (`sharp_retry = false`, la lettre du §15.7).
    pub noretry: bool,
}

impl Org {
    /// Les réglages fins : ceux du service (`OrganicTune::default()`) puis les options.
    pub fn tune(&self) -> ord_loops::OrganicTune {
        let mut t = ord_loops::OrganicTune::default();
        if let Some(w) = self.w_milli {
            t.corridor_w_milli = w;
        }
        if let Some(k) = self.kappa_q8 {
            t.kappa_q8 = k;
        }
        if let Some(p) = self.pairs {
            t.max_pairs = p;
        }
        if self.wide {
            t.band_milli = [250, 430];
            t.sep_deg = [25, 90];
            t.sub_bands = 4;
        }
        if let Some(b) = self.boost_q8 {
            t.tri_boost_q8 = b;
        }
        if self.noretry {
            t.sharp_retry = false;
        }
        t
    }

    /// Les figures d'une requête de la variante : le mélange `mix` de l'équipe (ou le général) sans triangle sous `tri:0`,
    /// et les réglages fins de la variante.
    pub fn figures(&self, mix: ord_loops::FigureMix) -> ord_loops::Figures {
        let mix = if self.tri0 { ord_loops::FigureMix { tri_q8: 0, ..mix } } else { mix };
        ord_loops::Figures::Organic(ord_loops::Organic { mix, tune: self.tune() })
    }

    /// Réécrit les figures d'une requête de l'apprentissage (`LearnFigures::Organic` : mélange de l'équipe, réglages du
    /// service) avec les options de la variante, pour que l'équipe apprenne ce qu'on lui servira (hors `lc`, où
    /// l'apprentissage est classique et n'est pas touché).
    pub fn rewrite(&self, f: ord_loops::Figures) -> ord_loops::Figures {
        match f {
            ord_loops::Figures::Organic(o) => self.figures(o.mix),
            ord_loops::Figures::Classic => f,
        }
    }
}

/// Une variante : `base`, ou la familiarité et/ou un goût.
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    /// Nom des fichiers et des tableaux : `base`, `fam075`, `ge10`, `ge10-fam100`, `rc05-p2`.
    pub name: String,
    /// Familiarité imposée (équipe d'abord, et « équipe d'abord à λ = 1 » en découverte).
    pub fam: Option<f32>,
    /// `famroad` : la familiarité ne touche que ROAD en équipe d'abord (découverte, strict et GRAVEL intacts).
    pub fam_road_only: bool,
    /// Source et exposant γ du goût.
    pub taste: Option<(TasteSource, f64)>,
    /// `Some` pour `rc` et `rt` (`rc` seul : `steps = 1`, nom sans suffixe).
    pub chain: Option<Chain>,
    /// `Some` pour `common` et `svc` (3b) : ni goût du banc, ni familiarité.
    pub svc: Option<Svc>,
    /// `Some` avec `org` (4c) : figures organiques, sur `common` ou `svc`.
    pub org: Option<Org>,
}

impl Variant {
    pub fn base() -> Self {
        Self {
            name: "base".into(),
            fam: None,
            fam_road_only: false,
            taste: None,
            chain: None,
            svc: None,
            org: None,
        }
    }

    pub fn is_base(&self) -> bool {
        self.fam.is_none() && self.taste.is_none() && self.svc.is_none()
    }

    /// Variante de recalé (commun ou par équipe).
    pub fn is_recal(&self) -> bool {
        matches!(self.taste, Some((TasteSource::Recal | TasteSource::RecalTeam, _)))
    }
}

/// Pose une option à valeur d'`org` (`w`, `k`, `tri`, `pairs`, `boost`).
fn set_option(o: &mut Org, key: &str, x: f64) {
    match key {
        "w" => o.w_milli = Some((x * 1000.0).round() as u16),
        "k" => o.kappa_q8 = Some((x * 256.0).round() as u16),
        "tri" => o.tri0 = true,
        "pairs" => o.pairs = Some(x as u8),
        _ => o.boost_q8 = Some((x * 256.0).round() as u16),
    }
}

fn number(what: &str, s: &str) -> Result<f64> {
    let x: f64 = s.parse().map_err(|_| anyhow!("variante : {what} illisible : « {s} »"))?;
    ensure!(x.is_finite(), "variante : {what} hors bornes : « {s} »");
    Ok(x)
}

/// Une spécification, développée : une variante, ou `k` si elle porte `n:<k>`.
fn one(spec: &str) -> Result<Vec<Variant>> {
    if spec == "base" {
        return Ok(vec![Variant::base()]);
    }
    let (mut fam, mut fam_road_only): (Option<f32>, bool) = (None, false);
    let mut taste: Option<(TasteSource, f64)> = None;
    let (mut n, mut dims): (Option<u8>, Option<Dims>) = (None, None);
    // Nom : les composants dans l'ordre donné ; `dim:me` vient juste après le goût.
    let mut names: Vec<String> = Vec::new();
    let mut taste_pos: Option<usize> = None;
    // Le chemin du service (3b) : `common` ou `svc`, `norm`, `cap:<n>`, `capt:<n>`.
    let (mut kind, mut norm, mut cap): (Option<SvcKind>, bool, Option<SvcCap>) = (None, false, None);
    let mut sonde = false;
    // Les figures organiques (4c) : `org`, puis ses options, dans l'ordre donné pour le nom.
    let mut org: Option<Org> = None;
    // Options données avant `org` : posées quand il paraît.
    let mut pending: Vec<(String, f64)> = Vec::new();
    let mut org_opt: Vec<&str> = Vec::new();
    ensure!(!spec.is_empty(), "variante vide");
    for comp in spec.split('+') {
        match comp {
            "org" => {
                ensure!(org.is_none(), "variante « {spec} » : deux `org`");
                org = Some(Org::default());
                names.push("org".into());
                continue;
            }
            "wide" | "lc" | "noretry" => {
                ensure!(!org_opt.contains(&comp), "variante « {spec} » : deux `{comp}`");
                org_opt.push(comp);
                names.push(comp.into());
                continue;
            }
            "common" | "svc" => {
                ensure!(kind.is_none(), "variante « {spec} » : `common` et `svc` ne vont pas ensemble");
                kind = Some(if comp == "svc" { SvcKind::Svc } else { SvcKind::Common });
                names.push(comp.to_string());
                continue;
            }
            "norm" => {
                ensure!(!norm, "variante « {spec} » : deux `norm`");
                norm = true;
                names.push("norm".into());
                continue;
            }
            _ => {}
        }
        let (key, val) =
            comp.split_once(':').ok_or_else(|| anyhow!("variante : composant inconnu : « {comp} »"))?;
        match key {
            "w" | "k" | "tri" | "pairs" | "boost" => {
                ensure!(!org_opt.contains(&key), "variante « {spec} » : deux `{key}`");
                org_opt.push(key);
                let x = number(&format!("`{key}`"), val)?;
                let name = match key {
                    "w" => {
                        ensure!((0.0..=0.2).contains(&x), "variante : `w` hors de [0, 0,2] : « {val} »");
                        format!("w{:03}", (x * 100.0).round() as u32)
                    }
                    "k" => {
                        ensure!((1.0..=8.0).contains(&x), "variante : `k` hors de [1, 8] : « {val} »");
                        format!("k{:02}", (x * 10.0).round() as u32)
                    }
                    "tri" => {
                        ensure!(x == 0.0, "variante : `tri` ne vaut que 0 (aucun triangle) : « {val} »");
                        "tri0".to_string()
                    }
                    "pairs" => {
                        ensure!(
                            x.fract() == 0.0 && (1.0..=32.0).contains(&x),
                            "variante : `pairs` est un entier de 1 à 32 : « {val} »"
                        );
                        format!("pairs{}", x as u32)
                    }
                    _ => {
                        ensure!((0.1..=4.0).contains(&x), "variante : `boost` hors de [0,1 ; 4] : « {val} »");
                        format!("b{:03}", (x * 100.0).round() as u32)
                    }
                };
                names.push(name);
                // L'option elle-même est posée plus bas, quand `org` est connu.
                if let Some(o) = org.as_mut() {
                    set_option(o, key, x);
                } else {
                    pending.push((key.to_string(), x));
                }
            }
            "cap" | "capt" => {
                ensure!(cap.is_none(), "variante « {spec} » : `cap` et `capt` ne vont pas ensemble");
                let n: usize =
                    val.parse().map_err(|_| anyhow!("variante : `{key}` attend un entier : « {val} »"))?;
                cap = Some(if key == "cap" { SvcCap::Team(n) } else { SvcCap::PerType(n) });
                names.push(format!("{key}{n}"));
            }
            "mix" => {
                ensure!(!sonde, "variante « {spec} » : deux `mix`");
                ensure!(
                    val == "sonde",
                    "variante : `mix` vaut `sonde` (le défaut `capped` est sans suffixe) : « {val} »"
                );
                sonde = true;
                names.push("sonde".into());
            }
            "fam" | "famroad" => {
                ensure!(fam.is_none(), "variante « {spec} » : deux familiarités");
                let f = number("la familiarité", val)?;
                ensure!((0.0..=1.0).contains(&f), "variante : familiarité hors de [0, 1] : « {val} »");
                fam = Some(f as f32);
                fam_road_only = key == "famroad";
                names.push(format!("{key}{:03}", (f * 100.0).round() as u32));
            }
            "ge" | "gg" | "rc" | "rt" => {
                ensure!(taste.is_none(), "variante « {spec} » : deux goûts");
                let g = number("γ", val)?;
                ensure!(g > 0.0 && g <= 4.0, "variante : γ hors de ]0, 4] : « {val} »");
                let src = match key {
                    "ge" => TasteSource::Team,
                    "gg" => TasteSource::General,
                    "rc" => TasteSource::Recal,
                    _ => TasteSource::RecalTeam,
                };
                taste = Some((src, g));
                taste_pos = Some(names.len());
                names.push(format!("{key}{:02}", (g * 10.0).round() as u32));
            }
            "n" => {
                ensure!(n.is_none(), "variante « {spec} » : deux `n`");
                let k: u8 = val
                    .parse()
                    .map_err(|_| anyhow!("variante : `n` doit être un entier de 1 à 5 : « {val} »"))?;
                ensure!((1..=5).contains(&k), "variante : `n` hors de [1, 5] : « {val} »");
                n = Some(k);
            }
            "dim" => {
                ensure!(dims.is_none(), "variante « {spec} » : deux `dim`");
                dims = Some(match val {
                    "72" => Dims::D72,
                    "me" => Dims::Me,
                    _ => bail!("variante : `dim` vaut `72` ou `me` : « {val} »"),
                });
            }
            _ => bail!("variante : composant inconnu : « {comp} »"),
        }
    }
    ensure!(!norm || kind.is_some(), "variante « {spec} » : `norm` demande `common` ou `svc`");
    ensure!(
        org.is_some() || org_opt.is_empty(),
        "variante « {spec} » : `{}` demande `org`",
        org_opt.join("`, `")
    );
    ensure!(org.is_none() || kind.is_some(), "variante « {spec} » : `org` demande `common` ou `svc`");
    if let Some(o) = org.as_mut() {
        for (k, x) in &pending {
            set_option(o, k, *x);
        }
        for flag in &org_opt {
            match *flag {
                "wide" => o.wide = true,
                "noretry" => o.noretry = true,
                "lc" => {
                    ensure!(kind == Some(SvcKind::Svc), "variante « {spec} » : `lc` demande `svc`");
                    o.lc = true;
                }
                _ => {}
            }
        }
    }
    ensure!(
        cap.is_none() || kind == Some(SvcKind::Svc),
        "variante « {spec} » : `cap` et `capt` demandent `svc`"
    );
    ensure!(!sonde || kind == Some(SvcKind::Svc), "variante « {spec} » : `mix` demande `svc`");
    if kind.is_some() {
        ensure!(
            taste.is_none() && fam.is_none() && n.is_none() && dims.is_none(),
            "variante « {spec} » : `common` et `svc` n'acceptent ni autre goût, ni `fam`, ni `famroad`, ni `n`, ni `dim`"
        );
    }
    let svc = kind.map(|kind| Svc { kind, norm, cap, sonde });
    let recal = matches!(taste, Some((TasteSource::Recal | TasteSource::RecalTeam, _)));
    ensure!(
        recal || (n.is_none() && dims.is_none()),
        "variante « {spec} » : `n` et `dim` demandent `rc` ou `rt`"
    );
    if dims == Some(Dims::Me) {
        names.insert(taste_pos.map_or(names.len(), |p| p + 1), "me".into());
    }
    let stem = names.join("-");
    let mk =
        |name: String, chain: Option<Chain>| Variant { name, fam, fam_road_only, taste, chain, svc, org };
    if !recal {
        return Ok(vec![mk(stem, None)]);
    }
    let dims = dims.unwrap_or(Dims::D72);
    match n {
        // `rc:<γ>` sans `n` : la chaîne d'un pas, sans suffixe (la première sonde se rejoue telle quelle).
        None => {
            let chain = Chain { stem: stem.clone(), step: 1, steps: 1, dims };
            Ok(vec![mk(stem, Some(chain))])
        }
        Some(k) => Ok((1..=k)
            .map(|j| {
                let chain = Chain { stem: stem.clone(), step: j, steps: k, dims };
                mk(format!("{stem}-p{j}"), Some(chain))
            })
            .collect()),
    }
}

/// Lit `--variants` : refuse un composant inconnu, un nombre illisible ou hors bornes, deux goûts, `n` ou `dim` sans
/// `rc` / `rt`, un nom en double (après développement des chaînes), et `rc` ou `rt` sans `base` **plus tôt** dans la
/// liste. Liste vide : aucune variante (le banc du 4a, une passe `base`).
pub fn parse_variants(s: &[String]) -> Result<Vec<Variant>> {
    let mut out: Vec<Variant> = Vec::new();
    for spec in s {
        for v in one(spec.trim())? {
            ensure!(out.iter().all(|x| x.name != v.name), "variante en double : « {} »", v.name);
            if v.svc.is_some() {
                ensure!(
                    out.iter().any(Variant::is_base),
                    "variante « {spec} » : `common` et `svc` ont besoin de `base` plus tôt dans la liste"
                );
            }
            if v.is_recal() {
                ensure!(
                    out.iter().any(Variant::is_base),
                    "variante « {spec} » : `rc` et `rt` ont besoin de `base` plus tôt dans la liste"
                );
            }
            out.push(v);
        }
    }
    Ok(out)
}

/// La requête d'une variante : celle du 4a, puis `fam` — en équipe d'abord, `familiarity = f` ; en découverte
/// (curseur 1), `mode = TeamFirst`, λ = 1 (`off_team_cost_q8 = 256`) et `familiarity = f`, car le moteur y ignore
/// l'équipe et la familiarité n'y aurait aucun effet (§12.0, points 2 et 6). Le strict n'est pas touché. Le goût ne
/// change pas la requête : il passe par `WindowTeam`. Avec `road_only` (`famroad`), seule la requête ROAD en équipe
/// d'abord est touchée : la découverte, le strict et le GRAVEL passent tels quels (§13.5).
pub fn adapt_request(mut req: Request, fam: Option<f32>, road_only: bool) -> Request {
    let Some(f) = fam else { return req };
    if road_only {
        if req.ride_type == RideType::Road && req.mode == Mode::TeamFirst {
            req.familiarity = f;
        }
        return req;
    }
    match req.mode {
        Mode::TeamFirst => req.familiarity = f,
        Mode::Discovery => {
            req.mode = Mode::TeamFirst;
            req.off_team_cost_q8 = 256;
            req.familiarity = f;
        }
        Mode::Strict => {}
    }
    req
}

/// Constructions de `WindowTeam` d'une variante.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct TeamWindowCounters {
    pub builds: u64,
    pub ms: f64,
    /// Le plus gros `WindowTeam::bytes()`.
    pub max_bytes: u64,
}

/// La chaîne dont une variante est un pas, telle que le rapport la décrit (§13.6).
#[derive(Clone, Debug, Serialize)]
pub struct ChainMeta {
    pub stem: String,
    pub step: u8,
    pub steps: u8,
    /// `base` au pas 1, sinon le nom de la variante du pas précédent.
    pub learned_from: String,
    /// `72` ou `me`.
    pub dims: String,
    pub team_n0: f64,
    pub mult_bound: f64,
    pub diag: Vec<ChainOwnerStep>,
}

/// L'écart L1 des parts de classes (réel contre généré) que les parcours d'une variante laissent, pour un propriétaire
/// de table et un type : `général` (ou `commun` pour `rt`, sur toutes équipes) ou une lettre.
#[derive(Clone, Debug, Serialize)]
pub struct OwnerGap {
    pub owner: String,
    pub ty: String,
    pub l1: f64,
}

/// Ce qu'une équipe a coûté à jouer une variante (§13.6) : lettre et agrégats seulement.
#[derive(Clone, Debug, Serialize)]
pub struct TeamTime {
    pub team: String,
    pub loops: usize,
    /// Jeux sur la surcouche complète (les générations d'un profil d'équipe appris au dépôt) et leur temps cumulé, s
    /// (`PlayRec::ms` : acquisition des fenêtres et appel du moteur).
    pub plays_full: usize,
    pub cpu_s_full: f64,
    /// `(curseur, s)` : le temps cumulé des jeux `full` à chaque curseur.
    pub cpu_s_full_by_cursor: Vec<(f64, f64)>,
    /// `build_team` des deux surcouches, s.
    pub build_s: f64,
    /// L'équipe entière (constructions et jeux des deux surcouches), s.
    pub wall_s: f64,
}

/// Mètres par classe de goût, pour un type, toutes équipes réunies (lot 4, 3b, §14.3.1) : le réel et le généré du premier
/// parcours gardé, sommés sur les boucles non mises à l'écart (jeux `full`, issue `OK`). **Jamais par équipe** : un
/// agrégat de 72 nombres par type. C'est ce dont la table commune du moteur tire ses poids `R` (décimètres générés par
/// boucle).
#[derive(Clone, Debug, Serialize)]
pub struct TypeMeters {
    /// `ROAD`, `GRAVEL` ou `MTB`.
    pub ride: String,
    pub real_m: Vec<f64>,
    pub gen_m: Vec<f64>,
    /// Boucles comptées (au moins une paire comptée) et paires (boucle, curseur) comptées.
    pub loops: u32,
    pub pairs: u32,
}

/// Les trois types, dans l'ordre `RideType::index()`.
#[derive(Clone, Debug, Serialize)]
pub struct ClassMetersOut {
    pub pooled: Vec<TypeMeters>,
}

/// Ce que chaque variante écrit sur elle-même (le rapport de la variante).
#[derive(Clone, Debug, Serialize)]
pub struct VariantMeta {
    pub name: String,
    pub fam: Option<f32>,
    pub fam_road_only: bool,
    /// `ge`, `gg`, `rc` ou `rt`.
    pub taste: Option<String>,
    pub gamma: Option<f64>,
    pub params: TasteParams,
    /// Tables jouées : (lettre, `général` ou `recalé`) × (`full`, `holdout`) × type, avec leurs parts, g et q8.
    pub tables: Vec<TasteTable>,
    /// xxh3 de toutes les q8, dans l'ordre (lettre, surcouche, type, classe), petit-boutiste.
    pub taste_xxh3: String,
    pub team_windows: TeamWindowCounters,
    pub secs: f64,
    pub peak_anon_mb: f64,
    /// `Some` pour `rc` et `rt`.
    pub chain: Option<ChainMeta>,
    pub l1_after: Vec<OwnerGap>,
    pub team_times: Vec<TeamTime>,
    /// Les mètres par classe de la variante, toutes équipes réunies (§14.3.1).
    pub class_meters: ClassMetersOut,
    /// `common` et `svc` (3b) : empreintes, plafond, et par équipe ce que l'apprentissage a coûté (§14.12.2).
    pub svc: Option<super::svc::SvcMeta>,
}

#[cfg(test)]
mod tests {
    use ord_loops::{LatLon, RideType};

    use super::*;

    fn parse(s: &[&str]) -> Result<Vec<Variant>> {
        parse_variants(&s.iter().map(|x| (*x).to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn names_follow_the_hundredths_and_tenths_rules() {
        let v =
            parse(&["base", "fam:0.75", "fam:1", "ge:0.5", "gg:1", "rc:1", "ge:2", "ge:1+fam:1"]).unwrap();
        let names: Vec<&str> = v.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["base", "fam075", "fam100", "ge05", "gg10", "rc10", "ge20", "ge10-fam100"]);
        assert!(v[0].is_base() && !v[1].is_base());
        assert!(v.iter().all(|x| !x.fam_road_only));
        assert_eq!(v[7].fam, Some(1.0));
        assert_eq!(v[7].taste, Some((TasteSource::Team, 1.0)));
        assert_eq!(parse(&["fam:1+ge:1"]).unwrap()[0].name, "fam100-ge10");
        assert!(parse(&[]).unwrap().is_empty());
    }

    #[test]
    fn bad_variants_are_refused_before_any_work() {
        for bad in [
            vec!["nope:1"],
            vec!["ge"],
            vec!["ge:x"],
            vec!["ge:0"],
            vec!["ge:4.5"],
            vec!["ge:-1"],
            vec!["fam:1.5"],
            vec!["fam:nan"],
            vec!["ge:1+gg:1"],
            vec!["fam:1+fam:0.5"],
            vec!["base", "base"],
            vec!["ge:1", "ge:1.0"],
            vec!["rc:1"],
            vec!["ge:1", "rc:1", "base"],
            vec!["base+fam:1"],
            vec![""],
            // §13.5 : `n` et `dim` demandent `rc` ou `rt`.
            vec!["base", "n:3"],
            vec!["base", "ge:0.5+n:2"],
            vec!["base", "gg:0.5+dim:me"],
            vec!["base", "fam:1+n:2"],
            vec!["base", "rc:0.5+n:0"],
            vec!["base", "rc:0.5+n:6"],
            vec!["base", "rc:0.5+n:1.5"],
            vec!["base", "rc:0.5+n:x"],
            vec!["base", "rc:0.5+dim:36"],
            vec!["base", "rc:0.5+n:2+n:3"],
            vec!["base", "rc:0.5+dim:me+dim:72"],
            vec!["base", "fam:1+famroad:1+rc:0.5"],
            vec!["fam:1+famroad:1"],
            vec!["rt:0.5"],
            vec!["rt:0.5+n:2", "base"],
            vec!["base", "rc:0.5+n:1", "rc:0.5+dim:72+n:1"],
            vec!["base", "rc:0.5+n:3", "rc:0.5+n:1"],
            vec!["base", "famroad:x"],
            vec!["base", "famroad:1.5"],
        ] {
            assert!(parse(&bad).is_err(), "{bad:?} aurait dû être refusée");
        }
        assert!(parse(&["base", "rc:1"]).is_ok());
        assert!(parse(&["base", "ge:1", "rc:1"]).is_ok());
        assert!(parse(&["base", "rt:0.5"]).is_ok());
    }

    #[test]
    fn the_service_path_variants_parse_and_refuse_what_they_cannot_mix() {
        let v = parse(&[
            "base",
            "common",
            "common+norm",
            "svc",
            "svc+norm",
            "svc+cap:0",
            "svc+capt:500",
            "svc+norm+cap:300",
            "svc+mix:sonde",
            "svc+mix:sonde+norm",
            "svc+mix:sonde+capt:500",
        ])
        .unwrap();
        let names: Vec<&str> = v.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "base",
                "common",
                "common-norm",
                "svc",
                "svc-norm",
                "svc-cap0",
                "svc-capt500",
                "svc-norm-cap300",
                "svc-sonde",
                "svc-sonde-norm",
                "svc-sonde-capt500"
            ]
        );
        let sv = |norm, cap, sonde| Some(Svc { kind: SvcKind::Svc, norm, cap, sonde });
        assert_eq!(v[6].svc, sv(false, Some(SvcCap::PerType(500)), false));
        assert_eq!(v[7].svc, sv(true, Some(SvcCap::Team(300)), false));
        assert_eq!(v[8].svc, sv(false, None, true));
        assert_eq!(v[9].svc, sv(true, None, true));
        assert_eq!(v[10].svc, sv(false, Some(SvcCap::PerType(500)), true));
        assert!(v[1].taste.is_none() && v[1].fam.is_none() && !v[1].is_base() && v[3].chain.is_none());
        for bad in [
            vec!["base", "norm"],
            vec!["base", "cap:500"],
            vec!["base", "capt:500"],
            vec!["base", "common+cap:5"],
            vec!["base", "svc+cap:5+capt:5"],
            vec!["base", "svc+fam:1"],
            vec!["base", "svc+rc:0.5"],
            vec!["base", "common+svc"],
            vec!["base", "svc+svc"],
            vec!["base", "svc+norm+norm"],
            vec!["base", "svc+n:2"],
            vec!["base", "svc+cap:x"],
            vec!["svc"],
            vec!["base", "mix:sonde"],
            vec!["base", "common+mix:sonde"],
            vec!["base", "svc+mix:capped"],
            vec!["base", "svc+mix:sonde+mix:sonde"],
            vec!["base", "svc+mix:sonde", "svc+mix:sonde"],
            vec!["common", "base"],
            vec!["base", "svc", "svc"],
            vec!["base", "svc+norm", "svc+norm"],
        ] {
            assert!(parse(&bad).is_err(), "{bad:?} aurait dû être refusée");
        }
    }

    #[test]
    fn the_organic_variants_parse_name_and_refuse_what_cannot_mix() {
        let v = parse(&[
            "base",
            "common",
            "svc",
            "common+org",
            "svc+org",
            "svc+org+w:0",
            "svc+org+w:0.05+k:1.5",
            "svc+org+tri:0",
            "svc+org+pairs:8",
            "svc+org+wide",
            "svc+org+boost:1.3",
            "svc+org+lc",
            "svc+org+tri:0+w:0",
            "svc+org+noretry",
        ])
        .unwrap();
        let names: Vec<&str> = v.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "base",
                "common",
                "svc",
                "common-org",
                "svc-org",
                "svc-org-w000",
                "svc-org-w005-k15",
                "svc-org-tri0",
                "svc-org-pairs8",
                "svc-org-wide",
                "svc-org-b130",
                "svc-org-lc",
                "svc-org-tri0-w000",
                "svc-org-noretry"
            ]
        );
        assert!(v[..3].iter().all(|x| x.org.is_none()));
        let o = |i: usize| v[i].org.expect("org");
        assert_eq!(o(3), Org::default());
        assert_eq!(o(5).tune().corridor_w_milli, 0);
        assert_eq!((o(6).tune().corridor_w_milli, o(6).tune().kappa_q8), (50, 384));
        assert!(o(7).tri0 && o(7).tune() == ord_loops::OrganicTune::default());
        assert_eq!(o(8).tune().max_pairs, 8);
        let wide = o(9).tune();
        assert_eq!((wide.band_milli, wide.sep_deg, wide.sub_bands), ([250, 430], [25, 90], 4));
        assert_eq!(o(10).tune().tri_boost_q8, 333);
        assert!(o(11).lc && !o(4).lc);
        assert!(!o(13).tune().sharp_retry && o(4).tune().sharp_retry);
        // `tri:0` retire la part de triangles du mélange, `org` la laisse.
        let mix = ord_loops::FigureMix { tri_q8: 145, rd_tri_milli: [1, 2, 3, 4, 5] };
        let tri = |f: ord_loops::Figures| match f {
            ord_loops::Figures::Organic(x) => x.mix.tri_q8,
            ord_loops::Figures::Classic => 999,
        };
        assert_eq!((tri(o(4).figures(mix)), tri(o(7).figures(mix))), (145, 0));
        // La réécriture de l'apprentissage applique les options ; un apprentissage classique reste classique.
        assert_eq!(tri(o(7).rewrite(o(4).figures(mix))), 0);
        assert_eq!(tri(o(7).rewrite(ord_loops::Figures::Classic)), 999);
        for bad in [
            vec!["base", "org"],
            vec!["base", "svc+org+org"],
            vec!["base", "svc", "w:0.03"],
            vec!["base", "svc+w:0.03"],
            vec!["base", "svc+wide"],
            vec!["base", "svc+lc"],
            vec!["base", "common+org+lc"],
            vec!["base", "svc+org+w:0.03+w:0.05"],
            vec!["base", "svc+org+wide+wide"],
            vec!["base", "svc+org+lc+lc"],
            vec!["base", "svc+noretry"],
            vec!["base", "svc+org+noretry+noretry"],
            vec!["base", "svc+org+tri:1"],
            vec!["base", "svc+org+pairs:0"],
            vec!["base", "svc+org+pairs:2.5"],
            vec!["base", "svc+org+k:0.5"],
            vec!["base", "svc+org+w:0.9"],
            vec!["base", "svc+org+boost:0"],
            vec!["base", "svc+org+fam:1"],
            vec!["base", "rc:0.5+org"],
            vec!["base", "svc+org", "svc+org"],
            vec!["base", "fam:1+org"],
        ] {
            assert!(parse(&bad).is_err(), "{bad:?} aurait dû être refusée");
        }
    }

    fn chain_of(v: &Variant) -> (String, u8, u8, Dims) {
        let c = v.chain.as_ref().expect("chaîne");
        (c.stem.clone(), c.step, c.steps, c.dims)
    }

    #[test]
    fn a_chain_expands_in_place_into_consecutive_steps() {
        let v = parse(&["base", "rc:0.5+n:3", "fam:1"]).unwrap();
        let names: Vec<&str> = v.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["base", "rc05-p1", "rc05-p2", "rc05-p3", "fam100"]);
        for (k, x) in v.iter().enumerate().skip(1).take(3) {
            assert_eq!(chain_of(x), ("rc05".to_string(), k as u8, 3, Dims::D72));
            assert_eq!(x.taste, Some((TasteSource::Recal, 0.5)));
        }
        assert!(v[0].chain.is_none() && v[4].chain.is_none());
        // `rc:<γ>` sans `n` : une variante, sans suffixe, chaîne d'un pas : la première sonde se rejoue telle quelle.
        let v = parse(&["base", "rc:0.5"]).unwrap();
        assert_eq!(v[1].name, "rc05");
        assert_eq!(chain_of(&v[1]), ("rc05".to_string(), 1, 1, Dims::D72));
        // Les composants dans l'ordre donné, `n` absent du nom, puis `-p<k>` ; `dim:me` après le goût.
        let v = parse(&["base", "famroad:1+rt:0.5+dim:me+n:2"]).unwrap();
        let names: Vec<&str> = v.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["base", "famroad100-rt05-me-p1", "famroad100-rt05-me-p2"]);
        assert_eq!(chain_of(&v[2]), ("famroad100-rt05-me".to_string(), 2, 2, Dims::Me));
        assert_eq!(v[1].taste, Some((TasteSource::RecalTeam, 0.5)));
        assert!(v[1].fam_road_only && v[1].fam == Some(1.0));
        // `dim:72` n'ajoute rien au nom, quelle que soit sa place.
        let v = parse(&["base", "dim:72+rc:0.5+n:1", "rc:1+dim:72"]).unwrap();
        assert_eq!(v[1].name, "rc05-p1");
        assert_eq!(v[2].name, "rc10");
        let v = parse(&["base", "dim:me+rc:0.5"]).unwrap();
        assert_eq!(v[1].name, "rc05-me");
        // Un `rt` seul est aussi une chaîne d'un pas.
        let v = parse(&["base", "rt:1"]).unwrap();
        assert_eq!((v[1].name.as_str(), chain_of(&v[1]).2), ("rt10", 1));
        assert!(v[1].is_recal() && !v[0].is_recal());
    }

    fn req(mode: Mode, q8: u32, fam: f32) -> Request {
        Request {
            start: LatLon::new(47.0, -1.5),
            end: None,
            bearing: None,
            distance_m: 50_000.0,
            ride_type: RideType::Road,
            mode,
            familiarity: fam,
            off_team_cost_q8: q8,
            seed: 1,
            figures: ord_loops::Figures::Classic,
        }
    }

    #[test]
    fn fam_plays_the_discovery_cursor_as_team_first_at_lambda_one() {
        use ord_server::generator::cursor::{apply, hundredths};
        let of = |c: f64| {
            let a = apply(hundredths(c).unwrap());
            req(a.mode, a.off_team_cost_q8, a.familiarity)
        };
        // Curseur 0,25 : équipe d'abord, λ = 2,5 (640), seule la familiarité change.
        let r = adapt_request(of(0.25), Some(0.75), false);
        assert_eq!((r.mode, r.off_team_cost_q8, r.familiarity), (Mode::TeamFirst, 640, 0.75));
        // Curseur 1 : la découverte devient équipe d'abord à λ = 1.
        let r = adapt_request(of(1.0), Some(0.75), false);
        assert_eq!((r.mode, r.off_team_cost_q8, r.familiarity), (Mode::TeamFirst, 256, 0.75));
        // Curseur 0 : strict, inchangé.
        let (before, after) = (of(0.0), adapt_request(of(0.0), Some(0.75), false));
        assert_eq!(after.mode, Mode::Strict);
        assert_eq!(
            (before.off_team_cost_q8, before.familiarity),
            (after.off_team_cost_q8, after.familiarity)
        );
        // Sans `fam`, la requête n'est pas touchée.
        let r = adapt_request(of(1.0), None, false);
        assert_eq!((r.mode, r.off_team_cost_q8), (Mode::Discovery, 256));
    }

    #[test]
    fn famroad_touches_only_road_in_team_first() {
        use ord_server::generator::cursor::{apply, hundredths};
        let of = |c: f64, ride: RideType| {
            let a = apply(hundredths(c).unwrap());
            Request { ride_type: ride, ..req(a.mode, a.off_team_cost_q8, a.familiarity) }
        };
        let key = |r: &Request| (r.mode, r.off_team_cost_q8, r.familiarity);
        // ROAD au curseur 0,25 : équipe d'abord, 640, seule la familiarité change.
        let r = adapt_request(of(0.25, RideType::Road), Some(1.0), true);
        assert_eq!(key(&r), (Mode::TeamFirst, 640, 1.0));
        // ROAD au curseur 1 : la découverte reste telle quelle.
        let (before, after) =
            (of(1.0, RideType::Road), adapt_request(of(1.0, RideType::Road), Some(1.0), true));
        assert_eq!(after.mode, Mode::Discovery);
        assert_eq!(key(&before), key(&after));
        // GRAVEL au curseur 0,25 : la familiarité n'est pas touchée.
        let (before, after) =
            (of(0.25, RideType::Gravel), adapt_request(of(0.25, RideType::Gravel), Some(1.0), true));
        assert_eq!(key(&before), key(&after));
        // Le strict est inchangé, comme sans `road_only`.
        let (before, after) =
            (of(0.0, RideType::Road), adapt_request(of(0.0, RideType::Road), Some(1.0), true));
        assert_eq!((after.mode, key(&before)), (Mode::Strict, key(&after)));
        // Sans familiarité, rien.
        let r = adapt_request(of(0.25, RideType::Road), None, true);
        assert_ne!(r.familiarity, 1.0);
    }
}
