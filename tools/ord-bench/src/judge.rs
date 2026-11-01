//! Jugement unique de la série tenue à l'écart (T05 §3.3, bench README §2, point 3) :
//! `ORD_JUDGE=1 ord-bench judge --increment MVP --gel <commit>`.
//!
//! Avant toute lecture : `ORD_JUDGE=1` exigé ; incrément absent de `bench/judgements.md` et sans
//! dossier `bench/reports/*-jugement-<incrément>`, quelle qu'en soit la date ; fichier
//! de gel `bench/gel/<incrément>.json` présent au commit de gel, ancêtre de `HEAD`, et empreintes
//! courantes (`manifest_sha256`, `criteria_sha256`, `bench_rules_sha256`) égales à celles du gel,
//! manifest et critères inchangés depuis ce commit. Puis la campagne ne lit que la série tenue à
//! l'écart (`campaign::run_series`) ; un cas sauté arrête tout sans rien consigner (le jugement
//! n'a pas eu lieu). Sinon `results.json` est écrit et le résultat ajouté à `bench/judgements.md`,
//! qu'il passe ou non.

use std::collections::BTreeSet;
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

/// Nom du dossier du rapport de jugement : `<date>-<rules_version>-<révision du banc>-jugement-<i>`,
/// révision omise quand elle vaut `None` (convention de `campaign::default_report_dir`).
pub fn report_dir_name(date: &str, rules_version: &str, revision: Option<&str>, increment: &str) -> String {
    match revision {
        Some(rev) => format!("{date}-{rules_version}-{rev}-jugement-{increment}"),
        None => format!("{date}-{rules_version}-jugement-{increment}"),
    }
}

/// Dossiers `bench/reports/*-jugement-<i>` déjà présents, quelle que soit leur date.
pub fn existing_reports(bench: &Path, increment: &str) -> Vec<String> {
    let suffix = format!("-jugement-{increment}");
    let mut found: Vec<String> = std::fs::read_dir(bench.join("reports"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|n| n.ends_with(&suffix))
        .collect();
    found.sort();
    found
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
    let reports = existing_reports(bench, increment);
    ensure!(
        reports.is_empty(),
        "incrément {increment} déjà jugé : rapport bench/reports/{} présent, jugement refusé",
        reports.join(", bench/reports/")
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

/// Chemins du code mesuré : service, banc et manifeste Cargo (`Cargo.toml`, `Cargo.lock`).
const CODE_PATHS: [&str; 3] = ["crates", "tools/ord-bench", ":(glob)Cargo.*"];

/// Dernier commit qui touche le code mesuré (`crates/`, `tools/ord-bench/`, `Cargo.*`), suffixé
/// `-dirty` si ces chemins ont des modifications non commitées ; `inconnu` hors dépôt git.
pub fn code_commit(repo: &Path) -> String {
    let run = |args: &[&str]| {
        git(repo, args)
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let mut log = vec!["log", "-1", "--format=%h", "--"];
    log.extend(CODE_PATHS);
    let Some(commit) = run(&log).filter(|c| !c.is_empty()) else {
        return "inconnu".into();
    };
    let mut status = vec!["status", "--porcelain", "--untracked-files=no", "--"];
    status.extend(CODE_PATHS);
    if run(&status).is_some_and(|s| !s.is_empty()) {
        format!("{commit}-dirty")
    } else {
        commit
    }
}

/// Verdict d'un critère : une chaîne (`pass`, `fail`, `skipped`, `reported`).
fn verdict_cell(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("—").to_owned()
}

/// Nombre décimal à la française, `digits` décimales.
fn fr(x: f64, digits: usize) -> String {
    format!("{x:.digits$}").replace('.', ",")
}

/// Résumé lisible d'une valeur annexe d'un critère (`<id>_echecs`, `E2_indicatif`…) : ids en
/// échec, ou médianes par groupe pour une liste d'objets ; jamais de JSON brut. `None` si vide.
fn aux_summary(key: &str, v: &Value) -> Option<String> {
    let items = v.as_array()?;
    if items.is_empty() {
        return None;
    }
    if items.iter().all(Value::is_string) {
        let ids: Vec<&str> = items.iter().filter_map(Value::as_str).collect();
        return Some(ids.join(", "));
    }
    let groups: Vec<String> = items
        .iter()
        .map(|g| {
            let name = g["groupe"].as_str().unwrap_or("?");
            let mut cell = match g["mediane"].as_f64() {
                Some(m) => format!("{name} {}", fr(m, 3)),
                None => name.to_owned(),
            };
            if g["mediane_dans_bornes"] == false {
                cell.push_str(" (médiane hors bornes)");
            }
            let out: Vec<&str> =
                g["traces_hors_bornes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            if !out.is_empty() {
                cell.push_str(&format!(" (hors bornes : {})", out.join(", ")));
            }
            cell
        })
        .collect();
    let label = key.split_once('_').map_or(key, |(_, rest)| rest).replace('_', " ");
    Some(format!("{label} : {}", groups.join(" ; ")))
}

/// Critère auquel se rattache une clé annexe : le plus long id de critère `c` tel que la clé
/// commence par `c_` (`R1_R2_echecs` → `R1_R2`, `E2_indicatif` → `E2`).
fn owner<'a>(key: &str, criteria: &'a [String]) -> Option<&'a String> {
    criteria
        .iter()
        .filter(|c| key.len() > c.len() + 1 && key.starts_with(c.as_str()) && key[c.len()..].starts_with('_'))
        .max_by_key(|c| c.len())
}

/// Commits de l'entrée : code mesuré et `HEAD` au jugement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commits<'a> {
    /// Dernier commit qui touche le code mesuré (`code_commit`).
    pub code: &'a str,
    /// Commit de gel (`bench/gel/<incrément>.json`).
    pub gel: &'a str,
}

/// Critères bloquants de `criteria.yaml` (`criteres[].bloquant: true`) : un échec de l'un d'eux
/// refuse l'incrément ; un critère non bloquant (R3 depuis le 2026-11-01, §4 bis.7) est rapporté.
pub fn blocking_criteria(bench: &Path) -> anyhow::Result<BTreeSet<String>> {
    let text = std::fs::read_to_string(bench.join("criteria.yaml")).context("criteria.yaml")?;
    let root: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).context("criteria.yaml illisible")?;
    Ok(root["criteres"]
        .as_sequence()
        .into_iter()
        .flatten()
        .filter(|c| c["bloquant"].as_bool() == Some(true))
        .filter_map(|c| c["id"].as_str().map(str::to_owned))
        .collect())
}

