//! Garde-fou de la série tenue à l'écart V1, sur un banc **synthétique** (aucune donnée réelle,
//! aucun réseau) : la campagne de calibration (`run`, et `check`, `v1_ctx`, `select_series` que
//! partage `fill-cache`) n'ouvre aucun fichier d'un cas tenu à l'écart et ne calcule aucun cas
//! tenu à l'écart (trace, itinéraire, ouvrage, croisement, témoin).
//!
//! Les fichiers des cas tenus à l'écart sont rendus illisibles (`chmod 000`) : toute ouverture
//! échouerait ; leurs identifiants ne doivent apparaître que dans les listes d'exclusion.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use ord_bench::manifest::{Manifest, Serie};
use ord_bench::BenchPaths;

fn sha(p: &Path) -> String {
    ord_bench::hash::sha256_file(p).unwrap()
}

fn gpx(lat0: f64, n: usize) -> String {
    let pts: String =
        (0..n).map(|i| format!("<trkpt lat=\"{}\" lon=\"0.5\"/>", lat0 + i as f64 * 0.001)).collect();
    format!("<gpx><trk><trkseg>{pts}</trkseg></trk></gpx>")
}

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn lock(p: &Path) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o000)).unwrap();
}

/// Banc synthétique : une trace et un itinéraire par série, un ouvrage, un croisement et un
/// témoin par série ; fichiers des cas tenus à l'écart verrouillés.
fn setup() -> (tempfile::TempDir, BenchPaths, Vec<std::path::PathBuf>) {
    let root = tempfile::tempdir().unwrap();
    let bench = root.path().join("bench");
    let private = bench.join("private");
    let repo = BenchPaths::repo_bench();
    for f in ["criteria.yaml", "README.md"] {
        std::fs::create_dir_all(&bench).unwrap();
        std::fs::copy(repo.join(f), bench.join(f)).unwrap();
    }
    // Calibration : perso-40 (recalée) et l'itinéraire pl-cal ; tenus à l'écart : perso-90, pl-held.
    write(&private.join("traces/perso-40.gpx"), &gpx(10.0, 30));
    write(&private.join("recales/perso-40.gpx"), &gpx(10.0, 30));
    write(
        &private.join("recales/perso-40.baro.csv"),
        "s_recale_m,baro_m,s_origine_m,distance_au_trace_m,insere\n0,1,0,1,0\n100,2,100,1,0\n",
    );
    write(&private.join("recales/journal.csv"), "perso-40,1,1,1,1,1,0\n");
    write(&private.join("v1/planifies/pl-cal.gpx"), &gpx(20.0, 30));
    let held: Vec<std::path::PathBuf> = [
        "traces/perso-90.gpx",
        "traces/perso-90.trim1km.gpx",
        "recales/perso-90.gpx",
        "recales/perso-90.baro.csv",
        "recales/perso-90.surface.csv",
        "v1/planifies/pl-held.gpx",
        "v1/r12/reference/pl-held.reference.json",
    ]
    .iter()
    .map(|f| private.join(f))
    .collect();
    for f in &held {
        write(f, "TENUE A L'ECART : ne jamais ouvrir");
    }
    let pt = |lat: f64| format!("[{lat}, 0.5]");
    write(
        &private.join("v1/ouvrages-v1.json"),
        &format!(
            r#"[{{"id":"nv-1","type":"bridge","chain":[],"length_m":50,"pts":{{"A":{a},"milieu":{m},"B":{b}}},"tags":{{}},"serie":"calibration"}},
               {{"id":"nv-2","type":"bridge","chain":[],"length_m":50,"pts":{{"A":{a2},"milieu":{m2},"B":{b2}}},"tags":{{}},"serie":"tenue_a_l_ecart"}},
               {{"id":"nv-3","type":"passage sous pont","chain":[],"length_m":0,"pts":{{"croisement":{c}}},"tags":{{}},"serie":"tenue_a_l_ecart"}}]"#,
            a = pt(10.005),
            m = pt(10.0055),
            b = pt(10.006),
            a2 = pt(10.015),
            m2 = pt(10.0155),
            b2 = pt(10.016),
            c = pt(20.01),
        ),
    );
    write(
        &private.join("v1/temoins-v1.json"),
        &format!(
            r#"[{{"id":"perso-40-s0","trace":"perso-40","planifie":false,"nature":"digue","length_m":500,"debut":{d0},"fin":{f0},"serie":"calibration"}},
               {{"id":"perso-40-s1","trace":"perso-40","planifie":false,"nature":"digue","length_m":500,"debut":{d1},"fin":{f1},"serie":"tenue_a_l_ecart"}},
               {{"id":"pl-held","trace":"pl-held","planifie":true,"nature":"digue","length_m":5000,"serie":"tenue_a_l_ecart"}}]"#,
            d0 = pt(10.001),
            f0 = pt(10.004),
            d1 = pt(10.020),
            f1 = pt(10.025),
        ),
    );
    write(
        &private.join("v1/series-v1.csv"),
        "id,type,groupe,serie,passages_masques\n\
         perso-40,trace,plaine,calibration,nv-2 perso-40-s1\n\
         perso-90,trace,plaine,tenue_a_l_ecart,\n\
         pl-cal,planifie,E4,par_cas,\n\
         pl-held,planifie,S2,tenue_a_l_ecart,\n\
         nv-1,ouvrage,,calibration,\n\
         nv-2,ouvrage,,tenue_a_l_ecart,\n\
         nv-3,ouvrage,,tenue_a_l_ecart,\n\
         perso-40-s0,temoin,digue,calibration,\n\
         perso-40-s1,temoin,digue,tenue_a_l_ecart,\n\
         pl-held,temoin,digue,tenue_a_l_ecart,\n",
    );
    let v1 = |f: &str| sha(&private.join(f));
    let manifest = format!(
        r#"version: 1
vcyclist: []
osm_surface: []
ouvrages:
  serie_tenue_a_l_ecart: []
perso:
  traces:
    - {{id: neg-01, groupe: negatif, serie: calibration, fichier: private/traces/neg-01.gpx, qualification: {{verdict: suspect}}}}
perso_v1:
  ouvrages_v1: {{fichier: private/v1/ouvrages-v1.json, sha256: {o}}}
  temoins_v1: {{fichier: private/v1/temoins-v1.json, sha256: {t}}}
  series_v1: {{fichier: private/v1/series-v1.csv, sha256: {s}}}
  planifies:
    - {{id: pl-cal, role: E4, fichier: private/v1/planifies/pl-cal.gpx, sha256: {pc}}}
    - {{id: pl-held, role: S2, nature: digue, fichier: private/v1/planifies/pl-held.gpx, sha256: {ph}}}
  traces:
    - {{id: perso-40, groupe: plaine, serie: calibration, passages_masques: [nv-2, perso-40-s1], fichier: private/traces/perso-40.gpx, sha256: {p40}}}
    - {{id: perso-90, groupe: plaine, serie: tenue_a_l_ecart, fichier: private/traces/perso-90.gpx, sha256: {p90}}}
    - {{id: neg-05, groupe: negatif, serie: tenue_a_l_ecart, fichier: private/traces/neg-05.gpx, qualification: {{verdict: suspect, gain_baro_m: 100, gain_mnt_m: 400}}}}
"#,
        o = v1("v1/ouvrages-v1.json"),
        t = v1("v1/temoins-v1.json"),
        s = v1("v1/series-v1.csv"),
        pc = v1("v1/planifies/pl-cal.gpx"),
        ph = v1("v1/planifies/pl-held.gpx"),
        p40 = v1("traces/perso-40.gpx"),
        p90 = v1("traces/perso-90.gpx"),
    );
    write(&bench.join("manifest.yaml"), &manifest);
    for f in &held {
        lock(f);
    }
    let paths = BenchPaths::new(&bench, Some(&root.path().join("cache")), Some(&private));
    (root, paths, held)
}

