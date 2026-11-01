//! Détecteur de galeries couvertes (pare-avalanche, pare-pierres) sur le MNT lu le long du
//! parcours : un « plateau bas » (T04 §4.2 bis). Pas de portage : vcyclist n'en a pas (ses
//! galeries restent exclues, `X`).
//!
//! Le toit d'une galerie, couvert de terre et raccordé au versant, est classé « sol » par le LiDAR :
//! le MNT y lit le toit, pas la chaussée, et le MNS l'égale (rien au-dessus du sol). Sur le profil,
//! c'est un plateau de quelques mètres au-dessus de la droite entre les portails, à peu près
//! parallèle à elle, entre deux marches presque verticales (les fronts de la galerie). Le
//! détecteur de tunnel ne le voit pas (il exige 15 m de hauteur et des flancs de montagne), ni
//! celui de tablier (MNS ≈ MNT). La route y suit la droite entre portails, que l'appelant réécrit.
//!
//! Pur et déterministe : tableaux en entrée (MNT et MNS, `NaN` hors terrain mesuré), portées en
//! sortie ; station `k` à la distance `k × step_m`.
//!
//! Les comparaisons niées (`!(x > 0.0)`) sont voulues : un NaN y échoue.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use serde::Serialize;

/// Seuils du détecteur, sur la grille de `step_m` mètres. Les bornes viennent de la géométrie d'une
/// galerie routière et de l'échantillonnage du MNT, pas des cas de calibration (T04 §4.2 bis).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GalleryDetectorOptions {
    /// Pas de la grille, en mètres.
    pub step_m: f64,
    /// Une marche de portail tient dans cette fenêtre : front vertical, lissé par le MNT à 1 m et
    /// par une station à au plus un demi-pas du front, ou par un front biais sur la route (3 pas).
    pub step_window_m: f64,
    /// Marche d'un portail, par rapport à la pente de la route : au moins ceci dans
    /// `step_window_m` (trois quarts de `min_height_m`, le reste se perdant au lissage du front).
    pub min_step_m: f64,
    /// Dont au moins ceci sur un seul pas (40 % de pente relative à 5 m, le double de la plus forte
    /// pente d'une route ouverte : une route ne le fait pas, un front vertical si).
    pub min_jump_m: f64,
    /// Approche d'un portail, côté route, où la pente de la route est lue.
    pub approach_m: f64,
    /// L'approche est une chaussée : aucune station à plus de ceci de sa droite (le MNT LiDAR d'une
    /// route est lisse à quelques décimètres).
    pub max_approach_residual_m: f64,
    /// Pente d'une route ouverte, au plus ceci (approches et droite entre portails).
    pub max_road_grade: f64,
    /// La pente de la droite entre portails ne s'écarte pas de plus de ceci de celle de chaque
    /// approche : sous une galerie la route garde sa pente (raccords verticaux lents).
    pub max_grade_change: f64,
    /// Hauteur médiane du toit au-dessus de la droite, au moins ceci : gabarit routier (4,30 m au
    /// moins, 4,50 à 4,75 m d'ordinaire) plus la dalle, moins le lissage du MNT.
    pub min_height_m: f64,
    /// Et au plus ceci : gabarit, dalle (≤ 1,5 m) et coussin de terre amortisseur (quelques
    /// mètres) ; au-delà, c'est un versant, et au-delà de 15 m le détecteur de tunnel.
    pub max_height_m: f64,
    /// Plus courte galerie, mètres entre les stations de route des deux portails : trois pas,
    /// pour qu'au moins deux stations lisent le toit.
    pub min_length_m: f64,
    /// Plus longue galerie ; borne aussi le coût de l'appariement des portails.
    pub max_length_m: f64,
    /// Une station du toit est « basse » sous ceci : une galerie est ouverte côté vallée, et une
    /// trace décalée de quelques mètres y lit la pente sous la galerie.
    pub low_height_m: f64,
    /// Part des stations du toit au-dessus de `low_height_m`, au moins ceci.
    pub min_high_share: f64,
    /// Le toit est parallèle à la route : sa pente propre (droite ajustée, hors stations de
    /// portail) ne le fait pas monter ni descendre de plus de ceci sur sa longueur (épaisseur du
    /// coussin de terre variable de quelques mètres).
    pub max_roof_tilt_m: f64,
    /// Et aucune station du toit ne s'écarte de plus de ceci de cette droite (une bosse arrondie,
    /// un versant ou une crête ne sont pas un toit).
    pub max_roof_residual_m: f64,
    /// MNS ≈ MNT sur le toit : médiane de (MNS − MNT) au plus ceci. Le toit est du sol pour le
    /// LiDAR ; ce qui le surmonte (garde-corps, arbustes) reste sous la moitié de `min_height_m`.
    /// Un bâtiment, une forêt ou un tablier qui surmonterait le plateau l'excèdent.
    pub max_roof_dsm_excess_m: f64,
}

