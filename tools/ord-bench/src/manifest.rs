//! `bench/manifest.yaml` (T05 §8 : `Manifest`, `ManifestEntry`, `Serie`).

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{bail, Context};
use serde_yaml_ng::Value;

use crate::hash::sha256_bytes;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Serie {
    Calibration,
    TenueALEcart,
}

impl Serie {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s {
            "calibration" => Ok(Self::Calibration),
            "tenue_a_l_ecart" => Ok(Self::TenueALEcart),
            other => bail!("série inconnue : {other}"),
        }
    }
}

/// Fichier versionné du banc avec son empreinte (`sha256` brut du manifest : une empreinte de
/// 64 caractères hexadécimaux, ou un texte qui renvoie aux `SHA256SUMS`).
#[derive(Clone, Debug)]
pub struct FileEntry {
    pub id: String,
    pub fichier: String,
    pub sha256: String,
    pub serie: Serie,
}

/// Copie d'un dépôt externe et son fichier `SHA256SUMS`.
#[derive(Clone, Debug)]
pub struct ExternalCopy {
    pub name: String,
    pub copie: String,
    pub sommes: String,
    pub sha256_sommes: String,
}

/// Fixture hors git (section `hors_git` : activité enregistrée, jamais versionnée), lue dans
/// `bench/private/`.
#[derive(Clone, Debug)]
pub struct HorsGitEntry {
    pub id: String,
    pub fichier: String,
    pub sha256: String,
}

/// Trace barométrique personnelle (hors git) : jamais de coordonnée dans le manifest.
#[derive(Clone, Debug)]
pub struct Trace {
    pub id: String,
    pub groupe: String,
    pub serie: Serie,
    pub fichier: String,
    pub sha256: Option<String>,
    pub sha256_trim1km: Option<String>,
    pub passages_masques: Vec<String>,
    /// Verdict de `qualify_baro.py` (Q1).
    pub verdict: Option<String>,
    /// Distance, D+ enregistré et D+ MNT de `qualify_baro.py` (groupe E2b d'une trace nouvelle,
    /// défaut attesté d'un négatif Q1 nouveau ; README §4 ter.4).
    pub dist_km: Option<f64>,
    pub gain_baro_m: Option<f64>,
    pub gain_mnt_m: Option<f64>,
    /// Bruit baro de `qualify_baro.py` (`noise_m`) : E1 plaine incertaine au-delà de 1,0 m
    /// (README §4 ter.8).
    pub noise_m: Option<f64>,
    /// Mois de départ (`mois`, `AAAA-MM`) : contrôle de la marque `reference_baro`.
    pub mois: Option<String>,
    /// Marque `reference_baro` du manifest (README §4 sexies.2, d4aad9a, 1c9edf8) :
    /// `periode_exclue` (départ de 2015-05 à 2020-09) ou `non_qualifiee` (avant 2014-12 sans
    /// qualification individuelle) ; la trace ne fournit alors aucune référence baro.
    pub reference_baro: Option<String>,
}

/// Valeurs de `reference_baro` (README §4 sexies.2) : la trace sort des références baro d'E1, E2,
/// E2b, E3 et E5 (rapportée `periode_baro_exclue`) ; elle reste un cas pour M1 et la typologie.
pub const REFERENCE_BARO_EXCLUE: [&str; 2] = ["periode_exclue", "non_qualifiee"];
/// Période exclue des références baro (mois de départ, bornes comprises ; README §1.3, §4 sexies).
pub const PERIODE_EXCLUE: (&str, &str) = ("2015-05", "2020-09");
/// Avant ce mois, une trace sans qualification individuelle est `non_qualifiee` (§4 sexies.1).
pub const QUALIFICATION_AVANT: &str = "2014-12";
/// Qualification individuelle d'une trace d'avant 2014-12 (README §4 sexies.1, 1c9edf8) : verdict
/// `fiable`, bruit ≤ 1,0 m et D+ baro / D+ MNT brut dans les bornes par trace d'E2.
const QUALIFICATION_BRUIT_MAX_M: f64 = 1.0;
const QUALIFICATION_RATIO: (f64, f64) = (0.90, 1.20);

