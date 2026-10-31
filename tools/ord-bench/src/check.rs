//! `ord-bench check` (T05 §5.1) : empreintes du manifest (traces privées comprises si présentes),
//! cohérence `criteria.yaml` ↔ `bench/README.md` §3–§4, et **Q1** (le banc rejette ses
//! négatifs), recopié du manifest sans recalcul.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Context;
use serde_yaml_ng::Value;

use crate::hash::sha256_file;
use crate::manifest::{is_plain_sha256, Manifest};
use crate::BenchPaths;

#[derive(Clone, Debug, Default)]
pub struct CheckReport {
    /// Fichiers vérifiés (empreinte conforme).
    pub verified: usize,
    /// Écarts : empreinte fausse, fichier manquant, identifiants incohérents.
    pub failures: Vec<String>,
    /// Vérifications sautées (traces privées absentes).
    pub skipped: Vec<String>,
    pub q1: Q1,
    pub private_present: bool,
    pub manifest_sha256: String,
    pub criteria_sha256: String,
}

/// Q1 : aucun négatif `fiable` (bench §3).
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Q1 {
    pub negatives: usize,
    pub verdicts: BTreeMap<String, usize>,
    pub reliable: usize,
}

impl Q1 {
    pub fn pass(&self) -> bool {
        self.negatives > 0 && self.reliable == 0
    }
}

impl CheckReport {
    pub fn ok(&self) -> bool {
        self.failures.is_empty() && self.q1.pass()
    }
}

fn verify(report: &mut CheckReport, path: &Path, expected: &str, label: &str) {
    match sha256_file(path) {
        Ok(actual) if actual == expected => report.verified += 1,
        Ok(_) => report.failures.push(format!("{label} : empreinte différente")),
        Err(_) => report.failures.push(format!("{label} : fichier absent")),
    }
}

/// Identifiants de `bench/README.md` §3 (gras) et §4 (première colonne).
fn readme_ids(readme: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut section = 0;
    let (mut s3, mut s4) = (BTreeSet::new(), BTreeSet::new());
    for line in readme.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            section = title.split('.').next().and_then(|n| n.trim().parse().ok()).unwrap_or(0);
            continue;
        }
        let Some(cell) = line.strip_prefix('|').and_then(|l| l.split('|').next()) else { continue };
        let id = cell.trim().trim_matches('*').trim();
        let looks_like_id = id.len() <= 16
            && id.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && id.chars().nth(1).is_some_and(|c| c.is_ascii_digit());
        if !looks_like_id {
            continue;
        }
        let id = id.replace(" hors France", "_hors_france");
        match section {
            3 => s3.insert(id),
            4 => s4.insert(id),
            _ => false,
        };
    }
    (s3, s4)
}

fn criteria_ids(criteria: &str) -> anyhow::Result<BTreeSet<String>> {
    let root: Value = serde_yaml_ng::from_str(criteria).context("criteria.yaml illisible")?;
    let Some(Value::Sequence(items)) = root.get("criteres") else {
        anyhow::bail!("criteria.yaml sans `criteres`")
    };
    Ok(items.iter().filter_map(|c| c.get("id").and_then(Value::as_str).map(str::to_owned)).collect())
}