impl Default for GalleryDetectorOptions {
    fn default() -> Self {
        Self {
            step_m: 5.0,
            step_window_m: 15.0,
            min_step_m: 3.0,
            min_jump_m: 2.0,
            approach_m: 20.0,
            max_approach_residual_m: 1.0,
            max_road_grade: 0.15,
            max_grade_change: 0.06,
            min_height_m: 4.0,
            max_height_m: 12.0,
            min_length_m: 15.0,
            max_length_m: 1_000.0,
            low_height_m: 2.0,
            min_high_share: 0.8,
            max_roof_tilt_m: 3.0,
            max_roof_residual_m: 3.0,
            max_roof_dsm_excess_m: 2.0,
        }
    }
}

/// Mètres en nombre entier de stations.
fn whole_stations(metres: f64, step_m: f64, name: &str) -> Result<usize, String> {
    let r = metres / step_m;
    let k = r.round();
    if (r - k).abs() < 1e-9 && k >= 0.0 {
        Ok(k as usize)
    } else {
        Err(format!("{name} = {metres} m is not a whole number of {step_m} m stations"))
    }
}

#[derive(Clone, Copy, Debug)]
struct Stations {
    window: usize,
    approach: usize,
    min_len: usize,
    max_len: usize,
}

impl GalleryDetectorOptions {
    /// Fenêtres en nombres entiers de stations, seuils cohérents (vérifiés au démarrage).
    pub fn validate(&self) -> Result<(), String> {
        self.stations().map(|_| ())
    }

    fn stations(&self) -> Result<Stations, String> {
        if !(self.step_m > 0.0) {
            return Err(format!("gallery step_m must be positive, got {}", self.step_m));
        }
        let st = |m: f64, name: &str| whole_stations(m, self.step_m, name);
        let window = st(self.step_window_m, "gallery step_window_m")?;
        let approach = st(self.approach_m, "gallery approach_m")?;
        let min_len = st(self.min_length_m, "gallery min_length_m")?;
        let max_len = st(self.max_length_m, "gallery max_length_m")?;
        if window < 1 || approach < 2 || min_len < 2 || max_len < min_len {
            return Err("gallery windows must cover stations (approach ≥ 2, min ≤ max length)".into());
        }
        let positive = [
            self.min_step_m,
            self.min_jump_m,
            self.max_approach_residual_m,
            self.max_road_grade,
            self.max_grade_change,
            self.min_height_m,
            self.low_height_m,
            self.max_roof_tilt_m,
            self.max_roof_residual_m,
            self.max_roof_dsm_excess_m,
        ];
        if positive.iter().any(|v| !(*v > 0.0)) || !(self.max_height_m > self.min_height_m) {
            return Err("gallery thresholds must be positive, min_height_m < max_height_m".into());
        }
        if !(0.0..=1.0).contains(&self.min_high_share) {
            return Err("gallery min_high_share must be in 0..1".into());
        }
        Ok(Stations { window, approach, min_len, max_len })
    }
}

/// Galerie acceptée : stations de route des deux portails (dernière avant la marche d'entrée,
/// première après la marche de sortie) ; la route suit la droite entre elles.
#[derive(Clone, Debug, PartialEq)]
pub struct GallerySpan {
    /// Portail d'entrée, indice de station (inclus).
    pub first: usize,
    /// Portail de sortie, indice de station (inclus).
    pub last: usize,
    /// Hauteur médiane du toit au-dessus de la droite entre portails.
    pub height_m: f64,
}

