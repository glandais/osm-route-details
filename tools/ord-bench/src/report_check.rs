//! `compare` et `bench-report` (T05 §4, §6) : sans cache ni `bench/private/`, sans réseau.
//!
//! `bench-report` (job CI) : un `results.json` de calibration commité dans `bench/reports/` doit
//! porter les empreintes **courantes** (règles du service, `manifest.yaml`, `criteria.yaml`,
//! `BenchRules`) ; si `bench/cache.lock` existe, la même empreinte de données ; si
//! `bench/baseline.json` existe, aucun verdict ne bascule de `pass` vers `fail` ou `skipped`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::{json, Value};

/// Empreintes qui rendent deux campagnes comparables (bench §2.4).
const BENCH_PRINTS: [&str; 3] = ["manifest_sha256", "criteria_sha256", "bench_rules_sha256"];

/// Empreintes courantes : règles du service et banc.
pub fn current_prints(paths: &crate::BenchPaths) -> anyhow::Result<Value> {
    let report = crate::check::check(paths)?;
    crate::campaign::ensure_check(&report)?;
    let rules = serde_json::to_string(&ord_core::Rules::default())?;
    let bench_rules = serde_json::to_string(&crate::BenchRules::default())?;
    Ok(json!({
        "rules_version": ord_core::RULES_VERSION,
        "rules_fingerprint": crate::hash::sha256_bytes(rules.as_bytes()),
        "manifest_sha256": report.manifest_sha256,
        "criteria_sha256": report.criteria_sha256,
        "bench_rules_sha256": crate::hash::sha256_bytes(bench_rules.as_bytes()),
    }))
}

/// Verdicts d'une campagne, par mode.
fn verdicts(r: &Value) -> [(&'static str, &Value); 2] {
    [("distance", &r["verdicts"]), ("input_with_breakpoints", &r["input_with_breakpoints"]["verdicts"])]
}

/// Différence de verdicts entre deux campagnes ; invalide si leurs empreintes de banc diffèrent.
pub fn compare(before: &Value, after: &Value) -> Value {
    let differing: Vec<&str> =
        BENCH_PRINTS.into_iter().filter(|k| before["campagne"][*k] != after["campagne"][*k]).collect();
    if !differing.is_empty() {
        return json!({"comparable": false, "empreintes_differentes": differing});
    }
    let mut changes = Vec::new();
    let mut degraded = Vec::new();
    for ((mode, a), (_, b)) in verdicts(before).into_iter().zip(verdicts(after)) {
        let ids: std::collections::BTreeSet<&String> =
            a.as_object().into_iter().chain(b.as_object()).flat_map(|m| m.keys()).collect();
        for id in ids {
            let (va, vb) = (&a[id.as_str()], &b[id.as_str()]);
            if va != vb {
                changes.push(json!({"id": id, "mode": mode, "avant": va, "apres": vb}));
                if va == "pass" && (vb == "fail" || vb == "skipped" || vb.is_null()) {
                    degraded.push(format!("{id} ({mode})"));
                }
            }
        }
    }
    json!({"comparable": true, "changements": changes, "degradations": degraded})
}

fn read_json(path: &Path) -> anyhow::Result<Value> {
    serde_json::from_str(
        &std::fs::read_to_string(path).with_context(|| format!("lecture de {}", path.display()))?,
    )
    .with_context(|| format!("{} illisible", path.display()))
}

/// Campagnes de calibration commitées, triées par date puis par dossier.
fn calibration_reports(bench: &Path) -> anyhow::Result<Vec<(PathBuf, Value)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(bench.join("reports")).context("bench/reports")? {
        let path = entry?.path().join("results.json");
        if path.is_file() {
            let v = read_json(&path)?;
            if v["campagne"]["type"] == "calibration" {
                out.push((path, v));
            }
        }
    }
    out.sort_by(|(pa, a), (pb, b)| {
        let d = |v: &Value| v["campagne"]["date"].as_str().unwrap_or("").to_owned();
        d(a).cmp(&d(b)).then_with(|| pa.cmp(pb))
    });
    Ok(out)
}