impl Trace {
    /// La trace ne fournit aucune référence baro (`reference_baro` du manifest, §4 sexies.2).
    pub fn baro_exclue(&self) -> bool {
        self.reference_baro.as_deref().is_some_and(|r| REFERENCE_BARO_EXCLUE.contains(&r))
    }

    /// Qualification individuelle (README §4 sexies.1) lue au manifest (`qualification`).
    pub fn qualifiee(&self) -> bool {
        let ratio = match (self.gain_baro_m, self.gain_mnt_m) {
            (Some(b), Some(m)) if m > 0.0 => b / m,
            _ => f64::NAN,
        };
        self.verdict.as_deref() == Some("fiable")
            && self.noise_m.is_some_and(|n| n <= QUALIFICATION_BRUIT_MAX_M)
            && ratio >= QUALIFICATION_RATIO.0
            && ratio <= QUALIFICATION_RATIO.1
    }
}

/// Marques `reference_baro` contraires au mois de départ (README §4 sexies.2 ; `check` refuse) :
/// une trace (hors négatifs Q1, non concernés) dont le mois est dans 2015-05 → 2020-09 porte
/// `periode_exclue`, et elle seule ; avant 2014-12, `non_qualifiee` si et seulement si la
/// qualification individuelle manque ; toute autre valeur, ou une marque sans mois, est refusée.
pub fn reference_baro_failures(traces: &[Trace]) -> Vec<String> {
    let mut out = Vec::new();
    for t in traces.iter().filter(|t| t.groupe != "negatif") {
        let mark = t.reference_baro.as_deref();
        if let Some(m) = mark.filter(|m| !REFERENCE_BARO_EXCLUE.contains(m)) {
            out.push(format!("{} : reference_baro {m} inconnue (README §4 sexies.2)", t.id));
            continue;
        }
        let Some(mois) = t.mois.as_deref() else {
            if mark.is_some() {
                out.push(format!("{} : reference_baro sans mois de départ, incontrôlable", t.id));
            }
            continue;
        };
        let in_period = mois >= PERIODE_EXCLUE.0 && mois <= PERIODE_EXCLUE.1;
        let before = mois < QUALIFICATION_AVANT;
        let expected = if in_period {
            Some("periode_exclue")
        } else if before && !t.qualifiee() {
            Some("non_qualifiee")
        } else {
            None
        };
        if mark != expected {
            out.push(format!(
                "{} : reference_baro {} contraire au mois {mois} (attendu : {}, README §4 sexies.2)",
                t.id,
                mark.unwrap_or("absente"),
                expected.unwrap_or("aucune marque")
            ));
        }
    }
    out
}

/// Itinéraire planifié de la série V1 (`perso_v1.planifies`) : sans donnée perso, hors git
/// jusqu'au jugement V1. Sa série est lue dans `series-v1.csv` (`calibration`,
/// `tenue_a_l_ecart` ou `par_cas`).
#[derive(Clone, Debug, Default)]
pub struct Planned {
    pub id: String,
    /// `E4`, `S2` ou `R1_R2`.
    pub role: String,
    pub nature: Option<String>,
    pub fichier: String,
    pub sha256: String,
    /// Référence R1/R2 au format osm-surface (`reference`, `sha256_reference`).
    pub reference: Option<(String, String)>,
    /// Empreintes de `<id>.surface.csv` et `<id>.baro.csv` de la réponse Valhalla d'un parcours
    /// R1/R2 (`sha256_surface`, `sha256_baro` ; README §4 bis.8, 40c65cd).
    pub sha256_surface: Option<String>,
    pub sha256_baro: Option<String>,
    /// Témoin retiré (`retire`) : l'itinéraire reste un cas pour ses ouvrages.
    pub retire: Option<String>,
}