/// Pente de la droite des stations `a..=b` si elles sont toutes mesurées (MNT et MNS) et à
/// `max_residual` au plus de la droite de leurs extrémités : une chaussée.
fn road_grade(z: &[f64], ok: &[bool], a: usize, b: usize, step: f64, max_residual: f64) -> Option<f64> {
    if !ok[a..=b].iter().all(|&x| x) {
        return None;
    }
    let g = (z[b] - z[a]) / ((b - a) as f64 * step);
    (a..=b).all(|k| (z[k] - (z[a] + g * (k - a) as f64 * step)).abs() <= max_residual).then_some(g)
}

/// Marche d'un portail vue depuis la station de route `i`, vers l'intérieur dans le sens `dir`
/// (+1 : entrée, −1 : sortie), la route ayant la pente `g` dans le sens du parcours.
fn is_step(
    z: &[f64],
    ok: &[bool],
    i: usize,
    dir: isize,
    g: f64,
    w: usize,
    o: &GalleryDetectorOptions,
) -> bool {
    let at = |k: usize| (i as isize + dir * k as isize) as usize;
    // Hauteur au-dessus du prolongement de la route, à `k` pas de `i`.
    let rel = |k: usize| z[at(k)] - (z[i] + g * dir as f64 * k as f64 * o.step_m);
    let mut jump = f64::NEG_INFINITY;
    for k in 1..=w {
        if !ok[at(k)] {
            return false;
        }
        jump = jump.max(rel(k) - rel(k - 1));
        if rel(k) >= o.min_step_m && jump >= o.min_jump_m {
            return true;
        }
    }
    false
}

