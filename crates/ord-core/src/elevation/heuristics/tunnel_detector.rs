//! Détecteur de tunnels routiers sur un MNT lu le long du parcours : portage fidèle de
//! `vc:G/TunnelDetector.kt` et de ses options (E4).
//!
//! Aucun MNT ne voit dans un tunnel : il y lit la montagne au-dessus. La route y suit à peu près
//! la droite entre les deux portails, que l'appelant réécrit. Pur et déterministe : tableaux en
//! entrée, portées en sortie ; station `k` à la distance `k × step_m`.
//!
//! Les comparaisons niées (`!(x >= seuil)`) sont voulues : un NaN y échoue comme en Kotlin.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use serde::Serialize;

/// Seuils du détecteur, sur la grille de `step_m` mètres (`vc:G/TunnelDetectorOptions.kt:19-94`).
/// Pentes en fractions ; chaque fenêtre doit tomber sur un nombre entier de stations.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TunnelDetectorOptions {
    /// Pas de la grille, en mètres.
    pub step_m: f64,
    /// Fenêtre courte de pente d'un portail, côté intérieur et côté extérieur.
    pub foot_window_m: f64,
    /// Fenêtre longue : côté route d'un portail et flancs raides de la bosse.
    pub steep_window_m: f64,
    /// Côté extérieur d'un portail : pente au plus ceci sur les deux fenêtres.
    pub max_road_grade: f64,
    /// Côté intérieur : monte (entrée) ou descend (sortie) d'au moins ceci sur `foot_window_m`.
    pub min_portal_grade: f64,
    /// Pente impossible des deux côtés : au moins ceci sur `steep_window_m`.
    pub min_side_grade: f64,
    /// Profondeur, à l'intérieur d'un portail, où `min_side_grade` est cherchée.
    pub side_search_m: f64,
    /// Pente de la corde entre les portails, au plus ceci.
    pub max_chord_grade: f64,
    /// Le terrain dépasse la corde d'au moins ceci, et déjà à `face_reach_m` de chaque portail.
    pub min_height_m: f64,
    /// Profondeur où le terrain doit déjà dépasser la corde de `min_height_m`.
    pub face_reach_m: f64,
    /// Le terrain ne passe jamais plus de ceci sous la corde.
    pub max_below_chord_m: f64,
    /// Une vallée intérieure reste au moins ceci au-dessus de la corde, sinon ce sont deux tunnels.
    pub min_valley_height_m: f64,
    /// Plus court tunnel, mètres entre portails.
    pub min_length_m: f64,
    /// Plus long tunnel, mètres entre portails.
    pub max_length_m: f64,
    /// Distance à vol d'oiseau entre portails sur longueur le long de la route, au moins ceci.
    pub min_straightness: f64,
    /// Plafond de paires de portails jugées (T04 §7.3 `max_tunnel_pairs`) : au-delà, aucune
    /// portée n'est rendue (déterministe). Absent du Kotlin, borne son coût O(E·X·L).
    pub max_pairs: usize,
}

impl Default for TunnelDetectorOptions {
    /// `TunnelDetectorOptions.DEFAULT` (`vc:G/TunnelDetectorOptions.kt:130`), plus `max_pairs`.
    fn default() -> Self {
        Self {
            step_m: 5.0,
            foot_window_m: 20.0,
            steep_window_m: 50.0,
            max_road_grade: 0.20,
            min_portal_grade: 0.25,
            min_side_grade: 0.30,
            side_search_m: 50.0,
            max_chord_grade: 0.12,
            min_height_m: 15.0,
            face_reach_m: 100.0,
            max_below_chord_m: 3.0,
            min_valley_height_m: 15.0,
            min_length_m: 50.0,
            max_length_m: 3000.0,
            min_straightness: 0.8,
            max_pairs: 200_000,
        }
    }
}

/// Fenêtres des options en stations (`vc:G/TunnelDetector.kt:145-152`).
#[derive(Clone, Copy, Debug)]
struct Stations {
    foot: usize,
    steep: usize,
    side: usize,
    face: usize,
}

/// Mètres en nombre entier de stations (`vc:G/TunnelDetectorOptions.kt:106-114`).
fn whole_stations(metres: f64, step_m: f64, name: &str) -> Result<i64, String> {
    let r = metres / step_m;
    let k = r.round();
    if (r - k).abs() < 1e-9 {
        Ok(k as i64)
    } else {
        Err(format!("{name} = {metres} m is not a whole number of {step_m} m stations"))
    }
}

