//! `diagnose-structures` : groupes d'ouvrages d'un cas rejoués étape par étape (typologie, MNT,
//! lectures des ouvrages, fusion) sur le cache de banc, hors ligne. Sortie sur la console :
//! distances curvilignes, chaînages et altitudes seulement ; ni coordonnée ni nom. Explorations :
//! `ORD_DIAG_TRAVERS=1` (MNS en travers de l'axe), `ORD_DIAG_PROFIL=1` (profil sur `[de, a]`).

use std::sync::Arc;

use ord_core::elevation::fusion::plan::AxisPoint;
use ord_core::elevation::{fuse, EleMethod};
use ord_core::prepare::prepare;
use ord_domain::budget::{Deadline, RequestId};
use ord_domain::ctx::RequestCtx;
use ord_domain::elevation::DsmSample;
use ord_domain::request::RequestOptions;
use ord_domain::LatLon;
use ord_pipeline::elevation::{sample_raw, sample_structures};
use serde_json::{json, Value};

use crate::service::{Pt, Service, Variant};

/// Points d'un cas : `vc-testN` (GPX public), témoin vcyclist, trace perso de calibration (premier
/// tronçon), ou extrait `<trace>#extraitK` d'une trace tenue à l'écart, comme la campagne : fenêtre
/// de ± 500 m autour d'un ouvrage de calibration, ± 1 km envoyé au service. Pour un extrait, la
/// fenêtre évaluée (abscisses de l'extrait) est renvoyée : rien n'est affiché hors d'elle.
pub fn case_points(
    paths: &crate::BenchPaths,
    id: &str,
) -> anyhow::Result<(Vec<Pt>, Option<std::ops::Range<f64>>)> {
    if let Some(route) = id.strip_prefix("vc-") {
        let pts = crate::gpx::read(&paths.bench.join(format!("data/vcyclist/{route}.gpx")))?;
        return Ok((pts.iter().map(|p| Pt { lat: p.lat, lon: p.lon }).collect(), None));
    }
    if let Some((dir, _, _)) = crate::ouvrages::controls(&paths.bench)?.into_iter().find(|c| c.1 == id) {
        return Ok((crate::ouvrages::control_points(&paths.bench, &dir, id)?, None));
    }
    let manifest = crate::manifest::Manifest::load(&paths.bench)?;
    if let Some((case_id, k)) = id.split_once("#extrait") {
        let k: usize = k
            .parse()
            .map_err(|_| anyhow::anyhow!("{id} : extrait attendu sous la forme <trace>#extraitK"))?;
        let base = case_id.split(".s").next().unwrap_or(case_id);
        let trace = manifest.trace(base).ok_or_else(|| anyhow::anyhow!("{id} : cas inconnu"))?;
        if trace.serie != crate::manifest::Serie::TenueALEcart {
            anyhow::bail!("{id} : les extraits ne concernent que les traces tenues à l'écart");
        }
        let rules = crate::BenchRules::default();
        let cases =
            crate::campaign::load_perso(paths, trace, &rules).map_err(|e| anyhow::anyhow!("{id} : {e}"))?;
        let case = cases
            .into_iter()
            .find(|c| c.id == case_id)
            .ok_or_else(|| anyhow::anyhow!("{id} : cas absent"))?;
        let refs = crate::campaign::ouvrage_refs(paths)?;
        let mut extracts = crate::campaign::held_out_extracts(&case, &refs, &rules);
        let n = extracts.len();
        if k >= n {
            anyhow::bail!("{id} : {n} extrait(s) pour ce cas");
        }
        let (extract, window) = extracts.swap_remove(k);
        return Ok((extract.parts.into_iter().next().unwrap_or_default(), Some(window)));
    }
    let base = id.split(".s").next().unwrap_or(id);
    let trace = manifest.trace(base).ok_or_else(|| anyhow::anyhow!("{id} : cas inconnu"))?;
    if trace.serie == crate::manifest::Serie::TenueALEcart {
        anyhow::bail!(
            "{id} : série tenue à l'écart, jamais diagnostiquée entière (extraits : {id}#extraitK)"
        );
    }
    let cases = crate::campaign::load_perso(paths, trace, &crate::BenchRules::default())
        .map_err(|e| anyhow::anyhow!("{id} : {e}"))?;
    let case = cases.into_iter().find(|c| c.id == id).ok_or_else(|| anyhow::anyhow!("{id} : cas absent"))?;
    Ok((case.parts.into_iter().next().unwrap_or_default(), None))
}

fn r2(x: f64) -> Value {
    if x.is_finite() {
        json!((x * 100.0).round() / 100.0)
    } else {
        Value::Null
    }
}