pub fn check(paths: &BenchPaths) -> anyhow::Result<CheckReport> {
    let bench = &paths.bench;
    let manifest = Manifest::load(bench)?;
    let mut report = CheckReport {
        manifest_sha256: manifest.sha256.clone(),
        private_present: paths.private_present(),
        ..CheckReport::default()
    };

    // Copies de dépôts externes : SHA256SUMS d'abord, puis chaque fichier qu'il liste.
    let mut sums: BTreeSet<String> = BTreeSet::new();
    for copy in &manifest.copies {
        let sums_path = bench.join(&copy.sommes);
        verify(&mut report, &sums_path, &copy.sha256_sommes, &copy.sommes);
        let text = std::fs::read_to_string(&sums_path).unwrap_or_default();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let Some((sum, rel)) = line.split_once(char::is_whitespace) else { continue };
            let rel = rel.trim().trim_start_matches('*').trim_start_matches("./");
            verify(&mut report, &bench.join(&copy.copie).join(rel), sum, &format!("{}{rel}", copy.copie));
            sums.insert(sum.to_owned());
        }
    }
    // Entrées du manifest : empreinte simple, ou renvoi à des sommes déjà vérifiées.
    for e in manifest.vcyclist.iter().chain(&manifest.osm_surface) {
        if is_plain_sha256(&e.sha256) {
            verify(&mut report, &bench.join(&e.fichier), &e.sha256, &e.id);
        } else {
            for token in e.sha256.split_whitespace().filter(|t| is_plain_sha256(t)) {
                if !sums.contains(token) {
                    report.failures.push(format!("{} : empreinte absente des SHA256SUMS", e.id));
                }
            }
        }
    }
    // Traces et ouvrages personnels, s'ils sont présents (jamais en CI).
    if report.private_present {
        for t in &manifest.traces {
            if let Some(sum) = &t.sha256 {
                verify(&mut report, &paths.resolve(&t.fichier), sum, &t.id);
            }
            if let Some(sum) = &t.sha256_trim1km {
                let trim = t.fichier.trim_end_matches(".gpx").to_owned() + ".trim1km.gpx";
                verify(&mut report, &paths.resolve(&trim), sum, &format!("{} (rognée)", t.id));
            }
        }
        if let Some((fichier, sum)) = &manifest.ouvrages_perso {
            verify(&mut report, &paths.resolve(fichier), sum, "ouvrages_perso");
        }
        for e in &manifest.hors_git {
            verify(&mut report, &paths.resolve(&e.fichier), &e.sha256, &e.id);
        }
    } else {
        report.skipped.push(format!("{} traces personnelles : bench/private/ absent", manifest.traces.len()));
        for e in &manifest.hors_git {
            report.skipped.push(format!("{} (hors git) : bench/private/ absent", e.id));
        }
    }

    // criteria.yaml ↔ README §3–§4.
    let criteria_text = std::fs::read_to_string(bench.join("criteria.yaml")).context("criteria.yaml")?;
    report.criteria_sha256 = crate::hash::sha256_bytes(criteria_text.as_bytes());
    let readme = std::fs::read_to_string(bench.join("README.md")).context("bench/README.md")?;
    let (s3, s4) = readme_ids(&readme);
    let crit = criteria_ids(&criteria_text)?;
    let crit_base: BTreeSet<String> =
        crit.iter().map(|id| id.split('_').next().unwrap_or(id).to_owned()).collect();
    if s3 != crit_base {
        report.failures.push(format!("identifiants README §3 {s3:?} ≠ criteria.yaml {crit_base:?}"));
    }
    if s4 != crit {
        report.failures.push(format!("identifiants README §4 {s4:?} ≠ criteria.yaml {crit:?}"));
    }

    // Q1 : verdicts de qualify_baro.py inscrits au manifest.
    for t in manifest.traces.iter().filter(|t| t.groupe == "negatif") {
        report.q1.negatives += 1;
        let verdict = t.verdict.clone().unwrap_or_else(|| "absent".into());
        if verdict == "fiable" {
            report.q1.reliable += 1;
        }
        *report.q1.verdicts.entry(verdict).or_default() += 1;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_ids_are_read_from_both_tables() {
        let md = "## 3. Métriques\n| Id | M |\n|----|---|\n| **E1** | x |\n| **E2b** | y |\n## 4. Seuils\n| Id | S |\n| E1 | a |\n| E3 hors France | b |\n";
        let (s3, s4) = readme_ids(md);
        assert_eq!(s3.into_iter().collect::<Vec<_>>(), ["E1", "E2b"]);
        assert_eq!(s4.into_iter().collect::<Vec<_>>(), ["E1", "E3_hors_france"]);
    }
}