impl TunnelDetectorOptions {
    /// Contrôles du bloc `init` Kotlin (`vc:G/TunnelDetectorOptions.kt:95-104`).
    pub fn validate(&self) -> Result<(), String> {
        self.stations().map(|_| ())
    }

    fn stations(&self) -> Result<Stations, String> {
        if !(self.step_m > 0.0) {
            return Err(format!("stepM must be positive, got {}", self.step_m));
        }
        let st = |m: f64, name: &str| whole_stations(m, self.step_m, name);
        let foot = st(self.foot_window_m, "footWindowM")?;
        let steep = st(self.steep_window_m, "steepWindowM")?;
        if !(foot >= 1 && steep >= 1) {
            return Err("grade windows must cover a station".into());
        }
        let side = st(self.side_search_m, "sideSearchM")?;
        if side < 0 {
            return Err("negative sideSearchM".into());
        }
        let face = st(self.face_reach_m, "faceReachM")?;
        if face < 1 {
            return Err("faceReachM must cover a station".into());
        }
        if !(self.max_road_grade >= 0.0 && self.min_portal_grade > 0.0 && self.min_side_grade > 0.0) {
            return Err("grades must be positive".into());
        }
        if !(self.max_chord_grade >= 0.0 && self.min_height_m > 0.0 && self.max_below_chord_m >= 0.0) {
            return Err("negative threshold".into());
        }
        if !(self.min_length_m >= 0.0 && self.min_length_m <= self.max_length_m) {
            return Err("minLengthM must be in 0..maxLengthM".into());
        }
        if !(0.0..=1.0).contains(&self.min_straightness) {
            return Err(format!("minStraightness must be in 0..1, got {}", self.min_straightness));
        }
        Ok(Stations { foot: foot as usize, steep: steep as usize, side: side as usize, face: face as usize })
    }
}

/// Tunnel accepté : la route suit la droite entre le terrain de ses deux portails
/// (`vc:G/TunnelDetector.kt:42-60`).
#[derive(Clone, Debug, PartialEq)]
pub struct TunnelSpan {
    /// Portail d'entrée, indice de station (inclus).
    pub first: usize,
    /// Portail de sortie, indice de station (inclus).
    pub last: usize,
    /// Pente de la corde entre les portails (fraction).
    pub chord_grade: f64,
    /// Hauteur maximale du terrain au-dessus de la corde, en mètres.
    pub height_m: f64,
    /// Plus forte montée sur `steep_window_m` juste après l'entrée (fraction).
    pub rise_grade: f64,
    /// Plus forte descente sur `steep_window_m` juste avant la sortie (fraction, négative).
    pub fall_grade: f64,
    /// Distance à vol d'oiseau entre portails sur `length_m`.
    pub straightness: f64,
    /// Pas de la grille du profil, en mètres.
    pub step_m: f64,
}

impl TunnelSpan {
    /// Distance entre portails le long de la route, en mètres.
    pub fn length_m(&self) -> f64 {
        (self.last - self.first) as f64 * self.step_m
    }

    /// Distance du portail d'entrée.
    pub fn start_m(&self) -> f64 {
        self.first as f64 * self.step_m
    }

    /// Distance du portail de sortie.
    pub fn end_m(&self) -> f64 {
        self.last as f64 * self.step_m
    }
}

/// Une lecture de terrain aussi proche de 0 m est un trou sans donnée (`vc:G/TunnelDetector.kt:113`).
const NO_DATA_ZERO_M: f64 = 1e-6;

/// Rayon de `vc:elevation/EarthConstants.MEAN_RADIUS`, gardé pour la parité de la rectitude.
const KOTLIN_MEAN_RADIUS_M: f64 = 6_371_000.0;

/// Haversine de `vc:elevation/Distance.kt:14-31`, à l'identique.
fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let d2r = std::f64::consts::PI / 180.0;
    let (p1, p2) = (lat1 * d2r, lat2 * d2r);
    let dlat = (lat2 - lat1) * d2r;
    let dlon = (lon2 - lon1) * d2r;
    let (sl, so) = ((dlat / 2.0).sin(), (dlon / 2.0).sin());
    let a = (sl * sl + p1.cos() * p2.cos() * so * so).clamp(0.0, 1.0);
    KOTLIN_MEAN_RADIUS_M * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())
}

