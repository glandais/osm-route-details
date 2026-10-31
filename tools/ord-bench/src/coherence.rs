//! Test de cohérence du demi-pixel sur `test4` de vcyclist (README technique §3, ligne 1) :
//! RMS(service − `T`) sur le terrain ouvert. C'est une **cohérence** avec la vérité de vcyclist
//! (tirée du MNT IGN), pas une métrique du banc. vcyclist : 0,58 m aux centres, 2,63 m aux coins.

use std::path::Path;

use anyhow::{bail, Context};
use ord_data::elevation::PixelConvention;
use serde_json::json;

use crate::service::{Pt, Service};

/// Une requête ne dépasse jamais les bornes d'entrée (16 000 points).
const CHUNK: usize = 15_000;

/// Stations de la vérité : position, altitude `truth`, et appartenance au terrain ouvert
/// (`kind = T` hors de toute plage de `structures.csv`).
#[derive(Clone, Debug)]
pub struct Truth {
    pub points: Vec<Pt>,
    pub z: Vec<f64>,
    pub open: Vec<bool>,
}

pub fn load_truth(bench: &Path, route: &str) -> anyhow::Result<Truth> {
    let dir = bench.join("data/vcyclist/analysis/truth");
    let text = std::fs::read_to_string(dir.join(format!("{route}.csv"))).context("vérité vcyclist")?;
    let (mut points, mut z, mut open) = (Vec::new(), Vec::new(), Vec::new());
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.starts_with("s,")) {
        let f: Vec<&str> = line.split(',').collect();
        if f.len() < 6 {
            bail!("ligne de vérité invalide");
        }
        points.push(Pt { lat: f[1].parse()?, lon: f[2].parse()? });
        z.push(f[3].parse::<f64>().unwrap_or(f64::NAN));
        open.push(f[5].trim() == "T");
    }
    let structures = std::fs::read_to_string(dir.join("structures.csv")).context("structures.csv")?;
    for line in structures.lines().skip(1) {
        let f: Vec<&str> = line.split(',').collect();
        if f.len() > 3 && f[0] == route {
            let (i0, i1): (usize, usize) = (f[2].parse()?, f[3].parse()?);
            for flag in open.iter_mut().take(i1 + 1).skip(i0) {
                *flag = false;
            }
        }
    }
    Ok(Truth { points, z, open })
}

/// Altitude du service à chaque point, en mode `input` (une valeur par point d'entrée).
pub async fn sample_points(service: &Service, points: &[Pt]) -> anyhow::Result<Vec<f64>> {
    let mut out = vec![f64::NAN; points.len()];
    for (c, chunk) in points.chunks(CHUNK).enumerate() {
        let r = service
            .route_details(chunk, json!({"sampling": {"mode": "input"}, "sections": ["elevation"]}))
            .await?;
        for p in r["points"].as_array().context("points")? {
            let i = p["index"].as_u64().context("index")? as usize;
            out[c * CHUNK + i] = p["ele"].as_f64().unwrap_or(f64::NAN);
        }
    }
    Ok(out)
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Coherence {
    pub stations: usize,
    pub rms_center_m: f64,
    pub rms_corner_m: f64,
}

fn rms(truth: &Truth, ele: &[f64]) -> (f64, usize) {
    let e: Vec<f64> = (0..ele.len())
        .filter(|&i| truth.open[i] && !ele[i].is_nan() && !truth.z[i].is_nan())
        .map(|i| ele[i] - truth.z[i])
        .collect();
    ((e.iter().map(|x| x * x).sum::<f64>() / e.len().max(1) as f64).sqrt(), e.len())
}

/// Mesure les deux conventions sur le cache de banc (`offline`) ou en ligne (remplissage).
pub async fn measure(cache: &Path, bench: &Path, route: &str, offline: bool) -> anyhow::Result<Coherence> {
    let truth = load_truth(bench, route)?;
    let center = Service::new(cache, offline, PixelConvention::Center)?;
    let corner = Service::new(cache, offline, PixelConvention::Corner)?;
    let (rms_center_m, stations) = rms(&truth, &sample_points(&center, &truth.points).await?);
    let (rms_corner_m, _) = rms(&truth, &sample_points(&corner, &truth.points).await?);
    Ok(Coherence { stations, rms_center_m, rms_corner_m })
}
