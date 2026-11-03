//! Parcours d'osm-surface (`os-*`, bench §1.2) : R1, R2 et M1 contre les classes Stadia/Valhalla
//! de tribly (`*.reference.json`), et latence de la typologie à chaud. Ces parcours sont publics
//! (géométrie OSM) ; les rapports n'en écrivent pourtant aucune coordonnée.
//!
//! Entrée : la ligne de la référence (`tracks[0].line`, celle qu'envoyait l'éval d'osm-surface),
//! dont la 4e coordonnée est l'abscisse des plages `tracks[0].surfaces`.

use std::path::Path;

use anyhow::Context;
use ord_domain::budget::{Deadline, RequestId};
use ord_domain::ctx::RequestCtx;
use ord_domain::LatLon;
use serde_json::{json, Value};

use crate::campaign::interp_one;
use crate::manifest::FileEntry;
use crate::service::{Pt, Service};
use crate::typology::{
    agreement, disagreements, metrics_json, ours_of, r_pass, unmatched_ranges, Class, Ref,
};

/// Référence d'un parcours : points envoyés, abscisse de chacun, plages de classe Stadia.
#[derive(Clone)]
pub struct OsRoute {
    pub id: String,
    pub points: Vec<Pt>,
    pub dist: Vec<f64>,
    pub reference: Vec<Ref>,
    /// Erreur de la référence hors décompte (README §4 bis.8), parcours planifiés seulement ; jamais
    /// les parcours d'osm-surface, dont la référence est la leur. `reference` vide : longueurs
    /// seulement.
    pub erreur_reference: Option<crate::reference_error::Exclusion>,
    /// Voies aux tags différents entre la date des tuiles et celle de la référence (README §4
    /// quater.1), parcours planifiés seulement : longueurs, et plages retirées (abscisse de
    /// l'itinéraire), que l'erreur de la référence ne voit plus.
    pub tags_differents: Option<(crate::reference_error::Exclusion, Vec<std::ops::Range<f64>>)>,
}

impl std::fmt::Debug for OsRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OsRoute").field("id", &self.id).field("n", &self.points.len()).finish_non_exhaustive()
    }
}