/// Section d'une série nouvelle du manifest (`perso_v1` pour V1, `perso_v11` pour V1.1,
/// `perso_v12` pour V1.2, `perso_v13` pour V1.3) : index
/// des ouvrages, témoins et séries (hors git, par empreinte), itinéraires planifiés et
/// identifiants des traces de la série. Le recalage et l'instantané OSM de ses références sont lus
/// dans la section, sinon dans `serie_<clé>` (V1.1 : `serie_v11.recalage`,
/// `serie_v11.osm_tags_reference`).
#[derive(Clone, Debug, Default)]
pub struct PersoV1 {
    pub ouvrages: (String, String),
    pub temoins: (String, String),
    /// Ouvrages réels sur la voie des témoins (`temoins_v1_tags`, README §4 ter.6, 6ed1da7) ;
    /// `None` : entrée absente du manifest, aucune plage.
    pub temoins_tags: Option<(String, String)>,
    pub series: (String, String),
    /// Retard du baro gelé de la calibration (`retard_baro_gele`, README §4 ter.9, 48ca44a) :
    /// δ des traces de calibration, appliqué à leurs extraits au jugement ; `None` : entrée
    /// absente du manifest, extraits au baro brut.
    pub retard_gele: Option<(String, String)>,
    /// Date des données du serveur Valhalla des références de revêtement (`recalage.donnees_serveur`).
    pub donnees_serveur: Option<String>,
    /// Instantané des tags OSM des voies de ces références (`osm_tags_reference` : fichier,
    /// empreinte, date ; README §4 bis.8).
    pub osm_tags: Option<(String, String, String)>,
    /// Instantanés de toutes les voies des références R1/R2 (`osm_tags_toutes` : fichier, empreinte,
    /// date ; README §4 quater.1, d6fd42c) : un à la date OSM des tuiles de la campagne, un à la date
    /// des données du serveur Valhalla. Vide : règle `tags_differents` non appliquée.
    pub osm_tags_toutes: Vec<(String, String, String)>,
    /// Campagne de tuiles de la série (`tuiles: tuiles_<date>`), sinon la dernière du manifest.
    pub tuiles: Option<String>,
    pub planifies: Vec<Planned>,
    pub traces: BTreeSet<String>,
    /// Série V1 jugée (README §2, point 3 ; `with_v1_judged`) : ses éléments tenus à l'écart
    /// (`ouvrages-v1.json`, `temoins-v1.json`, `temoins-v1-tags.json`, `series-v1.csv`) se lisent
    /// en calibration.
    pub judged: bool,
}