/// Ajustement linéaire de points `(x, y)` (x en pas) : (pente par pas, y moyen, x moyen).
fn fit(p: &[(f64, f64)]) -> (f64, f64, f64) {
    let n = p.len() as f64;
    let xm = p.iter().map(|q| q.0).sum::<f64>() / n;
    let ym = p.iter().map(|q| q.1).sum::<f64>() / n;
    let den: f64 = p.iter().map(|q| (q.0 - xm).powi(2)).sum();
    let num: f64 = p.iter().map(|q| (q.0 - xm) * (q.1 - ym)).sum();
    (if den > 0.0 { num / den } else { 0.0 }, ym, xm)
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

/// Galeries du profil `mnt` / `mns` (même longueur ; `NaN` = non mesuré), triées, disjointes. Des
/// options invalides ne rendent rien (elles sont refusées au démarrage).
pub fn detect_galleries(mnt: &[f64], mns: &[f64], o: &GalleryDetectorOptions) -> Vec<GallerySpan> {
    let Ok(s) = o.stations() else { return Vec::new() };
    let n = mnt.len().min(mns.len());
    let (w, a) = (s.window, s.approach);
    if n < 2 * a + s.min_len + 1 {
        return Vec::new();
    }
    let z = &mnt[..n];
    let ok: Vec<bool> = (0..n).map(|k| mnt[k].is_finite() && mns[k].is_finite()).collect();
    // Portails candidats : route lisse sur l'approche, puis marche vers l'intérieur. Dans une suite
    // de candidats consécutifs, la station de route du portail est la plus proche de la marche
    // (dernière pour une entrée, première pour une sortie).
    let mut entries: Vec<(usize, f64)> = Vec::new();
    for i in a..n.saturating_sub(w) {
        let Some(g) = road_grade(z, &ok, i - a, i, o.step_m, o.max_approach_residual_m) else { continue };
        if g.abs() <= o.max_road_grade && is_step(z, &ok, i, 1, g, w, o) {
            if entries.last().is_some_and(|e| e.0 + 1 == i) {
                entries.pop();
            }
            entries.push((i, g));
        }
    }
    let mut exits: Vec<(usize, f64)> = Vec::new();
    for j in w..n.saturating_sub(a) {
        let Some(g) = road_grade(z, &ok, j, j + a, o.step_m, o.max_approach_residual_m) else { continue };
        if g.abs() <= o.max_road_grade
            && is_step(z, &ok, j, -1, g, w, o)
            && !exits.last().is_some_and(|e| e.0 + 1 == j)
        {
            exits.push((j, g));
        }
    }
    let mut out: Vec<GallerySpan> = Vec::new();
    for &(i, gi) in &entries {
        if out.last().is_some_and(|p| i < p.last) {
            continue;
        }
        // Sortie la plus proche qui fait une galerie.
        for &(j, gj) in exits.iter().filter(|e| e.0 >= i + s.min_len && e.0 <= i + s.max_len) {
            if let Some(height_m) = judge(z, mns, &ok, i, j, gi, gj, o) {
                out.push(GallerySpan { first: i, last: j, height_m });
                break;
            }
        }
    }
    out
}

/// Plateau entre la station de route d'entrée `i` et celle de sortie `j` : hauteur médiane du toit
/// si c'est une galerie.
#[allow(clippy::too_many_arguments)]
fn judge(
    z: &[f64],
    mns: &[f64],
    ok: &[bool],
    i: usize,
    j: usize,
    gi: f64,
    gj: f64,
    o: &GalleryDetectorOptions,
) -> Option<f64> {
    if !ok[i..=j].iter().all(|&x| x) {
        return None;
    }
    let len = (j - i) as f64 * o.step_m;
    let gc = (z[j] - z[i]) / len;
    if gc.abs() > o.max_road_grade
        || (gc - gi).abs() > o.max_grade_change
        || (gc - gj).abs() > o.max_grade_change
    {
        return None;
    }
    let h: Vec<f64> = (i + 1..j).map(|k| z[k] - (z[i] + gc * (k - i) as f64 * o.step_m)).collect();
    // La route ne passe pas sous la droite (creux, vallée, pont).
    if h.iter().any(|&v| v < -o.low_height_m) {
        return None;
    }
    // Toit : de la première à la dernière station à la hauteur d'une marche.
    let first = h.iter().position(|&v| v >= o.min_step_m)?;
    let last = h.iter().rposition(|&v| v >= o.min_step_m)?;
    let roof = &h[first..=last];
    let height = median(&mut roof.to_vec());
    if height < o.min_height_m || height > o.max_height_m {
        return None;
    }
    let high = roof.iter().filter(|&&v| v >= o.low_height_m).count();
    if (high as f64) < o.min_high_share * roof.len() as f64 {
        return None;
    }
    // Forme, sur les stations hautes (les basses sont le côté ouvert, déjà bornées en part), hors
    // stations de portail (lecture mêlée du front) dès que le toit en a cinq.
    let core = if roof.len() >= 5 { &roof[1..roof.len() - 1] } else { roof };
    let pts: Vec<(f64, f64)> =
        core.iter().enumerate().filter(|(_, &v)| v >= o.low_height_m).map(|(x, &v)| (x as f64, v)).collect();
    if pts.len() >= 3 {
        let (slope, ym, xm) = fit(&pts);
        if slope.abs() * (core.len() - 1) as f64 > o.max_roof_tilt_m {
            return None;
        }
        let resid = pts.iter().map(|(x, v)| (v - (ym + slope * (x - xm))).abs()).fold(0.0, f64::max);
        if resid > o.max_roof_residual_m {
            return None;
        }
    }
    // MNS ≈ MNT sur le toit.
    let mut excess: Vec<f64> = (i + 1 + first..=i + 1 + last).map(|k| mns[k] - z[k]).collect();
    if median(&mut excess) > o.max_roof_dsm_excess_m {
        return None;
    }
    Some(height)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Route de pente `g` sur `n` stations, MNS = MNT.
    fn road(n: usize, g: f64) -> Vec<f64> {
        (0..n).map(|k| 100.0 + g * 5.0 * k as f64).collect()
    }

    /// Toit de `h` m sur les stations `a..=b` (fronts verticaux).
    fn roof(z: &mut [f64], a: usize, b: usize, h: f64) {
        for v in &mut z[a..=b] {
            *v += h;
        }
    }

    fn detect(z: &[f64], mns: &[f64]) -> Vec<GallerySpan> {
        detect_galleries(z, mns, &GalleryDetectorOptions::default())
    }

    #[test]
    fn default_options_are_valid() {
        assert_eq!(GalleryDetectorOptions::default().validate(), Ok(()));
        let bad = GalleryDetectorOptions { approach_m: 12.0, ..Default::default() };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn a_low_plateau_between_two_vertical_steps_is_a_gallery() {
        // Route à −6 %, galerie de 120 m (stations 41 à 64), toit à 7 m, MNS = MNT.
        let mut z = road(120, -0.06);
        roof(&mut z, 41, 64, 7.0);
        let g = detect(&z, &z.clone());
        assert_eq!(g.len(), 1, "{g:?}");
        assert_eq!((g[0].first, g[0].last), (40, 65));
        assert!((g[0].height_m - 7.0).abs() < 1e-9);
    }

    #[test]
    fn a_short_gallery_with_smeared_fronts_is_found() {
        // Galerie de 20 m : fronts lissés sur deux pas (4 m puis 6 m), route à +8 %.
        let mut z = road(60, 0.08);
        for (k, h) in [(30, 4.0), (31, 6.0), (32, 6.5), (33, 6.0), (34, 3.5)] {
            z[k] += h;
        }
        let g = detect(&z, &z.clone());
        assert_eq!(g.len(), 1, "{g:?}");
        assert_eq!((g[0].first, g[0].last), (29, 35));
    }

    #[test]
    fn a_gallery_open_on_the_valley_side_keeps_a_few_low_stations() {
        let mut z = road(140, 0.05);
        roof(&mut z, 41, 80, 6.0);
        // Trace décalée côté vallée sur 5 stations sur 40 : la pente sous la galerie.
        for k in [50, 51, 60, 61, 70] {
            z[k] -= 5.0;
        }
        assert_eq!(detect(&z, &z.clone()).len(), 1);
    }

    #[test]
    fn a_plateau_with_something_standing_on_it_is_not_a_gallery() {
        // Bâtiment, forêt ou tablier au-dessus du plateau : MNS − MNT de 10 m.
        let mut z = road(120, 0.0);
        roof(&mut z, 41, 64, 7.0);
        let mns: Vec<f64> =
            z.iter().enumerate().map(|(k, v)| v + if (41..=64).contains(&k) { 10.0 } else { 0.0 }).collect();
        assert!(detect(&z, &mns).is_empty());
    }

    #[test]
    fn too_low_or_too_high_plateaus_are_rejected() {
        for h in [2.5, 14.0] {
            let mut z = road(120, 0.02);
            roof(&mut z, 41, 64, h);
            assert!(detect(&z, &z.clone()).is_empty(), "{h} m");
        }
    }

    #[test]
    fn a_smooth_crest_or_an_embankment_is_not_a_gallery() {
        // Crête : la route monte à 8 % puis redescend, sans marche verticale.
        let z: Vec<f64> = (0..200).map(|k| 100.0 + 8.0 - (0.4 * (k as f64 - 100.0)).abs().min(8.0)).collect();
        assert!(detect(&z, &z.clone()).is_empty());
        // Remblai : une rampe de 20 % ne fait pas de marche.
        let mut z = road(120, 0.0);
        for (k, v) in z.iter_mut().enumerate() {
            let x = k as f64;
            *v += (x - 40.0).clamp(0.0, 6.0) - (x - 74.0).clamp(0.0, 6.0);
        }
        assert!(detect(&z, &z.clone()).is_empty());
    }

    #[test]
    fn a_roof_that_rises_away_from_the_road_is_a_slope_not_a_gallery() {
        // Marche de 5 m puis terrain qui monte de 8 m de plus sur 100 m (versant, déblai).
        let mut z = road(120, 0.0);
        for (k, v) in z.iter_mut().enumerate().take(61).skip(41) {
            *v += 5.0 + 8.0 * (k - 41) as f64 / 19.0;
        }
        z[61] += 5.0;
        assert!(detect(&z, &z.clone()).is_empty());
    }

    #[test]
    fn a_rough_approach_or_unmeasured_stations_give_nothing() {
        let mut z = road(120, 0.0);
        roof(&mut z, 41, 64, 7.0);
        let mut rough = z.clone();
        for k in (20..41).step_by(2) {
            rough[k] += 2.5;
        }
        for k in (65..90).step_by(2) {
            rough[k] += 2.5;
        }
        assert!(detect(&rough, &rough.clone()).is_empty());
        let mut mns = z.clone();
        mns[50] = f64::NAN;
        assert!(detect(&z, &mns).is_empty());
    }

    #[test]
    fn a_dip_below_the_chord_is_not_a_roof() {
        // Deux plateaux séparés par un creux de 6 m sous la droite.
        let mut z = road(140, 0.0);
        roof(&mut z, 41, 64, 7.0);
        for v in &mut z[50..=52] {
            *v -= 13.0;
        }
        let g = detect(&z, &z.clone());
        assert!(g.iter().all(|s| !(s.first <= 50 && s.last >= 52)), "{g:?}");
    }
}
