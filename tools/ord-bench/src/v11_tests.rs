//! Série tenue à l'écart V1.1 (manifest `perso_v11`, `serie_v11`) sur un banc synthétique, sans
//! réseau : aucun cas V1.1 lu en calibration (ni masque ni extrait), cas lus au jugement avec
//! l'instantané OSM de la date de leur recalage, instantané absent ou faux refusé.
//!
//! Les fichiers propres des cas V1.1 (traces, itinéraires, références) n'existent pas sur ce banc :
//! les ouvrir serait un échec (`fichier absent`) ; seuls les index et les instantanés existent.

use std::collections::BTreeMap;
use std::path::Path;

use ord_domain::LatLon;

use crate::campaign::{extract_sources, ouvrage_refs, select_series, v1_ctx, SnapshotCache};
use crate::check::check_series;
use crate::manifest::{Manifest, Serie};
use crate::BenchPaths;

const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const V1_DATE: &str = "2026-11-01T13:01Z";
const V11_DATE: &str = "2026-11-02T09:32Z";
/// Voie 11 dure dans l'instantané V1, voie 12 dure dans celui de V1.1.
const SNAP_V1: &str = r#"{"date": "2026-11-01T13:01:00Z", "tags": {"11": {"surface": "asphalt"}}}"#;
const SNAP_V11: &str = r#"{"date": "2026-11-02T09:32:00Z", "tags": {"12": {"surface": "asphalt"}}}"#;

/// Variante du banc synthétique.
#[derive(Clone, Copy, PartialEq)]
enum Snapshot {
    Ok,
    /// Empreinte du manifest différente du fichier.
    WrongSum,
    /// Fichier absent de `private/osm-tags/`.
    Missing,
    /// Aucune entrée `osm_tags_reference` dans `serie_v11`.
    NotInManifest,
}

fn write(path: &Path, text: &str) -> String {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    crate::hash::sha256_bytes(text.as_bytes())
}

