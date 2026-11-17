//! Variantes de la sonde du goût (note lot 4, §12.3) : la syntaxe de `--variants`, la requête que `fam` adapte, et ce
//! que chaque variante écrit sur elle-même.

use anyhow::{anyhow, bail, ensure, Result};
use ord_loops::{Mode, Request};
use serde::Serialize;

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
}

impl TasteSource {
    pub fn code(self) -> &'static str {
        match self {
            Self::Team => "ge",
            Self::General => "gg",
            Self::Recal => "rc",
        }
    }
}

/// Une variante : `base`, ou la familiarité et/ou un goût.
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    /// Nom des fichiers et des tableaux : `base`, `fam075`, `ge10`, `ge10-fam100`.
    pub name: String,
    /// Familiarité imposée (équipe d'abord, et « équipe d'abord à λ = 1 » en découverte).
    pub fam: Option<f32>,
    /// Source et exposant γ du goût.
    pub taste: Option<(TasteSource, f64)>,
}

impl Variant {
    pub fn base() -> Self {
        Self { name: "base".into(), fam: None, taste: None }
    }

    pub fn is_base(&self) -> bool {
        self.fam.is_none() && self.taste.is_none()
    }
}

fn number(what: &str, s: &str) -> Result<f64> {
    let x: f64 = s.parse().map_err(|_| anyhow!("variante : {what} illisible : « {s} »"))?;
    ensure!(x.is_finite(), "variante : {what} hors bornes : « {s} »");
    Ok(x)
}

fn one(spec: &str) -> Result<Variant> {
    if spec == "base" {
        return Ok(Variant::base());
    }
    let (mut fam, mut taste): (Option<f32>, Option<(TasteSource, f64)>) = (None, None);
    let mut names: Vec<String> = Vec::new();
    ensure!(!spec.is_empty(), "variante vide");
    for comp in spec.split('+') {
        let (key, val) =
            comp.split_once(':').ok_or_else(|| anyhow!("variante : composant inconnu : « {comp} »"))?;
        match key {
            "fam" => {
                ensure!(fam.is_none(), "variante « {spec} » : deux familiarités");
                let f = number("la familiarité", val)?;
                ensure!((0.0..=1.0).contains(&f), "variante : familiarité hors de [0, 1] : « {val} »");
                fam = Some(f as f32);
                names.push(format!("fam{:03}", (f * 100.0).round() as u32));
            }
            "ge" | "gg" | "rc" => {
                ensure!(taste.is_none(), "variante « {spec} » : deux goûts");
                let g = number("γ", val)?;
                ensure!(g > 0.0 && g <= 4.0, "variante : γ hors de ]0, 4] : « {val} »");
                let src = match key {
                    "ge" => TasteSource::Team,
                    "gg" => TasteSource::General,
                    _ => TasteSource::Recal,
                };
                taste = Some((src, g));
                names.push(format!("{key}{:02}", (g * 10.0).round() as u32));
            }
            _ => bail!("variante : composant inconnu : « {comp} »"),
        }
    }
    Ok(Variant { name: names.join("-"), fam, taste })
}

/// Lit `--variants` : refuse un composant inconnu, un nombre illisible ou hors bornes, deux goûts, un nom en double,
/// et `rc` sans `base` **plus tôt** dans la liste. Liste vide : aucune variante (le banc du 4a, une passe `base`).
pub fn parse_variants(s: &[String]) -> Result<Vec<Variant>> {
    let mut out: Vec<Variant> = Vec::new();
    for spec in s {
        let v = one(spec.trim())?;
        ensure!(out.iter().all(|x| x.name != v.name), "variante en double : « {} »", v.name);
        if matches!(v.taste, Some((TasteSource::Recal, _))) {
            ensure!(
                out.iter().any(Variant::is_base),
                "variante « {spec} » : `rc` a besoin de `base` plus tôt dans la liste"
            );
        }
        out.push(v);
    }
    Ok(out)
}

/// La requête d'une variante : celle du 4a, puis `fam` — en équipe d'abord, `familiarity = f` ; en découverte
/// (curseur 1), `mode = TeamFirst`, λ = 1 (`off_team_cost_q8 = 256`) et `familiarity = f`, car le moteur y ignore
/// l'équipe et la familiarité n'y aurait aucun effet (§12.0, points 2 et 6). Le strict n'est pas touché. Le goût ne
/// change pas la requête : il passe par `WindowTeam`.
pub fn adapt_request(mut req: Request, fam: Option<f32>) -> Request {
    let Some(f) = fam else { return req };
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

/// Ce que chaque variante écrit sur elle-même (le rapport de la variante).
#[derive(Clone, Debug, Serialize)]
pub struct VariantMeta {
    pub name: String,
    pub fam: Option<f32>,
    /// `ge`, `gg` ou `rc`.
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
        ] {
            assert!(parse(&bad).is_err(), "{bad:?} aurait dû être refusée");
        }
        assert!(parse(&["base", "rc:1"]).is_ok());
        assert!(parse(&["base", "ge:1", "rc:1"]).is_ok());
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
        let r = adapt_request(of(0.25), Some(0.75));
        assert_eq!((r.mode, r.off_team_cost_q8, r.familiarity), (Mode::TeamFirst, 640, 0.75));
        // Curseur 1 : la découverte devient équipe d'abord à λ = 1.
        let r = adapt_request(of(1.0), Some(0.75));
        assert_eq!((r.mode, r.off_team_cost_q8, r.familiarity), (Mode::TeamFirst, 256, 0.75));
        // Curseur 0 : strict, inchangé.
        let (before, after) = (of(0.0), adapt_request(of(0.0), Some(0.75)));
        assert_eq!(after.mode, Mode::Strict);
        assert_eq!(
            (before.off_team_cost_q8, before.familiarity),
            (after.off_team_cost_q8, after.familiarity)
        );
        // Sans `fam`, la requête n'est pas touchée.
        let r = adapt_request(of(1.0), None);
        assert_eq!((r.mode, r.off_team_cost_q8), (Mode::Discovery, 256));
    }
}
