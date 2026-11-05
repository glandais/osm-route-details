//! `ord-bench check` sur le banc du dépôt, sans traces privées (comme en CI) : empreintes des
//! fichiers versionnés, cohérence criteria.yaml ↔ README, Q1.

use ord_bench::check::check;
use ord_bench::BenchPaths;

#[test]
fn bench_fingerprints_and_q1() {
    let bench = BenchPaths::repo_bench();
    let missing_private = std::env::temp_dir().join("ord-bench-no-private");
    let paths = BenchPaths::new(&bench, None, Some(&missing_private));
    let report = check(&paths).expect("check");
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert!(report.verified > 100, "{}", report.verified);
    assert!(!report.private_present);
    // 4 négatifs du MVP, 3 de V1 et 2 de V1.1 (neg-08, neg-09), tous en calibration depuis le
    // jugement V1.1 (README §2, point 3 ; bench/judgements.md) : plus aucun tenu à l'écart.
    assert_eq!(report.q1.negatives, 9);
    assert_eq!(report.q1_tenue.negatives, 0);
    assert!(report.q1.pass(), "{:?}", report.q1);
}

#[test]
fn held_out_cases_are_excluded_before_any_read() {
    let manifest = ord_bench::manifest::Manifest::load(&BenchPaths::repo_bench()).expect("manifest");
    let all = ord_bench::campaign::select(&manifest, &[], &[]);
    // Série tenue à l'écart du manifest : exclue, négatifs à part. Depuis le jugement V1.2
    // (bench/judgements.md), ses 32 traces (`perso_v12`, perso-166 à perso-197, README §4
    // quinquies) sont en calibration, comme celles de V1.1, et depuis le jugement V1.3 ses 23
    // traces (`perso_v13`, perso-198 à perso-220, README §4 septies) aussi : plus aucune trace
    // tenue à l'écart.
    let held: Vec<&str> = manifest
        .traces
        .iter()
        .filter(|t| t.serie == ord_bench::manifest::Serie::TenueALEcart && t.groupe != "negatif")
        .map(|t| t.id.as_str())
        .collect();
    assert_eq!(all.excluded_held_out, held);
    assert!(held.is_empty(), "{held:?}");
    let v13 = manifest.section("V1.3").expect("section perso_v13");
    assert!(v13.judged);
    let expected: Vec<String> = (198..=220).map(|k| format!("perso-{k}")).collect();
    assert!(expected.iter().all(|id| v13.traces.contains(id)));
    assert_eq!(v13.traces.len(), 23);
    assert_eq!(all.cases.iter().filter(|t| v13.traces.contains(&t.id)).count(), 23);
    assert_eq!(v13.planifies.len(), 10);
    assert!(v13.planifies.iter().all(|p| p.id.starts_with("pl13-")));
    assert!(v13.planifies.iter().all(|p| manifest.is_new_series(&p.id)));
    assert_eq!(manifest.tile_campaign(v13).map(|c| c.id.as_str()), Some("tuiles_2026_11_04"));
    assert!(manifest.held_out_increment("V1.3").is_err());
    let v12 = manifest.section("V1.2").expect("section perso_v12");
    assert!(v12.judged);
    let expected: Vec<String> = (166..=197).map(|k| format!("perso-{k}")).collect();
    assert!(expected.iter().all(|id| v12.traces.contains(id)));
    assert_eq!(all.cases.iter().filter(|t| v12.traces.contains(&t.id)).count(), 32);
    assert_eq!(manifest.tile_campaign(v12).map(|c| c.id.as_str()), Some("tuiles_2026_11_03"));
    let v11 = manifest.section("V1.1").expect("section perso_v11");
    assert!(v11.judged);
    assert_eq!(all.cases.iter().filter(|t| v11.traces.contains(&t.id)).count(), 36);
    assert!(all.cases.iter().all(|t| t.serie == ord_bench::manifest::Serie::Calibration));
    assert!(all.cases.iter().all(|t| t.groupe != "negatif"));
    // Campagne de tuiles datée (`tuiles_2026_11_02`) : lue, rattachée à V1.1, date OSM du TileJSON.
    let campaign = manifest.tile_campaign(v11).expect("campagne de tuiles");
    assert_eq!(campaign.id, "tuiles_2026_11_02");
    assert_eq!(campaign.date_osm.as_deref(), Some("2026-10-19T00:00:04Z"));
    assert_eq!(manifest.tile_campaign(manifest.v1.as_ref().unwrap()), Some(campaign));
    // Son espace de cache est celui des tuiles de rues du service : `fill-cache` la verrouille.
    let config = ord_bench::service::bench_config_with(
        std::path::Path::new("/nulle-part"),
        true,
        ord_bench::service::Variant::DEFAULT,
        true,
    );
    assert_eq!(ord_server::wiring::streets_namespace(&config), campaign.espace_cache);
}

