//! `ord-bench check` (T05 §5.1) : empreintes du manifest (traces privées comprises si présentes),
//! cohérence `criteria.yaml` ↔ `bench/README.md` §3–§4, et **Q1** (le banc rejette ses
//! négatifs), recopié du manifest sans recalcul.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Context;
use serde_yaml_ng::Value;

use crate::hash::sha256_file;
use crate::manifest::{is_plain_sha256, Manifest, Serie};
use crate::BenchPaths;

#[derive(Clone, Debug, Default)]
pub struct CheckReport {
    /// Fichiers vérifiés (empreinte conforme).
    pub verified: usize,
    /// Écarts : empreinte fausse, fichier manquant, identifiants incohérents.
    pub failures: Vec<String>,
    /// Vérifications sautées (traces privées absentes).
    pub skipped: Vec<String>,
    /// Q1 de la calibration (jugé par `run`).
    pub q1: Q1,
    /// Q1 de la série tenue à l'écart (jugé par `judge` seulement) : négatifs nouveaux au
    /// défaut attesté hors du qualificateur (README §4 ter.4, 930f6e8 et 305abd7).
    pub q1_tenue: Q1,
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
    /// Négatifs nouveaux sans défaut attesté : pas des négatifs, non comptés.
    pub non_attestes: Vec<String>,
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

/// `check` de la calibration : les fichiers propres des cas de la série tenue à l'écart (traces,
/// recalages, itinéraires, références) ne sont pas ouverts ; leurs empreintes sont vérifiées au
/// jugement (`check_series(…, TenueALEcart)`).
pub fn check(paths: &BenchPaths) -> anyhow::Result<CheckReport> {
    check_series(paths, Serie::Calibration)
}

/// Empreintes des fichiers de la série `scope` (calibration), ou de tout le banc (série tenue à
/// l'écart, au jugement, qui lit aussi les extraits des traces de calibration).
pub fn check_series(paths: &BenchPaths, scope: Serie) -> anyhow::Result<CheckReport> {
    let bench = &paths.bench;
    let manifest = Manifest::load(bench)?;
    let opens = |serie: Serie| scope == Serie::TenueALEcart || serie == Serie::Calibration;
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
    // Retard du baro gelé de la calibration (versionné, README §4 ter.9, 48ca44a) : vérifié
    // comme les autres index, avec ou sans bench/private/.
    if let Some((fichier, sum)) = manifest.v1.as_ref().and_then(|v| v.retard_gele.as_ref()) {
        verify(&mut report, &paths.resolve(fichier), sum, "retard_baro_gele");
    }
    // Traces et ouvrages personnels, s'ils sont présents (jamais en CI).
    if report.private_present {
        let mut held_out = 0usize;
        for t in &manifest.traces {
            if !opens(t.serie) {
                held_out += 1;
                continue;
            }
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
        // Séries V1 et V1.1 : index (deux séries, lus et vérifiés), puis itinéraires selon leur
        // série. L'instantané OSM d'une série nouvelle n'est ouvert qu'avec ses cas : toujours pour
        // V1 (calibration), au jugement pour une série encore tenue à l'écart (V1.1).
        for (name, v1) in manifest.named_sections() {
            let suffix = if name == crate::manifest::SECTION_V1.name { "v1" } else { "v11" };
            for (label, (fichier, sum)) in
                [("ouvrages", &v1.ouvrages), ("temoins", &v1.temoins), ("series", &v1.series)]
            {
                verify(&mut report, &paths.resolve(fichier), sum, &format!("{label}_{suffix}"));
            }
            if let Some((fichier, sum)) = &v1.temoins_tags {
                verify(&mut report, &paths.resolve(fichier), sum, &format!("temoins_{suffix}_tags"));
            }
            // Instantané des tags OSM des références Valhalla (README §4 bis.8, 562d729).
            let held = suffix != "v1" && !v1.judged;
            match &v1.osm_tags {
                Some((fichier, sum, _)) if !held || scope == Serie::TenueALEcart => {
                    verify(&mut report, &paths.resolve(fichier), sum, &format!("osm_tags_reference ({name})"))
                }
                Some(_) => held_out += 1,
                None if held && scope == Serie::TenueALEcart => report
                    .failures
                    .push(format!("osm_tags_reference ({name}) : instantané absent du manifest")),
                None => {}
            }
            let rows = crate::v1::series_rows(paths, v1).unwrap_or_default();
            for p in &v1.planifies {
                let opened = match crate::v1::planned_serie(&rows, &p.id) {
                    Some(s) => s.in_series(Serie::Calibration) || scope == Serie::TenueALEcart,
                    None => {
                        report.failures.push(format!("{} : absent de series-{suffix}.csv", p.id));
                        false
                    }
                };
                if !opened {
                    held_out += 1;
                    continue;
                }
                verify(&mut report, &paths.resolve(&p.fichier), &p.sha256, &p.id);
                if let Some((fichier, sum)) = &p.reference {
                    verify(&mut report, &paths.resolve(fichier), sum, &format!("{} (référence)", p.id));
                }
                // Réponse Valhalla d'où la référence est tirée (README §4 bis.8, 40c65cd).
                if p.reference.is_some() && p.role == "R1_R2" {
                    match crate::osm_surface::planned_sources(paths, p) {
                        Ok(sources) => {
                            for ((path, sum), what) in sources.iter().zip(["surface.csv", "baro.csv"]) {
                                match sum {
                                    Some(sum) => {
                                        verify(&mut report, path, sum, &format!("{} ({what})", p.id))
                                    }
                                    None => report
                                        .failures
                                        .push(format!("{} : empreinte de {what} absente du manifest", p.id)),
                                }
                            }
                        }
                        Err(e) => report.failures.push(format!("{} : {e}", p.id)),
                    }
                }
            }
        }
        if held_out > 0 {
            report.skipped.push(format!(
                "{held_out} cas de la série tenue à l'écart : fichiers non ouverts, vérifiés au jugement"
            ));
        }
        report.failures.extend(missing_recalage(&paths.private.join("recales"), &manifest));
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

    // Groupe E2b d'une trace nouvelle (README §4 ter.4, 13e0ed1), lu au manifest.
    report.failures.extend(crate::v1::e2b_group_failures(&manifest));

    // Q1 : verdicts de qualify_baro.py inscrits au manifest, par série. Un négatif d'une série
    // nouvelle n'en est un que si son défaut est attesté hors du qualificateur (README §4 ter.4).
    for t in manifest.traces.iter().filter(|t| t.groupe == "negatif") {
        let q1 = if t.serie == Serie::Calibration { &mut report.q1 } else { &mut report.q1_tenue };
        if manifest.is_new_series(&t.id) && !crate::v1::q1_attested(t.gain_baro_m, t.gain_mnt_m) {
            q1.non_attestes.push(t.id.clone());
            continue;
        }
        q1.negatives += 1;
        let verdict = t.verdict.clone().unwrap_or_else(|| "absent".into());
        if verdict == "fiable" {
            q1.reliable += 1;
        }
        *q1.verdicts.entry(verdict).or_default() += 1;
    }
    Ok(report)
}

/// Garde-fou : chaque trace attendue par `recales/journal.csv` (première colonne, `<id>` ou
/// `<id>.sN`) a au moins une géométrie recalée (`<id>.gpx`, `<id>.sN.gpx` ou `<id>.partN.gpx`).
/// Une trace sans géométrie ferait sauter le cas en silence pendant la campagne. Les messages ne
/// portent que l'identifiant.
pub fn missing_recalage(recales: &Path, manifest: &Manifest) -> Vec<String> {
    let journal = recales.join("journal.csv");
    let Ok(text) = std::fs::read_to_string(&journal) else {
        return vec!["recales/journal.csv absent : traces recalées attendues inconnues".into()];
    };
    let mut expected: BTreeSet<String> = BTreeSet::new();
    for line in text.lines() {
        let Some(first) = line.split(',').next().map(str::trim) else { continue };
        if first.is_empty() || first.contains(' ') || first == "id" {
            continue;
        }
        let base = match first.rsplit_once(".s") {
            Some((b, n)) if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => b,
            _ => first,
        };
        expected.insert(base.to_owned());
    }
    let has_numbered = |id: &str, infix: &str| recales.join(format!("{id}.{infix}1.gpx")).is_file();
    let mut out = Vec::new();
    for id in expected {
        if !manifest.traces.iter().any(|t| t.id == id) {
            out.push(format!("{id} : dans recales/journal.csv mais absente du manifest"));
            continue;
        }
        if !(recales.join(format!("{id}.gpx")).is_file()
            || has_numbered(&id, "s")
            || has_numbered(&id, "part"))
        {
            out.push(format!("{id} : aucune géométrie recalée (ni .gpx, ni .sN, ni .partN)"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_recalage_fails_on_synthetic_data() {
        let manifest = Manifest::load(&crate::BenchPaths::repo_bench()).expect("manifest");
        let ids: Vec<&str> =
            manifest.traces.iter().filter(|t| t.groupe != "negatif").map(|t| t.id.as_str()).take(4).collect();
        assert_eq!(ids.len(), 4);
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        assert_eq!(missing_recalage(r, &manifest).len(), 1, "journal absent : échec");
        let journal = format!(
            "{a},1,2\n{b}.s1,1,2\n{b}.s2,1,2\n{c},1\n{d},1\nperso-inconnue,1\n",
            a = ids[0],
            b = ids[1],
            c = ids[2],
            d = ids[3]
        );
        std::fs::write(r.join("journal.csv"), journal).unwrap();
        std::fs::write(r.join(format!("{}.gpx", ids[0])), "").unwrap();
        std::fs::write(r.join(format!("{}.s1.gpx", ids[1])), "").unwrap();
        std::fs::write(r.join(format!("{}.part1.gpx", ids[2])), "").unwrap();
        let failures = missing_recalage(r, &manifest);
        assert_eq!(failures.len(), 2, "{failures:?}");
        assert!(failures.iter().any(|f| f.starts_with(ids[3]) && f.contains("aucune géométrie")));
        assert!(failures.iter().any(|f| f.starts_with("perso-inconnue")));
    }

    #[test]
    fn readme_ids_are_read_from_both_tables() {
        let md = "## 3. Métriques\n| Id | M |\n|----|---|\n| **E1** | x |\n| **E2b** | y |\n## 4. Seuils\n| Id | S |\n| E1 | a |\n| E3 hors France | b |\n";
        let (s3, s4) = readme_ids(md);
        assert_eq!(s3.into_iter().collect::<Vec<_>>(), ["E1", "E2b"]);
        assert_eq!(s4.into_iter().collect::<Vec<_>>(), ["E1", "E3_hors_france"]);
    }
}
