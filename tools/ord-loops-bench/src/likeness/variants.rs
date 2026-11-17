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
}

impl Variant {
    pub fn base() -> Self {
        Self { name: "base".into(), fam: None, fam_road_only: false, taste: None, chain: None }
    }

    pub fn is_base(&self) -> bool {
        self.fam.is_none() && self.taste.is_none()
    }

    /// Variante de recalé (commun ou par équipe).
    pub fn is_recal(&self) -> bool {
        matches!(self.taste, Some((TasteSource::Recal | TasteSource::RecalTeam, _)))
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
    ensure!(!spec.is_empty(), "variante vide");
    for comp in spec.split('+') {
        let (key, val) =
            comp.split_once(':').ok_or_else(|| anyhow!("variante : composant inconnu : « {comp} »"))?;
        match key {
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
    let recal = matches!(taste, Some((TasteSource::Recal | TasteSource::RecalTeam, _)));
    ensure!(
        recal || (n.is_none() && dims.is_none()),
        "variante « {spec} » : `n` et `dim` demandent `rc` ou `rt`"
    );
    if dims == Some(Dims::Me) {
        names.insert(taste_pos.map_or(names.len(), |p| p + 1), "me".into());
    }
    let stem = names.join("-");
    let mk = |name: String, chain: Option<Chain>| Variant { name, fam, fam_road_only, taste, chain };
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
