//! Jugement unique de la série tenue à l'écart (T05 §3.3, bench §2.3) :
//! `ORD_JUDGE=1 ord-bench judge --increment MVP --gel <commit>`.
//!
//! Avant toute lecture : `ORD_JUDGE=1` exigé ; incrément absent de `bench/judgements.md` ; fichier
//! de gel `bench/gel/<incrément>.json` présent au commit de gel, ancêtre de `HEAD`, et empreintes
//! courantes (`manifest_sha256`, `criteria_sha256`, `bench_rules_sha256`) égales à celles du gel,
//! manifest et critères inchangés depuis ce commit. Puis la campagne ne lit que la série tenue à
//! l'écart (`campaign::run_series`) ; un cas sauté arrête tout sans rien consigner (le jugement
//! n'a pas eu lieu). Sinon `results.json` est écrit et le résultat ajouté à `bench/judgements.md`,
//! qu'il passe ou non.

use std::future::Future;
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Variable d'environnement qui arme le jugement.
pub const JUDGE_ENV: &str = "ORD_JUDGE";

/// Empreintes du banc gelées avant le jugement (fichier `bench/gel/<incrément>.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gel {
    pub increment: String,
    pub manifest_sha256: String,
    pub criteria_sha256: String,
    pub bench_rules_sha256: String,
}

/// Fichier de gel, relatif au dépôt.
pub fn gel_rel_path(increment: &str) -> String {
    format!("bench/gel/{increment}.json")
}

/// Empreintes courantes du banc : celles qu'il faudrait geler (`ord-bench gel`).
pub fn current_gel(paths: &crate::BenchPaths, increment: &str) -> anyhow::Result<Gel> {
    let report = crate::check::check(paths)?;
    crate::campaign::ensure_check(&report)?;
    let rules = crate::BenchRules::default();
    Ok(Gel {
        increment: increment.to_owned(),
        manifest_sha256: report.manifest_sha256,
        criteria_sha256: report.criteria_sha256,
        bench_rules_sha256: crate::hash::sha256_bytes(serde_json::to_string(&rules)?.as_bytes()),
    })
}

/// Titre d'une entrée de `judgements.md`.
fn heading(increment: &str) -> String {
    format!("## Jugement {increment} ")
}

/// L'incrément a déjà été jugé (entrée présente dans `judgements.md`).
pub fn already_judged(judgements: &str, increment: &str) -> bool {
    let h = heading(increment);
    judgements.lines().any(|l| l.starts_with(&h) || l.trim_end() == h.trim_end())
}

fn git(repo: &Path, args: &[&str]) -> anyhow::Result<std::process::Output> {
    std::process::Command::new("git").args(args).current_dir(repo).output().context("git")
}

fn git_ok(repo: &Path, args: &[&str]) -> anyhow::Result<bool> {
    Ok(git(repo, args)?.status.success())
}

/// Vérifications préalables, sans rien lire de la série tenue à l'écart. Renvoie le gel lu au
/// commit de gel.
pub fn preflight(
    repo: &Path,
    bench: &Path,
    increment: &str,
    gel_commit: &str,
    current: &Gel,
    armed: bool,
) -> anyhow::Result<Gel> {
    ensure!(armed, "jugement non armé : {JUDGE_ENV}=1 exigé (jugement unique et irréversible)");
    let judgements = std::fs::read_to_string(bench.join("judgements.md")).unwrap_or_default();
    ensure!(
        !already_judged(&judgements, increment),
        "incrément {increment} déjà jugé dans bench/judgements.md : jugement refusé"
    );
    ensure!(
        git_ok(repo, &["cat-file", "-e", &format!("{gel_commit}^{{commit}}")])?,
        "commit de gel inconnu : {gel_commit}"
    );
    ensure!(
        git_ok(repo, &["merge-base", "--is-ancestor", gel_commit, "HEAD"])?,
        "le commit de gel {gel_commit} n'est pas un ancêtre de HEAD"
    );
    let rel = gel_rel_path(increment);
    let shown = git(repo, &["show", &format!("{gel_commit}:{rel}")])?;
    ensure!(shown.status.success(), "{rel} absent du commit de gel {gel_commit}");
    let gel: Gel = serde_json::from_slice(&shown.stdout).with_context(|| format!("{rel} illisible"))?;
    ensure!(gel.increment == increment, "{rel} gèle l'incrément {}, pas {increment}", gel.increment);
    for f in ["bench/manifest.yaml", "bench/criteria.yaml", &rel] {
        ensure!(
            git_ok(repo, &["diff", "--quiet", gel_commit, "--", f])?,
            "{f} modifié depuis le commit de gel {gel_commit} (index ou copie de travail)"
        );
    }
    let mismatches: Vec<&str> = [
        ("manifest_sha256", &gel.manifest_sha256, &current.manifest_sha256),
        ("criteria_sha256", &gel.criteria_sha256, &current.criteria_sha256),
        ("bench_rules_sha256", &gel.bench_rules_sha256, &current.bench_rules_sha256),
    ]
    .into_iter()
    .filter(|(_, a, b)| a != b)
    .map(|(k, _, _)| k)
    .collect();
    ensure!(mismatches.is_empty(), "empreintes différentes du gel : {}", mismatches.join(", "));
    Ok(gel)
}