/// Banc synthétique : MVP et V1 jugés (calibration), V1.1 tenue à l'écart. Renvoie le dossier et
/// ses chemins (`bench/`, `bench/private/`).
fn bench(snapshot: Snapshot, with_v11: bool) -> (tempfile::TempDir, BenchPaths) {
    let dir = tempfile::tempdir().unwrap();
    let b = dir.path().join("bench");
    let p = b.join("private");
    std::fs::create_dir_all(p.join("traces")).unwrap();
    write(&p.join("recales/journal.csv"), "id\n");
    write(&b.join("judgements.md"), "# J\n\n## Jugement MVP (x)\n\n## Jugement V1 (x)\n");
    write(&b.join("criteria.yaml"), "criteres:\n  - {id: E1, bloquant: true}\n");
    write(&b.join("README.md"), "## 3. M\n| **E1** | x |\n## 4. S\n| E1 | a |\n");
    let o1 = write(
        &p.join("v1/ouvrages-v1.json"),
        r#"[{"id": "nv-1", "serie": "tenue_a_l_ecart", "type": "bridge", "length_m": 100,
            "pts": {"A": [45.001, 5.0], "milieu": [45.0015, 5.0], "B": [45.002, 5.0]}}]"#,
    );
    let t1 = write(&p.join("v1/temoins-v1.json"), "[]");
    let s1 = write(
        &p.join("v1/series-v1.csv"),
        "id,type,groupe,serie,passages_masques\nnv-1,ouvrage,,tenue_a_l_ecart,\n",
    );
    let o11 = write(
        &p.join("v11/ouvrages-v11.json"),
        r#"[{"id": "nv-606", "serie": "tenue_a_l_ecart", "type": "bridge", "length_m": 100,
            "pts": {"A": [45.003, 5.0], "milieu": [45.0035, 5.0], "B": [45.004, 5.0]}}]"#,
    );
    let t11 = write(
        &p.join("v11/temoins-v11.json"),
        r#"[{"id": "perso-130-s0", "trace": "perso-130", "planifie": false, "nature": "digue",
             "serie": "tenue_a_l_ecart", "debut": [46.0, 5.0], "fin": [46.01, 5.0]},
            {"id": "pl11-s2-y", "trace": "pl11-s2-y", "planifie": true, "nature": "digue",
             "serie": "tenue_a_l_ecart"}]"#,
    );
    let s11 = write(
        &p.join("v11/series-v11.csv"),
        "id,type,groupe,serie,passages_masques,fenetres_calibration\n\
         perso-130,trace,plaine,tenue_a_l_ecart,,nv-1\npl11-r-x,planifie,R1_R2,tenue_a_l_ecart,,\n\
         pl11-s2-y,planifie,S2,tenue_a_l_ecart,,\nnv-606,ouvrage,,tenue_a_l_ecart,,\n\
         perso-130-s0,temoin,digue,tenue_a_l_ecart,,\npl11-s2-y,temoin,digue,tenue_a_l_ecart,,\n",
    );
    let snap1 = write(&p.join("osm-tags/2026-11-01T13-01Z.json"), SNAP_V1);
    let snap11 = if snapshot == Snapshot::Missing {
        crate::hash::sha256_bytes(SNAP_V11.as_bytes())
    } else {
        write(&p.join("osm-tags/2026-11-02T09-32Z.json"), SNAP_V11)
    };
    let snap11 = if snapshot == Snapshot::WrongSum { ZERO.to_owned() } else { snap11 };
    let osm11 = if snapshot == Snapshot::NotInManifest {
        String::new()
    } else {
        format!(
            "  osm_tags_reference: {{fichier: private/osm-tags/2026-11-02T09-32Z.json, date: \"2026-11-02T09:32:00Z\", sha256: {snap11}}}\n"
        )
    };
    let mut manifest = format!(
        "version: 1\nvcyclist: []\nosm_surface: []\nouvrages: {{serie_tenue_a_l_ecart: []}}\n\
         perso:\n  traces:\n    - {{id: perso-01, groupe: plaine, serie: calibration, fichier: private/traces/perso-01.gpx}}\n\
         perso_v1:\n  recalage: {{donnees_serveur: \"{V1_DATE}\"}}\n\
         \x20 ouvrages_v1: {{fichier: private/v1/ouvrages-v1.json, sha256: {o1}}}\n\
         \x20 temoins_v1: {{fichier: private/v1/temoins-v1.json, sha256: {t1}}}\n\
         \x20 series_v1: {{fichier: private/v1/series-v1.csv, sha256: {s1}}}\n\
         \x20 osm_tags_reference: {{fichier: private/osm-tags/2026-11-01T13-01Z.json, sha256: {snap1}, date: \"2026-11-01T13:01:00Z\"}}\n\
         \x20 traces:\n    - {{id: perso-40, groupe: plaine, serie: tenue_a_l_ecart, passages_masques: [nv-9], fichier: private/traces/perso-40.gpx}}\n\
         serie_v11:\n  recalage: {{service: Valhalla, donnees_serveur: \"{V11_DATE}\"}}\n{osm11}"
    );
    if with_v11 {
        manifest.push_str(&format!(
            "perso_v11:\n\
             \x20 ouvrages_v11: {{fichier: private/v11/ouvrages-v11.json, sha256: {o11}}}\n\
             \x20 temoins_v11: {{fichier: private/v11/temoins-v11.json, sha256: {t11}}}\n\
             \x20 series_v11: {{fichier: private/v11/series-v11.csv, sha256: {s11}}}\n\
             \x20 planifies:\n\
             \x20   - {{id: pl11-r-x, role: R1_R2, fichier: private/v11/planifies/pl11-r-x.gpx, sha256: {ZERO}, \
                     reference: private/v11/r12/reference/pl11-r-x.reference.json, sha256_reference: {ZERO}, \
                     sha256_surface: {ZERO}, sha256_baro: {ZERO}}}\n\
             \x20   - {{id: pl11-s2-y, role: S2, nature: digue, fichier: private/v11/planifies/pl11-s2-y.gpx, sha256: {ZERO}}}\n\
             \x20 traces:\n\
             \x20   - {{id: perso-130, groupe: plaine, serie: tenue_a_l_ecart, fenetres_calibration: 1, \
                     fichier: private/traces/perso-130.gpx, sha256: {ZERO}, sha256_trim1km: {ZERO}}}\n\
             \x20   - {{id: neg-08, groupe: negatif, serie: tenue_a_l_ecart, fichier: private/traces/neg-08.gpx, \
                     sha256: {ZERO}, qualification: {{verdict: suspect, gain_baro_m: 100, gain_mnt_m: 400}}}}\n"
        ));
    }
    write(&b.join("manifest.yaml"), &manifest);
    let paths = BenchPaths::new(&b, Some(&dir.path().join("cache")), Some(&p));
    (dir, paths)
}