fn unlock(held: &[std::path::PathBuf]) {
    for f in held {
        let _ = std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o600));
    }
}

#[test]
fn calibration_check_and_selection_never_open_held_out_files() {
    let (_root, paths, held) = setup();
    let report = ord_bench::check::check(&paths).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert!(
        report.skipped.iter().any(|s| s.contains("3 cas de la série tenue à l'écart")),
        "{:?}",
        report.skipped
    );
    // Q1 : le négatif tenu à l'écart n'est pas compté en calibration.
    assert_eq!(report.q1.negatives, 1);
    assert_eq!(report.q1_tenue.negatives, 1);
    let manifest = Manifest::load(&paths.bench).unwrap();
    let sel = ord_bench::campaign::select_series(&manifest, &[], &[], Serie::Calibration);
    assert_eq!(sel.cases.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["perso-40"]);
    let v1 = ord_bench::campaign::v1_ctx(&paths, &manifest, &[], Serie::Calibration).unwrap();
    assert_eq!(v1.planned.cases.iter().map(|c| c.0.id.as_str()).collect::<Vec<_>>(), ["pl-cal"]);
    assert_eq!(v1.planned.excluded, ["pl-held"]);
    assert_eq!(v1.temoins.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["perso-40-s0"]);
    let ouvrages = ord_bench::ouvrages::series_ouvrages(&paths, &manifest, Serie::Calibration).unwrap();
    assert_eq!(ouvrages.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["nv-1"]);
    // Le jugement, lui, vérifie aussi les fichiers tenus à l'écart (illisibles ici : échec).
    let all = ord_bench::check::check_series(&paths, Serie::TenueALEcart).unwrap();
    assert!(!all.failures.is_empty());
    unlock(&held);
}

#[tokio::test]
async fn calibration_run_computes_no_held_out_case() {
    let (_root, paths, held) = setup();
    let ids: Vec<String> = ["perso-40", "perso-90", "pl-cal", "pl-held"].map(String::from).to_vec();
    // Cache vide, hors ligne : les cas de calibration peuvent être sautés, jamais ouverts du côté
    // tenu à l'écart.
    let results = ord_bench::campaign::run_series(&paths, &[], &ids, Serie::Calibration).await;
    unlock(&held);
    let mut results = match results {
        Ok(r) => r,
        // Démarrage du service impossible sur un cache vide : la sélection a déjà été vérifiée
        // ci-dessus ; l'erreur ne doit pas venir d'un fichier tenu à l'écart.
        Err(e) => {
            let msg = format!("{e:#}");
            eprintln!("run_series : {msg}");
            assert!(!msg.contains("perso-90") && !msg.contains("pl-held"), "{msg}");
            return;
        }
    };
    assert_eq!(results["campagne"]["exclus_serie_tenue_a_l_ecart"], serde_json::json!(["perso-90"]));
    assert_eq!(results["campagne"]["itineraires_exclus"], serde_json::json!(["pl-held"]));
    if let Some(c) = results["campagne"].as_object_mut() {
        c.remove("exclus_serie_tenue_a_l_ecart");
        c.remove("itineraires_exclus");
    }
    // Fenêtres masquées déclarées au manifest (versionné) : ids rapportés comme masqués seulement.
    fn strip(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                m.remove("passages_masques");
                m.remove("passages_non_retrouves");
                m.values_mut().for_each(strip);
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut results);
    let text = results.to_string();
    for id in ["perso-90", "pl-held", "nv-2", "nv-3", "perso-40-s1"] {
        assert!(!text.contains(id), "{id} présent dans le rapport de calibration");
    }
}
