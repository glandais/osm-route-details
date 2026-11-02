//! Retard du baro, estimé par trace et corrigé (README §4 ter.9, criteria
//! `preparation.retard_baro`, décision de l'utilisateur du 2026-11-02). Fonctions pures.
//!
//! Le baromètre suit le relief avec retard. Un décalage d'abscisse δ par cas (trace entière,
//! tronçons `.partN` recousus ; chaque segment `.sN` à part) minimise l'écart robuste
//! (1,4826 × MAD) du résidu baro(s + δ) − MNT(s) sur la grille de 5 m, dérive lente retirée par
//! médiane glissante de 5 km ; recherche de −50 à +200 m par pas de 5 m, à écart égal le plus
//! petit |δ|. Le baro corrigé, baro(s + δ), sert aux références baro d'E1, E3 et E5 ; E2 et E2b
//! gardent le baro brut.

use serde_json::{json, Value};

use crate::metrics::{interpolate, median, rolling_median};

/// Bornes et pas de la recherche (criteria `retard_baro.estimation.recherche_m`, `pas_m`).
pub const RETARD_MIN_M: f64 = -50.0;
pub const RETARD_MAX_M: f64 = 200.0;
pub const RETARD_STEP_M: f64 = 5.0;
/// Stations utilisables minimales (2 km de grille) : en dessous, retard non estimable.
pub const RETARD_STATIONS_MIN: usize = 400;
/// Facteur de l'écart robuste (MAD → écart type d'une loi normale).
const MAD_K: f64 = 1.4826;

/// Résultat de l'estimation sur un cas.
#[derive(Clone, Debug, PartialEq)]
pub struct Retard {
    /// δ retenu (m) ; `None` : non estimable, baro brut.
    pub retard_m: Option<f64>,
    /// δ du minimum de l'écart, même à une borne (diagnostic).
    pub minimum_m: Option<f64>,
    /// Écart robuste du résidu à δ = 0 et au minimum.
    pub ecart_robuste_0_m: Option<f64>,
    pub ecart_robuste_min_m: Option<f64>,
    /// Stations utilisables à δ = 0.
    pub stations: usize,
    /// Raison de `retard_non_estimable`.
    pub raison: Option<&'static str>,
}

impl Retard {
    /// Retard non estimable sans calcul (extrait de trace tenue à l'écart, cas sans baro).
    pub fn not_estimated(raison: &'static str) -> Self {
        Self {
            retard_m: None,
            minimum_m: None,
            ecart_robuste_0_m: None,
            ecart_robuste_min_m: None,
            stations: 0,
            raison: Some(raison),
        }
    }

    /// Décalage appliqué au baro (0 si non estimable : baro brut).
    pub fn shift_m(&self) -> f64 {
        self.retard_m.unwrap_or(0.0)
    }

    /// Bloc rapporté par cas (sans coordonnée).
    pub fn to_json(&self) -> Value {
        let r = |x: Option<f64>| x.map_or(Value::Null, |v| json!((v * 1_000.0).round() / 1_000.0));
        let mut v = json!({
            "retard_m": self.retard_m,
            "minimum_m": self.minimum_m,
            "ecart_robuste_0_m": r(self.ecart_robuste_0_m),
            "ecart_robuste_min_m": r(self.ecart_robuste_min_m),
            "stations": self.stations,
        });
        if let Some(raison) = self.raison {
            v["mention"] = json!("retard_non_estimable");
            v["raison"] = json!(raison);
        }
        v
    }
}

/// Valeurs candidates de δ, par |δ| croissant (à écart égal, le premier rencontré l'emporte).
fn candidates() -> Vec<f64> {
    let n = ((RETARD_MAX_M - RETARD_MIN_M) / RETARD_STEP_M).round() as i64;
    let mut v: Vec<f64> = (0..=n).map(|k| RETARD_MIN_M + k as f64 * RETARD_STEP_M).collect();
    v.sort_by(|a, b| a.abs().total_cmp(&b.abs()).then(b.total_cmp(a)));
    v
}