fn ids<T: AsRef<str>>(v: impl IntoIterator<Item = T>) -> Vec<String> {
    v.into_iter().map(|x| x.as_ref().to_owned()).collect()
}

/// Calibration : aucun cas V1.1 lu, ni trace, ni itinéraire, ni extrait, ni témoin ; ses
/// éléments (index) masquent leurs fenêtres sur les traces de calibration ; l'instantané V1.1
/// n'est pas ouvert.
#[test]
fn calibration_reads_no_v11_case() {
    let (_d, paths) = bench(Snapshot::WrongSum, true);
    let m = Manifest::load(&paths.bench).unwrap();
    assert!(m.v1.as_ref().unwrap().judged, "V1 jugée");
    let v11 = m.v11.as_ref().expect("section perso_v11 lue");
    assert!(!v11.judged);
    // Le jugement du MVP ne rend pas les traces V1.1 à la calibration.
    assert_eq!(m.trace("perso-130").unwrap().serie, Serie::TenueALEcart);
    assert_eq!(m.trace("perso-40").unwrap().serie, Serie::Calibration);
    assert!(m.is_new_series("perso-130") && m.is_new_series("pl11-r-x") && !m.is_new_series("perso-01"));

    let sel = select_series(&m, &[], &[], Serie::Calibration);
    assert_eq!(ids(sel.cases.iter().map(|t| t.id.as_str())), ["perso-01", "perso-40"]);
    assert_eq!(sel.excluded_held_out, ["perso-130"]);
    // Extraits : jamais d'une trace V1.1 en calibration, même avec des passages masqués.
    let mut m2 = m.clone();
    m2.traces.iter_mut().find(|t| t.id == "perso-130").unwrap().passages_masques = vec!["nv-1".into()];
    assert!(extract_sources(&m2, &select_series(&m2, &[], &[], Serie::Calibration), Serie::Calibration)
        .is_empty());

    let ctx = v1_ctx(&paths, &m, &[], Serie::Calibration).unwrap();
    assert!(ctx.planned.cases.is_empty());
    assert_eq!(ctx.planned.excluded, ["pl11-r-x", "pl11-s2-y"]);
    assert!(ctx.temoins.is_empty(), "aucun témoin V1.1 en calibration");

    // Masques : l'élément V1.1 sur une trace de calibration (index seulement) ; l'élément V1 jugé
    // n'en est plus un.
    let refs = ouvrage_refs(&paths).unwrap();
    assert!(refs.contains_key("nv-606") && refs.contains_key("perso-130-s0"));
    let track: Vec<LatLon> = (0..60).map(|k| LatLon::new(45.0 + k as f64 * 0.0001, 5.0)).collect();
    assert_eq!(
        crate::v1::other_series_on(&track, &refs, &ctx.rows, Serie::Calibration, "perso-01"),
        ["nv-606"]
    );

    // Ouvrages mesurés : nv-1 (V1 jugée) en calibration, nv-606 jamais.
    let cal = crate::ouvrages::series_ouvrages(&paths, &m, Serie::Calibration).unwrap();
    assert_eq!(ids(cal.iter().map(|o| o.id.as_str())), ["nv-1"]);

    // `check` de la calibration : fichiers des cas V1.1 absents et instantané V1.1 faux, jamais
    // ouverts, donc aucun échec ; index V1.1 vérifiés.
    let report = check_series(&paths, Serie::Calibration).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert!(report.skipped.iter().any(|s| s.contains("série tenue à l'écart")), "{:?}", report.skipped);
}

