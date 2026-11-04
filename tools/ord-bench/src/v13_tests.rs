//! Série tenue à l'écart V1.3 (manifest `perso_v13`, `serie_v13`, `tuiles_2026_11_04` ; README
//! §4 septies, b34b23f) : sur un banc synthétique sans réseau, MVP, V1, V1.1 et V1.2 jugés
//! (calibration), aucun cas V1.3 lu en calibration, ses éléments masqués sur les cas de
//! calibration, cas lus entiers au jugement ; puis, sur les règles et le manifest du dépôt, ce que
//! le jugement V1.3 fera d'E3 (rapporté, non bloquant), d'E2b (bloquant dans ses trois groupes) et
//! de Q1 (rapporté).
//!
//! Les fichiers propres des cas V1.3 n'existent pas sur le banc synthétique : les ouvrir serait un
//! échec (`fichier absent`) ; seuls les index et les instantanés existent.

use std::collections::BTreeSet;
use std::path::Path;

use ord_domain::LatLon;
use serde_json::{json, Value};

use crate::campaign::{extract_sources, judged_whole, ouvrage_refs, select_series, v1_ctx, SnapshotCache};
use crate::check::check_series;
use crate::manifest::{Manifest, Serie};
use crate::BenchPaths;

const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const V1_DATE: &str = "2026-11-01T13:01Z";
const V12_DATE: &str = "2026-11-03T06:24:42Z";
const V13_DATE: &str = "2026-11-04T03:16:54Z";

fn write(path: &Path, text: &str) -> String {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    crate::hash::sha256_bytes(text.as_bytes())
}

fn ids<T: AsRef<str>>(v: impl IntoIterator<Item = T>) -> Vec<String> {
    v.into_iter().map(|x| x.as_ref().to_owned()).collect()
}

