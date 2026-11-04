//! Série tenue à l'écart V1.2 (manifest `perso_v12`, `serie_v12` ; README §4 quinquies) sur un
//! banc synthétique, sans réseau : MVP, V1 et V1.1 jugés (calibration, ouvrages `complement_v11`
//! compris) ; aucun cas V1.2 lu en calibration (ni masque inversé, ni extrait, ni témoin) ; cas
//! lus au jugement, entiers ; E3 bloquant si la couverture comptée avant toute mesure atteint son
//! minimum, Q1 rapporté.
//!
//! Les fichiers propres des cas V1.2 (traces, itinéraires, références) n'existent pas sur ce banc :
//! les ouvrir serait un échec (`fichier absent`) ; seuls les index et les instantanés existent.

use std::path::Path;

use ord_domain::LatLon;

use crate::campaign::{extract_sources, judged_whole, ouvrage_refs, select_series, v1_ctx, SnapshotCache};
use crate::check::check_series;
use crate::manifest::{Manifest, Serie};
use crate::BenchPaths;

const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const V1_DATE: &str = "2026-11-01T13:01Z";
const V11_DATE: &str = "2026-11-02T09:32Z";
const V12_DATE: &str = "2026-11-03T06:24Z";
const SNAP_V1: &str = r#"{"date": "2026-11-01T13:01:00Z", "tags": {}}"#;
const SNAP_V11: &str = r#"{"date": "2026-11-02T09:32:00Z", "tags": {}}"#;
const SNAP_V12: &str = r#"{"date": "2026-11-03T06:24:00Z", "tags": {"13": {"surface": "asphalt"}}}"#;

/// Couverture E3 de V1.2 au manifest (`serie_v12.couverture.E3`).
#[derive(Clone, Copy, PartialEq)]
enum Cov {
    /// 122 ponts, 19 à MNH ≥ 10 m, 20 de plus de 150 m : minimum atteint.
    Reached,
    /// Un sous-compte sous son minimum.
    Below,
    /// Aucune couverture au manifest.
    Absent,
    /// Minimum du manifest différent de celui de `criteria.yaml`.
    WrongMinimum,
}

fn write(path: &Path, text: &str) -> String {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    crate::hash::sha256_bytes(text.as_bytes())
}

const CRITERIA: &str = "criteres:\n  - {id: E1, bloquant: true}\n  - {id: E3, bloquant: true}\n  - {id: Q1, bloquant: true}\n\
serie_tenue_a_l_ecart:\n  couverture:\n    minimums:\n      E1: {traces_france_fiables: 8, km_compares: 800}\n\
\x20     E3: {ponts: 12, ponts_mnh_10m_min: 3, ponts_plus_de_150m: 3}\n      Q1: {traces_negatives: 2}\n\
\x20 serie_v11:\n    E3: {bloquant: false, seuils: inchanges, sous_minimum: rapporte}\n\
\x20 serie_v12:\n    Q1: {bloquant: false, sous_minimum: rapporte}\n\
\x20   E3: {bloquant: si_couverture_atteinte, couverture: comptee_avant_toute_mesure, seuils: inchanges, sous_minimum: rapporte}\n";