/// Jugement V1.1 : traces, itinéraires et témoins V1.1 lus ; chaque cas R prend l'instantané de la
/// date de son propre recalage.
#[test]
fn judgement_reads_v11_cases_with_their_snapshot() {
    let (_d, paths) = bench(Snapshot::Ok, true);
    let m = Manifest::load(&paths.bench).unwrap();
    m.held_out_increment("V1.1").unwrap();
    assert!(m.held_out_increment("V1").is_err(), "V1 déjà jugée");

    let sel = select_series(&m, &[], &[], Serie::TenueALEcart);
    assert_eq!(ids(sel.cases.iter().map(|t| t.id.as_str())), ["perso-130"]);
    let ctx = v1_ctx(&paths, &m, &[], Serie::TenueALEcart).unwrap();
    assert_eq!(ids(ctx.planned.cases.iter().map(|c| c.0.id.as_str())), ["pl11-r-x", "pl11-s2-y"]);
    assert_eq!(ids(ctx.temoins.iter().map(|t| t.id.as_str())), ["perso-130-s0", "pl11-s2-y"]);
    let held = crate::ouvrages::series_ouvrages(&paths, &m, Serie::TenueALEcart).unwrap();
    assert_eq!(ids(held.iter().map(|o| o.id.as_str())), ["nv-606"]);

    // Instantané par cas : V1.1 pour ses traces et itinéraires, V1 pour la calibration (MVP compris).
    let mut snaps = SnapshotCache::default();
    for (id, date) in
        [("perso-130", V11_DATE), ("pl11-r-x", V11_DATE), ("perso-40", V1_DATE), ("perso-01", V1_DATE)]
    {
        assert_eq!(snaps.for_case(&paths, &m, id).unwrap().date(), date, "{id}");
    }
    let csv = "s_debut_m,s_fin_m,surface,use,way_id\n0,100,gravel,road,11\n100,300,gravel,road,12\n";
    let cut = |id: &str| {
        let mut s = SnapshotCache::default();
        crate::reference_error::error_ranges(csv, s.for_case(&paths, &m, id).unwrap())
    };
    assert_eq!(cut("perso-130"), vec![100.0..300.0]);
    assert_eq!(cut("perso-40"), vec![0.0..100.0]);

    // `check` du jugement : les fichiers des cas V1.1 sont ouverts (absents ici : échecs).
    let report = check_series(&paths, Serie::TenueALEcart).unwrap();
    for id in ["perso-130 : fichier absent", "pl11-r-x : fichier absent", "pl11-s2-y : fichier absent"] {
        assert!(report.failures.iter().any(|f| f == id), "{id} : {:?}", report.failures);
    }
    assert!(!report.failures.iter().any(|f| f.contains("osm_tags")), "{:?}", report.failures);
    // Q1 de V1.1 : négatif nouveau au défaut attesté, compté dans la série tenue à l'écart.
    assert_eq!(report.q1_tenue.negatives, 1);
}

/// Instantané V1.1 faux, absent du disque ou du manifest : refus au jugement, calibration intacte.
#[test]
fn v11_snapshot_absent_or_wrong_is_refused() {
    for (variant, expected) in [
        (Snapshot::WrongSum, "empreinte"),
        (Snapshot::Missing, "instantané absent"),
        (Snapshot::NotInManifest, "aucun instantané"),
    ] {
        let (_d, paths) = bench(variant, true);
        let m = Manifest::load(&paths.bench).unwrap();
        let e = SnapshotCache::default().for_case(&paths, &m, "perso-130").unwrap_err();
        assert!(e.to_string().contains(expected), "{expected} : {e}");
        assert!(SnapshotCache::default().for_case(&paths, &m, "pl11-r-x").is_err());
        // Calibration : son instantané reste lisible.
        assert_eq!(SnapshotCache::default().for_case(&paths, &m, "perso-40").unwrap().date(), V1_DATE);
        let cal = check_series(&paths, Serie::Calibration).unwrap();
        assert!(cal.failures.is_empty(), "{:?}", cal.failures);
        let held = check_series(&paths, Serie::TenueALEcart).unwrap();
        assert!(
            held.failures.iter().any(|f| f.starts_with("osm_tags_reference (V1.1)")),
            "{:?}",
            held.failures
        );
    }
}

