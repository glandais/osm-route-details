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
    fn parse(s: &str) -> anyhow::Result<Self> {
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
}

#[derive(Clone, Debug)]
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
        let mut traces = Vec::new();
        if let Some(Value::Sequence(items)) = root.get("perso").and_then(|p| p.get("traces")) {
            for it in items {
                let id = req(it, "id", "perso")?;
                let passages_masques = match it.get("passages_masques") {
                    Some(Value::Sequence(v)) => {
                        v.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()
                    }
                    _ => Vec::new(),
                };
                traces.push(Trace {
                    groupe: req(it, "groupe", &id)?,
                    serie: Serie::parse(&req(it, "serie", &id)?)?,
                    fichier: req(it, "fichier", &id)?,
                    sha256: s(it, "sha256"),
                    sha256_trim1km: s(it, "sha256_trim1km"),
                    passages_masques,
                    verdict: it.get("qualification").and_then(|q| s(q, "verdict")),
                    id,
                });
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
        Ok(Self {
            hors_git,
            sha256: sha256_bytes(text.as_bytes()),
            vcyclist: files(&root, "vcyclist")?,
            osm_surface: files(&root, "osm_surface")?,
            copies,
            ouvrages_tenus,
            ouvrages_perso,
            traces,
        })
    }

    pub fn load(bench: &Path) -> anyhow::Result<Self> {
        let path = bench.join("manifest.yaml");
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("lecture de {}", path.display()))?;
        Self::parse(&text)
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