/// Écart robuste (1,4826 × MAD) du résidu baro(s + δ) − MNT(s), dérive retirée ; et nombre de
/// stations où le résidu existe.
#[allow(clippy::too_many_arguments)]
fn robust_spread(
    d: &[f64],
    baro_s: &[f64],
    baro_z: &[f64],
    mnt: &[f64],
    usable: &[bool],
    delta: f64,
    gap_max: f64,
    window: f64,
    step: f64,
) -> (f64, usize) {
    let idx: Vec<usize> = (0..d.len()).filter(|&i| usable[i] && mnt[i].is_finite()).collect();
    let query: Vec<f64> = idx.iter().map(|&i| d[i] + delta).collect();
    let baro = interpolate(baro_s, baro_z, &query, gap_max);
    let (mut dd, mut rr) = (Vec::with_capacity(idx.len()), Vec::with_capacity(idx.len()));
    for (k, &i) in idx.iter().enumerate() {
        if baro[k].is_finite() {
            dd.push(d[i]);
            rr.push(baro[k] - mnt[i]);
        }
    }
    if rr.is_empty() {
        return (f64::NAN, 0);
    }
    let trend = rolling_median(&dd, &rr, window, step);
    let e: Vec<f64> = rr.iter().zip(&trend).map(|(r, t)| r - t).collect();
    let m = median(&e);
    let dev: Vec<f64> = e.iter().map(|x| (x - m).abs()).collect();
    (MAD_K * median(&dev), rr.len())
}

/// Estimation de δ sur un cas.
///
/// - `d` : grille du cas (abscisse du service, pas de 5 m) ;
/// - `baro_s`, `baro_z` : points de baro bruts gardés (points insérés écartés retirés), abscisses
///   croissantes sur la même échelle que `d` ;
/// - `mnt` : MNT lu au mode `distance`, sans correction (README §2.2) ;
/// - `usable` : station hors fenêtre masquée ;
/// - `gap_max` : pas brut maximal interpolé (au-delà, station sans baro) ;
/// - `window`, `step` : médiane glissante de la dérive lente.
#[allow(clippy::too_many_arguments)]
pub fn estimate(
    d: &[f64],
    baro_s: &[f64],
    baro_z: &[f64],
    mnt: &[f64],
    usable: &[bool],
    gap_max: f64,
    window: f64,
    step: f64,
) -> Retard {
    let spread = |delta: f64| robust_spread(d, baro_s, baro_z, mnt, usable, delta, gap_max, window, step);
    let (e0, stations) = spread(0.0);
    if stations < RETARD_STATIONS_MIN || !e0.is_finite() {
        return Retard {
            retard_m: None,
            minimum_m: None,
            ecart_robuste_0_m: e0.is_finite().then_some(e0),
            ecart_robuste_min_m: None,
            stations,
            raison: Some("stations_insuffisantes"),
        };
    }
    let mut best = (0.0, e0);
    for delta in candidates().into_iter().filter(|&x| x != 0.0) {
        let (e, _) = spread(delta);
        if e.is_finite() && e < best.1 {
            best = (delta, e);
        }
    }
    let at_bound = best.0 <= RETARD_MIN_M || best.0 >= RETARD_MAX_M;
    Retard {
        retard_m: (!at_bound).then_some(best.0),
        minimum_m: Some(best.0),
        ecart_robuste_0_m: Some(e0),
        ecart_robuste_min_m: Some(best.1),
        stations,
        raison: at_bound.then_some("minimum_a_une_borne"),
    }
}