/// Banc synthétique : MVP, V1 et V1.1 jugés (calibration), V1.2 tenue à l'écart.
fn bench(cov: Cov, with_v12: bool) -> (tempfile::TempDir, BenchPaths) {
    let dir = tempfile::tempdir().unwrap();
    let b = dir.path().join("bench");
    let p = b.join("private");
    std::fs::create_dir_all(p.join("traces")).unwrap();
    write(&p.join("recales/journal.csv"), "id\n");
    write(
        &b.join("judgements.md"),
        "# J\n\n## Jugement MVP (x)\n\n## Jugement V1 (x)\n\n## Jugement V1.1 (x)\n",
    );
    write(&b.join("criteria.yaml"), CRITERIA);
    write(&b.join("README.md"), "## 3. M\n| **E1** | x |\n| **E3** | x |\n| **Q1** | x |\n## 4. S\n| E1 | a |\n| E3 | a |\n| Q1 | a |\n");
    let o1 = write(&p.join("v1/ouvrages-v1.json"), "[]");
    let t1 = write(&p.join("v1/temoins-v1.json"), "[]");
    let s1 = write(&p.join("v1/series-v1.csv"), "id,type,groupe,serie,passages_masques\n");
    // V1.1 : un ouvrage du verdict rendu (nv-606) et un du complément (nv-840), tous deux
    // `tenue_a_l_ecart` à l'index : calibration une fois V1.1 jugée.
    let o11 = write(
        &p.join("v11/ouvrages-v11.json"),
        r#"[{"id": "nv-606", "serie": "tenue_a_l_ecart", "type": "bridge", "length_m": 100,
            "pts": {"A": [45.003, 5.0], "milieu": [45.0035, 5.0], "B": [45.004, 5.0]}},
           {"id": "nv-840", "serie": "tenue_a_l_ecart", "complement_v11": true, "type": "bridge", "length_m": 100,
            "pts": {"A": [47.003, 5.0], "milieu": [47.0035, 5.0], "B": [47.004, 5.0]}}]"#,
    );
    let t11 = write(&p.join("v11/temoins-v11.json"), "[]");
    let s11 = write(
        &p.join("v11/series-v11.csv"),
        "id,type,groupe,serie,passages_masques,fenetres_calibration\n\
         perso-130,trace,plaine,tenue_a_l_ecart,,\nnv-606,ouvrage,,tenue_a_l_ecart,,\nnv-840,ouvrage,,tenue_a_l_ecart,,\n",
    );
    let o12 = write(
        &p.join("v12/ouvrages-v12.json"),
        r#"[{"id": "nv-948", "serie": "tenue_a_l_ecart", "type": "bridge", "length_m": 100,
            "pts": {"A": [45.001, 5.0], "milieu": [45.0015, 5.0], "B": [45.002, 5.0]}}]"#,
    );
    let t12 = write(
        &p.join("v12/temoins-v12.json"),
        r#"[{"id": "perso-166-s0", "trace": "perso-166", "planifie": false, "nature": "digue",
             "serie": "tenue_a_l_ecart", "debut": [46.0, 5.0], "fin": [46.01, 5.0]},
            {"id": "pl12-s2-y", "trace": "pl12-s2-y", "planifie": true, "nature": "couvert_arbore",
             "serie": "tenue_a_l_ecart"}]"#,
    );
    let tt12 = write(
        &p.join("v12/temoins-v12-tags.json"),
        r#"{"perso-166-s0": {"serie": "tenue_a_l_ecart", "plages": []}}"#,
    );
    let s12 = write(
        &p.join("v12/series-v12.csv"),
        "id,type,groupe,serie,passages_masques,fenetres_calibration\n\
         perso-166,trace,plaine,tenue_a_l_ecart,,nv-606\npl12-r-x,planifie,R1_R2,tenue_a_l_ecart,,\n\
         pl12-s2-y,planifie,S2,tenue_a_l_ecart,,\nnv-948,ouvrage,,tenue_a_l_ecart,,\n\
         perso-166-s0,temoin,digue,tenue_a_l_ecart,,\npl12-s2-y,temoin,couvert_arbore,tenue_a_l_ecart,,\n",
    );
    let snap1 = write(&p.join("osm-tags/2026-11-01T13-01Z.json"), SNAP_V1);
    let snap11 = write(&p.join("osm-tags/2026-11-02T09-32Z.json"), SNAP_V11);
    // Instantané V1.2 d'empreinte fausse : jamais ouvert en calibration.
    write(&p.join("osm-tags/2026-11-03T06-24Z.json"), SNAP_V12);
    let e3 = match cov {
        Cov::Reached => "    E3: {atteint: {ponts: 122, mnh_10m: 19, plus_de_150m: 20}, minimum: {ponts: 12, mnh_10m: 3, plus_de_150m: 3}}\n",
        Cov::Below => "    E3: {atteint: {ponts: 122, mnh_10m: 2, plus_de_150m: 20}, minimum: {ponts: 12, mnh_10m: 3, plus_de_150m: 3}}\n",
        Cov::Absent => "",
        Cov::WrongMinimum => "    E3: {atteint: {ponts: 122, mnh_10m: 19, plus_de_150m: 20}, minimum: {ponts: 10, mnh_10m: 3, plus_de_150m: 3}}\n",
    };
    let mut manifest = format!(
        "version: 1\nvcyclist: []\nosm_surface: []\nouvrages: {{serie_tenue_a_l_ecart: []}}\n\
         perso:\n  traces:\n    - {{id: perso-01, groupe: plaine, serie: calibration, fichier: private/traces/perso-01.gpx}}\n\
         perso_v1:\n  recalage: {{donnees_serveur: \"{V1_DATE}\"}}\n\
         \x20 ouvrages_v1: {{fichier: private/v1/ouvrages-v1.json, sha256: {o1}}}\n\
         \x20 temoins_v1: {{fichier: private/v1/temoins-v1.json, sha256: {t1}}}\n\
         \x20 series_v1: {{fichier: private/v1/series-v1.csv, sha256: {s1}}}\n\
         \x20 osm_tags_reference: {{fichier: private/osm-tags/2026-11-01T13-01Z.json, sha256: {snap1}, date: \"2026-11-01T13:01:00Z\"}}\n\
         \x20 traces:\n    - {{id: perso-40, groupe: plaine, serie: tenue_a_l_ecart, fichier: private/traces/perso-40.gpx}}\n\
         perso_v11:\n  recalage: {{donnees_serveur: \"{V11_DATE}\"}}\n\
         \x20 osm_tags_reference: {{fichier: private/osm-tags/2026-11-02T09-32Z.json, sha256: {snap11}, date: \"2026-11-02T09:32:00Z\"}}\n\
         \x20 ouvrages_v11: {{fichier: private/v11/ouvrages-v11.json, sha256: {o11}}}\n\
         \x20 temoins_v11: {{fichier: private/v11/temoins-v11.json, sha256: {t11}}}\n\
         \x20 series_v11: {{fichier: private/v11/series-v11.csv, sha256: {s11}}}\n\
         \x20 traces:\n    - {{id: perso-130, groupe: plaine, serie: tenue_a_l_ecart, fichier: private/traces/perso-130.gpx}}\n\
         serie_v12:\n  recalage: {{service: Valhalla, donnees_serveur: \"{V12_DATE}\"}}\n\
         \x20 osm_tags_reference: {{fichier: private/osm-tags/2026-11-03T06-24Z.json, date: \"2026-11-03T06:24:00Z\", sha256: {ZERO}}}\n\
         \x20 couverture:\n{e3}    Q1: {{atteint: {{traces: 0}}, minimum: {{traces: 2}}}}\n"
    );
    if with_v12 {
        manifest.push_str(&format!(
            "perso_v12:\n\
             \x20 ouvrages_v12: {{fichier: private/v12/ouvrages-v12.json, sha256: {o12}}}\n\
             \x20 temoins_v12: {{fichier: private/v12/temoins-v12.json, sha256: {t12}}}\n\
             \x20 temoins_v12_tags: {{fichier: private/v12/temoins-v12-tags.json, sha256: {tt12}}}\n\
             \x20 series_v12: {{fichier: private/v12/series-v12.csv, sha256: {s12}}}\n\
             \x20 planifies:\n\
             \x20   - {{id: pl12-r-x, role: R1_R2, fichier: private/v12/planifies/pl12-r-x.gpx, sha256: {ZERO}, \
                     reference: private/v12/r12/reference/pl12-r-x.reference.json, sha256_reference: {ZERO}, \
                     sha256_surface: {ZERO}, sha256_baro: {ZERO}}}\n\
             \x20   - {{id: pl12-s2-y, role: S2, nature: couvert_arbore, fichier: private/v12/planifies/pl12-s2-y.gpx, sha256: {ZERO}}}\n\
             \x20 traces:\n\
             \x20   - {{id: perso-166, groupe: plaine, serie: tenue_a_l_ecart, fenetres_calibration: 1, \
                     fichier: private/traces/perso-166.gpx, sha256: {ZERO}, sha256_trim1km: {ZERO}}}\n"
        ));
    }
    write(&b.join("manifest.yaml"), &manifest);
    let paths = BenchPaths::new(&b, Some(&dir.path().join("cache")), Some(&p));
    (dir, paths)
}