/// Banc synthétique : MVP, V1, V1.1 et V1.2 jugés, V1.3 tenue à l'écart (instantané V1.3 daté à la
/// seconde, d'empreinte fausse : jamais ouvert en calibration).
fn bench() -> (tempfile::TempDir, BenchPaths) {
    let dir = tempfile::tempdir().unwrap();
    let b = dir.path().join("bench");
    let p = b.join("private");
    std::fs::create_dir_all(p.join("traces")).unwrap();
    write(&p.join("recales/journal.csv"), "id\n");
    write(
        &b.join("judgements.md"),
        "# J\n\n## Jugement MVP (x)\n\n## Jugement V1 (x)\n\n## Jugement V1.1 (x)\n\n## Jugement V1.2 (x)\n",
    );
    write(
        &b.join("criteria.yaml"),
        "criteres:\n  - {id: E1, bloquant: true}\nserie_tenue_a_l_ecart:\n  couverture:\n    minimums:\n\
         \x20     E1: {traces_france_fiables: 8, km_compares: 800}\n",
    );
    write(&b.join("README.md"), "## 3. M\n| **E1** | x |\n## 4. S\n| E1 | a |\n");
    let empty = |s: &str| {
        (
            write(&p.join(format!("{s}/ouvrages-{s}.json")), "[]"),
            write(&p.join(format!("{s}/temoins-{s}.json")), "[]"),
            write(&p.join(format!("{s}/series-{s}.csv")), "id,type,groupe,serie,passages_masques\n"),
        )
    };
    let (o1, t1, s1) = empty("v1");
    let (o11, t11, s11) = empty("v11");
    // V1.2 jugée : son ouvrage `tenue_a_l_ecart` à l'index est de la calibration.
    let o12 = write(
        &p.join("v12/ouvrages-v12.json"),
        r#"[{"id": "nv-948", "serie": "tenue_a_l_ecart", "type": "bridge", "length_m": 100,
            "pts": {"A": [47.003, 5.0], "milieu": [47.0035, 5.0], "B": [47.004, 5.0]}}]"#,
    );
    let t12 = write(&p.join("v12/temoins-v12.json"), "[]");
    let s12 = write(
        &p.join("v12/series-v12.csv"),
        "id,type,groupe,serie,passages_masques,fenetres_calibration\n\
         perso-166,trace,plaine,tenue_a_l_ecart,,\nnv-948,ouvrage,,tenue_a_l_ecart,,\n",
    );
    let o13 = write(
        &p.join("v13/ouvrages-v13.json"),
        r#"[{"id": "nv-1267", "serie": "tenue_a_l_ecart", "type": "bridge", "length_m": 100,
            "pts": {"A": [45.001, 5.0], "milieu": [45.0015, 5.0], "B": [45.002, 5.0]}}]"#,
    );
    let t13 = write(
        &p.join("v13/temoins-v13.json"),
        r#"[{"id": "perso-198-s0", "trace": "perso-198", "planifie": false, "nature": "couvert_arbore",
             "serie": "tenue_a_l_ecart", "debut": [46.0, 5.0], "fin": [46.01, 5.0]},
            {"id": "pl13-s2-rhin", "trace": "pl13-s2-rhin", "planifie": true, "nature": "digue",
             "serie": "tenue_a_l_ecart"}]"#,
    );
    let tt13 = write(
        &p.join("v13/temoins-v13-tags.json"),
        r#"{"perso-198-s0": {"serie": "tenue_a_l_ecart", "plages": []}}"#,
    );
    let s13 = write(
        &p.join("v13/series-v13.csv"),
        "id,type,groupe,serie,passages_masques,fenetres_calibration\n\
         perso-198,trace,plaine,tenue_a_l_ecart,,nv-948\npl13-r-x,planifie,R1_R2,tenue_a_l_ecart,,\n\
         pl13-s2-rhin,planifie,S2,tenue_a_l_ecart,,\nnv-1267,ouvrage,,tenue_a_l_ecart,,\n\
         perso-198-s0,temoin,couvert_arbore,tenue_a_l_ecart,,\npl13-s2-rhin,temoin,digue,tenue_a_l_ecart,,\n",
    );
    let snap1 =
        write(&p.join("osm-tags/2026-11-01T13-01Z.json"), r#"{"date": "2026-11-01T13:01:00Z", "tags": {}}"#);
    let snap12 = write(
        &p.join("osm-tags/2026-11-03T06-24-42Z.json"),
        r#"{"date": "2026-11-03T06:24:42Z", "tags": {}}"#,
    );
    write(&p.join("osm-tags/2026-11-04T03-16-54Z.json"), r#"{"date": "2026-11-04T03:16:54Z", "tags": {}}"#);
    let manifest = format!(
        "version: 1\nvcyclist: []\nosm_surface: []\nouvrages: {{serie_tenue_a_l_ecart: []}}\n\
         perso:\n  traces:\n    - {{id: perso-01, groupe: plaine, serie: calibration, fichier: private/traces/perso-01.gpx}}\n\
         perso_v1:\n  recalage: {{donnees_serveur: \"{V1_DATE}\"}}\n\
         \x20 ouvrages_v1: {{fichier: private/v1/ouvrages-v1.json, sha256: {o1}}}\n\
         \x20 temoins_v1: {{fichier: private/v1/temoins-v1.json, sha256: {t1}}}\n\
         \x20 series_v1: {{fichier: private/v1/series-v1.csv, sha256: {s1}}}\n\
         \x20 osm_tags_reference: {{fichier: private/osm-tags/2026-11-01T13-01Z.json, sha256: {snap1}, date: \"2026-11-01T13:01:00Z\"}}\n\
         perso_v11:\n\
         \x20 ouvrages_v11: {{fichier: private/v11/ouvrages-v11.json, sha256: {o11}}}\n\
         \x20 temoins_v11: {{fichier: private/v11/temoins-v11.json, sha256: {t11}}}\n\
         \x20 series_v11: {{fichier: private/v11/series-v11.csv, sha256: {s11}}}\n\
         tuiles_2026_11_03: {{espace_cache: x, date_osm: {{planetiler_osm_osmosisreplicationtime: \"2026-10-19T00:00:04Z\"}}}}\n\
         perso_v12:\n  tuiles: tuiles_2026_11_03\n  recalage: {{donnees_serveur: \"{V12_DATE}\"}}\n\
         \x20 osm_tags_reference: {{fichier: private/osm-tags/2026-11-03T06-24-42Z.json, sha256: {snap12}, date: \"{V12_DATE}\"}}\n\
         \x20 ouvrages_v12: {{fichier: private/v12/ouvrages-v12.json, sha256: {o12}}}\n\
         \x20 temoins_v12: {{fichier: private/v12/temoins-v12.json, sha256: {t12}}}\n\
         \x20 series_v12: {{fichier: private/v12/series-v12.csv, sha256: {s12}}}\n\
         \x20 traces:\n    - {{id: perso-166, groupe: plaine, serie: tenue_a_l_ecart, fichier: private/traces/perso-166.gpx}}\n\
         tuiles_2026_11_04: {{espace_cache: x, date_osm: {{planetiler_osm_osmosisreplicationtime: \"2026-10-19T00:00:04Z\"}}}}\n\
         serie_v13:\n  recalage: {{donnees_serveur: \"{V13_DATE}\"}}\n  tuiles: tuiles_2026_11_04\n\
         perso_v13:\n  tuiles: tuiles_2026_11_04\n  recalage: {{donnees_serveur: \"{V13_DATE}\"}}\n\
         \x20 osm_tags_reference: {{fichier: private/osm-tags/2026-11-04T03-16-54Z.json, sha256: {ZERO}, date: \"{V13_DATE}\"}}\n\
         \x20 ouvrages_v13: {{fichier: private/v13/ouvrages-v13.json, sha256: {o13}}}\n\
         \x20 temoins_v13: {{fichier: private/v13/temoins-v13.json, sha256: {t13}}}\n\
         \x20 temoins_v13_tags: {{fichier: private/v13/temoins-v13-tags.json, sha256: {tt13}}}\n\
         \x20 series_v13: {{fichier: private/v13/series-v13.csv, sha256: {s13}}}\n\
         \x20 planifies:\n\
         \x20   - {{id: pl13-r-x, role: R1_R2, fichier: private/v13/planifies/pl13-r-x.gpx, sha256: {ZERO}, \
                 reference: private/v13/r12/reference/pl13-r-x.reference.json, sha256_reference: {ZERO}, \
                 sha256_surface: {ZERO}, sha256_baro: {ZERO}}}\n\
         \x20   - {{id: pl13-s2-rhin, role: S2, nature: digue, fichier: private/v13/planifies/pl13-s2-rhin.gpx, sha256: {ZERO}}}\n\
         \x20 traces:\n\
         \x20   - {{id: perso-198, groupe: plaine, serie: tenue_a_l_ecart, fenetres_calibration: 1, mois: \"2021-04\", \
                 fichier: private/traces/perso-198.gpx, sha256: {ZERO}, sha256_trim1km: {ZERO}}}\n"
    );
    write(&b.join("manifest.yaml"), &manifest);
    let paths = BenchPaths::new(&b, Some(&dir.path().join("cache")), Some(&p));
    (dir, paths)
}

/// Calibration : MVP, V1, V1.1 et V1.2 en calibration (V1.2 jugée, sa trace et son ouvrage
/// compris) ; aucun cas V1.3 lu, ni trace, ni itinéraire, ni extrait, ni témoin ; ses éléments
/// (index) masquent leurs fenêtres sur les cas de calibration ; l'instantané V1.3 n'est pas ouvert.
#[test]
fn calibration_reads_no_v13_case() {
    let (_d, paths) = bench();
    let m = Manifest::load(&paths.bench).unwrap();
    assert!(m.section("V1.2").unwrap().judged, "V1.2 jugée");
    let v13 = m.section("V1.3").expect("section perso_v13 lue");
    assert!(!v13.judged);
    assert_eq!(m.tile_campaign(v13).map(|c| c.id.as_str()), Some("tuiles_2026_11_04"));
    assert_eq!(m.trace("perso-198").unwrap().serie, Serie::TenueALEcart);
    assert_eq!(m.trace("perso-166").unwrap().serie, Serie::Calibration);
    assert!(m.is_new_series("perso-198") && m.is_new_series("pl13-r-x") && m.is_new_series("pl13-s2-rhin"));

    let sel = select_series(&m, &[], &[], Serie::Calibration);
    assert_eq!(ids(sel.cases.iter().map(|t| t.id.as_str())), ["perso-01", "perso-166"]);
    assert_eq!(sel.excluded_held_out, ["perso-198"]);
    let mut m2 = m.clone();
    m2.traces.iter_mut().find(|t| t.id == "perso-198").unwrap().passages_masques = vec!["nv-948".into()];
    assert!(extract_sources(&m2, &select_series(&m2, &[], &[], Serie::Calibration), Serie::Calibration)
        .is_empty());

    let ctx = v1_ctx(&paths, &m, &[], Serie::Calibration).unwrap();
    assert!(ctx.planned.cases.is_empty());
    assert_eq!(ctx.planned.excluded, ["pl13-r-x", "pl13-s2-rhin"]);
    assert!(ctx.temoins.is_empty(), "aucun témoin V1.3 en calibration");

    // Masques : l'élément V1.3 sur un cas de calibration ; l'élément V1.2 jugé n'en est plus un.
    let refs = ouvrage_refs(&paths).unwrap();
    assert!(refs.contains_key("nv-1267") && refs.contains_key("perso-198-s0"));
    let track: Vec<LatLon> = (0..60).map(|k| LatLon::new(45.0 + k as f64 * 0.0001, 5.0)).collect();
    assert_eq!(
        crate::v1::other_series_on(&track, &refs, &ctx.rows, Serie::Calibration, "perso-01"),
        ["nv-1267"]
    );
    assert!(!judged_whole(&m, Serie::Calibration, "perso-01"));

    // Ouvrages mesurés en calibration : V1.2 (jugée), jamais nv-1267.
    let cal = crate::ouvrages::series_ouvrages(&paths, &m, Serie::Calibration).unwrap();
    assert_eq!(ids(cal.iter().map(|o| o.id.as_str())), ["nv-948"]);

    // `check` de la calibration : index V1.3 vérifiés, fichiers de ses cas et instantané jamais ouverts.
    let report = check_series(&paths, Serie::Calibration).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert!(report.skipped.iter().any(|s| s.contains("série tenue à l'écart")), "{:?}", report.skipped);
    std::fs::write(paths.private.join("v13/temoins-v13-tags.json"), "{}").unwrap();
    let report = check_series(&paths, Serie::Calibration).unwrap();
    assert!(
        report.failures.iter().any(|f| f == "temoins_v13_tags : empreinte différente"),
        "{:?}",
        report.failures
    );
}

/// Jugement V1.3 : seule série non jugée ; traces, itinéraires, témoins et éléments V1.3 lus,
/// jugés entiers, avec l'instantané de la date de leur recalage (nom calculé à la seconde).
#[test]
fn judgement_reads_v13_cases_whole() {
    let (_d, paths) = bench();
    let m = Manifest::load(&paths.bench).unwrap();
    m.held_out_increment("V1.3").unwrap();
    assert!(m.held_out_increment("V1.2").is_err(), "V1.2 déjà jugée");

    let sel = select_series(&m, &[], &[], Serie::TenueALEcart);
    assert_eq!(ids(sel.cases.iter().map(|t| t.id.as_str())), ["perso-198"]);
    assert!(extract_sources(&m, &sel, Serie::TenueALEcart).is_empty(), "aucun extrait de calibration");
    let ctx = v1_ctx(&paths, &m, &[], Serie::TenueALEcart).unwrap();
    assert_eq!(ids(ctx.planned.cases.iter().map(|c| c.0.id.as_str())), ["pl13-r-x", "pl13-s2-rhin"]);
    assert_eq!(ids(ctx.temoins.iter().map(|t| t.id.as_str())), ["perso-198-s0", "pl13-s2-rhin"]);
    let held = crate::ouvrages::series_ouvrages(&paths, &m, Serie::TenueALEcart).unwrap();
    assert_eq!(ids(held.iter().map(|o| o.id.as_str())), ["nv-1267"], "E3 et E5 sur les éléments neufs");
    for id in ["perso-198", "pl13-r-x", "pl13-s2-rhin"] {
        assert!(judged_whole(&m, Serie::TenueALEcart, id), "{id}");
    }
    assert!(!judged_whole(&m, Serie::TenueALEcart, "perso-166"));
    // Instantané V1.3 attendu sous son nom à la seconde, d'empreinte fausse ici : refusé pour ses
    // cas ; celui de V1.2 lisible pour les siens.
    assert_eq!(crate::reference_error::snapshot_file(V13_DATE), "private/osm-tags/2026-11-04T03-16-54Z.json");
    let e = SnapshotCache::default().for_case(&paths, &m, "perso-198").unwrap_err();
    assert!(e.to_string().contains("empreinte"), "{e}");
    assert_eq!(SnapshotCache::default().for_case(&paths, &m, "perso-166").unwrap().date(), V12_DATE);
    let report = check_series(&paths, Serie::TenueALEcart).unwrap();
    for f in [
        "perso-198 : fichier absent",
        "pl13-r-x : fichier absent",
        "osm_tags_reference (V1.3) : empreinte différente",
    ] {
        assert!(report.failures.iter().any(|x| x == f), "{f} : {:?}", report.failures);
    }
}

/// Résultats d'un jugement V1.3 simulé sur les minimums de `criteria.yaml` : chaque critère à
/// minimum passe, sa couverture mesurée au minimum exact ; E2b par groupe.
fn simulated(mins: &Value) -> Value {
    let mut cov = serde_json::Map::new();
    let mut verdicts = serde_json::Map::new();
    for (crit, m) in mins.as_object().unwrap() {
        let mut c = serde_json::Map::new();
        for (k, v) in m.as_object().unwrap() {
            let Some(x) = v.as_f64() else { continue };
            if k == "traces_par_groupe" {
                c.insert(k.clone(), json!({"dplus": x, "plaine": x, "ouvrage": x}));
            } else {
                c.insert(k.clone(), json!(x));
            }
        }
        cov.insert(crit.clone(), Value::Object(c));
        verdicts.insert(crit.clone(), json!("pass"));
    }
    let mode = json!({
        "verdicts": verdicts,
        "couverture": cov,
        "agregats": {"E2b": {"dplus": {"verdict": "pass"}, "plaine": {"verdict": "pass"}, "ouvrage": {"verdict": "pass"}}},
    });
    let mut r = mode.clone();
    r["input_with_breakpoints"] = mode;
    r["campagne"] = json!({"date": "x"});
    r
}

/// Ce que `judge_with` fait des verdicts après la mesure, sur les règles du dépôt : groupes E2b,
/// minimums de jugement, puis entrée de `judgements.md`.
fn judged(bench: &Path, mut r: Value, blocking: &BTreeSet<String>) -> (Value, String) {
    let conditional = crate::judge::conditional_blocking(bench, "V1.3").unwrap();
    let mut mins = crate::coverage::minimums_for(bench, "V1.3").unwrap();
    for (crit, (_, detail)) in &conditional {
        if let Some((on, off)) = crate::coverage::blocking_groups(detail) {
            crate::coverage::apply_group_blocking(&mut r, &mut mins, crit, &on, &off);
        }
    }
    crate::coverage::apply_minimums(&mut r, &mins);
    let text = crate::judge::entry("V1.3", &crate::judge::Commits { code: "c", gel: "g" }, "x", &r, blocking);
    (r, text)
}

/// Règles et manifest du dépôt (criteria `serie_v13`, dc76a6e ; manifest `serie_v13.couverture`,
/// b34b23f) : E3 non bloquant (ponts à MNH ≥ 10 m : 1 pour 3 avant mesure ; `statut:
/// rapporte_non_bloquant` concordant), Q1 non bloquant, E2b bloquant dans ses trois groupes
/// (dplus 4, plaine 11, ouvrage 5 pour 3). Au jugement : un échec ou un sous-minimum mesuré d'E3
/// est rapporté sans refuser ; un sous-minimum mesuré ou un échec de tout groupe E2b refuse.
#[test]
fn repo_rules_for_v13_judgement() {
    let bench = BenchPaths::repo_bench();
    let blocking = crate::judge::blocking_criteria_for(&bench, "V1.3").unwrap();
    let all = crate::judge::blocking_criteria(&bench).unwrap();
    assert!(all.contains("E3") && all.contains("Q1") && all.contains("E2b"));
    assert!(!blocking.contains("E3") && !blocking.contains("Q1") && blocking.contains("E2b"), "{blocking:?}");
    assert_eq!(all.len(), blocking.len() + 2, "seuls E3 et Q1 sortent");
    // Le gel serait décidable : aucune erreur de couverture.
    let c = crate::judge::conditional_blocking(&bench, "V1.3").unwrap();
    assert_eq!(c.keys().map(String::as_str).collect::<Vec<_>>(), ["E2b", "E3"]);
    let (e3, d3) = &c["E3"];
    assert!(!e3, "{d3}");
    assert_eq!(d3["statut_manifest"], "rapporte_non_bloquant");
    assert_eq!(d3["comptes"]["ponts"], json!({"manifest": "ponts", "atteint": 26.0, "minimum": 12.0}));
    assert_eq!(d3["comptes"]["ponts_mnh_10m_min"]["atteint"], 1.0);
    assert_eq!(d3["comptes"]["ponts_mnh_10m_min"]["minimum"], 3.0);
    assert_eq!(d3["comptes"]["ponts_plus_de_150m"]["atteint"], 5.0);
    let (e2b, d2b) = &c["E2b"];
    assert!(e2b, "{d2b}");
    assert_eq!(d2b["minimum"], "traces_par_groupe");
    for (g, n) in [("dplus", 4.0), ("plaine", 11.0), ("ouvrage", 5.0)] {
        assert_eq!(d2b["groupes"][g], json!({"atteint": n, "minimum": 3.0, "bloquant": true}), "{g}");
    }
    let (on, off) = crate::coverage::blocking_groups(d2b).unwrap();
    assert_eq!((on.len(), off.len()), (3, 0));

    let mins = crate::coverage::minimums_for(&bench, "V1.3").unwrap();
    // Tout au minimum, E3 en échec et sous son minimum mesuré, Q1 sans négatif : accepté, E3 et
    // Q1 rapportés.
    let mut r = simulated(&mins);
    r["verdicts"]["E3"] = json!("fail");
    r["couverture"]["E3"]["ponts_mnh_10m_min"] = json!(1);
    r["input_with_breakpoints"]["couverture"]["E3"]["ponts_mnh_10m_min"] = json!(0);
    r["verdicts"]["Q1"] = json!("skipped");
    r["couverture"]["Q1"]["traces_negatives"] = json!(0);
    r["input_with_breakpoints"]["verdicts"]["Q1"] = json!("skipped");
    r["input_with_breakpoints"]["couverture"]["Q1"]["traces_negatives"] = json!(0);
    let (out, text) = judged(&bench, r, &blocking);
    assert_eq!(out["verdicts"]["E3"], "fail");
    assert_eq!(out["verdicts"]["E3_sous_minimum"], json!(["ponts_mnh_10m_min 1 < 3"]));
    assert_eq!(out["input_with_breakpoints"]["verdicts"]["E3"], "sous_minimum");
    assert_eq!(out["verdicts"]["E2b"], "pass");
    assert!(text.contains("**accepté**"), "{text}");
    assert!(text.contains("rapportés sans refuser : E3, Q1"), "{text}");
    assert!(text.contains("| E3 | non | fail | sous_minimum |"), "{text}");
    assert!(text.contains("| E2b | oui | pass | pass |"), "{text}");

    // Chaque groupe E2b est bloquant : un seul groupe sous son minimum mesuré (un mode suffit),
    // ou en échec, refuse.
    for g in ["dplus", "plaine", "ouvrage"] {
        let mut r = simulated(&mins);
        r["input_with_breakpoints"]["couverture"]["E2b"]["traces_par_groupe"][g] = json!(2);
        let (out, text) = judged(&bench, r, &blocking);
        assert_eq!(out["input_with_breakpoints"]["verdicts"]["E2b"], "sous_minimum", "{g}");
        assert_eq!(out["verdicts"]["E2b"], "pass", "{g}");
        assert!(
            text.contains("**refusé** (en échec, sautés ou sous leur minimum de couverture : E2b)"),
            "{g} : {text}"
        );

        let mut r = simulated(&mins);
        r["agregats"]["E2b"][g]["verdict"] = json!("fail");
        let (out, text) = judged(&bench, r, &blocking);
        assert_eq!(out["verdicts"]["E2b"], "fail", "{g}");
        assert_eq!(out["verdicts"]["E2b_groupes_en_echec"], json!([g]));
        assert_eq!(out["verdicts"]["E2b_groupes_rapportes"], json!([]));
        assert!(text.contains("**refusé**"), "{g} : {text}");
    }
    // Tout au minimum exact : accepté.
    let (_, text) = judged(&bench, simulated(&mins), &blocking);
    assert!(text.contains("**accepté**"), "{text}");
}

/// `statut` d'une couverture au manifest (b34b23f) contrôlé contre les comptes : contraire ou
/// inconnu = erreur, le gel et le jugement s'arrêtent ; groupe par groupe, non prévu.
#[test]
fn declared_coverage_status_is_checked_against_the_counts() {
    let dir = tempfile::tempdir().unwrap();
    let b = dir.path();
    write(
        &b.join("criteria.yaml"),
        "criteres:\n  - {id: E3, bloquant: true}\n  - {id: E2b, bloquant: true}\nserie_tenue_a_l_ecart:\n  couverture:\n    minimums:\n\
         \x20     E3: {ponts: 12, ponts_mnh_10m_min: 3, ponts_plus_de_150m: 3}\n\
         \x20     E2b: {traces_par_groupe: 3, groupes: [dplus, plaine, ouvrage]}\n\
         \x20 serie_v9:\n    E3: {bloquant: si_couverture_atteinte}\n",
    );
    let manifest = |e3: &str| {
        write(&b.join("manifest.yaml"), &format!("serie_v9:\n  couverture:\n    E3: {{{e3}}}\n"));
    };
    let below = "atteint: {ponts: 26, ponts_mnh_10m_min: 1, ponts_plus_de_150m: 5}";
    let reached = "atteint: {ponts: 26, ponts_mnh_10m_min: 3, ponts_plus_de_150m: 5}";
    manifest(&format!("{below}, statut: rapporte_non_bloquant"));
    assert!(!crate::judge::blocking_criteria_for(b, "V9").unwrap().contains("E3"));
    manifest(below);
    assert!(!crate::judge::blocking_criteria_for(b, "V9").unwrap().contains("E3"));
    manifest(reached);
    assert!(crate::judge::blocking_criteria_for(b, "V9").unwrap().contains("E3"));
    manifest(&format!("{reached}, statut: rapporte_non_bloquant"));
    let e = crate::judge::blocking_criteria_for(b, "V9").unwrap_err();
    assert!(e.to_string().contains("atteint le minimum"), "{e}");
    manifest(&format!("{below}, statut: autre"));
    assert!(crate::judge::blocking_criteria_for(b, "V9").is_err());
    write(
        &b.join("manifest.yaml"),
        "serie_v9:\n  couverture:\n    E2b: {atteint: {dplus: 4, plaine: 4, ouvrage: 4}, minimum: {par_groupe: 3}, statut: rapporte_non_bloquant}\n",
    );
    assert!(crate::coverage::coverage_before_measure_by_group(b, "V9", "E2b").is_err());
}