/// Campagne de tuiles de rues datée (`tuiles_<date>` du manifest ; décision de l'utilisateur,
/// 2026-11-02) : espace du cache de banc et date OSM des tuiles (TileJSON,
/// `planetiler_osm_osmosisreplicationtime`). Déclarée, elle verrouille les tuiles de rues :
/// `fill-cache` n'en télécharge aucune (README §2.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TileCampaign {
    pub id: String,
    pub espace_cache: String,
    pub date_osm: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Manifest {
    pub sha256: String,
    /// Campagnes de tuiles de rues datées (`tuiles_*`), dans l'ordre des clés.
    pub tuiles: Vec<TileCampaign>,
    pub vcyclist: Vec<FileEntry>,
    pub osm_surface: Vec<FileEntry>,
    pub copies: Vec<ExternalCopy>,
    pub ouvrages_tenus: BTreeSet<String>,
    /// `(fichier, sha256)` de l'extension perso des ouvrages.
    pub ouvrages_perso: Option<(String, String)>,
    pub traces: Vec<Trace>,
    pub hors_git: Vec<HorsGitEntry>,
    /// Section `perso_v1` (série V1), absente avant sa constitution.
    pub v1: Option<PersoV1>,
    /// Sections des séries tenues à l'écart entières après V1, dans l'ordre de `SECTIONS`
    /// (`perso_v11` : V1.1, décision de l'utilisateur, 2026-11-02 ; `perso_v12` : V1.2, README
    /// §4 quinquies ; `perso_v13` : V1.3, README §4 septies, b34b23f) ; une section absente du
    /// manifest n'y figure pas, ses cas ne sont jamais lus.
    /// Une série suivante s'ajoute par ses clés dans `SECTIONS`, sans autre changement ici.
    pub apres_v1: Vec<(&'static SectionKeys, PersoV1)>,
    /// Série tenue à l'écart du MVP jugée (`judgements.md`) : devenue calibration (README §2,
    /// point 3) ; ses traces, ses ouvrages et leurs fenêtres masquées le sont pour V1.
    pub mvp_judged: bool,
}

fn s(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn req(v: &Value, key: &str, ctx: &str) -> anyhow::Result<String> {
    s(v, key).with_context(|| format!("{ctx} : champ `{key}` manquant"))
}

fn f(v: &Value, key: &str) -> Option<f64> {
    v.get(key)?.as_f64()
}

/// Traces d'une section (`perso`, `perso_v1`) du manifest.
fn parse_traces(section: Option<&Value>, name: &str) -> anyhow::Result<Vec<Trace>> {
    let mut traces = Vec::new();
    if let Some(Value::Sequence(items)) = section.and_then(|p| p.get("traces")) {
        for it in items {
            let id = req(it, "id", name)?;
            let passages_masques = match it.get("passages_masques") {
                Some(Value::Sequence(v)) => v.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect(),
                _ => Vec::new(),
            };
            let q = it.get("qualification");
            traces.push(Trace {
                groupe: req(it, "groupe", &id)?,
                serie: Serie::parse(&req(it, "serie", &id)?)?,
                fichier: req(it, "fichier", &id)?,
                sha256: s(it, "sha256"),
                sha256_trim1km: s(it, "sha256_trim1km"),
                passages_masques,
                verdict: q.and_then(|q| s(q, "verdict")),
                dist_km: f(it, "dist_km"),
                gain_baro_m: q.and_then(|q| f(q, "gain_baro_m")),
                gain_mnt_m: q.and_then(|q| f(q, "gain_mnt_m")),
                noise_m: q.and_then(|q| f(q, "noise_m")),
                mois: s(it, "mois"),
                reference_baro: s(it, "reference_baro"),
                id,
            });
        }
    }
    Ok(traces)
}

fn files(root: &Value, section: &str) -> anyhow::Result<Vec<FileEntry>> {
    let Some(Value::Sequence(items)) = root.get(section) else { bail!("section `{section}` absente") };
    items
        .iter()
        .map(|it| {
            let id = req(it, "id", section)?;
            Ok(FileEntry {
                fichier: req(it, "fichier", &id)?,
                sha256: req(it, "sha256", &id)?,
                serie: Serie::parse(&req(it, "serie", &id)?)?,
                id,
            })
        })
        .collect()
}

/// Clés du manifest d'une série nouvelle : section des cas, section de la série (tirage,
/// recalage, instantané OSM), suffixe des index et incrément.
#[derive(Debug)]
pub struct SectionKeys {
    pub perso: &'static str,
    pub serie: &'static str,
    pub suffix: &'static str,
    pub name: &'static str,
}

pub const SECTION_V1: SectionKeys =
    SectionKeys { perso: "perso_v1", serie: "serie_v1", suffix: "v1", name: "V1" };
pub const SECTION_V11: SectionKeys =
    SectionKeys { perso: "perso_v11", serie: "serie_v11", suffix: "v11", name: "V1.1" };
pub const SECTION_V12: SectionKeys =
    SectionKeys { perso: "perso_v12", serie: "serie_v12", suffix: "v12", name: "V1.2" };
/// Série tenue à l'écart V1.3 (README §4 septies ; manifest `perso_v13`, `serie_v13`,
/// `tuiles_2026_11_04`, b34b23f).
pub const SECTION_V13: SectionKeys =
    SectionKeys { perso: "perso_v13", serie: "serie_v13", suffix: "v13", name: "V1.3" };

/// Séries nouvelles du manifest, dans l'ordre des incréments. V1 d'abord (champ `v1`), puis les
/// séries tenues à l'écart entières (`apres_v1`) : une série suivante s'ajoute ici, avec ses clés
/// du manifest, une fois sa section écrite par data-init.
pub const SECTIONS: [&SectionKeys; 4] = [&SECTION_V1, &SECTION_V11, &SECTION_V12, &SECTION_V13];

/// Clés d'un incrément (`V1`, `V1.1`, `V1.2`, `V1.3`).
pub fn section_keys(name: &str) -> Option<&'static SectionKeys> {
    SECTIONS.into_iter().find(|k| k.name == name)
}

/// Section d'une série nouvelle (`perso_v1`, `perso_v11`, `perso_v12`, `perso_v13`) ; ses traces
/// sont ajoutées à `traces`.
/// Absente : `None`, ses cas ne sont jamais lus.
fn parse_section(
    root: &Value,
    keys: &SectionKeys,
    traces: &mut Vec<Trace>,
) -> anyhow::Result<Option<PersoV1>> {
    // Section des cas : `perso_<clé>` ; à défaut, `serie_<clé>` si elle porte les index des cas.
    let index = format!("ouvrages_{}", keys.suffix);
    let Some(p) = root.get(keys.perso).or_else(|| root.get(keys.serie).filter(|x| x.get(&index).is_some()))
    else {
        return Ok(None);
    };
    let serie = root.get(keys.serie);
    let section_traces = parse_traces(Some(p), keys.perso)?;
    let suffixed = |base: &str| format!("{base}_{}", keys.suffix);
    let pair_of = |e: &Value, key: &str| -> anyhow::Result<(String, String)> {
        Ok((req(e, "fichier", key)?, req(e, "sha256", key)?))
    };
    let pair = |base: &str| -> anyhow::Result<(String, String)> {
        let key = suffixed(base);
        let e = p.get(&key).with_context(|| format!("{} : `{key}` manquant", keys.perso))?;
        pair_of(e, &key)
    };
    let optional = |key: &str| -> anyhow::Result<Option<(String, String)>> {
        p.get(key).map(|e| pair_of(e, key)).transpose()
    };
    // Recalage et instantané OSM : dans la section, sinon dans `serie_<clé>`.
    let local = |key: &str| p.get(key).or_else(|| serie.and_then(|x| x.get(key)));
    let planifies = match p.get("planifies") {
        Some(Value::Sequence(items)) => items
            .iter()
            .map(|it| {
                let id = req(it, "id", "planifies")?;
                Ok(Planned {
                    role: req(it, "role", &id)?,
                    nature: s(it, "nature"),
                    fichier: req(it, "fichier", &id)?,
                    sha256: req(it, "sha256", &id)?,
                    reference: s(it, "reference").zip(s(it, "sha256_reference")),
                    sha256_surface: s(it, "sha256_surface"),
                    sha256_baro: s(it, "sha256_baro"),
                    retire: s(it, "retire"),
                    id,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?,
        _ => Vec::new(),
    };
    let ids = section_traces.iter().map(|t| t.id.clone()).collect();
    traces.extend(section_traces);
    Ok(Some(PersoV1 {
        ouvrages: pair("ouvrages")?,
        temoins: pair("temoins")?,
        temoins_tags: optional(&format!("temoins_{}_tags", keys.suffix))?,
        series: pair("series")?,
        retard_gele: p.get("retard_baro_gele").map(|e| pair_of(e, "retard_baro_gele")).transpose()?,
        donnees_serveur: local("recalage").and_then(|r| s(r, "donnees_serveur")),
        osm_tags: match local("osm_tags_reference") {
            Some(e) => {
                let (fichier, sum) = pair_of(e, "osm_tags_reference")?;
                Some((fichier, sum, req(e, "date", "osm_tags_reference")?))
            }
            None => None,
        },
        osm_tags_toutes: match local("osm_tags_toutes") {
            Some(Value::Sequence(items)) => items
                .iter()
                .map(|e| {
                    let (fichier, sum) = pair_of(e, "osm_tags_toutes")?;
                    Ok((fichier, sum, req(e, "date", "osm_tags_toutes")?))
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
            _ => Vec::new(),
        },
        tuiles: local("tuiles").and_then(|v| v.as_str().map(str::to_owned)),
        planifies,
        traces: ids,
        judged: false,
    }))
}

impl Manifest {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let root: Value = serde_yaml_ng::from_str(text).context("manifest.yaml illisible")?;
        let copies = match root.get("depots_externes") {
            Some(Value::Mapping(m)) => m
                .iter()
                .map(|(k, v)| {
                    let name = k.as_str().unwrap_or_default().to_owned();
                    Ok(ExternalCopy {
                        copie: req(v, "copie", &name)?,
                        sommes: req(v, "sommes", &name)?,
                        sha256_sommes: req(v, "sha256_sommes", &name)?,
                        name,
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
            _ => Vec::new(),
        };
        let ouvrages = root.get("ouvrages").context("section `ouvrages` absente")?;
        let ouvrages_tenus = match ouvrages.get("serie_tenue_a_l_ecart") {
            Some(Value::Sequence(ids)) => ids.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
            _ => BTreeSet::new(),
        };
        let ouvrages_perso =
            ouvrages.get("ouvrages_perso").and_then(|p| Some((s(p, "fichier")?, s(p, "sha256")?)));
        let mut traces = parse_traces(root.get("perso"), "perso")?;
        let v1 = parse_section(&root, &SECTION_V1, &mut traces)?;
        let mut apres_v1 = Vec::new();
        for keys in SECTIONS.into_iter().skip(1) {
            if let Some(sec) = parse_section(&root, keys, &mut traces)? {
                apres_v1.push((keys, sec));
            }
        }
        let hors_git = match root.get("hors_git") {
            Some(Value::Sequence(items)) => items
                .iter()
                .map(|it| {
                    let id = req(it, "id", "hors_git")?;
                    Ok(HorsGitEntry {
                        fichier: req(it, "fichier", &id)?,
                        sha256: req(it, "sha256", &id)?,
                        id,
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
            _ => Vec::new(),
        };
        let tuiles = match &root {
            Value::Mapping(m) => m
                .iter()
                .filter_map(|(k, v)| {
                    let id = k.as_str().filter(|k| k.starts_with("tuiles_"))?;
                    Some(TileCampaign {
                        id: id.to_owned(),
                        espace_cache: s(v, "espace_cache")?,
                        date_osm: v
                            .get("date_osm")
                            .and_then(|d| s(d, "planetiler_osm_osmosisreplicationtime")),
                    })
                })
                .collect(),
            _ => Vec::new(),
        };
        Ok(Self {
            hors_git,
            tuiles,
            sha256: sha256_bytes(text.as_bytes()),
            vcyclist: files(&root, "vcyclist")?,
            osm_surface: files(&root, "osm_surface")?,
            copies,
            ouvrages_tenus,
            ouvrages_perso,
            traces,
            v1,
            apres_v1,
            mvp_judged: false,
        })
    }

    /// Série MVP jugée (README §2, point 3) : ses traces et ses ouvrages tenus à l'écart deviennent
    /// calibration, et les fenêtres masquées des traces du MVP (ouvrages d'une autre série au MVP)
    /// n'ont plus lieu d'être. Les traces V1, V1.1, V1.2 et V1.3 gardent leur série et leurs
    /// fenêtres.
    pub fn with_mvp_judged(mut self) -> Self {
        // Traces des séries nouvelles (V1 à V1.3) : leur série et leurs fenêtres restent.
        let new: BTreeSet<String> = self.sections().flat_map(|v| v.traces.iter().cloned()).collect();
        for t in self.traces.iter_mut().filter(|t| !new.contains(&t.id)) {
            t.serie = Serie::Calibration;
            t.passages_masques.clear();
        }
        self.ouvrages_tenus.clear();
        self.mvp_judged = true;
        self
    }

    /// Série V1 jugée (README §2, point 3 ; décision de l'utilisateur, 2026-11-02 : « la série V1
    /// passe en calibration ») : ses traces deviennent calibration, sans fenêtres masquées ; ses
    /// ouvrages, croisements, témoins et itinéraires tenus à l'écart sont lus en calibration
    /// (`PersoV1::judged`). La série jugée n'a plus ni masques ni extraits. Les cas absents du
    /// manifest ne sont jamais lus.
    pub fn with_v1_judged(self) -> Self {
        self.with_judged("V1")
    }

    /// Série nouvelle `name` (`V1`, `V1.1`, `V1.2`, `V1.3`) jugée (README §2, point 3) : comme
    /// `with_v1_judged`.
    pub fn with_judged(mut self, name: &str) -> Self {
        let slot = if name == SECTION_V1.name {
            self.v1.as_mut()
        } else {
            self.apres_v1.iter_mut().find(|(k, _)| k.name == name).map(|(_, v)| v)
        };
        let Some(sec) = slot else { return self };
        sec.judged = true;
        for t in self.traces.iter_mut().filter(|t| sec.traces.contains(&t.id)) {
            t.serie = Serie::Calibration;
            t.passages_masques.clear();
        }
        self
    }

    /// Sections des séries nouvelles présentes au manifest, avec leur incrément, dans l'ordre (V1,
    /// V1.1, V1.2, puis V1.3).
    pub fn named_sections(&self) -> impl Iterator<Item = (&'static str, &PersoV1)> {
        self.v1
            .as_ref()
            .map(|v| (SECTION_V1.name, v))
            .into_iter()
            .chain(self.apres_v1.iter().map(|(k, v)| (k.name, v)))
    }

    /// Sections des séries nouvelles présentes au manifest (V1, V1.1, V1.2, puis V1.3).
    pub fn sections(&self) -> impl Iterator<Item = &PersoV1> {
        self.named_sections().map(|(_, v)| v)
    }

    /// Section d'un incrément (`V1`, `V1.1`, `V1.2`, `V1.3`).
    pub fn section(&self, name: &str) -> Option<&PersoV1> {
        self.named_sections().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    /// Trace ou itinéraire d'une série nouvelle (V1 à V1.3) : ses fichiers ne sont jamais ouverts
    /// tant que la série est tenue à l'écart, extraits compris.
    pub fn is_new_series(&self, id: &str) -> bool {
        self.sections().any(|v| v.traces.contains(id) || v.planifies.iter().any(|p| p.id == id))
    }

    /// Section dont le recalage a produit la référence Valhalla d'un cas (trace ou itinéraire) :
    /// celle du cas, ou `perso_v1` pour une trace hors des séries nouvelles (MVP, recalée avec la
    /// calibration V1 ; manifest `perso_v1.recalage`).
    pub fn recalage_section(&self, id: &str) -> Option<&PersoV1> {
        self.sections()
            .find(|v| v.traces.contains(id) || v.planifies.iter().any(|p| p.id == id))
            .or(self.v1.as_ref())
    }

    /// Incrément tenu à l'écart que `judge` et `gel` peuvent viser (à partir de V1) : la section
    /// doit être au manifest, non jugée, et être la seule série nouvelle non jugée (la campagne
    /// de la série tenue à l'écart mesure tout ce qui l'est).
    pub fn held_out_increment(&self, increment: &str) -> anyhow::Result<&PersoV1> {
        let sec = self.section(increment).with_context(|| {
            format!(
                "incrément {increment} : section de ses cas absente du manifest ({})",
                SECTIONS.map(|k| k.perso).join(", ")
            )
        })?;
        anyhow::ensure!(!sec.judged, "incrément {increment} déjà jugé (judgements.md)");
        let others: Vec<&str> =
            self.named_sections().filter(|(n, v)| !v.judged && *n != increment).map(|(n, _)| n).collect();
        anyhow::ensure!(
            others.is_empty(),
            "incrément {increment} : série(s) {} encore tenue(s) à l'écart, à juger d'abord",
            others.join(", ")
        );
        anyhow::ensure!(
            !self.traces.iter().any(|t| t.serie == Serie::TenueALEcart && !sec.traces.contains(&t.id)),
            "incrément {increment} : traces tenues à l'écart hors de sa section"
        );
        Ok(sec)
    }

    /// Campagne de tuiles d'une section : celle qu'elle nomme (`tuiles`), sinon la dernière.
    pub fn tile_campaign(&self, sec: &PersoV1) -> Option<&TileCampaign> {
        match &sec.tuiles {
            Some(id) => self.tuiles.iter().find(|t| &t.id == id),
            None => self.tuiles.last(),
        }
    }

    /// Trace de la section `perso_v1`.
    pub fn is_v1(&self, id: &str) -> bool {
        self.v1.as_ref().is_some_and(|v| v.traces.contains(id))
    }

    pub fn load(bench: &Path) -> anyhow::Result<Self> {
        let path = bench.join("manifest.yaml");
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("lecture de {}", path.display()))?;
        let manifest = Self::parse(&text)?;
        // Après le jugement du MVP, sa série tenue à l'écart est de la calibration (README §2,
        // point 3) : lu dans `judgements.md`, jamais deviné.
        let judgements = std::fs::read_to_string(bench.join("judgements.md")).unwrap_or_default();
        let manifest = if crate::judge::already_judged(&judgements, "MVP") {
            manifest.with_mvp_judged()
        } else {
            manifest
        };
        // De même pour V1 (README §2, point 3), jugée le 2026-11-02, puis V1.1, V1.2 et V1.3.
        let mut manifest = manifest;
        for name in SECTIONS.map(|k| k.name) {
            if crate::judge::already_judged(&judgements, name) {
                manifest = manifest.with_judged(name);
            }
        }
        Ok(manifest)
    }

    pub fn hors_git(&self, id: &str) -> Option<&HorsGitEntry> {
        self.hors_git.iter().find(|e| e.id == id)
    }

    pub fn trace(&self, id: &str) -> Option<&Trace> {
        self.traces.iter().find(|t| t.id == id)
    }
}

/// Une empreinte du manifest est-elle un SHA-256 simple ?
pub fn is_plain_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(id: &str, groupe: &str, mois: Option<&str>, mark: Option<&str>) -> Trace {
        Trace {
            id: id.into(),
            groupe: groupe.into(),
            serie: Serie::Calibration,
            fichier: String::new(),
            sha256: None,
            sha256_trim1km: None,
            passages_masques: Vec::new(),
            verdict: Some("fiable".into()),
            dist_km: None,
            gain_baro_m: Some(986.0),
            gain_mnt_m: Some(933.0),
            noise_m: Some(0.9),
            mois: mois.map(str::to_owned),
            reference_baro: mark.map(str::to_owned),
        }
    }

    /// README §4 sexies.2 (d4aad9a, 1c9edf8) : marque `reference_baro` contrôlée contre `mois` ;
    /// négatifs Q1 non concernés.
    #[test]
    fn the_reference_baro_mark_follows_the_month() {
        let unqualified = Trace {
            noise_m: Some(1.19),
            gain_baro_m: Some(869.0),
            gain_mnt_m: Some(706.0),
            ..trace("u", "plaine", Some("2014-06"), Some("non_qualifiee"))
        };
        let ok = [
            trace("a", "plaine", Some("2015-05"), Some("periode_exclue")),
            trace("b", "dplus", Some("2020-09"), Some("periode_exclue")),
            trace("c", "plaine", Some("2020-10"), None),
            trace("d", "plaine", Some("2015-04"), None),
            // Qualifiée (verdict fiable, bruit 0,9 m, rapport 1,06) : pas de marque.
            trace("e", "plaine", Some("2014-09"), None),
            unqualified.clone(),
            trace("neg", "negatif", Some("2016-01"), None),
            trace("sans-mois", "plaine", None, None),
        ];
        assert!(reference_baro_failures(&ok).is_empty(), "{:?}", reference_baro_failures(&ok));
        assert!(ok[0].baro_exclue() && unqualified.baro_exclue() && !ok[2].baro_exclue());
        assert!(ok[4].qualifiee() && !unqualified.qualifiee());
        let bad = [
            trace("p", "plaine", Some("2016-03"), None),
            trace("q", "plaine", Some("2021-01"), Some("periode_exclue")),
            trace("r", "plaine", Some("2014-09"), Some("non_qualifiee")),
            Trace { mois: Some("2014-06".into()), reference_baro: None, ..unqualified },
            trace("s", "plaine", Some("2016-03"), Some("non_qualifiee")),
            trace("t", "plaine", Some("2016-03"), Some("autre")),
            trace("v", "plaine", None, Some("periode_exclue")),
        ];
        let f = reference_baro_failures(&bad);
        assert_eq!(f.len(), bad.len(), "{f:?}");
        assert!(f[1].starts_with("q : reference_baro periode_exclue contraire au mois 2021-01"), "{f:?}");
    }

    /// Manifest du dépôt : marques cohérentes ; `perso-130` qualifiée, `perso-166` non.
    #[test]
    fn repo_reference_baro_marks_are_coherent() {
        let bench = crate::BenchPaths::repo_bench();
        let m = Manifest::parse(&std::fs::read_to_string(bench.join("manifest.yaml")).unwrap()).unwrap();
        assert_eq!(reference_baro_failures(&m.traces), Vec::<String>::new());
        let t = |id: &str| m.trace(id).unwrap();
        assert!(t("perso-130").qualifiee() && !t("perso-130").baro_exclue());
        assert_eq!(t("perso-166").reference_baro.as_deref(), Some("non_qualifiee"));
        assert!(t("perso-166").baro_exclue() && t("perso-131").baro_exclue());
        // Sections des séries après V1 lues dans l'ordre de `SECTIONS`.
        let names: Vec<&str> = m.named_sections().map(|(n, _)| n).collect();
        assert_eq!(names, SECTIONS.map(|k| k.name));
    }
}
