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
    /// Témoin retiré (`retire`) : l'itinéraire reste un cas pour ses ouvrages.
    pub retire: Option<String>,
}

/// Section `perso_v1` du manifest : index des ouvrages, témoins et séries V1 (hors git, par
/// empreinte), itinéraires planifiés et identifiants des traces V1.
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
    pub planifies: Vec<Planned>,
    pub traces: BTreeSet<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Manifest {
    pub sha256: String,
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
        let v1 = match root.get("perso_v1") {
            Some(p) => {
                let v1_traces = parse_traces(Some(p), "perso_v1")?;
                let pair = |key: &str| -> anyhow::Result<(String, String)> {
                    let e = p.get(key).with_context(|| format!("perso_v1 : `{key}` manquant"))?;
                    Ok((req(e, "fichier", key)?, req(e, "sha256", key)?))
                };
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
                                retire: s(it, "retire"),
                                id,
                            })
                        })
                        .collect::<anyhow::Result<Vec<_>>>()?,
                    _ => Vec::new(),
                };
                let ids = v1_traces.iter().map(|t| t.id.clone()).collect();
                traces.extend(v1_traces);
                Some(PersoV1 {
                    ouvrages: pair("ouvrages_v1")?,
                    temoins: pair("temoins_v1")?,
                    temoins_tags: p.get("temoins_v1_tags").map(|_| pair("temoins_v1_tags")).transpose()?,
                    series: pair("series_v1")?,
                    retard_gele: p.get("retard_baro_gele").map(|_| pair("retard_baro_gele")).transpose()?,
                    donnees_serveur: p.get("recalage").and_then(|r| s(r, "donnees_serveur")),
                    osm_tags: match p.get("osm_tags_reference") {
                        Some(e) => {
                            let (fichier, sum) = pair("osm_tags_reference")?;
                            Some((fichier, sum, req(e, "date", "osm_tags_reference")?))
                        }
                        None => None,
                    },
                    planifies,
                    traces: ids,
                })
            }
            None => None,
        };
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
        Ok(Self {
            hors_git,
            sha256: sha256_bytes(text.as_bytes()),
            vcyclist: files(&root, "vcyclist")?,
            osm_surface: files(&root, "osm_surface")?,
            copies,
            ouvrages_tenus,
            ouvrages_perso,
            traces,
            v1,
            mvp_judged: false,
        })
    }

    /// Série MVP jugée (README §2, point 3) : ses traces et ses ouvrages tenus à l'écart deviennent
    /// calibration, et les fenêtres masquées des traces du MVP (ouvrages d'une autre série au MVP)
    /// n'ont plus lieu d'être. Les traces V1 gardent leur série et leurs fenêtres.
    pub fn with_mvp_judged(mut self) -> Self {
        let v1: BTreeSet<String> = self.v1.as_ref().map(|v| v.traces.clone()).unwrap_or_default();
        for t in self.traces.iter_mut().filter(|t| !v1.contains(&t.id)) {
            t.serie = Serie::Calibration;
            t.passages_masques.clear();
        }
        self.ouvrages_tenus.clear();
        self.mvp_judged = true;
        self
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
        Ok(if crate::judge::already_judged(&judgements, "MVP") {
            manifest.with_mvp_judged()
        } else {
            manifest
        })
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