/// Sans section `perso_v11` : rien de V1.1 n'est lu, et `gel` / `judge --increment V1.1` refusent.
#[test]
fn without_perso_v11_nothing_is_read_and_v11_cannot_be_judged() {
    let (_d, paths) = bench(Snapshot::Ok, false);
    let m = Manifest::load(&paths.bench).unwrap();
    assert!(m.v11.is_none());
    assert!(m.trace("perso-130").is_none());
    let e = m.held_out_increment("V1.1").unwrap_err();
    assert!(e.to_string().contains("absente du manifest"), "{e}");
    assert!(crate::judge::current_gel(&paths, "V1.1").is_err());
    let ctx = v1_ctx(&paths, &m, &[], Serie::TenueALEcart).unwrap();
    assert!(ctx.planned.cases.is_empty() && ctx.temoins.is_empty());
    let none: BTreeMap<String, Vec<LatLon>> = ouvrage_refs(&paths).unwrap();
    assert!(!none.contains_key("nv-606"));
}

/// Règles V1.1 du dépôt (criteria.yaml, 7b1a4e9) : E3 non bloquant sur V1.1 seulement.
#[test]
fn e3_is_reported_on_v11_only() {
    let bench = BenchPaths::repo_bench();
    let v1 = crate::judge::blocking_criteria_for(&bench, "V1").unwrap();
    let v11 = crate::judge::blocking_criteria_for(&bench, "V1.1").unwrap();
    assert!(v1.contains("E3") && !v11.contains("E3"));
    assert_eq!(v1.len(), v11.len() + 1);
    // Clés des index lues comme au manifest du dépôt (`temoins_v1_tags`, `recalage`).
    let m = Manifest::parse(&std::fs::read_to_string(bench.join("manifest.yaml")).unwrap()).unwrap();
    let v1 = m.v1.as_ref().unwrap();
    assert!(v1.temoins_tags.is_some() && v1.osm_tags.is_some() && v1.donnees_serveur.is_some());
}

/// Au jugement V1.1, ses traces et itinéraires sont jugés entiers (22dbba7) : aucun masque ;
/// en calibration, les autres cas gardent les masques des éléments V1.1.
#[test]
fn v11_cases_are_judged_whole() {
    let (_d, paths) = bench(Snapshot::Ok, true);
    let m = Manifest::load(&paths.bench).unwrap();
    use crate::campaign::judged_whole;
    assert!(judged_whole(&m, Serie::TenueALEcart, "perso-130"));
    assert!(judged_whole(&m, Serie::TenueALEcart, "pl11-s2-y"));
    assert!(!judged_whole(&m, Serie::Calibration, "perso-130"));
    assert!(!judged_whole(&m, Serie::Calibration, "perso-01"));
    assert!(m.trace("perso-130").unwrap().passages_masques.is_empty());
    // Aucun extrait de calibration au jugement V1.1 : traces de calibration sans passages masqués.
    let sel = select_series(&m, &[], &[], Serie::TenueALEcart);
    assert!(extract_sources(&m, &sel, Serie::TenueALEcart).is_empty());
}

/// Section des cas sous `serie_v11` (nom non confirmé) : lue de même.
#[test]
fn cases_under_serie_v11_are_read_too() {
    let (_d, paths) = bench(Snapshot::Ok, true);
    let path = paths.bench.join("manifest.yaml");
    let text = std::fs::read_to_string(&path).unwrap();
    // `perso_v11` fondue dans `serie_v11` (même niveau d'indentation).
    let (head, cases) = text.split_once("perso_v11:\n").unwrap();
    std::fs::write(&path, format!("{head}{cases}")).unwrap();
    let m = Manifest::load(&paths.bench).unwrap();
    let v11 = m.v11.as_ref().expect("section lue dans serie_v11");
    assert!(v11.traces.contains("perso-130"));
    assert_eq!(v11.donnees_serveur.as_deref(), Some(V11_DATE));
}
