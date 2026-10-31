//! Campagne de calibration (`run --offline`) et remplissage du cache de banc (`fill-cache`),
//! T05 §3.3, §5.1, §5.3. Incrément 1 : métriques E1 et E2 sur les traces personnelles (groupe
//! `hors_france` par défaut), mesurées et rapportées sans seuil.
//!
//! Ordre imposé : empreintes vérifiées, puis cas `tenue_a_l_ecart` exclus, **avant** toute lecture
//! de tuile. Aucune coordonnée n'est écrite : identifiants `perso-NN`, distances curvilignes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use ord_core::rules::Rules;
use ord_data::elevation::PixelConvention;
use ord_domain::geo::haversine_m;
use ord_domain::request::GainPreset;
use ord_domain::LatLon;
use serde_json::{json, Value};

use crate::check::{check, CheckReport};
use crate::hash::{cache_fingerprint, sha256_bytes};
use crate::manifest::{Manifest, Serie, Trace};
use crate::metrics::{dplus, e1, interpolate, median, passage_windows};
use crate::service::{Pt, Service};
use crate::{gpx, BenchPaths, BenchRules};

/// Sélection des cas : la série tenue à l'écart est exclue ici, avant toute lecture.
#[derive(Clone, Debug)]
pub struct Selection<'a> {
    pub cases: Vec<&'a Trace>,
    pub excluded_held_out: Vec<String>,
}

pub fn select<'a>(manifest: &'a Manifest, groupes: &[String], ids: &[String]) -> Selection<'a> {
    let wanted = |t: &Trace| {
        (ids.is_empty() || ids.contains(&t.id))
            && (groupes.is_empty() || groupes.contains(&t.groupe))
            && t.groupe != "negatif"
    };
    let mut cases = Vec::new();
    let mut excluded_held_out = Vec::new();
    for t in manifest.traces.iter().filter(|t| wanted(t)) {
        if t.serie == Serie::TenueALEcart {
            excluded_held_out.push(t.id.clone());
        } else {
            cases.push(t);
        }
    }
    Selection { cases, excluded_held_out }
}

/// Points de référence des ouvrages (publics, puis extension perso si présente).
pub fn ouvrage_refs(paths: &BenchPaths) -> anyhow::Result<BTreeMap<String, Vec<LatLon>>> {
    let mut out = BTreeMap::new();
    let mut files = vec![paths.bench.join("data/ouvrages-geom.json")];
    if paths.private_present() {
        files.push(paths.private.join("ouvrages-perso.json"));
    }
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let v: Value = serde_json::from_str(&text).context("géométrie des ouvrages")?;
        for o in v.as_array().into_iter().flatten() {
            let Some(id) = o["id"].as_str() else { continue };
            let pts: Vec<LatLon> = o["pts"]
                .as_object()
                .into_iter()
                .flat_map(|m| m.values())
                .filter_map(|p| Some(LatLon::new(p[0].as_f64()?, p[1].as_f64()?)))
                .collect();
            out.insert(id.to_owned(), pts);
        }
    }
    Ok(out)
}

/// Requête d'un cas personnel : géométrie recalée, abscisse de chaque point envoyé sur cette
/// géométrie (`s` de `baro.csv`) et altitude baro transportée.
pub struct PersoCase {
    pub id: String,
    pub groupe: String,
    pub serie: Serie,
    pub request: Vec<Pt>,
    /// Abscisse sur la géométrie recalée de chaque point envoyé.
    pub s_ref: Vec<f64>,
    pub baro_s: Vec<f64>,
    pub baro_z: Vec<f64>,
    pub masks: Vec<String>,
}

impl std::fmt::Debug for PersoCase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersoCase")
            .field("id", &self.id)
            .field("points", &self.request.len())
            .finish_non_exhaustive()
    }
}

fn cumulative(pts: &[Pt]) -> Vec<f64> {
    let mut out = Vec::with_capacity(pts.len());
    let mut s = 0.0;
    for (i, p) in pts.iter().enumerate() {
        if i > 0 {
            s += haversine_m(LatLon::new(pts[i - 1].lat, pts[i - 1].lon), LatLon::new(p.lat, p.lon));
        }
        out.push(s);
    }
    out
}