/// Synthèse des retards des cas (agrégat du rapport) : estimés, non estimables (id et raison),
/// distribution de δ.
pub fn summary(cases: &[Value]) -> Value {
    let mut deltas = Vec::new();
    let mut not_estimable = Vec::new();
    for c in cases {
        let r = &c["retard_baro"];
        if r.is_null() {
            continue;
        }
        match r["retard_m"].as_f64() {
            Some(x) => deltas.push(x),
            None => not_estimable.push(json!({"id": c["id"], "raison": r["raison"]})),
        }
    }
    let mut sorted = deltas.clone();
    sorted.sort_by(f64::total_cmp);
    let mut hist: std::collections::BTreeMap<i64, usize> = std::collections::BTreeMap::new();
    for x in &deltas {
        *hist.entry(x.round() as i64).or_default() += 1;
    }
    json!({
        "estimes": deltas.len(),
        "non_estimables": not_estimable,
        "retard_m": {
            "min": sorted.first(),
            "mediane": (!sorted.is_empty()).then(|| median(&sorted)),
            "max": sorted.last(),
        },
        "repartition_m": hist.into_iter().map(|(k, n)| (k.to_string(), json!(n))).collect::<serde_json::Map<_, _>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Relief sinusoïdal (période 2 km, ± 40 m) sur 20 km ; baro = relief lu avec un retard de
    /// `lag` m (baro(s) = MNT(s − lag)), bruit déterministe, points tous les 25 m.
    fn profile(lag: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let relief = |s: f64| 200.0 + 40.0 * (s * std::f64::consts::TAU / 2_000.0).sin() + s * 0.002;
        let d: Vec<f64> = (0..=4_000).map(|k| k as f64 * 5.0).collect();
        let mnt: Vec<f64> = d.iter().map(|&s| relief(s)).collect();
        let bs: Vec<f64> = (0..=800).map(|k| k as f64 * 25.0).collect();
        let bz: Vec<f64> = bs
            .iter()
            .enumerate()
            .map(|(k, &s)| relief(s - lag) + 7.0 + 0.3 * ((k * 7919) % 13) as f64 / 13.0)
            .collect();
        (d, mnt, bs, bz)
    }

    #[test]
    fn the_lag_of_the_baro_is_found_on_the_grid() {
        for lag in [0.0, 40.0, 65.0, -10.0] {
            let (d, mnt, bs, bz) = profile(lag);
            let usable = vec![true; d.len()];
            let r = estimate(&d, &bs, &bz, &mnt, &usable, 100.0, 5_000.0, 50.0);
            assert_eq!(r.retard_m, Some(lag), "{r:?}");
            assert!(r.ecart_robuste_min_m.unwrap() <= r.ecart_robuste_0_m.unwrap());
            assert!(r.stations >= RETARD_STATIONS_MIN);
        }
    }

    #[test]
    fn a_minimum_at_a_bound_or_too_few_stations_is_not_estimable() {
        let (d, mnt, bs, bz) = profile(230.0);
        let usable = vec![true; d.len()];
        let r = estimate(&d, &bs, &bz, &mnt, &usable, 100.0, 5_000.0, 50.0);
        assert_eq!(r.retard_m, None);
        assert_eq!(r.minimum_m, Some(RETARD_MAX_M));
        assert_eq!(r.raison, Some("minimum_a_une_borne"));
        assert_eq!(r.shift_m(), 0.0);
        // Fenêtres masquées : 399 stations utilisables seulement.
        let (d, mnt, bs, bz) = profile(40.0);
        let usable: Vec<bool> = (0..d.len()).map(|i| i < 399).collect();
        let r = estimate(&d, &bs, &bz, &mnt, &usable, 100.0, 5_000.0, 50.0);
        assert_eq!((r.retard_m, r.raison, r.stations), (None, Some("stations_insuffisantes"), 399));
        assert_eq!(r.to_json()["mention"], "retard_non_estimable");
    }

    #[test]
    fn equal_spreads_keep_the_smallest_lag() {
        // Relief plat : tous les δ donnent le même écart, δ = 0 retenu.
        let d: Vec<f64> = (0..=1_000).map(|k| k as f64 * 5.0).collect();
        let mnt = vec![100.0; d.len()];
        let bs: Vec<f64> = (0..=200).map(|k| k as f64 * 25.0).collect();
        let bz = vec![103.0; bs.len()];
        let r = estimate(&d, &bs, &bz, &mnt, &vec![true; d.len()], 100.0, 5_000.0, 50.0);
        assert_eq!(r.retard_m, Some(0.0));
        assert_eq!(&candidates()[..3], &[0.0, 5.0, -5.0]);
    }
}