fn ids<T: AsRef<str>>(v: impl IntoIterator<Item = T>) -> Vec<String> {
    v.into_iter().map(|x| x.as_ref().to_owned()).collect()
}

/// Calibration : MVP, V1 et V1.1 (complément compris) en calibration ; aucun cas V1.2 lu, ni
/// trace, ni itinéraire, ni extrait, ni témoin ; ses éléments (index) masquent leurs fenêtres sur
/// les cas de calibration ; l'instantané V1.2 (faux ici) n'est pas ouvert.
#[test]
fn calibration_reads_no_v12_case() {
    let (_d, paths) = bench(Cov::Reached, true);
    let m = Manifest::load(&paths.bench).unwrap();
    assert!(m.v1.as_ref().unwrap().judged && m.section("V1.1").unwrap().judged, "V1 et V1.1 jugées");
    let v12 = m.section("V1.2").expect("section perso_v12 lue");
    assert!(!v12.judged);
    assert_eq!(v12.tuiles, None);
    assert_eq!(m.trace("perso-166").unwrap().serie, Serie::TenueALEcart);
    assert_eq!(m.trace("perso-130").unwrap().serie, Serie::Calibration);
    assert!(m.is_new_series("perso-166") && m.is_new_series("pl12-r-x") && m.is_new_series("pl12-s2-y"));

    let sel = select_series(&m, &[], &[], Serie::Calibration);
    assert_eq!(ids(sel.cases.iter().map(|t| t.id.as_str())), ["perso-01", "perso-40", "perso-130"]);
    assert_eq!(sel.excluded_held_out, ["perso-166"]);
    let mut m2 = m.clone();
    m2.traces.iter_mut().find(|t| t.id == "perso-166").unwrap().passages_masques = vec!["nv-606".into()];
    assert!(extract_sources(&m2, &select_series(&m2, &[], &[], Serie::Calibration), Serie::Calibration)
        .is_empty());

    let ctx = v1_ctx(&paths, &m, &[], Serie::Calibration).unwrap();
    assert!(ctx.planned.cases.is_empty());
    assert_eq!(ctx.planned.excluded, ["pl12-r-x", "pl12-s2-y"]);
    assert!(ctx.temoins.is_empty(), "aucun témoin V1.2 en calibration");

    // Masques : l'élément V1.2 sur un cas de calibration ; les éléments V1.1 jugés n'en sont plus.
    let refs = ouvrage_refs(&paths).unwrap();
    assert!(refs.contains_key("nv-948") && refs.contains_key("perso-166-s0"));
    let track: Vec<LatLon> = (0..60).map(|k| LatLon::new(45.0 + k as f64 * 0.0001, 5.0)).collect();
    assert_eq!(
        crate::v1::other_series_on(&track, &refs, &ctx.rows, Serie::Calibration, "perso-01"),
        ["nv-948"]
    );
    assert!(!judged_whole(&m, Serie::Calibration, "perso-01"));

    // Ouvrages mesurés en calibration : V1.1 entière (nv-606 et le complément nv-840), jamais nv-948.
    let cal = crate::ouvrages::series_ouvrages(&paths, &m, Serie::Calibration).unwrap();
    assert_eq!(ids(cal.iter().map(|o| o.id.as_str())), ["nv-606", "nv-840"]);

    // `check` de la calibration : index V1.2 vérifiés, fichiers de ses cas et instantané jamais ouverts.
    let report = check_series(&paths, Serie::Calibration).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert!(report.skipped.iter().any(|s| s.contains("série tenue à l'écart")), "{:?}", report.skipped);
    // Index V1.2 modifié : refus dès la calibration.
    std::fs::write(paths.private.join("v12/series-v12.csv"), "id\n").unwrap();
    let report = check_series(&paths, Serie::Calibration).unwrap();
    assert!(
        report.failures.iter().any(|f| f == "series_v12 : empreinte différente"),
        "{:?}",
        report.failures
    );
}