/// Retard du baro gelé (README §4 ter.9, manifest `perso_v1.retard_baro_gele`, 48ca44a) : le
/// fichier du dépôt est lu à son empreinte (101 cas, tous estimés) ; `check` le vérifie, sans
/// bench/private/.
#[test]
fn frozen_baro_lag_is_read_at_its_manifest_fingerprint() {
    let bench = BenchPaths::repo_bench();
    let missing_private = std::env::temp_dir().join("ord-bench-no-private");
    let paths = BenchPaths::new(&bench, None, Some(&missing_private));
    let manifest = ord_bench::manifest::Manifest::load(&bench).expect("manifest");
    let entry = manifest.v1.as_ref().and_then(|v| v.retard_gele.clone()).expect("perso_v1.retard_baro_gele");
    assert_eq!(entry.0, "data/retard-baro-v1.json");
    let frozen = ord_bench::retard::RetardGele::load(&paths, Some(&entry)).expect("fichier gelé");
    assert_eq!(frozen.cas.len(), 101);
    assert!(frozen.cas.values().all(Option::is_some));
    let report = check(&paths).expect("check");
    assert!(report.failures.is_empty(), "{:?}", report.failures);
}

/// Un fichier gelé modifié (ou absent) est refusé, au chargement comme par `check`. Copie
/// temporaire du banc : le fichier du dépôt n'est jamais touché.
#[test]
fn an_altered_frozen_baro_lag_is_refused() {
    let repo = BenchPaths::repo_bench();
    let root = tempfile::tempdir().unwrap();
    let bench = root.path().join("bench");
    std::fs::create_dir_all(bench.join("data")).unwrap();
    for f in ["manifest.yaml", "criteria.yaml", "README.md", "data/retard-baro-v1.json"] {
        std::fs::copy(repo.join(f), bench.join(f)).unwrap();
    }
    let private = root.path().join("no-private");
    let paths = BenchPaths::new(&bench, None, Some(&private));
    let manifest = ord_bench::manifest::Manifest::load(&bench).expect("manifest");
    let entry = manifest.v1.as_ref().and_then(|v| v.retard_gele.clone()).unwrap();
    assert!(ord_bench::retard::RetardGele::load(&paths, Some(&entry)).is_ok());
    let label = |r: &ord_bench::check::CheckReport| {
        r.failures.iter().filter(|f| f.starts_with("retard_baro_gele")).cloned().collect::<Vec<_>>()
    };
    assert!(label(&check(&paths).expect("check")).is_empty());
    // Un δ modifié : même structure, empreinte différente.
    let file = bench.join("data/retard-baro-v1.json");
    let text = std::fs::read_to_string(&file).unwrap();
    let altered = text.replacen("\"retard_m\": 15.0", "\"retard_m\": 20.0", 1);
    assert_ne!(altered, text);
    std::fs::write(&file, altered).unwrap();
    let err = ord_bench::retard::RetardGele::load(&paths, Some(&entry)).unwrap_err();
    assert!(err.to_string().contains("empreinte"), "{err}");
    assert_eq!(label(&check(&paths).expect("check")), ["retard_baro_gele : empreinte différente"]);
    // Absent : refusé aussi.
    std::fs::remove_file(&file).unwrap();
    assert!(ord_bench::retard::RetardGele::load(&paths, Some(&entry)).is_err());
    assert_eq!(label(&check(&paths).expect("check")), ["retard_baro_gele : fichier absent"]);
}