/// `data/osm-surface/<nom>.reference.json`, à côté du GPX du manifest.
pub fn load(bench: &Path, entry: &FileEntry) -> anyhow::Result<OsRoute> {
    let path = bench.join(entry.fichier.trim_end_matches(".gpx").to_owned() + ".reference.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).context("référence osm-surface")?)?;
    parse(&entry.id, &v)
}

/// Parcours planifié R1/R2 de la série V1 (README §4 ter.4) : référence Valhalla FOSSGIS au format
/// osm-surface (`tools/reference_r12_v1.py`, empreinte du manifest vérifiée) ; entrée = sa ligne
/// (l'itinéraire tel quel), rééchantillonnée à 25 m, abscisse de référence interpolée.
pub fn load_planned(paths: &crate::BenchPaths, p: &crate::manifest::Planned) -> anyhow::Result<OsRoute> {
    let (fichier, sum) =
        p.reference.as_ref().with_context(|| format!("{} : référence R1/R2 absente", p.id))?;
    let bytes =
        std::fs::read(paths.resolve(fichier)).with_context(|| format!("{} : référence absente", p.id))?;
    anyhow::ensure!(crate::hash::sha256_bytes(&bytes) == *sum, "{} : référence d'empreinte différente", p.id);
    let route = parse(&p.id, &serde_json::from_slice(&bytes).context("référence R1/R2 illisible")?)?;
    let pts: Vec<LatLon> = route.points.iter().map(|q| LatLon::new(q.lat, q.lon)).collect();
    let (pts, _, dist) = crate::v1::resample(&pts, &route.dist, crate::v1::PLANNED_STEP_M);
    Ok(OsRoute { points: pts.iter().map(|q| Pt { lat: q.lat, lon: q.lon }).collect(), dist, ..route })
}

/// `<id>.surface.csv` et `<id>.baro.csv` de la réponse Valhalla d'un parcours planifié R1/R2 (d'où
/// `tools/reference_r12_v1.py` a tiré la référence, à côté du dossier `reference/`), avec leurs
/// empreintes du manifest (`sha256_surface`, `sha256_baro`, 40c65cd).
pub fn planned_sources(
    paths: &crate::BenchPaths,
    p: &crate::manifest::Planned,
) -> anyhow::Result<[(std::path::PathBuf, Option<String>); 2]> {
    let (fichier, _) = p.reference.as_ref().with_context(|| format!("{} : référence R1/R2 absente", p.id))?;
    let reference = paths.resolve(fichier);
    let dir = reference
        .parent()
        .and_then(Path::parent)
        .with_context(|| format!("{} : dossier de la réponse Valhalla introuvable", p.id))?;
    Ok([
        (dir.join(format!("{}.surface.csv", p.id)), p.sha256_surface.clone()),
        (dir.join(format!("{}.baro.csv", p.id)), p.sha256_baro.clone()),
    ])
}

/// Erreur de la référence d'un parcours planifié R1/R2 (README §4 bis.8, 562d729) : plages de
/// `<id>.surface.csv` en erreur selon l'instantané `tags`, ramenées à l'abscisse de l'itinéraire
/// par `<id>.baro.csv`, contrôlées contre les plages non revêtues de la référence empreintée, puis
/// retirées de la référence. Les deux fichiers sont lus contre leur empreinte du manifest : absente
/// ou différente, refus.
pub fn exclude_reference_errors(
    paths: &crate::BenchPaths,
    p: &crate::manifest::Planned,
    route: &mut OsRoute,
    tags: &crate::reference_error::OsmTags,
) -> anyhow::Result<()> {
    use crate::reference_error::{check_on_unpaved, error_ranges, exclude, minus, to_origin};
    let (surface, baro) = read_sources(paths, p)?;
    let cut = to_origin(&error_ranges(&surface, tags), &baro)?;
    // Plages déjà retirées pour tags différents entre dates (README §4 quater.1) : plus vues ici.
    let cut = match &route.tags_differents {
        Some((_, done)) => minus(&cut, done),
        None => cut,
    };
    check_on_unpaved(&cut, &route.reference).with_context(|| p.id.clone())?;
    let x = exclude(std::mem::take(&mut route.reference), &cut);
    route.reference = x.reference.clone();
    route.erreur_reference = Some(crate::reference_error::Exclusion { reference: Vec::new(), ..x });
    Ok(())
}

/// `<id>.surface.csv` et `<id>.baro.csv` d'un parcours planifié, lus contre leur empreinte du
/// manifest : absente ou différente, refus.
fn read_sources(paths: &crate::BenchPaths, p: &crate::manifest::Planned) -> anyhow::Result<(String, String)> {
    let [surface, baro] = planned_sources(paths, p)?.map(|(path, sum)| {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bytes = std::fs::read(&path).with_context(|| format!("{name} absent"))?;
        let sum = sum.with_context(|| format!("{name} : empreinte absente du manifest"))?;
        anyhow::ensure!(
            crate::hash::sha256_bytes(&bytes) == sum,
            "{name} : empreinte différente de celle du manifest"
        );
        String::from_utf8(bytes).with_context(|| format!("{name} illisible"))
    });
    Ok((surface?, baro?))
}

/// Voies aux tags différents entre la date des tuiles et celle de la référence (README §4
/// quater.1, d6fd42c ; criteria `reference_R1_R2_v12.tags_differents`) : plages de
/// `<id>.surface.csv` (toutes classes) ramenées à l'itinéraire et retirées de la référence de R1 et
/// R2, avant l'erreur de la référence ; longueur et part rapportées à part.
pub fn exclude_tags_differents(
    paths: &crate::BenchPaths,
    p: &crate::manifest::Planned,
    route: &mut OsRoute,
    tags: &crate::reference_error::TagsBetweenDates,
) -> anyhow::Result<()> {
    use crate::reference_error::{differing_ranges, exclude, to_origin};
    let (surface, baro) = read_sources(paths, p)?;
    let cut = to_origin(&differing_ranges(&surface, tags).with_context(|| p.id.clone())?, &baro)?;
    let x = exclude(std::mem::take(&mut route.reference), &cut);
    route.reference = x.reference.clone();
    route.tags_differents = Some((crate::reference_error::Exclusion { reference: Vec::new(), ..x }, cut));
    Ok(())
}

fn parse(id: &str, v: &Value) -> anyhow::Result<OsRoute> {
    let track = &v["tracks"][0];
    let mut points = Vec::new();
    let mut dist = Vec::new();
    for c in track["line"]["coordinates"].as_array().context("ligne de référence")? {
        let (Some(lon), Some(lat), Some(d)) = (c[0].as_f64(), c[1].as_f64(), c[3].as_f64()) else { continue };
        points.push(Pt { lat, lon });
        dist.push(d);
    }
    let mut reference: Vec<Ref> = track["surfaces"]
        .as_array()
        .context("plages de référence")?
        .iter()
        .filter_map(|s| {
            Some(Ref {
                from: s["from"].as_f64()?,
                to: s["to"].as_f64()?,
                class: Class::of_tribly(s["surface"].as_str().unwrap_or("")),
            })
        })
        .collect();
    reference.sort_by(|a, b| a.from.total_cmp(&b.from));
    Ok(OsRoute { id: id.to_owned(), points, dist, reference, erreur_reference: None, tags_differents: None })
}

/// Distance cumulée du service (haversine, comme `prepare`).
fn cumulative(points: &[Pt]) -> Vec<f64> {
    let mut out = vec![0.0];
    for w in points.windows(2) {
        let d =
            ord_domain::geo::haversine_m(LatLon::new(w[0].lat, w[0].lon), LatLon::new(w[1].lat, w[1].lon));
        out.push(out.last().copied().unwrap_or(0.0) + d);
    }
    out
}

pub fn options() -> Value {
    json!({"sampling": {"mode": "input"}, "sections": ["typology", "summary"]})
}

/// R1, R2, M1 d'un parcours.
pub async fn measure(service: &Service, route: &OsRoute) -> anyhow::Result<Value> {
    let r = service.route_details(&route.points, options()).await?;
    let cum = cumulative(&route.points);
    let to_ref = |x: f64| interp_one(&cum, &route.dist, x);
    let to_d = |x: f64| interp_one(&route.dist, &cum, x);
    let ours = ours_of(std::slice::from_ref(&r), &[0.0], &to_ref);
    let a = agreement(&ours, &route.reference);
    let mut typo = metrics_json(&a, "R1");
    if let Some(x) = &route.erreur_reference {
        x.json_into(&mut typo, "R1");
    }
    if let Some((x, _)) = &route.tags_differents {
        x.tags_json_into(&mut typo, "R1");
    }
    let mut warnings: std::collections::BTreeMap<String, usize> = Default::default();
    for c in r["quality"]["warnings"].as_array().into_iter().flatten().filter_map(|w| w["code"].as_str()) {
        *warnings.entry(c.to_owned()).or_default() += 1;
    }
    let classed = |c: Option<Class>| -> f64 {
        route
            .reference
            .iter()
            .filter(|x| x.class != Class::Unknown && c.is_none_or(|c| x.class == c))
            .map(|x| x.to - x.from)
            .sum()
    };
    Ok(json!({
        "id": route.id,
        "serie": "calibration",
        "longueur_m": (cum.last().copied().unwrap_or(0.0) * 10.0).round() / 10.0,
        "reference_classee_m": (classed(None) * 10.0).round() / 10.0,
        "reference_non_revetu_m": (classed(Some(Class::Unpaved)) * 10.0).round() / 10.0,
        "groupe": "osm_surface",
        "mode": "input",
        "typologie": typo,
        "verdicts": {"R1_R2": if r_pass(&a) { "pass" } else { "fail" }, "M1": if crate::typology::m1_pass(&a) { "pass" } else { "fail" }},
        "diagnostics": {
            "status": r["status"],
            "segments": r["segments"].as_array().map_or(0, Vec::len),
            "matched_ratio": r["quality"]["matched_ratio"],
            "inferred_surface_ratio": r["quality"]["inferred_surface_ratio"],
            "by_surface_class_m": r["summary"]["by_surface_class_m"],
            "by_structure_m": r["summary"]["by_structure_m"],
            "avertissements": warnings,
            "typologie": {
                "M1.plages_non_appariees": unmatched_ranges(&ours, 10, &to_d),
                "R2.desaccords_revetu": disagreements(&ours, &route.reference, 10, &to_d)
            }
        }
    }))
}

/// Typologie à chaud (chaîne seule, tuiles en mémoire) : médiane de `trials` appels après un
/// appel de chauffe, en millisecondes. Mesure du test nommé « < 160 ms sur 180 km ».
pub async fn typology_warm_ms(cache: &Path, route: &OsRoute, trials: usize) -> anyhow::Result<(f64, Value)> {
    use ord_pipeline::typology::TypologyChain;
    let config = crate::service::bench_config(cache, true, crate::service::Variant::DEFAULT);
    let chain = ord_server::wiring::typology_chain(&config)?;
    let pts: Vec<LatLon> = route.points.iter().map(|p| LatLon::new(p.lat, p.lon)).collect();
    let prepared = std::sync::Arc::new(ord_core::prepare::prepare(
        &pts,
        ord_core::Rules::default().elevation.station_step_m,
    ));
    let ctx =
        || RequestCtx::new(RequestId("bench".into()), Deadline::after(std::time::Duration::from_secs(90)));
    let warm = chain.run(std::sync::Arc::clone(&prepared), &ctx()).await?;
    let mut times = Vec::new();
    for _ in 0..trials.max(1) {
        let start = std::time::Instant::now();
        chain.run(std::sync::Arc::clone(&prepared), &ctx()).await?;
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let stats = json!({
        "tuiles": warm.stats.tiles,
        "segments_graphe": warm.stats.segments,
        "observations": warm.stats.observations,
        "observations_appariees": warm.stats.matched_observations,
        "km": (prepared.length_m / 100.0).round() / 10.0
    });
    Ok((crate::metrics::median(&times), stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 562d729 (README §4 bis.8) : parcours planifié R1/R2, plage d'erreur de `surface.csv` ramenée
    /// à l'itinéraire et retirée de la référence ; voie absente de l'instantané : rien n'est retiré.
    #[test]
    fn planned_reference_errors_leave_the_reference() {
        let dir = tempfile::tempdir().unwrap();
        let private = dir.path().join("private");
        let r12 = private.join("v1/r12");
        std::fs::create_dir_all(r12.join("reference")).unwrap();
        std::fs::write(
            r12.join("pl-x.surface.csv"),
            "s_debut_m,s_fin_m,surface,road_class,use,way_id\n0,100,paved,tertiary,road,1\n100,200,dirt,unclassified,road,2\n200,300,dirt,track,road,3\n",
        )
        .unwrap();
        let surface_csv = std::fs::read(r12.join("pl-x.surface.csv")).unwrap();
        let baro_csv = "s_recale_m,s_origine_m,insere\n0,0,0\n300,330,0\n";
        std::fs::write(r12.join("pl-x.baro.csv"), baro_csv).unwrap();
        let reference = json!({"tracks": [{"line": {"coordinates": [[-1.5, 47.2, 0.0, 0.0], [-1.5, 47.203, 0.0, 330.0]]},
            "surfaces": [{"from": 0.0, "to": 110.0, "surface": "PAVED"}, {"from": 110.0, "to": 300.0, "surface": "DIRT"},
                {"from": 300.0, "to": 330.0, "surface": "UNKNOWN"}]}]});
        let text = reference.to_string();
        std::fs::write(r12.join("reference/pl-x.reference.json"), &text).unwrap();
        let paths = crate::BenchPaths::new(dir.path(), Some(dir.path()), Some(&private));
        let pl = crate::manifest::Planned {
            id: "pl-x".into(),
            role: "R1_R2".into(),
            reference: Some((
                "private/v1/r12/reference/pl-x.reference.json".into(),
                crate::hash::sha256_bytes(text.as_bytes()),
            )),
            sha256_surface: Some(crate::hash::sha256_bytes(&surface_csv)),
            sha256_baro: Some(crate::hash::sha256_bytes(baro_csv.as_bytes())),
            ..Default::default()
        };
        let snapshot = |w2: &str| {
            crate::reference_error::OsmTags::from_json(
                format!(r#"{{"date":"2026-11-01T13:01:00Z","tags":{{"2":{{"surface":"{w2}"}},"4":{{"surface":"asphalt"}}}}}}"#)
                    .as_bytes(),
                "2026-11-01T13:01Z",
            )
            .unwrap()
        };
        // Voie 2 en asphalte : 110–220 m de l'itinéraire hors de la référence.
        let mut route = parse("pl-x", &reference).unwrap();
        exclude_reference_errors(&paths, &pl, &mut route, &snapshot("asphalt")).unwrap();
        let x = route.erreur_reference.as_ref().unwrap();
        // Base de la part : référence classée, sans la plage UNKNOWN (40c65cd).
        assert!((x.excluded_m - 110.0).abs() < 1e-9 && x.reference_m == 300.0, "{x:?}");
        let kept: Vec<(f64, f64)> = route.reference.iter().map(|r| (r.from, r.to)).collect();
        assert_eq!(kept.len(), 3);
        assert!((kept[1].0 - 220.0).abs() < 1e-9);
        // Voie 2 aux tags différents entre dates (README §4 quater.1) : retirée avant l'erreur de la
        // référence, qui ne la voit plus ; rapportée à part.
        let mut route = parse("pl-x", &reference).unwrap();
        let toutes = |date: &str, grade: &str| {
            crate::reference_error::WayTags::from_json(
                format!(r#"{{"date":"{date}","voies_retenues":"toutes","demandees":[1,2,3],"tags":{{"2":{{"highway":"track","tracktype":"{grade}"}}}}}}"#)
                    .as_bytes(),
                date,
            )
            .unwrap()
        };
        let between = crate::reference_error::TagsBetweenDates::new(
            toutes("2026-10-19T00:00:04Z", "grade1"),
            toutes("2026-11-01T13:01:00Z", "grade2"),
        );
        exclude_tags_differents(&paths, &pl, &mut route, &between).unwrap();
        exclude_reference_errors(&paths, &pl, &mut route, &snapshot("asphalt")).unwrap();
        let (t, _) = route.tags_differents.as_ref().unwrap();
        assert!((t.excluded_m - 110.0).abs() < 1e-9 && t.reference_m == 300.0, "{t:?}");
        assert_eq!(route.erreur_reference.as_ref().unwrap().excluded_m, 0.0);
        let mut typo = json!({});
        t.tags_json_into(&mut typo, "R1");
        assert_eq!(typo["R1.tags_differents"], json!({"longueur_m": 110.0, "part": 0.3667}));
        // Voie 2 en `paved` (ambigu), voie 3 absente de l'instantané : rien n'est retiré.
        let mut route = parse("pl-x", &reference).unwrap();
        exclude_reference_errors(&paths, &pl, &mut route, &snapshot("paved")).unwrap();
        assert_eq!(route.erreur_reference.as_ref().unwrap().excluded_m, 0.0);
        assert_eq!(route.reference.len(), 3);
        // Empreinte fausse ou absente de surface.csv ou de baro.csv : refus.
        let wrong = Some("0".repeat(64));
        for bad in [
            crate::manifest::Planned { sha256_surface: wrong.clone(), ..pl.clone() },
            crate::manifest::Planned { sha256_baro: wrong, ..pl.clone() },
            crate::manifest::Planned { sha256_baro: None, ..pl.clone() },
        ] {
            let mut route = parse("pl-x", &reference).unwrap();
            let e = exclude_reference_errors(&paths, &bad, &mut route, &snapshot("asphalt")).unwrap_err();
            assert!(e.to_string().contains("empreinte"), "{e}");
        }
    }
}