/// Contrôle du job CI `bench-report` ; `Ok((rapport, vert))`.
pub fn bench_report(bench: &Path, current: &Value) -> anyhow::Result<(Value, bool)> {
    let reports = calibration_reports(bench)?;
    let keys = ["rules_fingerprint", "manifest_sha256", "criteria_sha256", "bench_rules_sha256"];
    let matching: Vec<&(PathBuf, Value)> =
        reports.iter().filter(|(_, v)| keys.iter().all(|k| v["campagne"][*k] == current[*k])).collect();
    let Some((path, report)) = matching.last().copied() else {
        let last = reports.last().map(|(p, v)| {
            let differing: Vec<&str> =
                keys.into_iter().filter(|k| v["campagne"][*k] != current[*k]).collect();
            json!({"rapport": p.display().to_string(), "empreintes_differentes": differing})
        });
        return Ok((
            json!({"verdict": "fail", "raison": "aucun results.json de calibration aux empreintes courantes",
                   "courantes": current, "dernier": last}),
            false,
        ));
    };
    let mut ok = true;
    let lock = bench.join("cache.lock");
    let data = if lock.is_file() {
        let l = read_json(&lock)?;
        let same = l["data_fingerprint"] == report["campagne"]["data_fingerprint"];
        ok &= same;
        json!({"cache_lock": l["data_fingerprint"], "rapport": report["campagne"]["data_fingerprint"], "egales": same})
    } else {
        json!("bench/cache.lock absent : empreinte des données non vérifiée")
    };
    let baseline = bench.join("baseline.json");
    let comparison = if baseline.is_file() {
        let c = compare(&read_json(&baseline)?, report);
        ok &= c["comparable"] == false || c["degradations"].as_array().is_some_and(Vec::is_empty);
        c
    } else {
        json!("bench/baseline.json absent : comparaison sautée")
    };
    let verdict = if ok { "pass" } else { "fail" };
    let rel = path.strip_prefix(bench).unwrap_or(path).display().to_string();
    Ok((
        json!({"verdict": verdict, "rapport": rel, "private_present": report["campagne"]["private_present"],
               "donnees": data, "baseline": comparison}),
        ok,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn campaign(date: &str, rules: &str, verdict: &str) -> Value {
        json!({"campagne": {"type": "calibration", "date": date, "rules_fingerprint": rules,
                "manifest_sha256": "m", "criteria_sha256": "c", "bench_rules_sha256": "b",
                "data_fingerprint": "d", "private_present": true},
               "verdicts": {"E1": verdict, "E3": "fail"},
               "input_with_breakpoints": {"verdicts": {"E1": "pass"}}})
    }

    fn current(rules: &str) -> Value {
        json!({"rules_fingerprint": rules, "manifest_sha256": "m", "criteria_sha256": "c", "bench_rules_sha256": "b"})
    }

    #[test]
    fn compare_flags_degradations_and_incomparable_campaigns() {
        let c = compare(&campaign("a", "r", "pass"), &campaign("b", "r", "fail"));
        assert_eq!(c["degradations"], json!(["E1 (distance)"]));
        let mut other = campaign("b", "r", "pass");
        other["campagne"]["criteria_sha256"] = json!("c2");
        assert_eq!(compare(&campaign("a", "r", "pass"), &other)["comparable"], false);
    }

    #[test]
    fn bench_report_needs_a_report_at_current_prints() {
        let dir = tempfile::tempdir().unwrap();
        let b = dir.path();
        for (name, v) in [
            ("2026-10-31-x", campaign("2026-10-31", "old", "pass")),
            ("2026-11-01-y", campaign("2026-11-01", "new", "pass")),
        ] {
            std::fs::create_dir_all(b.join("reports").join(name)).unwrap();
            std::fs::write(b.join("reports").join(name).join("results.json"), v.to_string()).unwrap();
        }
        let (r, ok) = bench_report(b, &current("new")).unwrap();
        assert!(ok, "{r}");
        assert_eq!(r["rapport"], "reports/2026-11-01-y/results.json");
        let (r, ok) = bench_report(b, &current("newer")).unwrap();
        assert!(!ok);
        assert_eq!(r["dernier"]["empreintes_differentes"], json!(["rules_fingerprint"]));
        // Référence : une bascule pass → fail échoue ; un cache.lock différent aussi.
        std::fs::write(b.join("baseline.json"), campaign("2026-10-30", "x", "pass").to_string()).unwrap();
        let mut failing = campaign("2026-11-02", "new", "fail");
        failing["campagne"]["date"] = json!("2026-11-02");
        std::fs::create_dir_all(b.join("reports/2026-11-02-z")).unwrap();
        std::fs::write(b.join("reports/2026-11-02-z/results.json"), failing.to_string()).unwrap();
        assert!(!bench_report(b, &current("new")).unwrap().1);
        std::fs::remove_file(b.join("baseline.json")).unwrap();
        std::fs::write(b.join("cache.lock"), json!({"data_fingerprint": "autre"}).to_string()).unwrap();
        assert!(!bench_report(b, &current("new")).unwrap().1);
    }
}