/// Charge un cas personnel ; `Err` = cas sauté (raison sans coordonnée).
pub fn load_perso(paths: &BenchPaths, trace: &Trace, rules: &BenchRules) -> Result<PersoCase, String> {
    let dir = paths.private.join("recales");
    let geom = dir.join(format!("{}.gpx", trace.id));
    if !geom.exists() {
        return Err(
            "géométrie recalée absente ou découpée (.sN, .partN : non pris en charge à l'incrément 1)".into(),
        );
    }
    let vertices: Vec<Pt> =
        gpx::read(&geom).map_err(|e| e.to_string())?.iter().map(|p| Pt { lat: p.lat, lon: p.lon }).collect();
    if vertices.len() < 2 {
        return Err("géométrie recalée vide".into());
    }
    let cum = cumulative(&vertices);
    // Au-delà des bornes d'entrée : rééchantillonnage à 10 m sur la géométrie recalée (T05 §3.4).
    let (request, s_ref) = if vertices.len() <= 16_000 {
        (vertices, cum)
    } else {
        let total = *cum.last().unwrap_or(&0.0);
        let mut s: Vec<f64> =
            (0..).map(|k| k as f64 * rules.request_resample_m).take_while(|x| *x < total).collect();
        s.push(total);
        let lat = interpolate(&cum, &vertices.iter().map(|p| p.lat).collect::<Vec<_>>(), &s, f64::INFINITY);
        let lon = interpolate(&cum, &vertices.iter().map(|p| p.lon).collect::<Vec<_>>(), &s, f64::INFINITY);
        (lat.iter().zip(&lon).map(|(&lat, &lon)| Pt { lat, lon }).collect(), s)
    };
    let baro_text = std::fs::read_to_string(dir.join(format!("{}.baro.csv", trace.id)))
        .map_err(|_| "baro.csv absent".to_owned())?;
    let mut rows: Vec<(f64, f64)> = Vec::new();
    for line in baro_text.lines().skip(1) {
        let f: Vec<&str> = line.split(',').collect();
        let (Some(s), Some(z)) =
            (f.first().and_then(|x| x.parse::<f64>().ok()), f.get(1).and_then(|x| x.parse::<f64>().ok()))
        else {
            continue;
        };
        let dist = f.get(3).and_then(|x| x.trim().parse::<f64>().ok());
        if dist.is_some_and(|d| d > rules.match_max_m) {
            continue;
        }
        rows.push((s, z));
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(PersoCase {
        id: trace.id.clone(),
        groupe: trace.groupe.clone(),
        serie: trace.serie,
        request,
        s_ref,
        baro_s: rows.iter().map(|r| r.0).collect(),
        baro_z: rows.iter().map(|r| r.1).collect(),
        masks: trace.passages_masques.clone(),
    })
}

fn distance_options(structure_correction: bool, step: f64) -> Value {
    json!({
        "sampling": {"mode": "distance", "step_m": step},
        "sections": ["elevation", "summary"],
        "structure_correction": structure_correction
    })
}

/// Profil `distance` d'une réponse : distances, positions, altitudes (`NaN` = absente).
struct Profile {
    d: Vec<f64>,
    pts: Vec<LatLon>,
    ele: Vec<f64>,
}

fn profile(r: &Value) -> anyhow::Result<Profile> {
    let points = r["points"].as_array().context("points")?;
    let mut p = Profile { d: Vec::new(), pts: Vec::new(), ele: Vec::new() };
    for x in points {
        p.d.push(x["d_m"].as_f64().context("d_m")?);
        p.pts.push(LatLon::new(x["lat"].as_f64().unwrap_or(f64::NAN), x["lon"].as_f64().unwrap_or(f64::NAN)));
        p.ele.push(x["ele"].as_f64().unwrap_or(f64::NAN));
    }
    Ok(p)
}

fn round(x: f64, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    (x * k).round() / k
}

/// Mesure E1 et E2 d'un cas (service appelé deux fois : corrections actives, puis MNT brut).
pub async fn measure_case(
    service: &Service,
    case: &PersoCase,
    refs: &BTreeMap<String, Vec<LatLon>>,
    rules: &BenchRules,
    rules_core: &Rules,
) -> anyhow::Result<(Value, Value)> {
    let r = service.route_details(&case.request, distance_options(true, rules.grid_step_m)).await?;
    let raw = service.route_details(&case.request, distance_options(false, rules.grid_step_m)).await?;
    let p = profile(&r)?;
    let p_raw = profile(&raw)?;
    if p_raw.d.len() != p.d.len() {
        bail!("profils de longueurs différentes");
    }
    // Abscisse recalée → distance du service (identiques sans rééchantillonnage).
    let d_srv = cumulative(&case.request);
    let baro_d = interpolate(&case.s_ref, &d_srv, &case.baro_s, f64::INFINITY);
    let (bd, bz): (Vec<f64>, Vec<f64>) =
        baro_d.iter().zip(&case.baro_z).filter(|(d, _)| !d.is_nan()).map(|(d, z)| (*d, *z)).unzip();
    let baro = interpolate(&bd, &bz, &p.d, rules.baro_gap_max_m);

    // Fenêtres masquées (ouvrages d'une autre série que la trace).
    let mut windows = Vec::new();
    let mut not_found = Vec::new();
    for id in &case.masks {
        let w = refs
            .get(id)
            .map(|pts| passage_windows(&p.d, &p.pts, pts, rules.passage_tol_m, rules.mask_half_m))
            .unwrap_or_default();
        if w.is_empty() {
            not_found.push(id.clone());
        }
        windows.extend(w);
    }
    let masked: Vec<bool> = p.d.iter().map(|d| windows.iter().any(|w| w.contains(d))).collect();
    let keep = |v: &[f64], i: usize| {
        if masked[i] || p.ele[i].is_nan() || baro[i].is_nan() {
            f64::NAN
        } else {
            v[i]
        }
    };
    let n = p.d.len();
    let svc: Vec<f64> = (0..n).map(|i| keep(&p.ele, i)).collect();
    let svc_raw: Vec<f64> = (0..n).map(|i| keep(&p_raw.ele, i)).collect();
    let bar: Vec<f64> = (0..n).map(|i| keep(&baro, i)).collect();

    let m1 = e1(&p.d, &svc, &bar, rules.detrend_window_m, rules.detrend_step_m);
    let g = &rules_core.elevation.gain;
    let (dplus_svc, dplus_raw, dplus_baro) = (
        dplus(&p.d, &svc, g.preset(GainPreset::Dem)),
        dplus(&p.d, &svc_raw, g.preset(GainPreset::Dem)),
        dplus(&p.d, &bar, g.preset(GainPreset::Barometric)),
    );
    let step = rules.grid_step_m;
    let masked_m = masked.iter().filter(|m| **m).count() as f64 * step;
    let compared = bar.iter().filter(|x| !x.is_nan()).count();
    let case_json = json!({
        "id": case.id,
        "serie": case.serie,
        "groupe": case.groupe,
        "mode": "distance",
        "cache": "disque",
        "metriques": {
            "E1.rmse_m": round(m1.rmse_m, 3),
            "E1.p95_m": round(m1.p95_m, 3),
            "E2.dplus_service_m": round(dplus_svc, 1),
            "E2.dplus_baro_m": round(dplus_baro, 1),
            "E2.ratio": round(dplus_svc / dplus_baro, 4),
            "E2.dplus_mnt_brut_m": round(dplus_raw, 1),
            "E2.ratio_mnt_brut": round(dplus_raw / dplus_baro, 4)
        },
        "diagnostics": {
            "E1.offset_baro_moins_service_m": round(m1.offset_m, 2),
            "E1.max_abs_m": round(m1.max_abs_m, 2),
            "E1.part_au_dela_de_10m": round(m1.share_over_10m, 4),
            "stations_comparees": compared,
            "stations": n,
            "part_comparee": round(compared as f64 / n.max(1) as f64, 4),
            "masque_m": masked_m,
            "passages_masques": case.masks,
            "passages_non_retrouves": not_found,
            "points_envoyes": case.request.len(),
            "summary_gain_m": r["summary"]["gain_m"],
            "summary_length_m": r["summary"]["length_m"],
            "status": r["status"],
            "terrain_m": r["summary"]["terrain_m"]
        }
    });
    Ok((case_json, r["meta"]["data_versions"].clone()))
}

/// Date UTC du jour (`AAAA-MM-JJ`), sans dépendance de calendrier.
pub fn today_utc() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn git_commit(repo: &Path) -> String {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let head = run(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "inconnu".into());
    let dirty = run(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    if dirty {
        format!("{head}-dirty")
    } else {
        head
    }
}

/// Emplacement de `results.json` prévu par T05 §5.2.
pub fn default_report_dir(paths: &BenchPaths) -> PathBuf {
    paths.bench.join("reports").join(format!("{}-{}", today_utc(), ord_core::RULES_VERSION))
}

pub fn ensure_check(report: &CheckReport) -> anyhow::Result<()> {
    if !report.failures.is_empty() {
        bail!("empreintes du banc non conformes : {}", report.failures.join(" ; "));
    }
    Ok(())
}

/// `run --offline` : campagne de calibration ; renvoie le contenu de `results.json`.
pub async fn run(paths: &BenchPaths, groupes: &[String], ids: &[String]) -> anyhow::Result<Value> {
    let report = check(paths)?;
    ensure_check(&report)?;
    let manifest = Manifest::load(&paths.bench)?;
    let selection = select(&manifest, groupes, ids);
    eprintln!(
        "cas retenus : {} ; exclus (série tenue à l'écart) avant toute lecture : {}",
        selection.cases.len(),
        selection.excluded_held_out.len()
    );
    let rules = BenchRules::default();
    let rules_core = Rules::default();
    let refs = ouvrage_refs(paths)?;
    let service = Service::new(&paths.cache, true, PixelConvention::Center)?;
    let mut cases = Vec::new();
    let mut skipped = Vec::new();
    let mut data_versions = Value::Null;
    for t in &selection.cases {
        if !paths.private_present() {
            skipped.push(json!({"id": t.id, "raison": "bench/private/ absent"}));
            continue;
        }
        let case = match load_perso(paths, t, &rules) {
            Ok(c) => c,
            Err(reason) => {
                skipped.push(json!({"id": t.id, "raison": reason}));
                continue;
            }
        };
        match measure_case(&service, &case, &refs, &rules, &rules_core).await {
            Ok((c, dv)) => {
                eprintln!("{} : mesuré", t.id);
                cases.push(c);
                if data_versions.is_null() {
                    data_versions = dv;
                }
            }
            Err(e) => skipped.push(json!({"id": t.id, "raison": e.to_string()})),
        }
    }
    // Par groupe : RMSE E1, P95 E1, rapport E2, rapport E2 du MNT brut.
    type Series = [Vec<f64>; 4];
    let mut by_group: BTreeMap<String, Series> = BTreeMap::new();
    for c in &cases {
        let g = by_group.entry(c["groupe"].as_str().unwrap_or("").to_owned()).or_default();
        let m = &c["metriques"];
        for (series, key) in g.iter_mut().zip(["E1.rmse_m", "E1.p95_m", "E2.ratio", "E2.ratio_mnt_brut"]) {
            series.push(m[key].as_f64().unwrap_or(f64::NAN));
        }
    }
    let mut e1_agg = serde_json::Map::new();
    let mut e2_agg = serde_json::Map::new();
    for (g, [rmse, p95, ratio, ratio_raw]) in &by_group {
        e1_agg.insert(format!("{g}.rmse_m_mediane"), json!(round(median(rmse), 3)));
        e1_agg.insert(format!("{g}.p95_m_mediane"), json!(round(median(p95), 3)));
        e2_agg.insert(format!("{g}.ratio_mediane"), json!(round(median(ratio), 4)));
        e2_agg.insert(format!("{g}.ratio_mnt_brut_mediane"), json!(round(median(ratio_raw), 4)));
    }
    let fingerprint = cache_fingerprint(&paths.cache).context("empreinte du cache de banc")?;
    let rules_fingerprint = sha256_bytes(serde_json::to_string(&rules_core)?.as_bytes());
    let bench_rules_sha256 = sha256_bytes(serde_json::to_string(&rules)?.as_bytes());
    let measured = if cases.is_empty() { "skipped" } else { "reported" };
    Ok(json!({
        "schema": 1,
        "campagne": {
            "date": today_utc(),
            "type": "calibration",
            "increment": "MVP",
            "increment_plan": 1,
            "commit": git_commit(&paths.bench),
            "api_version": ord_server::API_VERSION,
            "rules_version": ord_core::RULES_VERSION,
            "rules_fingerprint": rules_fingerprint,
            "data_fingerprint": fingerprint.sha256,
            "data_versions": data_versions,
            "manifest_sha256": report.manifest_sha256,
            "criteria_sha256": report.criteria_sha256,
            "bench_rules_sha256": bench_rules_sha256,
            "bench_rules": rules,
            "private_present": paths.private_present(),
            "groupes": groupes,
            "exclus_serie_tenue_a_l_ecart": selection.excluded_held_out
        },
        "cas": cases,
        "cas_sautes": skipped,
        "agregats": {"E1": e1_agg, "E2": e2_agg},
        "verdicts": {"E1": measured, "E2": measured, "Q1": if report.q1.pass() { "pass" } else { "fail" }},
        "diagnostics": {"cache": {"tuiles": fingerprint.tiles, "octets": fingerprint.bytes}}
    }))
}

/// `fill-cache` : mêmes requêtes que `run`, en ligne, plus la cohérence `test4` (deux conventions
/// de pixel) et des GPX supplémentaires (test de méthode). Écrit `cache.lock` dans le cache.
pub async fn fill_cache(
    paths: &BenchPaths,
    groupes: &[String],
    ids: &[String],
    coherence_routes: &[String],
    extra_gpx: &[PathBuf],
) -> anyhow::Result<Value> {
    let report = check(paths)?;
    ensure_check(&report)?;
    let manifest = Manifest::load(&paths.bench)?;
    let selection = select(&manifest, groupes, ids);
    let rules = BenchRules::default();
    let service = Service::new(&paths.cache, false, PixelConvention::Center)?;
    let mut filled = Vec::new();
    if paths.private_present() {
        for t in &selection.cases {
            let Ok(case) = load_perso(paths, t, &rules) else { continue };
            for sc in [true, false] {
                service.route_details(&case.request, distance_options(sc, rules.grid_step_m)).await?;
            }
            eprintln!("{} : tuiles en cache", t.id);
            filled.push(t.id.clone());
        }
    }
    for route in coherence_routes {
        crate::coherence::measure(&paths.cache, &paths.bench, route, false).await?;
        let gpx_path = paths.bench.join(format!("data/vcyclist/{route}.gpx"));
        let pts: Vec<Pt> = gpx::read(&gpx_path)?.iter().map(|p| Pt { lat: p.lat, lon: p.lon }).collect();
        service.route_details(&pts, distance_options(true, rules.grid_step_m)).await?;
        eprintln!("vc-{route} : tuiles en cache (cohérence, deux conventions)");
        filled.push(format!("vc-{route}"));
    }
    for path in extra_gpx {
        let pts: Vec<Pt> = gpx::read(path)?.iter().map(|p| Pt { lat: p.lat, lon: p.lon }).collect();
        crate::coherence::sample_points(&service, &pts).await?;
        eprintln!("GPX supplémentaire : tuiles en cache");
        filled.push("gpx".into());
    }
    let fingerprint = cache_fingerprint(&paths.cache)?;
    let lock = json!({
        "date": today_utc(),
        "data_fingerprint": fingerprint.sha256,
        "tuiles": fingerprint.tiles,
        "octets": fingerprint.bytes,
        "cas": filled,
        "exclus_serie_tenue_a_l_ecart": selection.excluded_held_out
    });
    std::fs::create_dir_all(&paths.cache)?;
    std::fs::write(paths.cache.join("cache.lock"), serde_json::to_string_pretty(&lock)? + "\n")?;
    Ok(lock)
}