/// Tunnels du profil, triés, sans chevauchement (`vc:G/TunnelDetector.kt:116-142`).
///
/// `terrain_m` : MNT à chaque station (NaN = pas de donnée) ; `lat_deg`/`lon_deg` : position de
/// chaque station. Options invalides, longueurs différentes, ou plus de `o.max_pairs` paires
/// jugées : aucune portée.
pub fn detect_tunnels(
    terrain_m: &[f64],
    lat_deg: &[f64],
    lon_deg: &[f64],
    o: &TunnelDetectorOptions,
) -> Vec<TunnelSpan> {
    let z = terrain_m;
    if z.len() < 2 || lat_deg.len() != z.len() || lon_deg.len() != z.len() {
        return Vec::new();
    }
    let Ok(st) = o.stations() else {
        return Vec::new();
    };
    let p = Profile { z, lat: lat_deg, lon: lon_deg };
    let entries = portals(z, o, true, &st);
    let exits = portals(z, o, false, &st);
    let mut out: Vec<TunnelSpan> = Vec::new();
    let mut next = 0usize;
    let mut pairs = 0usize;
    for &a in &entries {
        if a < next {
            continue;
        }
        let mut best: Option<TunnelSpan> = None;
        for &b in &exits[exits.partition_point(|&x| x <= a)..] {
            if (b - a) as f64 * o.step_m > o.max_length_m + 1e-9 {
                break;
            }
            pairs += 1;
            if pairs > o.max_pairs {
                return Vec::new();
            }
            let Some(span) = judge(&p, a, b, &entries, &exits, o, &st) else {
                continue;
            };
            if best.as_ref().is_none_or(|bs| span.height_m > bs.height_m + 1e-9) {
                best = Some(span);
            }
        }
        if let Some(bs) = best {
            next = bs.last;
            out.push(bs);
        }
    }
    out
}

struct Profile<'a> {
    z: &'a [f64],
    lat: &'a [f64],
    lon: &'a [f64],
}

/// Pente de la station `i` à `j` (`j > i`), NaN hors du profil (`vc:G/TunnelDetector.kt:155-160`).
fn grade(z: &[f64], i: isize, j: isize, step_m: f64) -> f64 {
    if i < 0 || j >= z.len() as isize {
        f64::NAN
    } else {
        (z[j as usize] - z[i as usize]) / ((j - i) as f64 * step_m)
    }
}

/// Portails d'entrée ou de sortie : route côté extérieur, front de taille côté intérieur ; parmi
/// ceux à moins de `foot_window_m`, le plus fort changement de pente, égalités gardées
/// (`vc:G/TunnelDetector.kt:166-195`).
fn portals(z: &[f64], o: &TunnelDetectorOptions, entry: bool, st: &Stations) -> Vec<usize> {
    let n = z.len();
    let (m, w) = (st.foot as isize, st.steep as isize);
    let step = o.step_m;
    let mut score = vec![f64::NEG_INFINITY; n];
    for (k, s) in score.iter_mut().enumerate() {
        let k = k as isize;
        let back = grade(z, k - m, k, step);
        let ahead = grade(z, k, k + m, step);
        if back.is_nan() || ahead.is_nan() {
            continue;
        }
        let ok = if entry {
            let outer = grade(z, k - w, k, step);
            back.abs() <= o.max_road_grade && outer.abs() <= o.max_road_grade && ahead >= o.min_portal_grade
        } else {
            let outer = grade(z, k, k + w, step);
            ahead.abs() <= o.max_road_grade && outer.abs() <= o.max_road_grade && back <= -o.min_portal_grade
        };
        if ok {
            *s = ahead - back;
        }
    }
    let m = st.foot;
    (0..n)
        .filter(|&k| {
            score[k] > f64::NEG_INFINITY
                && (k.saturating_sub(m)..=(k + m).min(n - 1)).all(|i| score[i] <= score[k])
        })
        .collect()
}