/// Un verdict de `results.json` porte-t-il un critère bloquant ? `R1_R2` réunit R1 et R2 ; un id
/// absent de `criteria.yaml` (contrôle rapporté) ne l'est pas.
fn is_blocking(id: &str, blocking: &BTreeSet<String>) -> bool {
    match id {
        "R1_R2" => blocking.contains("R1") || blocking.contains("R2"),
        _ => blocking.contains(id),
    }
}

/// Entrée Markdown de `judgements.md` : identification de la campagne (une seule liste) puis un
/// tableau d'une ligne par critère, deux modes, avec les ids en échec. Aucune valeur mesurée ni
/// coordonnée, hors le résumé d'`E2_indicatif` : les valeurs sont dans `results.json`. Seul un
/// critère bloquant (`blocking`) en échec ou sauté refuse l'incrément ; les autres sont rapportés.
pub fn entry(
    increment: &str,
    commits: &Commits<'_>,
    report_rel: &str,
    results: &Value,
    blocking: &BTreeSet<String>,
) -> String {
    let c = &results["campagne"];
    let s = |k: &str| c[k].as_str().unwrap_or("?").to_owned();
    let main = results["verdicts"].as_object().cloned().unwrap_or_default();
    let bp = results["input_with_breakpoints"]["verdicts"].as_object().cloned().unwrap_or_default();
    // Critères : clés dont la valeur est un verdict (chaîne) dans l'un des modes ; les autres
    // clés sont des annexes rattachées à leur critère.
    let mut criteria: Vec<String> =
        main.iter().chain(bp.iter()).filter(|(_, v)| v.is_string()).map(|(k, _)| k.clone()).collect();
    criteria.sort();
    criteria.dedup();
    let failed = |id: &String| {
        [main.get(id), bp.get(id)]
            .into_iter()
            .flatten()
            .any(|v| matches!(v.as_str(), Some("fail" | "skipped" | "sous_minimum")))
    };
    let reported: Vec<&str> =
        criteria.iter().filter(|id| !is_blocking(id, blocking) && failed(id)).map(String::as_str).collect();
    let failing: Vec<&str> =
        criteria.iter().filter(|id| is_blocking(id, blocking) && failed(id)).map(String::as_str).collect();
    let mut out = format!("{}({})\n\n", heading(increment), s("date"));
    out.push_str(&format!(
        "- Commit du code mesuré (`crates/`, `tools/ord-bench/`, `Cargo.*`) : `{}`\n",
        commits.code
    ));
    out.push_str(&format!("- HEAD au jugement : `{}` ; commit de gel : `{}`\n", s("commit"), commits.gel));
    out.push_str(&format!(
        "- `rules_version` : {} ; `api_version` : {}\n",
        s("rules_version"),
        s("api_version")
    ));
    out.push_str(&format!(
        "- Révision du banc (`revision_banc`) : {}\n",
        c["revision_banc"].as_str().map_or_else(|| "aucune".to_owned(), |r| format!("`{r}`"))
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
        "- Résultat : **{}**{}\n",
        if failing.is_empty() { "accepté" } else { "refusé" },
        if failing.is_empty() {
            String::new()
        } else {
            format!(" (en échec, sautés ou sous leur minimum de couverture : {})", failing.join(", "))
        }
    ));
    if !reported.is_empty() {
        out.push_str(&format!(
            "- Non bloquants en échec ou sautés, rapportés sans refuser : {}\n",
            reported.join(", ")
        ));
    }
    out.push_str(
        "\n| Critère | bloquant | distance | input_with_breakpoints | ids en échec |\n|---|---|---|---|---|\n",
    );
    for id in &criteria {
        // Annexes du critère, résumées par mode ; une seule fois si les deux modes concordent.
        let mut keys: Vec<&String> = main
            .keys()
            .chain(bp.keys())
            .filter(|k| !criteria.contains(k) && owner(k, &criteria) == Some(id))
            .collect();
        keys.sort();
        keys.dedup();
        let mut notes = Vec::new();
        for k in keys {
            let a = main.get(k).and_then(|v| aux_summary(k, v));
            let b = bp.get(k).and_then(|v| aux_summary(k, v));
            match (a, b) {
                (None, None) => {}
                (a, b) if a == b => notes.extend(a),
                (a, b) => notes.push(format!(
                    "distance : {} ; input_with_breakpoints : {}",
                    a.unwrap_or_else(|| "—".into()),
                    b.unwrap_or_else(|| "—".into())
                )),
            }
        }
        let ids = if notes.is_empty() { "—".to_owned() } else { notes.join(" ; ") };
        let b = if is_blocking(id, blocking) { "oui" } else { "non" };
        out.push_str(&format!(
            "| {id} | {b} | {} | {} | {ids} |\n",
            verdict_cell(main.get(id)),
            verdict_cell(bp.get(id))
        ));
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
    let blocking = blocking_criteria(bench)?;
    let out = report_dir.join("results.json");
    ensure!(!out.exists(), "{} existe déjà : rapport non réécrit", out.display());
    let mut results = measure().await?;
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
    // Contrôle de couverture au jugement (README §2 point 3, §4 ter.4 ; à partir de V1) : un
    // critère sous son **minimum de jugement** (pas la cible majorée du tirage) vaut refus.
    if increment != "MVP" {
        let minimums = crate::coverage::minimums(bench)?;
        crate::coverage::apply_minimums(&mut results, &minimums);
        results["campagne"]["couverture_minimums"] = minimums;
    }
    std::fs::create_dir_all(&report_dir)?;
    std::fs::write(&out, serde_json::to_string_pretty(&results)? + "\n")?;
    let rel = out.strip_prefix(bench).map_or_else(|_| out.display().to_string(), |p| p.display().to_string());
    let path = bench.join("judgements.md");
    let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        "# Jugements de la série tenue à l'écart\n\nUn jugement par incrément, consigné qu'il passe ou \
         non (bench README §2, point 3 ; T05 §3.3).\n\n"
            .to_owned()
    });
    let code = code_commit(repo);
    text.push_str(&entry(increment, &Commits { code: &code, gel: gel_commit }, &rel, &results, &blocking));
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
    let dir = paths.bench.join("reports").join(report_dir_name(
        &crate::campaign::today_utc(),
        ord_core::RULES_VERSION,
        crate::BENCH_REVISION,
        increment,
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
        repo_for("MVP")
    }

    fn repo_for(increment: &str) -> (tempfile::TempDir, String, Gel) {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        run(r, &["init", "-q"]);
        run(r, &["config", "user.email", "t@example.invalid"]);
        run(r, &["config", "user.name", "t"]);
        run(r, &["config", "commit.gpgsign", "false"]);
        std::fs::create_dir_all(r.join("bench/gel")).unwrap();
        std::fs::write(r.join("bench/manifest.yaml"), "version: 1\n").unwrap();
        std::fs::write(
            r.join("bench/criteria.yaml"),
            "criteres:\n  - {id: E1, bloquant: true}\n  - {id: E3, bloquant: true}\n  - {id: R3, bloquant: false}\n\
             serie_tenue_a_l_ecart:\n  couverture:\n    minimums:\n      E1: {traces_france_fiables: 8, km_compares: 800}\n\
             \x20     E3: {ponts: 12, ponts_mnh_10m_min: 3, ponts_plus_de_150m: 3}\n",
        )
        .unwrap();
        let gel = Gel {
            increment: increment.into(),
            manifest_sha256: "m".into(),
            criteria_sha256: "c".into(),
            bench_rules_sha256: "b".into(),
        };
        std::fs::write(r.join(format!("bench/gel/{increment}.json")), serde_json::to_string(&gel).unwrap())
            .unwrap();
        run(r, &["add", "."]);
        run(r, &["commit", "-q", "-m", "gel"]);
        let head = String::from_utf8(git(r, &["rev-parse", "HEAD"]).unwrap().stdout).unwrap();
        (dir, head.trim().to_owned(), gel)
    }

    fn results(skipped: Value) -> Value {
        json!({
            "campagne": {"type": "jugement", "date": "2026-11-02", "commit": "abc", "rules_version": "r",
                "revision_banc": "bench9",
                "api_version": "1.0", "data_versions": {"x": 1}, "manifest_sha256": "m",
                "criteria_sha256": "c", "bench_rules_sha256": "b", "data_fingerprint": "d"},
            "cas_sautes": skipped,
            "verdicts": {"E1": "pass", "E3": "fail", "S3": "reported", "R3": "fail"},
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
        assert!(
            text.contains("**refusé** (en échec, sautés ou sous leur minimum de couverture : E3)"),
            "un échec est consigné : {text}"
        );
        assert!(text.contains("| E3 | oui | fail | pass |"), "{text}");
        assert!(text.contains("| R3 | non | fail | — |"), "{text}");
        assert!(text.contains("rapportés sans refuser : R3"), "{text}");
        assert!(text.contains(&format!("commit de gel : `{gel_commit}`")));
        assert!(text.contains("Révision du banc (`revision_banc`) : `bench9`"), "{text}");
        // Second jugement du même incrément : refusé avant toute mesure.
        let err = judge_with(r, &bench, "MVP", &gel_commit, &gel, true, bench.join("reports/j2"), || async {
            panic!("aucune mesure attendue")
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("déjà jugé"), "{err}");
    }

    /// V1 : un critère bloquant qui passe mais sous son minimum de jugement (README §2 point 3,
    /// §4 ter.4) refuse l'incrément ; la cible majorée du tirage ne compte pas ; un mode sans
    /// couverture n'est réputé couvert pour aucun critère.
    #[tokio::test]
    async fn v1_coverage_under_the_judgement_minimum_refuses() {
        let (dir, gel_commit, gel) = repo_for("V1");
        let (r, bench) = (dir.path(), dir.path().join("bench"));
        let mut res = results(json!([]));
        res["verdicts"] = json!({"E1": "pass", "E3": "pass", "R3": "fail"});
        // E1 au minimum exact (8 traces, 800 km : sous la cible majorée de 25 %, pas sous le
        // minimum) ; E3 : 11 ponts pour 12.
        let cov = json!({"E1": {"traces_france_fiables": 8, "km_compares": 800.0},
            "E3": {"ponts": 11, "ponts_mnh_10m_min": 4, "ponts_plus_de_150m": 4}});
        res["couverture"] = cov.clone();
        res["input_with_breakpoints"]["couverture"] = cov;
        let out = judge_with(r, &bench, "V1", &gel_commit, &gel, true, bench.join("reports/v1"), || async {
            Ok(res)
        })
        .await
        .unwrap();
        let written: Value = serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap();
        assert_eq!(written["verdicts"]["E1"], "pass");
        assert_eq!(written["verdicts"]["E3"], "sous_minimum");
        assert_eq!(written["verdicts"]["E3_sous_minimum"], json!(["ponts 11 < 12"]));
        assert_eq!(written["input_with_breakpoints"]["verdicts"]["E3"], "sous_minimum");
        assert_eq!(written["campagne"]["couverture_minimums"]["E3"]["ponts"], 12);
        let text = std::fs::read_to_string(bench.join("judgements.md")).unwrap();
        assert!(text.contains("## Jugement V1"), "{text}");
        assert!(
            text.contains("**refusé** (en échec, sautés ou sous leur minimum de couverture : E3)"),
            "{text}"
        );
        assert!(text.contains("| E3 | oui | sous_minimum | sous_minimum | ponts 11 < 12 |"), "{text}");
    }

    /// V1 sans couverture mesurée : tout critère à minimum est sous son minimum (refus).
    #[tokio::test]
    async fn v1_without_coverage_refuses() {
        let (dir, gel_commit, gel) = repo_for("V1");
        let (r, bench) = (dir.path(), dir.path().join("bench"));
        let mut res = results(json!([]));
        res["verdicts"] = json!({"E1": "pass", "E3": "pass"});
        judge_with(r, &bench, "V1", &gel_commit, &gel, true, bench.join("reports/v1"), || async { Ok(res) })
            .await
            .unwrap();
        let text = std::fs::read_to_string(bench.join("judgements.md")).unwrap();
        assert!(
            text.contains("**refusé** (en échec, sautés ou sous leur minimum de couverture : E1, E3)"),
            "{text}"
        );
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
            dist_km: None,
            gain_baro_m: None,
            gain_mnt_m: None,
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
            v1: None,
            mvp_judged: false,
        };
        let held = crate::campaign::select_series(&m, &[], &[], Serie::TenueALEcart);
        assert_eq!(held.cases.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["b"]);
        assert_eq!(held.excluded_held_out, ["a", "c"]);
        let cal = crate::campaign::select(&m, &[], &[]);
        assert_eq!(cal.cases.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["a", "c"]);
    }

    #[test]
    fn report_dir_name_follows_the_campaign_convention() {
        assert_eq!(
            report_dir_name("2026-11-02", "2026.10.10", Some("bench1"), "MVP"),
            "2026-11-02-2026.10.10-bench1-jugement-MVP"
        );
        assert_eq!(
            report_dir_name("2026-11-02", "2026.10.10", None, "MVP"),
            "2026-11-02-2026.10.10-jugement-MVP"
        );
    }

    const COMMITS: Commits<'static> = Commits { code: "c0de", gel: "g" };

    #[test]
    fn entry_names_the_measured_code_and_head_separately() {
        let text = entry("MVP", &COMMITS, "x", &results(json!([])), &BTreeSet::new());
        assert!(
            text.contains("- Commit du code mesuré (`crates/`, `tools/ord-bench/`, `Cargo.*`) : `c0de`\n"),
            "{text}"
        );
        assert!(text.contains("- HEAD au jugement : `abc` ; commit de gel : `g`\n"), "{text}");
        assert!(!text.contains("Commit du service"), "{text}");
    }

    #[test]
    fn entry_is_one_list_then_one_row_per_criterion() {
        let mut r = results(json!([]));
        let indicatif = |m: f64| {
            json!([{"groupe": "dplus", "mediane": m, "mediane_dans_bornes": true, "traces_hors_bornes": []},
                {"groupe": "plaine", "mediane": 0.9757, "mediane_dans_bornes": false, "traces_hors_bornes": ["perso-99"]}])
        };
        r["verdicts"] = json!({"E1": "pass", "E1_echecs": [], "E2": "reported", "E2_indicatif": indicatif(0.9861),
            "M1": "fail", "M1_echecs": ["perso-23"], "R1_R2": "skipped", "R1_R2_echecs": [], "R3": "fail",
            "R3_echecs": ["perso-13.s1"], "E3_hors_france": "reported"});
        r["input_with_breakpoints"]["verdicts"] = json!({"E1": "pass", "E1_echecs": [], "E2": "reported",
            "E2_indicatif": indicatif(0.9854), "M1": "fail", "M1_echecs": ["perso-23"], "R3": "fail", "R3_echecs": []});
        let blocking: BTreeSet<String> = ["E1", "M1", "R1", "R2"].map(String::from).into();
        let text = entry("MVP", &COMMITS, "x", &r, &blocking);
        // Une seule liste : aucune ligne vide entre deux éléments.
        let list: Vec<&str> = text.lines().skip(2).take_while(|l| !l.is_empty()).collect();
        assert!(list.iter().all(|l| l.starts_with("- ")), "{text}");
        assert!(
            list.iter().any(|l| l.starts_with(
                "- Résultat : **refusé** (en échec, sautés ou sous leur minimum de couverture : M1, R1_R2)"
            )),
            "{text}"
        );
        assert!(list.iter().any(|l| l.starts_with("- Non bloquants en échec ou sautés")), "{text}");
        // Tableau : une ligne par critère, sans ligne d'annexe ni JSON brut.
        assert!(
            text.contains("| Critère | bloquant | distance | input_with_breakpoints | ids en échec |"),
            "{text}"
        );
        assert!(!text.contains("_echecs |") && !text.contains("| E2_indicatif"), "{text}");
        let table = &text[text.find("| Critère").unwrap()..];
        assert!(!table.contains('{') && !table.contains('['), "{text}");
        assert!(text.contains("| M1 | oui | fail | fail | perso-23 |"), "{text}");
        assert!(text.contains("| E1 | oui | pass | pass | — |"), "{text}");
        assert!(text.contains("| R1_R2 | oui | skipped | — | — |"), "{text}");
        assert!(text.contains("| E3_hors_france | non | reported | — | — |"), "{text}");
        assert!(
            text.contains("| R3 | non | fail | fail | distance : perso-13.s1 ; input_with_breakpoints : — |"),
            "{text}"
        );
        assert!(
            text.contains(
                "| E2 | non | reported | reported | distance : indicatif : dplus 0,986 ; plaine 0,976 (médiane hors \
                 bornes) (hors bornes : perso-99) ; input_with_breakpoints : indicatif : dplus 0,985 ; plaine \
                 0,976 (médiane hors bornes) (hors bornes : perso-99) |"
            ),
            "{text}"
        );
        let rows = text.lines().filter(|l| l.starts_with("| ") && !l.starts_with("| Critère")).count();
        assert_eq!(rows, 6, "E1, E2, E3_hors_france, M1, R1_R2, R3 : {text}");
    }

    #[test]
    fn aux_keys_attach_to_the_longest_criterion_prefix() {
        let c: Vec<String> = ["E2", "E2b", "R1_R2", "E3", "E3_hors_france"].map(String::from).into();
        assert_eq!(owner("E2b_groupes_en_echec", &c).map(String::as_str), Some("E2b"));
        assert_eq!(owner("E2_indicatif", &c).map(String::as_str), Some("E2"));
        assert_eq!(owner("R1_R2_echecs", &c).map(String::as_str), Some("R1_R2"));
        assert_eq!(owner("E3_echecs", &c).map(String::as_str), Some("E3"));
        assert_eq!(owner("S9_x", &c), None);
    }

    #[test]
    fn code_commit_follows_code_paths_only() {
        let (dir, _, _) = repo();
        let r = dir.path();
        std::fs::create_dir_all(r.join("crates/x")).unwrap();
        std::fs::write(r.join("crates/x/lib.rs"), "// 1\n").unwrap();
        run(r, &["add", "."]);
        run(r, &["commit", "-q", "-m", "code"]);
        let code = String::from_utf8(git(r, &["rev-parse", "--short", "HEAD"]).unwrap().stdout).unwrap();
        // Un commit sous bench/ ne change pas le code mesuré.
        std::fs::write(r.join("bench/judgements.md"), "x\n").unwrap();
        run(r, &["add", "."]);
        run(r, &["commit", "-q", "-m", "banc"]);
        assert_eq!(code_commit(r), code.trim());
        std::fs::write(r.join("Cargo.toml"), "[workspace]\n").unwrap();
        run(r, &["add", "."]);
        run(r, &["commit", "-q", "-m", "cargo"]);
        let cargo = String::from_utf8(git(r, &["rev-parse", "--short", "HEAD"]).unwrap().stdout).unwrap();
        assert_eq!(code_commit(r), cargo.trim());
        std::fs::write(r.join("crates/x/lib.rs"), "// 2\n").unwrap();
        assert_eq!(code_commit(r), format!("{}-dirty", cargo.trim()));
    }

    #[test]
    fn entry_without_bench_revision_says_so() {
        let mut r = results(json!([]));
        r["campagne"]["revision_banc"] = Value::Null;
        assert!(entry("MVP", &COMMITS, "x", &r, &BTreeSet::new())
            .contains("Révision du banc (`revision_banc`) : aucune"));
    }

    #[test]
    fn a_non_blocking_failure_is_reported_without_refusing() {
        // R3 non bloquant (§4 bis.7) : son échec n'est pas un refus ; R1_R2 suit R1 et R2.
        let mut r = results(json!([]));
        r["verdicts"] = json!({"E1": "pass", "R3": "fail", "R1_R2": "pass", "E3_hors_france": "skipped"});
        r["input_with_breakpoints"]["verdicts"] = json!({"E1": "pass", "R3": "fail"});
        let blocking: BTreeSet<String> = ["E1", "R1", "R2"].map(String::from).into();
        let text = entry("MVP", &COMMITS, "x", &r, &blocking);
        assert!(text.contains("**accepté**"), "{text}");
        assert!(text.contains("rapportés sans refuser : E3_hors_france, R3"), "{text}");
        assert!(text.contains("| R1_R2 | oui | pass | — | — |"), "{text}");
        r["verdicts"]["R1_R2"] = json!("fail");
        assert!(entry("MVP", &COMMITS, "x", &r, &blocking)
            .contains("**refusé** (en échec, sautés ou sous leur minimum de couverture : R1_R2)"));
    }

    #[tokio::test]
    async fn an_existing_judgement_report_refuses_whatever_its_date() {
        let (dir, gel_commit, gel) = repo();
        let (r, bench) = (dir.path(), dir.path().join("bench"));
        // Rapport d'un jugement antérieur, autre date et autres versions, sans entrée consignée.
        std::fs::create_dir_all(bench.join("reports/2025-01-01-2025.01.1-bench2-jugement-MVP")).unwrap();
        // Un autre incrément ou un nom voisin ne compte pas.
        std::fs::create_dir_all(bench.join("reports/2025-01-01-2025.01.1-jugement-MVP2")).unwrap();
        assert_eq!(existing_reports(&bench, "MVP"), ["2025-01-01-2025.01.1-bench2-jugement-MVP"]);
        assert!(existing_reports(&bench, "V1").is_empty());
        let err = judge_with(r, &bench, "MVP", &gel_commit, &gel, true, bench.join("reports/j"), || async {
            panic!("aucune mesure attendue")
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("2025-01-01-2025.01.1-bench2-jugement-MVP"), "{err}");
        assert!(!bench.join("judgements.md").exists());
        assert!(!bench.join("reports/j").exists());
    }

    #[test]
    fn already_judged_reads_headings_only() {
        assert!(already_judged("# J\n\n## Jugement MVP (2026-11-02)\n", "MVP"));
        assert!(!already_judged("## Jugement MVP2 (x)\nle MVP est cité\n", "MVP"));
    }
}