/// Jugement V1.2 : seule série non jugée ; traces, itinéraires, témoins et éléments V1.2 lus,
/// jugés entiers, avec l'instantané de la date de leur recalage.
#[test]
fn judgement_reads_v12_cases_whole() {
    let (_d, paths) = bench(Cov::Reached, true);
    let m = Manifest::load(&paths.bench).unwrap();
    m.held_out_increment("V1.2").unwrap();
    assert!(m.held_out_increment("V1.1").is_err(), "V1.1 déjà jugée");

    let sel = select_series(&m, &[], &[], Serie::TenueALEcart);
    assert_eq!(ids(sel.cases.iter().map(|t| t.id.as_str())), ["perso-166"]);
    assert!(extract_sources(&m, &sel, Serie::TenueALEcart).is_empty(), "aucun extrait de calibration");
    let ctx = v1_ctx(&paths, &m, &[], Serie::TenueALEcart).unwrap();
    assert_eq!(ids(ctx.planned.cases.iter().map(|c| c.0.id.as_str())), ["pl12-r-x", "pl12-s2-y"]);
    assert_eq!(ids(ctx.temoins.iter().map(|t| t.id.as_str())), ["perso-166-s0", "pl12-s2-y"]);
    let held = crate::ouvrages::series_ouvrages(&paths, &m, Serie::TenueALEcart).unwrap();
    assert_eq!(ids(held.iter().map(|o| o.id.as_str())), ["nv-948"], "E3 et E5 sur les éléments neufs");
    for id in ["perso-166", "pl12-r-x", "pl12-s2-y"] {
        assert!(judged_whole(&m, Serie::TenueALEcart, id), "{id}");
    }
    assert!(!judged_whole(&m, Serie::TenueALEcart, "perso-130"));
    // Instantané V1.2 d'empreinte fausse : refusé pour ses cas ; V1.1 et V1 lisibles pour les leurs.
    let e = SnapshotCache::default().for_case(&paths, &m, "perso-166").unwrap_err();
    assert!(e.to_string().contains("empreinte"), "{e}");
    assert_eq!(SnapshotCache::default().for_case(&paths, &m, "perso-130").unwrap().date(), V11_DATE);
    assert_eq!(SnapshotCache::default().for_case(&paths, &m, "perso-01").unwrap().date(), V1_DATE);
    let report = check_series(&paths, Serie::TenueALEcart).unwrap();
    for f in [
        "perso-166 : fichier absent",
        "pl12-r-x : fichier absent",
        "osm_tags_reference (V1.2) : empreinte différente",
    ] {
        assert!(report.failures.iter().any(|x| x == f), "{f} : {:?}", report.failures);
    }
}