/// Cas sautés d'une campagne (deux modes) : un jugement n'en tolère aucun.
pub fn skipped_cases(results: &Value) -> Vec<String> {
    [&results["cas_sautes"], &results["input_with_breakpoints"]["cas_sautes"]]
        .into_iter()
        .flat_map(|v| v.as_array().cloned().unwrap_or_default())
        .map(|s| format!("{} ({})", s["id"].as_str().unwrap_or("?"), s["raison"].as_str().unwrap_or("?")))
        .collect()
}

fn verdict_cell(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_owned)
}

/// Entrée Markdown de `judgements.md` : identification de la campagne et verdicts par id, deux
/// modes. Aucune valeur mesurée ni coordonnée : les valeurs sont dans `results.json`.
pub fn entry(increment: &str, gel_commit: &str, report_rel: &str, results: &Value) -> String {
    let c = &results["campagne"];
    let s = |k: &str| c[k].as_str().unwrap_or("?").to_owned();
    let main = results["verdicts"].as_object().cloned().unwrap_or_default();
    let bp = results["input_with_breakpoints"]["verdicts"].as_object().cloned().unwrap_or_default();
    let mut ids: Vec<&String> = main.keys().chain(bp.keys()).collect();
    ids.sort();
    ids.dedup();
    let failing: Vec<&str> = ids
        .iter()
        .filter(|id| {
            [main.get(**id), bp.get(**id)]
                .into_iter()
                .flatten()
                .any(|v| matches!(v.as_str(), Some("fail" | "skipped")))
        })
        .map(|id| id.as_str())
        .collect();
    let mut out = format!("{}({})\n\n", heading(increment), s("date"));
    out.push_str(&format!("- Commit du service : `{}` ; commit de gel : `{gel_commit}`\n", s("commit")));
    out.push_str(&format!(
        "- `rules_version` : {} ; `api_version` : {}\n",
        s("rules_version"),
        s("api_version")
    ));
    out.push_str(&format!("- `data_versions` : `{}`\n", c["data_versions"]));
    out.push_str(&format!(
        "- Empreintes : manifest `{}`, critères `{}`, règles du banc `{}`, données `{}`\n",
        s("manifest_sha256"),
        s("criteria_sha256"),
        s("bench_rules_sha256"),
        s("data_fingerprint")
    ));
    out.push_str(&format!("- Rapport : `{report_rel}`\n"));
    out.push_str(&format!(
        "- Résultat : **{}**{}\n\n",
        if failing.is_empty() { "accepté" } else { "refusé" },
        if failing.is_empty() {
            String::new()
        } else {
            format!(" (en échec ou sautés : {})", failing.join(", "))
        }
    ));
    out.push_str("| Id | distance | input_with_breakpoints |\n|---|---|---|\n");
    for id in ids {
        let cell =
            |m: &serde_json::Map<String, Value>| m.get(id).map_or_else(|| "—".to_owned(), verdict_cell);
        out.push_str(&format!("| {id} | {} | {} |\n", cell(&main), cell(&bp)));
    }
    out.push_str("\nL1 et L2 (latences) sont mesurés à part (`ord-bench latency`).\n\n");
    out
}

/// Jugement : vérifications, mesure de la série tenue à l'écart (`measure`), consignation.
/// Renvoie le chemin de `results.json`.
#[allow(clippy::too_many_arguments)]
pub async fn judge_with<F, Fut>(
    repo: &Path,
    bench: &Path,
    increment: &str,
    gel_commit: &str,
    current: &Gel,
    armed: bool,
    report_dir: PathBuf,
    measure: F,
) -> anyhow::Result<PathBuf>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = anyhow::Result<Value>>,
{
    preflight(repo, bench, increment, gel_commit, current, armed)?;
    let out = report_dir.join("results.json");
    ensure!(!out.exists(), "{} existe déjà : rapport non réécrit", out.display());
    let results = measure().await?;
    ensure!(results["campagne"]["type"] == "jugement", "la campagne mesurée n'est pas un jugement");
    let skipped = skipped_cases(&results);
    if !skipped.is_empty() {
        bail!(
            "jugement interrompu, rien n'est consigné : {} cas sautés ({}) ; compléter le cache \
             (fill-cache --serie tenue_a_l_ecart) ou bench/private/",
            skipped.len(),
            skipped.join(" ; ")
        );
    }
    std::fs::create_dir_all(&report_dir)?;
    std::fs::write(&out, serde_json::to_string_pretty(&results)? + "\n")?;
    let rel = out.strip_prefix(bench).map_or_else(|_| out.display().to_string(), |p| p.display().to_string());
    let path = bench.join("judgements.md");
    let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        "# Jugements de la série tenue à l'écart\n\nUn jugement par incrément, consigné qu'il passe ou \
         non (bench §2.3, T05 §3.3).\n\n"
            .to_owned()
    });
    text.push_str(&entry(increment, gel_commit, &rel, &results));
    std::fs::write(&path, text)?;
    Ok(out)
}