/// Groupes d'ouvrages dont un passage touche `[from, to]`.
pub async fn diagnose_structures(
    paths: &crate::BenchPaths,
    id: &str,
    from: f64,
    to: f64,
) -> anyhow::Result<Value> {
    let (pts, window) = case_points(paths, id)?;
    // Extrait d'une trace tenue à l'écart : seule la fenêtre évaluée est lue et affichée.
    let (from, to) = match &window {
        Some(w) => (from.max(w.start), to.min(w.end)),
        None => (from, to),
    };
    let shown = |d: f64| d >= from && d <= to;
    let clip = |x: f64| x.clamp(from, to);
    let service = Service::start(&paths.cache, true, Variant::DEFAULT).await?;
    let orch = service.state().orchestrator();
    let sources = orch.sources();
    let rules = orch.rules().clone();
    let points: Vec<LatLon> = pts.iter().map(|p| LatLon::new(p.lat, p.lon)).collect();
    let route = Arc::new(prepare(&points, rules.elevation.station_step_m));
    let ctx = RequestCtx::new(RequestId("diag".into()), Deadline::after(std::time::Duration::from_secs(600)));
    let typo = sources.typology.run(Arc::clone(&route), &ctx).await?;
    let mut raw = sample_raw(&route, sources.terrain.as_ref(), &ctx).await?;
    sample_structures(&mut raw, &route, &typo, sources.terrain.as_ref(), &ctx, &rules).await?;
    let profile = fuse(&route, &raw, Some(&typo), &RequestOptions::default(), &rules)?;
    let s = raw.structures.as_ref().expect("lectures des ouvrages");
    let mut groups = Vec::new();
    for g in &s.plan.groups {
        let span = g.passages.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &k| {
            (lo.min(typo.passages[k].from_m), hi.max(typo.passages[k].to_m))
        });
        if span.1 < from || span.0 > to {
            continue;
        }
        let terrain = |a: AxisPoint| match a {
            AxisPoint::Route(i) => raw.terrain[i],
            AxisPoint::Off(k) => s.off_terrain[k],
        };
        let dsm = |a: AxisPoint| match a {
            AxisPoint::Route(i) => s.plan.route_dsm.binary_search(&i).ok().map(|k| s.route_dsm[k]),
            AxisPoint::Off(k) => s.off_dsm[k],
        };
        let in_window = |a: &AxisPoint| match a {
            AxisPoint::Route(i) => window.is_none() || shown(route.samples[*i].d_m),
            AxisPoint::Off(_) => true,
        };
        let stations: Vec<Value> = g
            .axis
            .iter()
            .filter(|a| in_window(&a.at))
            .map(|a| {
                let t = terrain(a.at);
                let mns = match dsm(a.at) {
                    Some(DsmSample::Value(v)) => r2(f64::from(v)),
                    Some(DsmSample::NoData) => json!("nodata"),
                    Some(DsmSample::Unavailable) => json!("echec"),
                    None => Value::Null,
                };
                let (d, out, m) = match a.at {
                    AxisPoint::Route(i) => (r2(route.samples[i].d_m), r2(profile.ele[i]), json!(format!("{:?}", profile.method[i]))),
                    AxisPoint::Off(_) => (Value::Null, Value::Null, Value::Null),
                };
                json!({"c": r2(a.c), "d": d, "mnt": r2(t.ele.map_or(f64::NAN, f64::from)), "origine": format!("{:?}", t.origin), "mns": mns, "sortie": out, "methode": m})
            })
            .collect();
        let probes: Vec<Value> = g
            .probes
            .iter()
            .map(|p| p.filter(|p| in_window(&p.at)).map_or(Value::Null, |p| json!({"c": r2(p.c), "mnt": r2(terrain(p.at).ele.map_or(f64::NAN, f64::from)), "mns": match dsm(p.at) { Some(DsmSample::Value(v)) => r2(f64::from(v)), _ => Value::Null }, "hors_parcours": matches!(p.at, AxisPoint::Off(_))})))
            .collect();
        groups.push(json!({
            "passages": g.passages.iter().map(|&k| {
                let p = &typo.passages[k];
                json!({"from_m": r2(clip(p.from_m)), "to_m": r2(clip(p.to_m)), "doutes": format!("{:?}", p.doubts)})
            }).collect::<Vec<_>>(),
            "lecture": format!("{:?}", g.mapping),
            "suites": g.runs.iter().map(|r| match g.mapping {
                ord_core::elevation::fusion::plan::Mapping::Route => json!([r2(clip(r.0)), r2(clip(r.1)), r.2.as_str()]),
                ord_core::elevation::fusion::plan::Mapping::Chainage => json!([r2(r.0), r2(r.1), r.2.as_str()]),
            }).collect::<Vec<_>>(),
            "non_corrige": g.uncorrected.map(|u| format!("{u:?}")),
            "sondes": probes,
            "stations": stations,
        }));
    }
    let outcomes: Vec<Value> = profile
        .structures
        .iter()
        .filter(|o| o.to_m >= from && o.from_m <= to)
        .map(|o| json!({"kind": o.kind.as_str(), "from_m": r2(clip(o.from_m)), "to_m": r2(clip(o.to_m)), "method": o.method.as_str()}))
        .collect();
    let segs = ord_core::typology::build_segments(
        &typo.runs,
        &typo.classes,
        &profile.verdicts,
        &route,
        rules.typology.min_segment_m,
    );
    let segments: Vec<Value> = segs
        .iter()
        .filter(|x| x.to_m >= from && x.from_m <= to)
        .map(|x| {
            let kind =
                x.class.and_then(|c| typo.classes.get(c as usize)).map(|c| c.attrs.highway.to_string());
            json!([r2(clip(x.from_m)), r2(clip(x.to_m)), x.structure.as_str(), kind, x.matched])
        })
        .collect();
    // Exploration : MNS en travers de l'axe (± 8 m tous les 1 m) aux stations d'axe sur le
    // parcours, avec ORD_DIAG_TRAVERS=1.
    let mut travers: Vec<Value> = Vec::new();
    if std::env::var_os("ORD_DIAG_TRAVERS").is_some() {
        for g in &s.plan.groups {
            for (k, a) in g.axis.iter().enumerate() {
                let AxisPoint::Route(i) = a.at else { continue };
                let d = route.samples[i].d_m;
                if d < from || d > to || k % 2 != 0 {
                    continue;
                }
                let j = (i + 1).min(route.samples.len() - 1);
                let h = if j > i { i } else { i - 1 };
                let (p, q) = (route.samples[h].at, route.samples[j.max(h + 1)].at);
                let (dn, de) = (
                    (q.lat - p.lat).to_radians() * 6_371_008.8,
                    (q.lon - p.lon).to_radians() * 6_371_008.8 * p.lat.to_radians().cos(),
                );
                let norm = (dn * dn + de * de).sqrt().max(1e-9);
                let (nx, ny) = (-dn / norm, de / norm); // perpendiculaire (est, nord)
                let c = route.samples[i].at;
                let pts: Vec<LatLon> = (-8..=8)
                    .map(|o| {
                        let o = f64::from(o);
                        LatLon::new(
                            c.lat + (o * ny / 6_371_008.8).to_degrees(),
                            c.lon + (o * nx / (6_371_008.8 * c.lat.to_radians().cos())).to_degrees(),
                        )
                    })
                    .collect();
                let v: Vec<Value> = sources
                    .terrain
                    .sample_dsm(&pts, &ctx)
                    .await
                    .into_iter()
                    .map(|x| match x {
                        DsmSample::Value(v) => r2(f64::from(v)),
                        _ => Value::Null,
                    })
                    .collect();
                travers.push(json!({"d": r2(d), "sortie": r2(profile.ele[i]), "mns": v}));
            }
        }
    }
    // Exploration : profil complet (MNT brut, sortie, méthode) des stations du parcours dans
    // `[de, a]`, avec ORD_DIAG_PROFIL=1 (distances et altitudes seulement).
    let mut profil: Vec<Value> = Vec::new();
    if std::env::var_os("ORD_DIAG_PROFIL").is_some() {
        for (i, x) in route.samples.iter().enumerate() {
            if x.d_m < from || x.d_m > to {
                continue;
            }
            profil.push(json!([
                r2(x.d_m),
                r2(raw.terrain[i].ele.map_or(f64::NAN, f64::from)),
                r2(profile.ele[i]),
                format!("{:?}", profile.method[i])
            ]));
        }
    }
    let corrected = profile
        .method
        .iter()
        .zip(&route.samples)
        .filter(|(m, x)| {
            matches!(m, EleMethod::DsmDeck | EleMethod::InterpolatedStructure)
                && (window.is_none() || shown(x.d_m))
        })
        .count();
    let fenetre = window.as_ref().map(|w| json!([r2(w.start), r2(w.end)]));
    let out = json!({"id": id, "fenetre_extrait": fenetre, "profil": profil, "travers": travers, "segments": segments, "issues": outcomes, "groupes": groups, "stations_corrigees": corrected});
    crate::privacy::assert_report_clean(&out, &crate::privacy::private_names(paths))?;
    Ok(out)
}