/// Juge la paire de portails `(a, b)` (`vc:G/TunnelDetector.kt:197-267`).
fn judge(
    p: &Profile<'_>,
    a: usize,
    b: usize,
    entries: &[usize],
    exits: &[usize],
    o: &TunnelDetectorOptions,
    st: &Stations,
) -> Option<TunnelSpan> {
    let z = p.z;
    let n = z.len();
    let w = st.steep;
    let step = o.step_m;
    let length = (b - a) as f64 * step;
    if length < o.min_length_m - 1e-9 {
        return None;
    }
    let chord = haversine(p.lat[a], p.lon[a], p.lat[b], p.lon[b]);
    let straightness = chord / length;
    if !(straightness >= o.min_straightness) {
        return None;
    }
    // Un côté route lu à 0 m exactement est un trou sans donnée, pas le niveau de la mer.
    if z[a.saturating_sub(w)..=a].iter().any(|v| v.abs() < NO_DATA_ZERO_M)
        || z[b..=(b + w).min(n - 1)].iter().any(|v| v.abs() < NO_DATA_ZERO_M)
    {
        return None;
    }
    let c = (z[b] - z[a]) / length;
    if !(c.abs() <= o.max_chord_grade) {
        return None;
    }
    let above = |k: usize| z[k] - (z[a] + c * (k - a) as f64 * step);
    let mut height = f64::NEG_INFINITY;
    for k in a..=b {
        let h = above(k);
        if h.is_nan() || h < -o.max_below_chord_m {
            return None;
        }
        if h > height {
            height = h;
        }
    }
    if height < o.min_height_m {
        return None;
    }
    // Un front de montagne à chaque portail, pas la paroi d'un ravin sous un pont.
    let f = st.face;
    let near_entry = (a..=b.min(a + f)).map(above).fold(f64::NEG_INFINITY, f64::max);
    let near_exit = (b.saturating_sub(f).max(a)..=b).map(above).fold(f64::NEG_INFINITY, f64::max);
    if near_entry < o.min_height_m || near_exit < o.min_height_m {
        return None;
    }
    // Les deux côtés impossibles pour une route : montée après l'entrée, descente avant la sortie.
    let (wi, si) = (w as isize, st.side as isize);
    let (ai, bi) = (a as isize, b as isize);
    let mut rise = f64::NEG_INFINITY;
    for k in (ai - wi / 2).max(0)..=(ai + si).min(n as isize - 1) {
        let g = grade(z, k, k + wi, step);
        if !g.is_nan() && g > rise {
            rise = g;
        }
    }
    let mut fall = f64::INFINITY;
    for k in (bi - wi - si).max(0)..=(bi - wi / 2) {
        let g = grade(z, k, k + wi, step);
        if !g.is_nan() && g < fall {
            fall = g;
        }
    }
    if rise < o.min_side_grade || fall > -o.min_side_grade {
        return None;
    }
    // Un tunnel, pas deux : une sortie puis une entrée intérieures, route à l'air libre entre.
    let inner = |v: &[usize]| -> Vec<usize> { v.iter().copied().filter(|&x| x > a && x < b).collect() };
    let (inner_exits, inner_entries) = (inner(exits), inner(entries));
    for &x in &inner_exits {
        for &y in &inner_entries {
            if y <= x {
                continue;
            }
            if (x..=y).any(|k| above(k) < o.min_valley_height_m) {
                return None;
            }
        }
    }
    Some(TunnelSpan {
        first: a,
        last: b,
        chord_grade: c,
        height_m: height,
        rise_grade: rise,
        fall_grade: fall,
        straightness,
        step_m: step,
    })
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    //! Profils synthétiques au pas de 5 m, options par défaut, portés de `vc:G/TunnelDetectorTest.kt`.

    use std::f64::consts::PI;

    use super::*;

    const STEP: f64 = 5.0;
    const LAT0: f64 = 45.0;
    const LON0: f64 = 6.0;

    fn metres_per_degree_lat() -> f64 {
        haversine(LAT0, LON0, LAT0 + 0.001, LON0) * 1000.0
    }

    /// Parcours rectiligne vers le nord de `n` stations.
    fn straight(n: usize) -> (Vec<f64>, Vec<f64>) {
        let m = metres_per_degree_lat();
        ((0..n).map(|k| LAT0 + k as f64 * STEP / m).collect(), vec![LON0; n])
    }

    fn detect(z: &[f64]) -> Vec<TunnelSpan> {
        let (lat, lon) = straight(z.len());
        detect_tunnels(z, &lat, &lon, &TunnelDetectorOptions::default())
    }

    fn road(n: usize, grade: f64, z0: f64) -> Vec<f64> {
        (0..n).map(|k| z0 + grade * k as f64 * STEP).collect()
    }

    /// Montagne sur `a..=b` : fronts de pente `face`, puis dôme de `height` m.
    fn mountain(mut z: Vec<f64>, a: usize, b: usize, height: f64) -> Vec<f64> {
        for k in a..=b {
            let from_end = (k - a).min(b - k) as f64 * STEP;
            let dome = height * (PI * (k - a) as f64 / (b - a) as f64).sin();
            z[k] += (from_end).min(dome).max(0.0);
        }
        z
    }

    fn near(t: &TunnelSpan, first: usize, last: usize) -> bool {
        t.first.abs_diff(first) <= 2 && t.last.abs_diff(last) <= 2
    }

    #[test]
    fn a_tunnel_under_a_mountain_is_found_between_its_portals() {
        let spans = detect(&mountain(road(400, 0.03, 500.0), 100, 220, 120.0));
        assert_eq!(spans.len(), 1, "spans: {spans:?}");
        let t = &spans[0];
        assert!(near(t, 100, 220), "{t:?}");
        assert!((t.chord_grade - 0.03).abs() <= 0.005);
        assert!(t.height_m > 100.0);
        assert!(t.rise_grade >= 0.30 && t.fall_grade <= -0.30);
        assert!((t.straightness - 1.0).abs() <= 1e-3);
    }

    #[test]
    fn a_long_tunnel_keeps_its_far_portal_past_a_terrace() {
        let mut z = mountain(road(600, 0.045, 500.0), 100, 450, 200.0);
        for k in 250..=270 {
            z[k] = z[249] - 0.5 * (k - 249) as f64 * STEP;
        }
        for k in 271..=290 {
            z[k] = z[270];
        }
        let spans = detect(&z);
        assert_eq!(spans.len(), 1, "spans: {spans:?}");
        assert!(near(&spans[0], 100, 450), "{:?}", spans[0]);
    }

    #[test]
    fn two_tunnels_with_open_road_between_are_two() {
        let z = mountain(mountain(road(700, -0.06, 500.0), 100, 200, 100.0), 300, 420, 100.0);
        let spans = detect(&z);
        assert_eq!(spans.len(), 2, "spans: {spans:?}");
        assert!(near(&spans[0], 100, 200) && near(&spans[1], 300, 420), "{spans:?}");
    }

    #[test]
    fn a_hairpin_climb_is_no_tunnel() {
        let n = 600;
        let mut z = vec![0.0; n];
        for k in 1..n {
            z[k] = z[k - 1] + STEP * if (200..=260).contains(&k) { 0.18 } else { 0.10 };
        }
        let m_lat = metres_per_degree_lat();
        let m_lon = m_lat * (LAT0 * PI / 180.0).cos();
        let (mut lat, mut lon) = (vec![0.0; n], vec![0.0; n]);
        let (mut x, mut y) = (0.0, 0.0);
        for k in 0..n {
            lat[k] = LAT0 + y / m_lat;
            lon[k] = LON0 + x / m_lon;
            if (k / 20) % 2 == 0 {
                x += STEP;
            } else {
                x -= STEP;
            }
            if k % 20 == 19 {
                y += 2.0;
            }
        }
        assert_eq!(detect_tunnels(&z, &lat, &lon, &TunnelDetectorOptions::default()), vec![]);
    }

    #[test]
    fn a_real_col_and_a_steep_urban_hump_are_no_tunnels() {
        let col: Vec<f64> = (0..500_i64)
            .map(|k| 1_000.0 + 0.12 * STEP * (if k <= 200 { k } else { 400 - k }) as f64)
            .collect();
        assert_eq!(detect(&col), vec![]);
        let mut hump = road(300, 0.0, 500.0);
        for k in 100..=140 {
            hump[k] += 0.25 * STEP * (k - 100).min(140 - k) as f64;
        }
        assert_eq!(detect(&hump), vec![]);
    }

    fn balcony(width: usize) -> Vec<f64> {
        let mut z = road(400, -0.055, 500.0);
        for c in [100, 230] {
            for k in c..c + width {
                z[k] += 22.0;
            }
        }
        for c in [160, 300] {
            for k in c..c + width {
                z[k] -= 19.0;
            }
        }
        z
    }

    #[test]
    fn a_balcony_reading_the_cliff_in_narrow_spikes_is_left_alone() {
        for width in 1..=3 {
            assert_eq!(detect(&balcony(width)), vec![], "spikes {width} stations wide");
        }
    }

    #[test]
    fn a_wider_cliff_spike_is_at_most_put_back_on_the_road() {
        let z = balcony(5);
        let spans = detect(&z);
        assert_eq!(spans.len(), 2, "spans: {spans:?}");
        for t in &spans {
            assert!((t.chord_grade + 0.055).abs() <= 1e-9);
            assert!((z[t.first] - (500.0 - 0.055 * t.first as f64 * STEP)).abs() <= 1e-9);
            assert!((95..=100).contains(&t.first) || (225..=230).contains(&t.first), "{t:?}");
        }
    }

    #[test]
    fn a_gorge_glimpsed_between_two_tunnels_splits_them() {
        let mut z = mountain(road(400, 0.0, 500.0), 100, 260, 120.0);
        for v in &mut z[170..190] {
            *v = 492.0;
        }
        let spans = detect(&z);
        assert_eq!(spans.len(), 2, "spans: {spans:?}");
        assert!(spans[0].last <= 175 && spans[1].first >= 185, "{spans:?}");
    }

    #[test]
    fn a_steep_chord_and_a_cut_and_cover_are_not_seen() {
        assert_eq!(detect(&mountain(road(400, 0.15, 500.0), 100, 220, 120.0)), vec![]);
        // Limite connue : tranchée couverte sous un sol plat, sous `min_height_m`.
        assert_eq!(detect(&mountain(road(400, 0.0, 500.0), 100, 270, 5.0)), vec![]);
    }

    #[test]
    fn known_limit_a_curved_tunnel_is_not_seen() {
        let n = 400;
        let z = mountain(road(n, 0.02, 500.0), 100, 220, 120.0);
        let m_lat = metres_per_degree_lat();
        let m_lon = m_lat * (LAT0 * PI / 180.0).cos();
        let r = 120.0 * STEP / PI;
        let (mut lat, mut lon) = (vec![0.0; n], vec![0.0; n]);
        for k in 0..n {
            let (x, y) = if k < 100 {
                (0.0, k as f64 * STEP)
            } else if k <= 220 {
                let a = PI * (k - 100) as f64 / 120.0;
                (r - r * a.cos(), 100.0 * STEP + r * a.sin())
            } else {
                (2.0 * r, 100.0 * STEP - (k - 220) as f64 * STEP)
            };
            lat[k] = LAT0 + y / m_lat;
            lon[k] = LON0 + x / m_lon;
        }
        assert_eq!(detect_tunnels(&z, &lat, &lon, &TunnelDetectorOptions::default()), vec![]);
        assert_eq!(detect(&z).len(), 1, "the same profile on a straight route is a tunnel");
    }

    #[test]
    fn no_data_inside_a_candidate_is_no_tunnel() {
        let mut z = mountain(road(400, 0.03, 500.0), 100, 220, 120.0);
        z[160] = f64::NAN;
        assert_eq!(detect(&z), vec![]);
    }

    /// `sample.gpx` km 18.50–19.95 sur Mapterhorn z12 (D 109, Héry-sur-Ugine), tous les 5 m.
    const D109: &str = "
        922.6 922.8 922.9 923.0 923.3 923.8 924.3 924.8 925.0 925.1 925.2 925.9 926.4 926.8 927.1
        927.0 926.9 927.1 927.2 927.7 927.9 928.0 928.2 928.3 928.4 928.8 929.1 929.5 929.9 930.3
        930.7 931.0 931.4 931.6 932.1 933.0 932.7 932.4 932.2 932.0 932.3 933.2 933.5 933.4 933.7
        934.7 935.2 935.9 936.7 937.5 937.7 937.9 938.1 938.6 938.9 939.2 938.7 938.0 937.2 934.1
        930.3 926.9 925.0 927.3 931.5 935.4 938.8 940.5 940.9 941.3 941.2 941.4 941.6 941.5 941.5
        941.4 941.0 940.4 940.7 941.1 941.2 941.1 941.5 942.5 942.6 942.4 942.7 943.1 943.6 943.7
        943.7 943.8 944.2 944.8 945.1 945.5 945.7 945.8 946.3 946.6 947.1 947.6 948.1 948.3 949.0
        949.3 948.6 947.3 945.1 944.1 944.8 946.9 950.1 951.8 953.2 954.3 954.7 954.9 955.2 955.7
        956.2 956.5 956.8 957.1 957.7 958.4 959.1 959.4 959.8 960.2 960.8 961.3 961.8 962.5 963.0
        963.3 963.7 964.4 964.9 965.7 966.8 967.2 967.5 968.1 968.7 969.3 969.8 970.1 970.1 970.5
        971.3 971.8 972.0 972.1 972.8 973.3 973.5 974.0 974.6 975.0 975.0 975.4 976.1 976.9 977.0
        977.3 977.7 978.1 978.4 978.7 979.2 980.0 980.6 980.7 981.1 981.9 982.5 982.7 983.1 983.7
        984.1 984.4 984.7 985.0 985.2 985.4 985.6 985.8 986.3 986.3 986.4 986.8 987.4 987.9 987.9
        988.0 987.9 987.6 987.6 987.8 987.3 987.6 988.1 987.7 987.3 987.0 987.2 987.4 987.6 987.8
        987.7 987.6 987.6 987.8 988.0 988.2 988.4 988.5 988.5 988.6 988.7 989.0 989.3 989.6 989.8
        990.1 990.2 990.3 990.7 991.0 991.3 991.7 992.0 992.3 992.3 992.5 992.9 993.5 993.3 992.9
        993.5 994.0 994.0 994.4 995.4 995.7 995.2 995.8 996.5 996.6 996.4 996.3 996.3 997.4 998.5
        999.2 999.7 999.7 999.0 999.5 1000.6 1000.6 1000.7 1000.3 1000.0 1000.3 1000.5 1000.1 999.9
        999.3 999.3 999.3 999.4 999.4 999.3 999.2 999.2 999.9 999.7 999.3 999.1 998.9 998.9 999.3
        999.4 999.5 1000.2 1000.8 1000.6 1000.4 1001.1
    ";

    #[test]
    fn a_real_hill_between_two_bridged_gullies_is_no_tunnel() {
        let d109: Vec<f64> = D109.split_whitespace().map(|s| s.parse().unwrap()).collect();
        let mut z = d109.clone();
        z.extend(d109.iter().rev().skip(1));
        assert_eq!(detect(&z), vec![], "the hill keeps its 70 m climb");
        let no_face = TunnelDetectorOptions { face_reach_m: 3_000.0, ..TunnelDetectorOptions::default() };
        let (lat, lon) = straight(z.len());
        let spans = detect_tunnels(&z, &lat, &lon, &no_face);
        assert_eq!(spans.len(), 1, "{spans:?}");
        assert!(spans[0].height_m > 60.0 && spans[0].length_m() > 2_000.0, "{:?}", spans[0]);
    }

    #[test]
    fn a_covered_island_between_two_holes_read_as_zero_is_no_tunnel() {
        let island = |hole: f64| -> Vec<f64> {
            (0..300)
                .map(|k| {
                    if (100..=200).contains(&k) {
                        300.0
                    } else if k == 99 || k == 201 {
                        hole + 1.5
                    } else {
                        hole
                    }
                })
                .collect()
        };
        assert_eq!(detect(&island(0.0)), vec![]);
        assert_eq!(detect(&island(1.0)).len(), 1, "the same shape on real ground is a tunnel");
    }

    #[test]
    fn windows_must_be_whole_stations() {
        let d = TunnelDetectorOptions::default;
        assert!(d().validate().is_ok());
        assert!(TunnelDetectorOptions { foot_window_m: 22.0, ..d() }.validate().is_err());
        assert!(TunnelDetectorOptions { face_reach_m: 0.0, ..d() }.validate().is_err());
        assert!(TunnelDetectorOptions { min_straightness: 1.5, ..d() }.validate().is_err());
        assert!(TunnelDetectorOptions { step_m: f64::NAN, ..d() }.validate().is_err());
        // Longueurs différentes : aucune portée (Kotlin refuse le profil).
        assert_eq!(detect_tunnels(&[0.0; 3], &[0.0; 2], &[0.0; 3], &d()), vec![]);
    }

    #[test]
    fn past_max_pairs_no_span_is_returned() {
        let z = mountain(road(400, 0.03, 500.0), 100, 220, 120.0);
        let (lat, lon) = straight(z.len());
        let d = TunnelDetectorOptions::default();
        assert_eq!(detect_tunnels(&z, &lat, &lon, &d).len(), 1);
        let capped = TunnelDetectorOptions { max_pairs: 0, ..d };
        assert_eq!(detect_tunnels(&z, &lat, &lon, &capped), vec![]);
    }
}