/// `ORD_JUDGE=1 ord-bench judge --increment <i> --gel <commit>` sur le banc réel.
pub async fn judge(paths: &crate::BenchPaths, increment: &str, gel_commit: &str) -> anyhow::Result<PathBuf> {
    let armed = std::env::var(JUDGE_ENV).is_ok_and(|v| v == "1");
    ensure!(armed, "jugement non armé : {JUDGE_ENV}=1 exigé (jugement unique et irréversible)");
    ensure!(paths.private_present(), "bench/private/ absent : la série tenue à l'écart n'est pas lisible");
    let repo = paths.bench.parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let repo = if repo.as_os_str().is_empty() { PathBuf::from(".") } else { repo };
    let current = current_gel(paths, increment)?;
    let dir = paths.bench.join("reports").join(format!(
        "{}-{}-jugement-{increment}",
        crate::campaign::today_utc(),
        ord_core::RULES_VERSION
    ));
    let groupes = ["dplus", "plaine", "ouvrage", "hors_france", "osm_surface"].map(String::from);
    judge_with(&repo, &paths.bench, increment, gel_commit, &current, armed, dir, || {
        crate::campaign::run_series(paths, &groupes, &[], crate::manifest::Serie::TenueALEcart)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(repo: &Path, args: &[&str]) {
        let o = git(repo, args).unwrap();
        assert!(o.status.success(), "git {args:?} : {}", String::from_utf8_lossy(&o.stderr));
    }

    /// Dépôt synthétique : banc minimal, gel commité ; renvoie (dossier, commit de gel, gel).
    fn repo() -> (tempfile::TempDir, String, Gel) {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        run(r, &["init", "-q"]);
        run(r, &["config", "user.email", "t@example.invalid"]);
        run(r, &["config", "user.name", "t"]);
        run(r, &["config", "commit.gpgsign", "false"]);
        std::fs::create_dir_all(r.join("bench/gel")).unwrap();
        std::fs::write(r.join("bench/manifest.yaml"), "version: 1\n").unwrap();
        std::fs::write(r.join("bench/criteria.yaml"), "criteres: []\n").unwrap();
        let gel = Gel {
            increment: "MVP".into(),
            manifest_sha256: "m".into(),
            criteria_sha256: "c".into(),
            bench_rules_sha256: "b".into(),
        };
        std::fs::write(r.join("bench/gel/MVP.json"), serde_json::to_string(&gel).unwrap()).unwrap();
        run(r, &["add", "."]);
        run(r, &["commit", "-q", "-m", "gel"]);
        let head = String::from_utf8(git(r, &["rev-parse", "HEAD"]).unwrap().stdout).unwrap();
        (dir, head.trim().to_owned(), gel)
    }

    fn results(skipped: Value) -> Value {
        json!({
            "campagne": {"type": "jugement", "date": "2026-11-02", "commit": "abc", "rules_version": "r",
                "api_version": "1.0", "data_versions": {"x": 1}, "manifest_sha256": "m",
                "criteria_sha256": "c", "bench_rules_sha256": "b", "data_fingerprint": "d"},
            "cas_sautes": skipped,
            "verdicts": {"E1": "pass", "E3": "fail", "S3": "reported"},
            "input_with_breakpoints": {"cas_sautes": [], "verdicts": {"E1": "pass", "E3": "pass"}}
        })
    }

    #[tokio::test]
    async fn judges_once_and_refuses_a_second_time() {
        let (dir, gel_commit, gel) = repo();
        let (r, bench) = (dir.path(), dir.path().join("bench"));
        let report = bench.join("reports/j1");
        let out = judge_with(r, &bench, "MVP", &gel_commit, &gel, true, report.clone(), || async {
            Ok(results(json!([])))
        })
        .await
        .unwrap();
        assert!(out.exists());
        let text = std::fs::read_to_string(bench.join("judgements.md")).unwrap();
        assert!(text.contains("## Jugement MVP (2026-11-02)"), "{text}");
        assert!(text.contains("**refusé** (en échec ou sautés : E3)"), "un échec est consigné : {text}");
        assert!(text.contains("| E3 | fail | pass |"));
        assert!(text.contains(&format!("commit de gel : `{gel_commit}`")));
        // Second jugement du même incrément : refusé avant toute mesure.
        let err = judge_with(r, &bench, "MVP", &gel_commit, &gel, true, bench.join("reports/j2"), || async {
            panic!("aucune mesure attendue")
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("déjà jugé"), "{err}");
    }

    #[tokio::test]
    async fn refuses_before_any_measure() {
        let (dir, gel_commit, gel) = repo();
        let (r, bench) = (dir.path(), dir.path().join("bench"));
        let never = || async { panic!("aucune mesure attendue") };
        let report = bench.join("reports/j");
        let e = |r: anyhow::Result<PathBuf>| r.unwrap_err().to_string();
        // Non armé.
        assert!(e(judge_with(r, &bench, "MVP", &gel_commit, &gel, false, report.clone(), never).await)
            .contains("ORD_JUDGE=1"));
        // Empreinte courante différente du gel.
        let other = Gel { criteria_sha256: "autre".into(), ..gel.clone() };
        assert!(e(judge_with(r, &bench, "MVP", &gel_commit, &other, true, report.clone(), never).await)
            .contains("criteria_sha256"));
        // Commit inconnu, incrément non gelé.
        assert!(e(judge_with(r, &bench, "MVP", "0000000", &gel, true, report.clone(), never).await)
            .contains("inconnu"));
        assert!(e(judge_with(r, &bench, "V1", &gel_commit, &gel, true, report.clone(), never).await)
            .contains("absent du commit de gel"));
        // Critères modifiés depuis le gel (copie de travail).
        std::fs::write(bench.join("criteria.yaml"), "criteres: [x]\n").unwrap();
        assert!(e(judge_with(r, &bench, "MVP", &gel_commit, &gel, true, report.clone(), never).await)
            .contains("modifié depuis le commit de gel"));
        assert!(!bench.join("judgements.md").exists());
    }

    #[tokio::test]
    async fn skipped_case_records_nothing() {
        let (dir, gel_commit, gel) = repo();
        let (r, bench) = (dir.path(), dir.path().join("bench"));
        let report = bench.join("reports/j");
        let err = judge_with(r, &bench, "MVP", &gel_commit, &gel, true, report.clone(), || async {
            Ok(results(json!([{"id": "perso-99", "raison": "tuile absente du cache"}])))
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("rien n'est consigné"), "{err}");
        assert!(!bench.join("judgements.md").exists());
        assert!(!report.exists());
        // Une campagne de calibration n'est jamais un jugement.
        let mut cal = results(json!([]));
        cal["campagne"]["type"] = json!("calibration");
        assert!(judge_with(r, &bench, "MVP", &gel_commit, &gel, true, report, || async { Ok(cal) })
            .await
            .is_err());
    }

    /// La sélection du jugement ne retient que les traces de la série tenue à l'écart ; celle de
    /// la calibration, jamais.
    #[test]
    fn judgement_selects_held_out_traces_only() {
        use crate::manifest::{Manifest, Serie, Trace};
        let trace = |id: &str, serie| Trace {
            id: id.into(),
            groupe: "plaine".into(),
            serie,
            fichier: format!("private/traces/{id}.gpx"),
            sha256: None,
            sha256_trim1km: None,
            passages_masques: Vec::new(),
            verdict: None,
        };
        let m = Manifest {
            sha256: String::new(),
            vcyclist: Vec::new(),
            osm_surface: Vec::new(),
            copies: Vec::new(),
            ouvrages_tenus: Default::default(),
            ouvrages_perso: None,
            traces: vec![
                trace("a", Serie::Calibration),
                trace("b", Serie::TenueALEcart),
                trace("c", Serie::Calibration),
            ],
            hors_git: Vec::new(),
        };
        let held = crate::campaign::select_series(&m, &[], &[], Serie::TenueALEcart);
        assert_eq!(held.cases.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["b"]);
        assert_eq!(held.excluded_held_out, ["a", "c"]);
        let cal = crate::campaign::select(&m, &[], &[]);
        assert_eq!(cal.cases.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["a", "c"]);
    }

    #[test]
    fn already_judged_reads_headings_only() {
        assert!(already_judged("# J\n\n## Jugement MVP (2026-11-02)\n", "MVP"));
        assert!(!already_judged("## Jugement MVP2 (x)\nle MVP est cité\n", "MVP"));
    }
}