/// Sans section `perso_v12` : rien de V1.2 n'est lu, et `gel` / `judge --increment V1.2` refusent.
#[test]
fn without_perso_v12_nothing_is_read() {
    let (_d, paths) = bench(Cov::Reached, false);
    let m = Manifest::load(&paths.bench).unwrap();
    assert!(m.section("V1.2").is_none() && m.trace("perso-166").is_none());
    assert!(m.held_out_increment("V1.2").unwrap_err().to_string().contains("absente du manifest"));
    assert!(crate::judge::current_gel(&paths, "V1.2").is_err());
    assert!(!ouvrage_refs(&paths).unwrap().contains_key("nv-948"));
}

/// E3 sur V1.2 (README §4 quinquies.2) : bloquant si la couverture comptée avant toute mesure
/// atteint le minimum, sinon rapporté ; Q1 rapporté (§4 quinquies.1) ; indécidable = refus.
#[test]
fn e3_blocks_on_v12_only_if_coverage_reached() {
    use crate::judge::{blocking_criteria_for, conditional_blocking};
    let (_d, paths) = bench(Cov::Reached, true);
    let b = blocking_criteria_for(&paths.bench, "V1.2").unwrap();
    assert!(b.contains("E3") && b.contains("E1") && !b.contains("Q1"), "{b:?}");
    let c = conditional_blocking(&paths.bench, "V1.2").unwrap();
    let (reached, detail) = &c["E3"];
    assert!(*reached);
    assert_eq!(detail["comptes"]["ponts_mnh_10m_min"]["manifest"], "mnh_10m");
    assert_eq!(detail["comptes"]["ponts_plus_de_150m"]["atteint"], 20.0);
    // Gel décidable : le seul refus vient de l'absence des fichiers des cas, pas d'E3.
    let v11 = blocking_criteria_for(&paths.bench, "V1.1").unwrap();
    assert!(!v11.contains("E3") && v11.contains("Q1"));

    let (_d, paths) = bench(Cov::Below, true);
    let b = blocking_criteria_for(&paths.bench, "V1.2").unwrap();
    assert!(!b.contains("E3") && !b.contains("Q1") && b.contains("E1"), "{b:?}");

    for cov in [Cov::Absent, Cov::WrongMinimum] {
        let (_d, paths) = bench(cov, true);
        assert!(blocking_criteria_for(&paths.bench, "V1.2").is_err());
    }
}

/// Règles et manifest du dépôt : E3 bloquant sur V1.2 (couverture du 2026-11-03 atteinte), Q1
/// non bloquant ; la section `perso_v12` est lue avec sa campagne de tuiles.
#[test]
fn repo_rules_for_v12() {
    let bench = BenchPaths::repo_bench();
    let v1 = crate::judge::blocking_criteria_for(&bench, "V1").unwrap();
    let v12 = crate::judge::blocking_criteria_for(&bench, "V1.2").unwrap();
    assert!(v12.contains("E3") && !v12.contains("Q1"), "{v12:?}");
    assert_eq!(v1.len(), v12.len() + 1);
    let m = Manifest::parse(&std::fs::read_to_string(bench.join("manifest.yaml")).unwrap()).unwrap();
    let v12 = m.section("V1.2").expect("perso_v12");
    assert_eq!(v12.tuiles.as_deref(), Some("tuiles_2026_11_03"));
    assert_eq!(m.tile_campaign(v12).map(|c| c.id.as_str()), Some("tuiles_2026_11_03"));
    assert_eq!(m.tile_campaign(m.section("V1.1").unwrap()).map(|c| c.id.as_str()), Some("tuiles_2026_11_02"));
    assert!(v12.temoins_tags.is_some() && v12.osm_tags.is_some() && v12.donnees_serveur.is_some());
    assert_eq!(v12.osm_tags_toutes.len(), 2);
    assert!(v12.traces.contains("perso-166") && v12.traces.contains("perso-197"));
    assert!(v12.planifies.iter().all(|p| p.id.starts_with("pl12-")));
}
