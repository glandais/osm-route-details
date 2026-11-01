//! Détecteur de tabliers de pont sur MNS/MNT LiDAR le long du parcours : portage fidèle de
//! `vc:G/DeckDetector.kt` (lui-même port d'`analyze.py`, bloc des ponts) et de ses options.
//!
//! Pur et déterministe : tableaux en entrée, plages en sortie, aucune E/S. Les stations sont
//! uniformes au pas `step_m` (station `k` à la distance `k × step_m`) ; toute fenêtre et tout
//! seuil de portée se comptent en stations de ce pas.
//!
//! Les comparaisons niées (`!(x >= seuil)`) sont voulues : un NaN y échoue comme en Kotlin.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::ops::RangeInclusive;

use serde::Serialize;

/// Seuils du détecteur, en mètres sur la grille du parcours (`vc:G/DeckDetectorOptions.kt:28-149`).
///
/// Une *fenêtre* compte des stations (`min_tall_run_m = 15` : 3 stations) ; une *portée* compte
/// des intervalles (`short_span_m = 25` : `last − first ≤ 5`). Chaque fenêtre doit tomber sur un
/// nombre entier de stations, les deux fenêtres glissantes sur un nombre impair ([`Self::validate`]).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DeckDetectorOptions {
    /// Pas de la grille, en mètres.
    pub step_m: f64,
    /// Une station est *haute* quand son MNH dépasse ce seuil.
    pub tall_mnh_m: f64,
    /// Une suite haute plus courte (stations × pas) n'est pas candidate.
    pub min_tall_run_m: f64,
    /// Médiane glissante (sans NaN) du MNS : le profil du tablier ; nombre impair de stations.
    pub surface_median_window_m: f64,
    /// Rugosité : RMS de `MNS − médiane` sur cette fenêtre ; nombre impair de stations.
    pub roughness_window_m: f64,
    /// Une station haute est *lisse* quand sa rugosité est sous ce seuil.
    pub max_roughness_m: f64,
    /// Une suite haute est gardée quand au moins cette part de ses stations est lisse.
    pub min_smooth_fraction: f64,
    /// Les suites gardées fusionnent quand il manque moins de ce nombre de mètres entre elles.
    pub merge_gap_m: f64,
    /// Extension d'une candidate le long de `MNH > deck_mnh_m`, et seuil du masque de tablier.
    pub deck_mnh_m: f64,
    /// Demi-fenêtre de la pente du tablier.
    pub slope_half_window_m: f64,
    /// Une portée longue est un tablier quand le P95 de |pente| est au plus ceci (fraction).
    pub max_slope_p95: f64,
    /// Une portée longue mesure aussi au moins ceci, `(last − first) × pas`.
    pub min_length_m: f64,
    /// Portée *courte* : `(last − first) × pas` au plus ceci, jugée sur sa rugosité.
    pub short_span_m: f64,
    /// Une portée courte est un tablier quand sa rugosité médiane est sous ce seuil.
    pub max_short_roughness_m: f64,
    /// Test du canyon : le MNT le plus bas de la portée est plus de ceci sous ses deux abords.
    pub min_canyon_drop_m: f64,
    /// Masque de tablier : une suite `MNH > deck_mnh_m` d'au moins ceci est tablier.
    pub deck_min_run_m: f64,
    /// Masque de tablier : sur une portée d'au plus ceci (intervalles), toute suite est tablier.
    pub small_span_m: f64,
    /// Plus longue culée, par côté, au-delà du cœur haut.
    pub max_abutment_m: f64,
    /// E2 ter : pente lue sur le MNS dont les stations rugueuses sont pontées linéairement.
    pub slope_on_clean_deck: bool,
    /// Sous cette part de stations propres, la portée est lue sur son profil brut.
    pub min_clean_fraction: f64,
    /// Sous ce nombre de stations propres, même repli sur le profil brut (au moins 2).
    pub min_clean_stations_count: usize,
    /// E2 ter : `MNT > MNS + tolérance` (impossible) lu comme un MNH haut (`+∞`).
    pub mnt_above_mns_as_tall: bool,
    /// Tolérance de [`Self::mnt_above_mns_as_tall`].
    pub mnt_above_mns_tolerance_m: f64,
    /// E2 quinquies : un trou n'est ponté que si aucune de ses stations n'a un MNH ≤ ceci.
    pub clean_gap_ground_mnh_m: f64,
    /// E2 quinquies : une portée fusionnée `Steep` est rejugée en ses parties.
    pub split_steep_merged: bool,
}

impl Default for DeckDetectorOptions {
    /// `DeckDetectorOptions.DEFAULT` (`vc:G/DeckDetectorOptions.kt:245`).
    fn default() -> Self {
        Self {
            step_m: 5.0,
            tall_mnh_m: 5.0,
            min_tall_run_m: 15.0,
            surface_median_window_m: 25.0,
            roughness_window_m: 35.0,
            max_roughness_m: 0.8,
            min_smooth_fraction: 0.6,
            merge_gap_m: 300.0,
            deck_mnh_m: 1.0,
            slope_half_window_m: 10.0,
            max_slope_p95: 0.10,
            min_length_m: 30.0,
            short_span_m: 25.0,
            max_short_roughness_m: 0.4,
            min_canyon_drop_m: 3.0,
            deck_min_run_m: 40.0,
            small_span_m: 60.0,
            max_abutment_m: 100.0,
            slope_on_clean_deck: true,
            min_clean_fraction: 0.25,
            min_clean_stations_count: 5,
            mnt_above_mns_as_tall: true,
            mnt_above_mns_tolerance_m: 0.5,
            clean_gap_ground_mnh_m: 1.0,
            split_steep_merged: true,
        }
    }
}

/// Fenêtres des options en stations, calculées une fois (`vc:G/DeckDetectorOptions.kt:205-237`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Windows {
    min_tall_run: usize,
    surface_median: usize,
    roughness: usize,
    merge_gap: usize,
    slope_half: usize,
    short_span: usize,
    deck_min_run: usize,
    max_abutment: usize,
    small_span: usize,
}

impl Windows {
    /// Plus large trou qu'un tablier franchit : moitié de la médiane de surface.
    fn max_deck_gap(&self) -> usize {
        self.surface_median / 2
    }
}

/// Mètres en nombre entier de stations, ou erreur (`vc:G/DeckDetectorOptions.kt:195-203`).
fn whole_stations(metres: f64, step_m: f64, name: &str) -> Result<i64, String> {
    let r = metres / step_m;
    let k = r.round();
    if (r - k).abs() < 1e-9 {
        Ok(k as i64)
    } else {
        Err(format!("{name} = {metres} m is not a whole number of {step_m} m stations"))
    }
}

impl DeckDetectorOptions {
    /// Seuils d'`analyze.py` : DEFAULT sans les options E2 ter et E2 quinquies
    /// (`vc:G/DeckDetectorOptions.kt:254-255`).
    pub fn python() -> Self {
        Self {
            slope_on_clean_deck: false,
            mnt_above_mns_as_tall: false,
            split_steep_merged: false,
            ..Self::default()
        }
    }

    /// Contrôles du bloc `init` Kotlin (`vc:G/DeckDetectorOptions.kt:150-193`).
    pub fn validate(&self) -> Result<(), String> {
        self.windows().map(|_| ())
    }

    fn windows(&self) -> Result<Windows, String> {
        if !(self.step_m > 0.0 && self.step_m.is_finite()) {
            return Err(format!("stepM must be positive, got {}", self.step_m));
        }
        let thresholds = [
            ("tallMnhM", self.tall_mnh_m),
            ("minTallRunM", self.min_tall_run_m),
            ("surfaceMedianWindowM", self.surface_median_window_m),
            ("roughnessWindowM", self.roughness_window_m),
            ("maxRoughnessM", self.max_roughness_m),
            ("minSmoothFraction", self.min_smooth_fraction),
            ("mergeGapM", self.merge_gap_m),
            ("deckMnhM", self.deck_mnh_m),
            ("slopeHalfWindowM", self.slope_half_window_m),
            ("maxSlopeP95", self.max_slope_p95),
            ("minLengthM", self.min_length_m),
            ("shortSpanM", self.short_span_m),
            ("maxShortRoughnessM", self.max_short_roughness_m),
            ("minCanyonDropM", self.min_canyon_drop_m),
            ("deckMinRunM", self.deck_min_run_m),
            ("smallSpanM", self.small_span_m),
            ("maxAbutmentM", self.max_abutment_m),
            ("minCleanFraction", self.min_clean_fraction),
            ("mntAboveMnsToleranceM", self.mnt_above_mns_tolerance_m),
            ("cleanGapGroundMnhM", self.clean_gap_ground_mnh_m),
        ];
        for (name, v) in thresholds {
            if !v.is_finite() {
                return Err(format!("{name} must be finite, got {v}"));
            }
        }
        let st = |m: f64, name: &str| whole_stations(m, self.step_m, name);
        let surface_median = st(self.surface_median_window_m, "surfaceMedianWindowM")?;
        if surface_median.rem_euclid(2) != 1 {
            return Err("surfaceMedianWindowM must be an odd number of stations".into());
        }
        let roughness = st(self.roughness_window_m, "roughnessWindowM")?;
        if roughness.rem_euclid(2) != 1 {
            return Err("roughnessWindowM must be an odd number of stations".into());
        }
        let min_tall_run = st(self.min_tall_run_m, "minTallRunM")?;
        if min_tall_run < 1 {
            return Err("minTallRunM must cover at least one station".into());
        }
        let slope_half = st(self.slope_half_window_m, "slopeHalfWindowM")?;
        if slope_half < 1 {
            return Err("slopeHalfWindowM must cover at least one station".into());
        }
        let merge_gap = st(self.merge_gap_m, "mergeGapM")?;
        let deck_min_run = st(self.deck_min_run_m, "deckMinRunM")?;
        if !(merge_gap >= 0 && deck_min_run >= 1) {
            return Err("negative window".into());
        }
        let short_span = st(self.short_span_m, "shortSpanM")?;
        let small_span = st(self.small_span_m, "smallSpanM")?;
        if !(short_span >= 0 && small_span >= 0) {
            return Err("negative span threshold".into());
        }
        let max_abutment = st(self.max_abutment_m, "maxAbutmentM")?;
        if max_abutment < 0 {
            return Err("negative maxAbutmentM".into());
        }
        if !(0.0..=1.0).contains(&self.min_clean_fraction) {
            return Err(format!("minCleanFraction must be in 0..1, got {}", self.min_clean_fraction));
        }
        if !(self.mnt_above_mns_tolerance_m >= 0.0) {
            return Err(format!(
                "mntAboveMnsToleranceM must be non-negative, got {}",
                self.mnt_above_mns_tolerance_m
            ));
        }
        if self.min_clean_stations_count < 2 {
            return Err(format!(
                "minCleanStationsCount must be at least 2, got {}",
                self.min_clean_stations_count
            ));
        }
        if short_span < 2 * slope_half - 1 {
            return Err(
                "shortSpanM must be at least 2·slopeHalfWindowM − stepM, or a long span has no slope sample"
                    .into(),
            );
        }
        // Tous positifs ici : les conversions ne perdent rien.
        let u = |v: i64| v as usize;
        Ok(Windows {
            min_tall_run: u(min_tall_run),
            surface_median: u(surface_median),
            roughness: u(roughness),
            merge_gap: u(merge_gap),
            slope_half: u(slope_half),
            short_span: u(short_span),
            deck_min_run: u(deck_min_run),
            max_abutment: u(max_abutment),
            small_span: u(small_span),
        })
    }
}

/// Pourquoi une candidate n'est pas un tablier, dans l'ordre des tests (`vc:G/DeckDetector.kt:49-71`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeckRejection {
    /// Suites hautes au MNS rugueux : canopée ou bâti ; jamais étendues ni mesurées.
    Rough,
    /// Le MNT sous la portée ne descend pas de plus de `min_canyon_drop_m` sous ses abords.
    NoCanyon,
    /// Portée longue au P95 de pente au-dessus de `max_slope_p95` (ou plus courte que `min_length_m`).
    Steep,
    /// Portée courte dont la rugosité médiane n'est pas sous `max_short_roughness_m`.
    ShortRough,
    /// Une valeur lue par la décision est NaN.
    NoData,
}

/// Mesures d'une candidate ; pentes en fractions (`vc:G/DeckDetector.kt:93-105`).
#[derive(Clone, Debug, PartialEq)]
pub struct DeckMetrics {
    /// `(last − first + 1) × pas` : compte des stations.
    pub length_m: f64,
    /// P95 de |pente| sur le profil lu par le verdict ; NaN sur une portée courte.
    pub slope_p95: f64,
    /// Maximum du même ; NaN sur une portée courte.
    pub slope_max: f64,
    /// P95 de |pente| de la médiane brute du MNS ; NaN sur une portée courte.
    pub raw_slope_p95: f64,
    /// Part de stations propres parmi les stations intérieures ; NaN sur une portée courte.
    pub clean_fraction: f64,
    /// `slope_p95` lu sur le profil ponté.
    pub slope_on_clean_deck: bool,
    /// Rugosité médiane des stations hautes de la portée.
    pub roughness_m: f64,
    /// Plus petite part lisse des suites hautes qui forment la candidate.
    pub smooth_fraction: f64,
    /// Max de `médiane MNS − MNT` sur la portée, NaN ignorés (diagnostic).
    pub canyon_m: f64,
    /// `min(MNT juste hors portée) − min(MNT sur la portée)`.
    pub mnt_drop_m: f64,
    /// Max de la médiane du MNS sur la portée, NaN ignorés (diagnostic).
    pub deck_top_m: f64,
}

/// Une portée examinée (`vc:G/DeckDetector.kt:124-134`). Indices de stations, bornes incluses.
#[derive(Clone, Debug, PartialEq)]
pub struct DeckCandidate {
    pub first: usize,
    pub last: usize,
    /// `None` pour un groupe `Rough` ou un `NoData` non mesuré.
    pub metrics: Option<DeckMetrics>,
    /// `None` : acceptée.
    pub rejection: Option<DeckRejection>,
    /// Acceptée seulement : stations dont la route est le tablier ; vide sinon.
    pub deck_ranges: Vec<RangeInclusive<usize>>,
}

impl DeckCandidate {
    pub fn accepted(&self) -> bool {
        self.rejection.is_none()
    }

    /// Distance de la première station, `first × step_m`.
    pub fn start_m(&self, step_m: f64) -> f64 {
        self.first as f64 * step_m
    }

    /// Distance de la dernière station, `last × step_m`.
    pub fn end_m(&self, step_m: f64) -> f64 {
        self.last as f64 * step_m
    }
}

/// Résultat de [`detect_decks`], listes triées par `first` (`vc:G/DeckDetector.kt:141-145`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct DeckDetection {
    pub accepted: Vec<DeckCandidate>,
    pub rejected: Vec<DeckCandidate>,
    /// Suites hautes plus courtes que `min_tall_run_m`.
    pub tall_runs_too_short: usize,
}

/// Stations uniformes à pas `o.step_m`; NaN = pas de donnée. mnh = mns - mnt (DeckProfile.fromSurface).
///
/// Options invalides ([`DeckDetectorOptions::validate`]) ou longueurs différentes : détection vide
/// (Kotlin refuse de construire).
pub fn detect_decks(mnt: &[f64], mns: &[f64], o: &DeckDetectorOptions) -> DeckDetection {
    if mnt.len() != mns.len() {
        return DeckDetection::default();
    }
    let mnh: Vec<f64> = mns.iter().zip(mnt).map(|(s, t)| s - t).collect();
    detect_profile(&mnh, mns, mnt, o)
}

/// État par station partagé par la détection (`vc:G/DeckDetector.kt:462-491`).
struct SurfaceState {
    /// MNH effectif : `+∞` où `MNT > MNS + tolérance` si `mnt_above_mns_as_tall`.
    mnh: Vec<f64>,
    mf: Vec<f64>,
    rough: Vec<f64>,
    clean: Vec<bool>,
}

fn surface_state(
    mnh: &[f64],
    mns: &[f64],
    mnt: &[f64],
    o: &DeckDetectorOptions,
    w: &Windows,
) -> SurfaceState {
    let n = mns.len();
    let mnh: Vec<f64> = if o.mnt_above_mns_as_tall {
        let tol = o.mnt_above_mns_tolerance_m;
        (0..n).map(|k| if mnt[k] > mns[k] + tol { f64::INFINITY } else { mnh[k] }).collect()
    } else {
        mnh.to_vec()
    };
    let mf = rolling_nan_median(mns, w.surface_median / 2);
    let rough = roughness(mns, &mf, w.roughness / 2);
    let clean = rolling_max(&rough, w.roughness / 2).iter().map(|&m| m < o.max_roughness_m).collect();
    SurfaceState { mnh, mf, rough, clean }
}

/// Plage de stations `(first, last)`, bornes incluses.
type Span = (usize, usize);

/// `DeckDetector.detect` sur un MNH explicite (`vc:G/DeckDetector.kt:235-418`).
fn detect_profile(mnh_in: &[f64], mns: &[f64], mnt: &[f64], o: &DeckDetectorOptions) -> DeckDetection {
    let n = mns.len();
    let Ok(w) = o.windows() else {
        return DeckDetection::default();
    };
    if n == 0 || mnh_in.len() != n || mnt.len() != n {
        return DeckDetection::default();
    }
    let step = o.step_m;
    let state = surface_state(mnh_in, mns, mnt, o, &w);
    let mnh = &state.mnh;
    let mf = &state.mf;
    let rough = &state.rough;

    let tall: Vec<bool> = mnh.iter().map(|&v| v > o.tall_mnh_m).collect();
    let tall_runs = runs(&tall);
    let raw: Vec<Span> = tall_runs.iter().copied().filter(|r| r.1 - r.0 + 1 >= w.min_tall_run).collect();
    let too_short = tall_runs.len() - raw.len();

    let mut kept: Vec<(Span, f64)> = Vec::new();
    let mut rough_runs: Vec<Span> = Vec::new();
    let mut no_data_runs: Vec<Span> = Vec::new();
    for &r in &raw {
        let len = (r.1 - r.0 + 1) as f64;
        let smooth = (r.0..=r.1).filter(|&k| rough[k] < o.max_roughness_m).count();
        let unknown = (r.0..=r.1).filter(|&k| rough[k].is_nan()).count();
        let fraction = smooth as f64 / len;
        if fraction >= o.min_smooth_fraction {
            kept.push((r, fraction));
        } else if (smooth + unknown) as f64 / len >= o.min_smooth_fraction {
            no_data_runs.push(r);
        } else {
            rough_runs.push(r);
        }
    }

    // Fusion des suites gardées (étape 5), avec la plus petite part lisse du groupe.
    let gap = w.merge_gap;
    let mut merged: Vec<(Span, f64)> = Vec::new();
    for &(r, f) in &kept {
        match merged.last_mut() {
            Some(prev) if r.0 - prev.0 .1 - 1 < gap => *prev = ((prev.0 .0, r.1), prev.1.min(f)),
            _ => merged.push((r, f)),
        }
    }

    // Extension (étape 6), puis union des portées qui se chevauchent.
    let island = w.max_deck_gap();
    let extend_union = |groups: &[(Span, f64)]| -> Vec<(Span, f64)> {
        let mut out: Vec<(Span, f64)> = Vec::new();
        for &(r, f) in groups {
            let a = extend(mnh, r.0, -1, o.deck_mnh_m, island, w.max_abutment);
            let b = extend(mnh, r.1, 1, o.deck_mnh_m, island, w.max_abutment);
            match out.last_mut() {
                Some(prev) if a <= prev.0 .1 => *prev = ((prev.0 .0, prev.0 .1.max(b)), prev.1.min(f)),
                _ => out.push(((a, b), f)),
            }
        }
        out
    };
    let spans = extend_union(&merged);
    // Parties (E2 quinquies) : chaque suite gardée étendue seule, chevauchements unis.
    let parts = extend_union(&kept);

    let slope = centred_slope(mf, w.slope_half, step);
    let judge = |span: Span, fraction: f64| classify(span, fraction, mns, mnt, &state, &slope, o, &w);

    let mut classified: Vec<(Span, DeckMetrics, Option<DeckRejection>)> = Vec::new();
    for &(span, fraction) in &spans {
        let (metrics, rejection) = judge(span, fraction);
        let inside: Vec<(Span, f64)> = if o.split_steep_merged && rejection == Some(DeckRejection::Steep) {
            parts.iter().copied().filter(|p| p.0 .0 >= span.0 && p.0 .1 <= span.1).collect()
        } else {
            Vec::new()
        };
        if inside.len() >= 2 {
            for (part, f) in inside {
                let (m, r) = judge(part, f);
                classified.push((part, m, r));
            }
        } else {
            classified.push((span, metrics, rejection));
        }
    }

    // Masque de tablier (étape 9) : MNH > deck_mnh_m, trous courts fermés dans les portées acceptées.
    let mut deck_mask: Vec<bool> = mnh.iter().map(|&v| v > o.deck_mnh_m).collect();
    for (span, _, rejection) in &classified {
        if rejection.is_none() {
            close_holes(&mut deck_mask, *span, island, |k| !mf[k].is_nan());
        }
    }
    let deck_runs = runs(&deck_mask);

    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for (span, metrics, rejection) in classified {
        let (a, b) = span;
        let deck_ranges = if rejection.is_some() {
            Vec::new()
        } else {
            deck_runs
                .iter()
                .filter(|r| {
                    (r.1 - r.0 + 1 >= w.deck_min_run || b - a <= w.small_span) && r.0 <= b && r.1 >= a
                })
                .map(|r| r.0.max(a)..=r.1.min(b))
                .collect()
        };
        let candidate = DeckCandidate { first: a, last: b, metrics: Some(metrics), rejection, deck_ranges };
        if rejection.is_none() {
            accepted.push(candidate);
        } else {
            rejected.push(candidate);
        }
    }
    for (groups, why) in [(&rough_runs, DeckRejection::Rough), (&no_data_runs, DeckRejection::NoData)] {
        for g in merge_runs(groups, gap) {
            rejected.push(DeckCandidate {
                first: g.0,
                last: g.1,
                metrics: None,
                rejection: Some(why),
                deck_ranges: Vec::new(),
            });
        }
    }
    rejected.sort_by_key(|c| (c.first, c.last));
    DeckDetection { accepted, rejected, tall_runs_too_short: too_short }
}

/// Mesures et verdict d'une portée (`vc:G/DeckDetector.kt:298-351`).
#[allow(clippy::too_many_arguments)]
fn classify(
    (a, b): Span,
    fraction: f64,
    mns: &[f64],
    mnt: &[f64],
    state: &SurfaceState,
    slope: &[f64],
    o: &DeckDetectorOptions,
    w: &Windows,
) -> (DeckMetrics, Option<DeckRejection>) {
    let n = mns.len();
    let (mnh, mf, rough) = (&state.mnh, &state.mf, &state.rough);
    let short = b - a <= w.short_span;
    let h = w.slope_half;
    let raw_slopes: Vec<f64> =
        if short || b - a < 2 * h { Vec::new() } else { (a + h..=b - h).map(|k| slope[k].abs()).collect() };
    let clean_fraction = if short { f64::NAN } else { inner_clean_fraction(&state.clean, a, b, h) };
    let clean_slopes = if !short && o.slope_on_clean_deck {
        clean_deck_slopes(mns, mnh, &state.clean, clean_fraction, a, b, o, w)
    } else {
        None
    };
    let slopes = clean_slopes.as_deref().unwrap_or(&raw_slopes);
    let slope_p95 = p95_or_nan(slopes);
    let slope_max = if slopes.is_empty() { f64::NAN } else { max_or_nan(slopes) };
    let rough_on_tall: Vec<f64> = (a..=b).filter(|&k| mnh[k] > o.tall_mnh_m).map(|k| rough[k]).collect();
    let roughness = if rough_on_tall.is_empty() || rough_on_tall.iter().any(|v| v.is_nan()) {
        f64::NAN
    } else {
        median(&rough_on_tall)
    };
    // `minOf` Kotlin sur deux Double propage NaN, contrairement à `f64::min`.
    let outer_min = nan_min(mnt[a.saturating_sub(1)], mnt[(b + 1).min(n - 1)]);
    let span_mnt = &mnt[a..=b];
    let span_min = if span_mnt.iter().any(|v| v.is_nan()) {
        f64::NAN
    } else {
        span_mnt.iter().copied().fold(f64::INFINITY, f64::min)
    };
    let mnt_drop = outer_min - span_min;
    // L'extension s'est arrêtée sur un MNH NaN qu'elle n'a pu franchir : la rive est inconnue.
    let stopped_on_nan = (a > 0 && mnh[a - 1].is_nan()) || (b < n - 1 && mnh[b + 1].is_nan());
    let max_finite = |it: &mut dyn Iterator<Item = f64>| {
        it.filter(|v| !v.is_nan()).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))))
    };
    let metrics = DeckMetrics {
        length_m: (b - a + 1) as f64 * o.step_m,
        slope_p95,
        slope_max,
        raw_slope_p95: p95_or_nan(&raw_slopes),
        clean_fraction,
        slope_on_clean_deck: clean_slopes.is_some(),
        roughness_m: roughness,
        smooth_fraction: fraction,
        canyon_m: max_finite(&mut (a..=b).map(|k| mf[k] - mnt[k])).unwrap_or(f64::NAN),
        mnt_drop_m: mnt_drop,
        deck_top_m: max_finite(&mut (a..=b).map(|k| mf[k])).unwrap_or(f64::NAN),
    };
    let rejection = if stopped_on_nan || mnt_drop.is_nan() {
        Some(DeckRejection::NoData)
    } else if !(mnt_drop > o.min_canyon_drop_m) {
        Some(DeckRejection::NoCanyon)
    } else if short && roughness.is_nan() {
        Some(DeckRejection::NoData)
    } else if short {
        if roughness < o.max_short_roughness_m {
            None
        } else {
            Some(DeckRejection::ShortRough)
        }
    } else if slope_p95.is_nan() {
        Some(DeckRejection::NoData)
    } else if slope_p95 <= o.max_slope_p95 && (b - a) as f64 * o.step_m >= o.min_length_m {
        None
    } else {
        Some(DeckRejection::Steep)
    };
    (metrics, rejection)
}

fn nan_min(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        f64::NAN
    } else {
        x.min(y)
    }
}

/// Part de stations propres parmi `a + h ..= b − h`, NaN s'il n'y en a pas (`vc:G/DeckDetector.kt:500-505`).
fn inner_clean_fraction(clean: &[bool], a: usize, b: usize, h: usize) -> f64 {
    if b < a + 2 * h {
        return f64::NAN;
    }
    let count = (a + h..=b - h).filter(|&k| clean[k]).count();
    count as f64 / (b - a - 2 * h + 1) as f64
}

/// |pente| sur `a + h ..= b − h` du MNS ponté à travers ses stations non propres, ou `None` pour
/// lire le profil brut (`vc:G/DeckDetector.kt:524-559`).
#[allow(clippy::too_many_arguments)]
fn clean_deck_slopes(
    mns: &[f64],
    mnh: &[f64],
    clean: &[bool],
    clean_fraction: f64,
    a: usize,
    b: usize,
    o: &DeckDetectorOptions,
    w: &Windows,
) -> Option<Vec<f64>> {
    let n = mns.len();
    let h = w.slope_half;
    if b < a + 2 * h {
        return None;
    }
    let in_span: Vec<usize> = (a..=b).filter(|&k| clean[k]).collect();
    if in_span.len() < o.min_clean_stations_count {
        return None;
    }
    if !(clean_fraction >= o.min_clean_fraction) {
        return None;
    }
    let median_half = w.surface_median / 2;
    let lo = a.saturating_sub(median_half);
    let hi = (b + median_half).min(n - 1);
    let mut x = mns[lo..=hi].to_vec();
    for pair in in_span.windows(2) {
        let (c0, c1) = (pair[0], pair[1]);
        if (c0 + 1..c1).any(|k| mnh[k] <= o.clean_gap_ground_mnh_m) {
            continue;
        }
        for k in c0 + 1..c1 {
            x[k - lo] = mns[c0] + (mns[c1] - mns[c0]) * (k - c0) as f64 / (c1 - c0) as f64;
        }
    }
    let mf_clean = rolling_nan_median(&x, median_half);
    let denom = (2 * h) as f64 * o.step_m;
    Some((a + h..=b - h).map(|k| ((mf_clean[k + h - lo] - mf_clean[k - h - lo]) / denom).abs()).collect())
}

/// Marche depuis `from` dans le sens `dir` tant que le MNH suivant dépasse `threshold` ; franchit
/// un îlot NaN d'au plus `max_island` stations ; au-delà de `max_steps`, rend `from`
/// (`vc:G/DeckDetector.kt:567-591`).
fn extend(
    mnh: &[f64],
    from: usize,
    dir: isize,
    threshold: f64,
    max_island: usize,
    max_steps: usize,
) -> usize {
    let n = mnh.len() as isize;
    let inside = |k: isize| (0..n).contains(&k);
    let ok = |k: isize| inside(k) && mnh[k as usize] > threshold;
    let mut k = from as isize;
    loop {
        let next = k + dir;
        if ok(next) {
            k = next;
            continue;
        }
        if !inside(next) || !mnh[next as usize].is_nan() {
            break;
        }
        let mut end = next;
        while inside(end + dir) && mnh[(end + dir) as usize].is_nan() {
            end += dir;
        }
        let width = (end - next).unsigned_abs() + 1;
        if width <= max_island && ok(end + dir) {
            k = end + dir;
        } else {
            break;
        }
    }
    if (k - from as isize).unsigned_abs() > max_steps {
        from
    } else {
        k as usize
    }
}

/// Pose `mask` sur tout trou d'au plus `max_hole` stations, borné dans `span`, où `fill` tient
/// (`vc:G/DeckDetector.kt:597-615`).
fn close_holes(mask: &mut [bool], (first, last): Span, max_hole: usize, fill: impl Fn(usize) -> bool) {
    let mut k = first;
    while k <= last {
        if mask[k] {
            k += 1;
            continue;
        }
        let mut end = k;
        while end < last && !mask[end + 1] {
            end += 1;
        }
        let bounded = k > first && end < last;
        if bounded && end - k < max_hole && (k..=end).all(&fill) {
            mask[k..=end].fill(true);
        }
        k = end + 1;
    }
}

/// RMS glissant de `MNS − mf` (`vc:G/DeckDetector.kt:617-625`).
fn roughness(mns: &[f64], mf: &[f64], half: usize) -> Vec<f64> {
    let sq: Vec<f64> = mns.iter().zip(mf).map(|(s, m)| (s - m) * (s - m)).collect();
    rolling_mean(&sq, half).into_iter().map(f64::sqrt).collect()
}

/// Indice `k + j − half` borné à `[0, n − 1]`.
fn clamped(k: usize, j: usize, half: usize, n: usize) -> usize {
    (k + j).saturating_sub(half).min(n - 1)
}

/// Médiane glissante centrée sur `2·half + 1` stations, indices bornés, NaN ignorés ; tout NaN
/// donne NaN, un compte pair la moyenne de la paire centrale (`vc:G/DeckDetector.kt:632-651`).
fn rolling_nan_median(x: &[f64], half: usize) -> Vec<f64> {
    let n = x.len();
    let mut window = Vec::with_capacity(2 * half + 1);
    (0..n)
        .map(|k| {
            window.clear();
            window.extend((0..=2 * half).map(|j| x[clamped(k, j, half, n)]).filter(|v| !v.is_nan()));
            let m = window.len();
            if m == 0 {
                return f64::NAN;
            }
            window.sort_by(f64::total_cmp);
            if m % 2 == 1 {
                window[m / 2]
            } else {
                (window[m / 2 - 1] + window[m / 2]) / 2.0
            }
        })
        .collect()
}

/// Moyenne glissante centrée, indices bornés, NaN propagé (`vc:G/DeckDetector.kt:654-664`).
fn rolling_mean(x: &[f64], half: usize) -> Vec<f64> {
    let n = x.len();
    (0..n)
        .map(|k| {
            let mut s = 0.0;
            for j in 0..=2 * half {
                s += x[clamped(k, j, half, n)];
            }
            s / (2 * half + 1) as f64
        })
        .collect()
}

/// Maximum glissant centré, indices bornés, NaN propagé (`vc:G/DeckDetector.kt:667-681`).
fn rolling_max(x: &[f64], half: usize) -> Vec<f64> {
    let n = x.len();
    (0..n)
        .map(|k| {
            let mut m = f64::NEG_INFINITY;
            for j in 0..=2 * half {
                let v = x[clamped(k, j, half, n)];
                if v.is_nan() {
                    return f64::NAN;
                }
                if v > m {
                    m = v;
                }
            }
            m
        })
        .collect()
}

/// `(x[k+h] − x[k−h]) / (2h·pas)`, indices bornés (`vc:G/DeckDetector.kt:684-691`).
fn centred_slope(x: &[f64], h: usize, step_m: f64) -> Vec<f64> {
    let n = x.len();
    (0..n).map(|k| (x[(k + h).min(n - 1)] - x[k.saturating_sub(h)]) / ((2 * h) as f64 * step_m)).collect()
}

/// Suites de `true`, bornes incluses (`vc:G/DeckDetector.kt:694-706`).
fn runs(mask: &[bool]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, &m) in mask.iter().enumerate() {
        match (m, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i - 1));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, mask.len() - 1));
    }
    out
}

/// Joint chaque suite à la précédente quand moins de `gap` stations les séparent
/// (`vc:G/DeckDetector.kt:712-726`).
fn merge_runs(rs: &[Span], gap: usize) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    for &r in rs {
        match out.last_mut() {
            Some(prev) if r.0 - prev.1 - 1 < gap => prev.1 = prev.1.max(r.1),
            _ => out.push(r),
        }
    }
    out
}

/// Percentile linéaire de numpy, `q` dans [0, 1] (`vc:G/DeckDetector.kt:729-738`).
fn percentile_linear(values: &[f64], q: f64) -> f64 {
    let mut s = values.to_vec();
    s.sort_by(f64::total_cmp);
    let rank = (s.len() - 1) as f64 * q;
    let lo = rank.floor() as usize;
    let hi = (lo + 1).min(s.len() - 1);
    s[lo] + (s[hi] - s[lo]) * (rank - lo as f64)
}

fn median(values: &[f64]) -> f64 {
    let mut s = values.to_vec();
    s.sort_by(f64::total_cmp);
    let m = s.len();
    if m % 2 == 1 {
        s[m / 2]
    } else {
        (s[m / 2 - 1] + s[m / 2]) / 2.0
    }
}

fn p95_or_nan(values: &[f64]) -> f64 {
    if values.is_empty() || values.iter().any(|v| v.is_nan()) {
        f64::NAN
    } else {
        percentile_linear(values, 0.95)
    }
}

fn max_or_nan(values: &[f64]) -> f64 {
    if values.iter().any(|v| v.is_nan()) {
        f64::NAN
    } else {
        values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
    }
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    //! Profils synthétiques au pas de 5 m, portés de `vc:G/DeckDetectorTest.kt`.

    use std::f64::consts::PI;

    use super::*;

    fn d() -> DeckDetectorOptions {
        DeckDetectorOptions::default()
    }

    /// Parcours de `n` stations sur sol plat : MNS = MNT, MNH = 0.
    #[derive(Clone)]
    struct Route {
        mnt: Vec<f64>,
        mns: Vec<f64>,
    }

    impl Route {
        fn new(n: usize, ground: f64) -> Self {
            Self { mnt: vec![ground; n], mns: vec![ground; n] }
        }

        /// Tablier sur `range` à `top(k)`, sol dessous à `under`.
        fn deck(mut self, range: RangeInclusive<usize>, under: f64, top: impl Fn(usize) -> f64) -> Self {
            for k in range {
                self.mnt[k] = under;
                self.mns[k] = top(k);
            }
            self
        }

        fn mnh(&self) -> Vec<f64> {
            self.mns.iter().zip(&self.mnt).map(|(s, t)| s - t).collect()
        }
    }

    fn detect(r: &Route) -> DeckDetection {
        detect_with(r, &d())
    }

    fn detect_with(r: &Route, o: &DeckDetectorOptions) -> DeckDetection {
        detect_decks(&r.mnt, &r.mns, o)
    }

    fn only(det: &DeckDetection) -> DeckCandidate {
        let all: Vec<&DeckCandidate> = det.accepted.iter().chain(&det.rejected).collect();
        assert_eq!(all.len(), 1, "expected one candidate, got {all:?}");
        all[0].clone()
    }

    fn span(c: &DeckCandidate) -> RangeInclusive<usize> {
        c.first..=c.last
    }

    fn spans(cs: &[DeckCandidate]) -> Vec<RangeInclusive<usize>> {
        cs.iter().map(span).collect()
    }

    fn metrics(c: &DeckCandidate) -> &DeckMetrics {
        c.metrics.as_ref().expect("metrics")
    }

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} vs {b}");
    }

    /// Pont sur une rivière à 2 m, rives à 10 m, tablier plat à 12 m de 55 à 124.
    fn canyon_bridge() -> Route {
        Route::new(200, 10.0).deck(55..=124, 10.0, |_| 12.0).deck(60..=119, 2.0, |_| 12.0)
    }

    fn cambered_deck(end_grade: f64, piles: bool) -> Route {
        let half = 35.0 * 5.0;
        let mut r = Route::new(200, 10.0).deck(55..=124, 2.0, |k| {
            let x = (k as f64 - 89.5) * 5.0;
            30.0 - end_grade / (2.0 * half) * (x * x)
        });
        if piles {
            for k in [74, 75, 94, 95, 114, 115] {
                r.mnt[k] = r.mns[k] - 2.0;
            }
        }
        r
    }

    fn crown(k: usize) -> f64 {
        (((k as i64 * 7919) % 13 - 6) as f64) * 0.5
    }

    /// Bruit déterministe dans [0, 1) (hachage multiplicatif de Knuth).
    fn noise(k: usize) -> f64 {
        ((k as i64 * 2_654_435_761_i64) % 4_294_967_296_i64) as f64 / 4_294_967_296.0
    }

    // ── Options ──

    #[test]
    fn default_windows_are_the_reference_station_counts() {
        let w = d().windows().unwrap();
        assert_eq!(
            w,
            Windows {
                min_tall_run: 3,
                surface_median: 5,
                roughness: 7,
                merge_gap: 60,
                slope_half: 2,
                short_span: 5,
                deck_min_run: 8,
                max_abutment: 20,
                small_span: 12,
            }
        );
        assert_eq!(w.max_deck_gap(), 2);
    }

    #[test]
    fn invalid_options_are_refused() {
        let bad = [
            DeckDetectorOptions { step_m: 4.0, ..d() },
            DeckDetectorOptions { surface_median_window_m: 20.0, ..d() },
            DeckDetectorOptions { roughness_window_m: 30.0, ..d() },
            DeckDetectorOptions { step_m: 0.0, ..d() },
            DeckDetectorOptions { short_span_m: 0.0, ..d() },
            DeckDetectorOptions { short_span_m: 10.0, ..d() },
            DeckDetectorOptions { max_slope_p95: f64::NAN, ..d() },
            DeckDetectorOptions { tall_mnh_m: f64::INFINITY, ..d() },
            DeckDetectorOptions { max_abutment_m: -5.0, ..d() },
            DeckDetectorOptions { clean_gap_ground_mnh_m: f64::NAN, ..d() },
            DeckDetectorOptions { min_clean_fraction: 1.5, ..d() },
            DeckDetectorOptions { min_clean_fraction: f64::NAN, ..d() },
            DeckDetectorOptions { min_clean_stations_count: 1, ..d() },
            DeckDetectorOptions { mnt_above_mns_tolerance_m: f64::NAN, ..d() },
            DeckDetectorOptions { mnt_above_mns_tolerance_m: -5.0, ..d() },
        ];
        for o in &bad {
            assert!(o.validate().is_err(), "{o:?}");
        }
        assert!(d().validate().is_ok());
        assert!(DeckDetectorOptions { short_span_m: 15.0, ..d() }.validate().is_ok());
        assert_eq!(
            DeckDetectorOptions::python(),
            DeckDetectorOptions {
                slope_on_clean_deck: false,
                mnt_above_mns_as_tall: false,
                split_steep_merged: false,
                ..d()
            }
        );
        // Options invalides : détection vide plutôt qu'une panique.
        let o = DeckDetectorOptions { step_m: 4.0, ..d() };
        assert_eq!(detect_with(&canyon_bridge(), &o), DeckDetection::default());
    }

    // ── Route ouverte, pont sur canyon ──

    #[test]
    fn a_flat_open_road_has_nothing_to_say() {
        assert_eq!(detect(&Route::new(300, 10.0)), DeckDetection::default());
    }

    #[test]
    fn a_flat_bridge_over_a_canyon_is_accepted_out_to_its_abutments() {
        let c = only(&detect(&canyon_bridge()));
        assert_eq!(c.rejection, None);
        assert_eq!(span(&c), 55..=124);
        close(c.start_m(5.0), 275.0, 1e-12);
        close(c.end_m(5.0), 620.0, 1e-12);
        let m = metrics(&c);
        close(m.length_m, 350.0, 1e-12);
        close(m.slope_p95, 0.0, 1e-12);
        close(m.roughness_m, 0.0, 1e-12);
        close(m.smooth_fraction, 1.0, 1e-12);
        close(m.canyon_m, 10.0, 1e-12);
        close(m.mnt_drop_m, 8.0, 1e-12);
        close(m.deck_top_m, 12.0, 1e-12);
        assert_eq!(c.deck_ranges, vec![55..=124]);
    }

    // ── Pente ──

    #[test]
    fn a_cambered_deck_with_piles_is_accepted_and_a_steeper_one_is_not() {
        let det = detect(&cambered_deck(0.059, true));
        assert_eq!(det.accepted.len(), 1);
        let c = &det.accepted[0];
        assert_eq!(span(c), 55..=124);
        let m = metrics(c);
        assert!((0.045..=0.059).contains(&m.slope_p95), "P95 {}", m.slope_p95);
        assert!(m.slope_max < 0.059, "max {}", m.slope_max);
        assert!(det.rejected.is_empty(), "{:?}", det.rejected);

        let c = only(&detect(&cambered_deck(0.12, false)));
        assert_eq!(c.rejection, Some(DeckRejection::Steep));
        assert!(metrics(&c).slope_p95 > 0.10);
        assert!(c.deck_ranges.is_empty());
    }

    #[test]
    fn a_ten_percent_deck_sits_on_the_inclusive_boundary() {
        let ramp =
            |per: f64| Route::new(200, 10.0).deck(55..=124, 2.0, move |k| 30.0 + per * (k - 55) as f64);
        let at = only(&detect(&ramp(0.5)));
        close(metrics(&at).slope_p95, 0.10, 1e-12);
        assert_eq!(at.rejection, None);
        assert_eq!(only(&detect(&ramp(0.505))).rejection, Some(DeckRejection::Steep));
    }

    // ── Canopée, canyon, longueurs ──

    #[test]
    fn a_rough_canopy_is_rejected_as_one_merged_group_never_measured() {
        let mut r = Route::new(220, 10.0);
        for k in 0..220 {
            r.mnt[k] = 100.0 + 0.1 * k as f64;
            r.mns[k] = r.mnt[k];
        }
        for k in (50..=89).chain(100..=139) {
            r.mns[k] = r.mnt[k] + 15.0 + crown(k);
        }
        let det = detect(&r);
        assert!(det.accepted.is_empty());
        assert_eq!(det.rejected.len(), 1);
        let c = &det.rejected[0];
        assert_eq!(c.rejection, Some(DeckRejection::Rough));
        assert_eq!(span(c), 50..=139);
        assert_eq!(c.metrics, None);
    }

    #[test]
    fn an_elevated_span_without_drop_is_no_bridge_and_the_margin_is_strict() {
        let roof = |under: f64| Route::new(200, 50.0).deck(50..=89, under, |_| 60.0);
        let flat = only(&detect(&roof(50.0)));
        assert_eq!(flat.rejection, Some(DeckRejection::NoCanyon));
        close(metrics(&flat).mnt_drop_m, 0.0, 1e-12);
        assert_eq!(only(&detect(&roof(47.0))).rejection, Some(DeckRejection::NoCanyon));
        assert_eq!(only(&detect(&roof(46.99))).rejection, None);
    }

    #[test]
    fn a_tall_run_needs_three_stations() {
        let two = detect(&Route::new(100, 10.0).deck(50..=51, 0.0, |_| 20.0));
        assert!(two.accepted.is_empty() && two.rejected.is_empty());
        assert_eq!(two.tall_runs_too_short, 1);
        let three = detect(&Route::new(100, 10.0).deck(50..=52, 0.0, |_| 20.0));
        assert_eq!(three.tall_runs_too_short, 0);
        assert_eq!(span(&only(&three)), 50..=52);
    }

    #[test]
    fn a_run_is_kept_at_exactly_sixty_percent_smooth_stations() {
        let with_dip = |dip: usize| {
            let mut r = Route::new(200, 10.0).deck(100..=104, 10.0, |_| 20.0);
            r.mns[dip] = -10.0;
            only(&detect(&r))
        };
        let kept = with_dip(106);
        assert_eq!(kept.rejection, Some(DeckRejection::NoCanyon));
        close(metrics(&kept).smooth_fraction, 0.6, 1e-12);
        assert_eq!(with_dip(105).rejection, Some(DeckRejection::Rough));
    }

    // ── Fusion ──

    fn two_spans(missing: usize) -> Route {
        let first_end = 69;
        let second_start = first_end + missing + 1;
        let mut r = Route::new(300, 10.0).deck(50..=second_start + 19, 2.0, |_| 12.0);
        for k in first_end + 1..second_start {
            r.mnt[k] = 10.0;
        }
        r.mnt[(first_end + second_start) / 2] = 11.5;
        r
    }

    fn long_abutments() -> DeckDetectorOptions {
        DeckDetectorOptions { max_abutment_m: 1000.0, ..d() }
    }

    #[test]
    fn smooth_runs_merge_across_fewer_than_sixty_missing_stations() {
        let merged = detect_with(&two_spans(59), &long_abutments());
        assert_eq!(spans(&merged.accepted), vec![50..=148]);
        let apart = detect_with(&two_spans(60), &long_abutments());
        assert_eq!(spans(&apart.accepted), vec![50..=98, 100..=149]);
    }

    // ── Portées courtes ──

    fn short_deck(noise: f64, under: f64) -> Route {
        Route::new(100, 10.0).deck(50..=54, under, move |k| 20.0 + if k % 2 == 0 { noise } else { -noise })
    }

    #[test]
    fn a_short_span_is_judged_on_its_roughness() {
        let smooth = only(&detect(&short_deck(0.1, 2.0)));
        assert_eq!(smooth.rejection, None, "{:?}", smooth.metrics);
        let m = metrics(&smooth);
        assert!(m.roughness_m < 0.4, "{}", m.roughness_m);
        assert!(m.slope_p95.is_nan() && m.slope_max.is_nan());
        let rough = only(&detect(&short_deck(0.45, 2.0)));
        assert_eq!(rough.rejection, Some(DeckRejection::ShortRough));
        assert!((0.4..=0.8).contains(&metrics(&rough).roughness_m));
        assert_eq!(only(&detect(&short_deck(0.1, 10.0))).rejection, Some(DeckRejection::NoCanyon));
    }

    #[test]
    fn a_deck_whose_edge_alternates_passes_at_0_8_not_at_0_35() {
        let r = Route::new(200, 10.0).deck(55..=124, 2.0, |k| if k % 3 == 0 { 11.0 } else { 12.0 });
        let c = only(&detect(&r));
        assert_eq!(c.rejection, None);
        let rough = metrics(&c).roughness_m;
        assert!(rough > 0.35 && rough < 0.8, "roughness {rough}");
        let strict = DeckDetectorOptions { max_roughness_m: 0.35, ..d() };
        assert_eq!(only(&detect_with(&r, &strict)).rejection, Some(DeckRejection::Rough));
    }

    // ── Bords ──

    #[test]
    fn a_deck_touching_either_end_reads_its_own_end_mnt_as_the_outer_bank() {
        let start = only(&detect(&Route::new(100, 10.0).deck(0..=29, 2.0, |_| 12.0)));
        assert_eq!(span(&start), 0..=29);
        assert_eq!(start.rejection, Some(DeckRejection::NoCanyon));
        let end = only(&detect(&Route::new(100, 10.0).deck(70..=99, 2.0, |_| 12.0)));
        assert_eq!(span(&end), 70..=99);
        assert_eq!(end.rejection, Some(DeckRejection::NoCanyon));
        let mns: Vec<f64> = (0..30).map(|k| if k < 19 { 106.0 } else { 100.0 }).collect();
        let mnt: Vec<f64> = (0..30).map(|k| if (1..=18).contains(&k) { 90.0 } else { 100.0 }).collect();
        let clamped = only(&detect_decks(&mnt, &mns, &d()));
        assert_eq!(span(&clamped), 0..=18);
        assert_eq!(clamped.rejection, None);
        close(metrics(&clamped).mnt_drop_m, 10.0, 1e-12);
    }

    // ── Pas de donnée ──

    #[test]
    fn no_data_outside_the_coverage_is_no_detection() {
        let nan = vec![f64::NAN; 100];
        assert_eq!(detect_profile(&nan, &nan, &nan, &d()), DeckDetection::default());
        assert_eq!(detect_decks(&[], &[], &d()), DeckDetection::default());
    }

    #[test]
    fn an_mns_hole_inside_a_deck_is_bridged_by_the_nan_median() {
        let mut r = canyon_bridge();
        let mnh = r.mnh();
        for k in 80..=82 {
            r.mns[k] = f64::NAN;
        }
        let c = only(&detect_profile(&mnh, &r.mns, &r.mnt, &d()));
        assert_eq!(c.rejection, None);
        assert_eq!(span(&c), 55..=124);
        let m = metrics(&c);
        close(m.slope_p95, 0.0, 1e-12);
        assert!(m.roughness_m.is_nan());
        close(m.smooth_fraction, 17.0 / 20.0, 1e-12);
    }

    #[test]
    fn an_mnt_hole_on_the_span_is_no_data() {
        let mut r = canyon_bridge();
        r.mnt[90] = f64::NAN;
        let mnh: Vec<f64> = (0..r.mns.len())
            .map(|k| if (60..=119).contains(&k) { 10.0 } else { r.mns[k] - r.mnt[k] })
            .collect();
        assert_eq!(only(&detect_profile(&mnh, &r.mns, &r.mnt, &d())).rejection, Some(DeckRejection::NoData));
    }

    #[test]
    fn a_nan_island_of_two_stations_in_the_extension_is_stepped_over() {
        let r = canyon_bridge();
        let mut mnh = r.mnh();
        mnh[56] = f64::NAN;
        mnh[57] = f64::NAN;
        let c = only(&detect_profile(&mnh, &r.mns, &r.mnt, &d()));
        assert_eq!(span(&c), 55..=124);
        assert_eq!(c.rejection, None);
    }

    #[test]
    fn a_wider_nan_stops_the_extension_and_the_span_is_no_data() {
        let mut r = canyon_bridge();
        for k in 56..=58 {
            r.mns[k] = f64::NAN;
        }
        let c = only(&detect(&r));
        assert_eq!(span(&c), 59..=124);
        assert_eq!(c.rejection, Some(DeckRejection::NoData));
    }

    #[test]
    fn one_no_data_mns_station_over_the_river_keeps_the_bridge() {
        for hole in [62, 70, 117] {
            let mut r = canyon_bridge();
            r.mns[hole] = f64::NAN;
            let det = detect(&r);
            assert_eq!(det.accepted.len(), 1, "hole at {hole}");
            assert_eq!(span(&det.accepted[0]), 55..=124, "hole at {hole}");
            assert_eq!(det.accepted[0].deck_ranges, vec![55..=124], "hole at {hole}");
            assert!(det.rejected.is_empty(), "hole at {hole}: {:?}", det.rejected);
        }
    }

    #[test]
    fn a_low_outlier_is_closed_in_the_deck_mask_a_three_station_hole_is_not() {
        let mut low = canyon_bridge();
        low.mns[90] = low.mnt[90] - 1e6;
        assert_eq!(detect(&low).accepted[0].deck_ranges, vec![55..=124]);
        let mut wide = canyon_bridge();
        for k in 70..=72 {
            wide.mns[k] = f64::NAN;
        }
        assert_eq!(detect(&wide).accepted[0].deck_ranges, vec![55..=69, 73..=124]);
    }

    #[test]
    fn tall_runs_with_a_valid_mnh_but_no_mns_are_no_data() {
        let nan = vec![f64::NAN; 3];
        let c = only(&detect_profile(&[10.0; 3], &nan, &nan, &d()));
        assert_eq!(c.rejection, Some(DeckRejection::NoData));
        assert_eq!(c.metrics, None);
    }

    // ── Chevauchement, masque, culées ──

    #[test]
    fn candidates_whose_extensions_overlap_become_one_span() {
        let mut r = Route::new(300, 10.0).deck(50..=169, 2.0, |_| 12.0);
        for k in 70..=149 {
            r.mnt[k] = 10.0;
        }
        let det = detect_with(&r, &long_abutments());
        assert_eq!(spans(&det.accepted), vec![50..=169]);
        assert!(det.rejected.is_empty());
    }

    fn deck_with_blip(
        a: RangeInclusive<usize>,
        blip: RangeInclusive<usize>,
        b: RangeInclusive<usize>,
    ) -> Route {
        let mut r = Route::new(200, 10.0).deck(*a.start()..=*b.end(), 19.5, |_| 20.0);
        for k in a.chain(b) {
            r.mnt[k] = 2.0;
        }
        for k in blip {
            r.mnt[k] = 18.0;
        }
        r
    }

    #[test]
    fn the_deck_mask_keeps_long_runs_or_every_run_on_a_small_span() {
        let long = only(&detect(&deck_with_blip(100..=107, 115..=117, 125..=132)));
        assert_eq!(long.rejection, None);
        assert_eq!(span(&long), 100..=132);
        assert_eq!(long.deck_ranges, vec![100..=107, 125..=132]);
        let small = only(&detect(&deck_with_blip(100..=102, 106..=106, 110..=112)));
        assert_eq!(small.rejection, None);
        assert_eq!(small.deck_ranges, vec![100..=102, 106..=106, 110..=112]);
        let closed = only(&detect(&deck_with_blip(100..=102, 105..=105, 108..=110)));
        assert_eq!(closed.deck_ranges, vec![100..=110]);
    }

    fn bridge_then_field(field: usize) -> Route {
        let mut r = Route::new(800, 10.0).deck(100..=139, 2.0, |_| 12.0);
        for k in 140..140 + field {
            r.mns[k] = r.mnt[k] + 2.0;
        }
        r
    }

    #[test]
    fn an_abutment_is_at_most_100_m() {
        assert_eq!(span(&detect(&bridge_then_field(20)).accepted[0]), 100..=159);
        let field = detect(&bridge_then_field(21));
        assert_eq!(span(&field.accepted[0]), 100..=139);
        assert_eq!(field.accepted[0].deck_ranges, vec![100..=139]);
        assert_eq!(detect(&bridge_then_field(400)).accepted[0].deck_ranges, vec![100..=139]);
    }

    #[test]
    fn known_limits_roof_borrows_a_canyon_and_trees_share_a_run() {
        let mut r = Route::new(700, 10.0).deck(100..=299, 2.0, |_| 12.0);
        for k in 340..=351 {
            r.mns[k] = 16.0;
        }
        let c = &detect(&r).accepted[0];
        assert_eq!(span(c), 100..=351);
        assert_eq!(c.deck_ranges, vec![100..=299, 340..=351]);

        let mut t = Route::new(500, 10.0).deck(100..=299, 2.0, |_| 12.0);
        for k in 300..=315 {
            t.mns[k] = 22.0 + crown(k);
        }
        assert_eq!(span(&detect(&t).accepted[0]), 100..=315);
    }

    // ── E2 ter : ponts raides ──

    fn steep_bridge(
        deck_stations: usize,
        under: impl Fn(usize) -> f64,
        right: f64,
        top: impl Fn(usize) -> f64,
    ) -> Route {
        let mut r = Route::new(deck_stations + 60, 0.0);
        for k in 30 + deck_stations..r.mns.len() {
            r.mns[k] = right;
            r.mnt[k] = right;
        }
        let mut r = r.deck(30..=29 + deck_stations, 0.0, |k| top(k - 30));
        for k in 30..30 + deck_stations {
            r.mnt[k] = under(k - 30);
        }
        r
    }

    fn default_top(j: usize) -> f64 {
        45.0 - 0.3 * (j as f64 - 150.0).abs()
    }

    #[test]
    fn edge_mixing_sag_is_bridged() {
        let mut r = steep_bridge(300, |_| -8.0, 0.0, default_top);
        for run in [80..=99, 140..=164, 210..=234] {
            for k in run {
                if k % 2 == 1 {
                    r.mns[k] -= 5.0 + 10.0 * noise(k);
                }
            }
        }
        let python = detect_with(&r, &DeckDetectorOptions::python());
        assert_eq!(python.rejected.len(), 1);
        assert_eq!(python.rejected[0].rejection, Some(DeckRejection::Steep));
        let det = detect(&r);
        assert!(det.rejected.is_empty(), "{:?}", det.rejected);
        let c = &det.accepted[0];
        assert_eq!(span(c), 30..=329);
        let m = metrics(c);
        assert!(m.slope_on_clean_deck);
        assert!(m.slope_p95 <= 0.07, "bridged P95 {}", m.slope_p95);
        assert!(m.raw_slope_p95 > 0.10, "raw P95 {}", m.raw_slope_p95);
        assert!((0.25..=1.0).contains(&m.clean_fraction), "clean {}", m.clean_fraction);
    }

    #[test]
    fn one_sided_hole_band_is_bridged() {
        let mut r = steep_bridge(300, |_| -80.0, 44.85, |j| 0.15 * j as f64);
        for k in 150..=249 {
            if k % 2 == 0 {
                r.mns[k] -= 10.0 + 50.0 * noise(k);
            }
        }
        let det = detect(&r);
        let m = metrics(&det.accepted[0]);
        assert!(m.slope_on_clean_deck);
        close(m.slope_p95, 0.03, 0.005);
        assert!(m.raw_slope_p95 > 0.10, "raw P95 {}", m.raw_slope_p95);
    }

    #[test]
    fn pylon_spikes_and_an_impossible_mnt() {
        let mut r = steep_bridge(400, |_| -8.0, 0.0, |j| 30.0 - 0.15 * (j as f64 - 200.0).abs());
        for k in (100..=139).chain(160..=199) {
            r.mns[k] += 10.0 + 35.0 * noise(k);
        }
        for k in 225..=230 {
            r.mns[k] -= 25.0;
        }
        for k in 330..=409 {
            r.mnt[k] = r.mns[k] + 40.0;
        }
        let python = detect_with(&r, &DeckDetectorOptions::python());
        let why: Vec<_> = python.rejected.iter().map(|c| c.rejection).collect();
        assert_eq!(why, vec![Some(DeckRejection::Steep)]);
        assert_eq!(spans(&python.accepted), vec![410..=429]);
        let split = detect_with(&r, &DeckDetectorOptions { mnt_above_mns_as_tall: false, ..d() });
        assert_eq!(spans(&split.accepted), vec![30..=329, 410..=429]);
        let det = detect(&r);
        assert!(det.rejected.is_empty(), "{:?}", det.rejected);
        assert_eq!(det.accepted.len(), 1);
        let c = &det.accepted[0];
        assert_eq!(span(c), 30..=429);
        assert_eq!(c.deck_ranges, vec![30..=429]);
        assert!(metrics(c).slope_p95 <= 0.035, "P95 {}", metrics(c).slope_p95);
    }

    #[test]
    fn bridging_measures_the_step_across_a_rough_gap() {
        let r = steep_bridge(
            130,
            |_| -8.0,
            30.0,
            |j| {
                if j < 60 {
                    10.0
                } else if j < 70 {
                    15.0 + 10.0 * noise(j)
                } else {
                    30.0
                }
            },
        );
        let c = only(&detect(&r));
        assert_eq!(c.rejection, Some(DeckRejection::Steep));
        let m = metrics(&c);
        assert!(m.slope_on_clean_deck);
        assert!(m.slope_p95 > 0.10, "P95 {}", m.slope_p95);
    }

    #[test]
    fn a_rough_tail_is_not_bridged() {
        let r = steep_bridge(240, |_| -8.0, 0.0, |j| if j < 200 { 10.0 } else { 18.0 + 15.0 * noise(j) });
        let c = only(&detect(&r));
        assert_eq!(c.rejection, Some(DeckRejection::Steep), "{c:?}");
        let m = metrics(&c);
        assert!(m.slope_on_clean_deck);
        assert!(m.slope_p95 > 0.10, "P95 {}", m.slope_p95);
    }

    #[test]
    fn too_few_clean_stations_falls_back_to_the_raw_profile() {
        let r = steep_bridge(
            70,
            |_| -8.0,
            0.0,
            |j| if !(10..60).contains(&j) { 0.0 } else { -7.5 + 4.0 * noise(j) },
        );
        let whole = DeckDetectorOptions { split_steep_merged: false, ..d() };
        let c = only(&detect_with(&r, &whole));
        let m = metrics(&c);
        assert!(m.clean_fraction < 0.25, "clean {}", m.clean_fraction);
        assert!(!m.slope_on_clean_deck);
        close(m.slope_p95, m.raw_slope_p95, 1e-12);
        assert_eq!(c.rejection, Some(DeckRejection::Steep));
        let floorless = only(&detect_with(&r, &DeckDetectorOptions { min_clean_fraction: 0.0, ..whole }));
        let fm = metrics(&floorless);
        assert!(fm.slope_on_clean_deck);
        close(fm.slope_p95, fm.raw_slope_p95, 1e-12);
        assert_eq!(floorless.rejection, Some(DeckRejection::Steep));
        let split = detect(&r);
        assert!(split.accepted.is_empty(), "{:?}", split.accepted);
        let why: Vec<_> = split.rejected.iter().map(|c| c.rejection).collect();
        assert_eq!(why, vec![Some(DeckRejection::NoCanyon); 2]);

        // p08 : un cœur de 8 stations et une culée rugueuse mais plate sous la médiane.
        let cycle = [1.8, 0.0, -1.8];
        let s = steep_bridge(
            16,
            |j| if j < 8 { -8.0 } else { 7.0 },
            10.0,
            |j| {
                if j < 8 {
                    10.0
                } else {
                    10.0 + cycle[(j - 8) % 3]
                }
            },
        );
        let p = only(&detect(&s));
        assert!(!metrics(&p).slope_on_clean_deck, "{p:?}");
        assert_eq!(p.rejection, None, "{:?}", p.metrics);
    }

    #[test]
    fn known_limit_a_smooth_long_sag_stays_in_the_slope() {
        let r = steep_bridge(
            300,
            |_| -8.0,
            0.0,
            |j| {
                if (100..140).contains(&j) {
                    30.0 - 5.0 * (1.0 - (PI * (j as f64 - 100.0) / 20.0).cos())
                } else {
                    30.0
                }
            },
        );
        let c = only(&detect(&r));
        let m = metrics(&c);
        close(m.clean_fraction, 1.0, 1e-12);
        close(m.slope_p95, m.raw_slope_p95, 1e-12);
    }

    fn wooded_ravine() -> Route {
        let mut r = Route::new(260, 10.0);
        for k in (60..=89).chain(145..=174) {
            r = r.deck(k..=k, 10.0, |_| 18.0);
        }
        for k in 90..=144 {
            let t = 10.0 - 20.0 * (PI * (k as f64 - 90.0) / 55.0).sin();
            r.mnt[k] = t;
            r.mns[k] = t + 15.0 + 4.0 * noise(k);
        }
        for k in (91..=93).chain(141..=143) {
            r.mns[k] = r.mnt[k];
        }
        r
    }

    #[test]
    fn a_canopy_reaching_the_ground_between_two_flat_roofs_is_not_bridged() {
        let r = wooded_ravine();
        let measured = |det: &DeckDetection| -> DeckCandidate {
            let all: Vec<_> =
                det.accepted.iter().chain(&det.rejected).filter(|c| c.metrics.is_some()).collect();
            assert_eq!(all.len(), 1, "{all:?}");
            all[0].clone()
        };
        assert_eq!(
            measured(&detect_with(&r, &DeckDetectorOptions::python())).rejection,
            Some(DeckRejection::Steep)
        );
        let whole = DeckDetectorOptions { split_steep_merged: false, ..d() };
        let c = measured(&detect_with(&r, &whole));
        assert_eq!(span(&c), 60..=174);
        let m = metrics(&c);
        assert!(m.slope_on_clean_deck, "{m:?}");
        assert!(m.clean_fraction >= 0.25, "clean {}", m.clean_fraction);
        assert_eq!(c.rejection, Some(DeckRejection::Steep), "{m:?}");
        assert!(m.slope_p95 > 0.10, "P95 {}", m.slope_p95);
        close(m.slope_p95, m.raw_slope_p95, 1e-12);
        let det = detect(&r);
        assert!(det.accepted.is_empty(), "{:?}", det.accepted);
        let measured: Vec<_> = det.rejected.iter().filter(|c| c.metrics.is_some()).cloned().collect();
        assert_eq!(spans(&measured), vec![60..=90, 144..=174]);
        assert!(measured.iter().all(|c| c.rejection == Some(DeckRejection::NoCanyon)), "{:?}", det.rejected);
    }

    #[test]
    fn a_gap_is_bridged_unless_it_reaches_the_ground() {
        let with_gap_station = |mnh: f64| {
            let mut r = steep_bridge(300, |_| -60.0, 0.0, |j| 0.05 * j as f64);
            for k in 150..=189 {
                r.mns[k] += 10.0 + 35.0 * noise(k);
            }
            r.mnt[170] = r.mns[170] - mnh;
            r
        };
        let whole = DeckDetectorOptions { split_steep_merged: false, ..d() };
        for (mnh, bridged) in [(4.4, true), (0.5, false)] {
            let c = only(&detect_with(&with_gap_station(mnh), &whole));
            let m = metrics(&c);
            assert!(m.slope_on_clean_deck, "{m:?}");
            if bridged {
                assert_eq!(c.rejection, None, "MNH {mnh}: {m:?}");
                assert!(m.slope_p95 < 0.06, "MNH {mnh}: P95 {}", m.slope_p95);
            } else {
                assert_eq!(c.rejection, Some(DeckRejection::Steep), "MNH {mnh}: {m:?}");
            }
            let e2ter = DeckDetectorOptions { clean_gap_ground_mnh_m: 5.0, ..whole.clone() };
            assert_eq!(
                only(&detect_with(&with_gap_station(mnh), &e2ter)).rejection,
                Some(DeckRejection::Steep),
                "MNH {mnh} at E2 ter's 5 m"
            );
        }
    }

    fn building_then_deck(top: impl Fn(usize) -> f64) -> Route {
        let mut r = Route::new(260, 10.0);
        for k in 60..=69 {
            r.mns[k] = 16.0;
        }
        for k in 70..=79 {
            r.mns[k] = 22.0;
        }
        r.deck(115..=154, 0.0, top)
    }

    #[test]
    fn a_steep_merged_span_is_judged_as_its_parts() {
        let r = building_then_deck(|_| 11.0);
        let python = detect_with(&r, &DeckDetectorOptions::python());
        assert_eq!(spans(&python.rejected), vec![60..=154]);
        assert_eq!(python.rejected[0].rejection, Some(DeckRejection::Steep));
        let det = detect(&r);
        assert_eq!(spans(&det.accepted), vec![115..=154]);
        assert_eq!(spans(&det.rejected), vec![60..=79]);
        assert_eq!(det.rejected[0].rejection, Some(DeckRejection::NoCanyon));
        assert_eq!(det.accepted[0].deck_ranges, vec![115..=154]);
        // Une portée fusionnée qui passe entière n'est jamais coupée.
        let mut r2 = Route::new(700, 10.0).deck(100..=299, 2.0, |_| 12.0);
        for k in 340..=351 {
            r2.mns[k] = 16.0;
        }
        assert_eq!(spans(&detect(&r2).accepted), vec![100..=351]);
    }

    #[test]
    fn a_split_part_can_stay_steep() {
        let r = building_then_deck(|k| 11.0 + 0.75 * (k - 115) as f64);
        let merged = detect_with(&r, &DeckDetectorOptions::python());
        assert_eq!(merged.rejected.len(), 1);
        let merged = &merged.rejected[0];
        assert_eq!(span(merged), 60..=154);
        assert_eq!(merged.rejection, Some(DeckRejection::Steep));
        let det = detect(&r);
        assert!(det.accepted.is_empty(), "{:?}", det.accepted);
        let parts: Vec<_> = det.rejected.iter().map(|c| (span(c), c.rejection)).collect();
        assert_eq!(
            parts,
            vec![(60..=79, Some(DeckRejection::NoCanyon)), (115..=154, Some(DeckRejection::Steep))]
        );
        assert!(parts.iter().all(|(p, _)| p.start() >= &merged.first && p.end() <= &merged.last));
    }

    #[test]
    fn a_steep_merged_span_whose_part_extensions_overlap_stays_whole() {
        let mut r = Route::new(300, 10.0).deck(100..=154, 0.0, |k| 13.0 + 0.75 * (k - 100) as f64);
        for k in 140..=144 {
            r.mnt[k] = r.mns[k] - 3.0;
        }
        let whole = DeckDetectorOptions { split_steep_merged: false, ..d() };
        let c = only(&detect_with(&r, &whole));
        assert_eq!(span(&c), 100..=154);
        assert_eq!(c.rejection, Some(DeckRejection::Steep));
        let c = only(&detect(&r));
        assert_eq!((span(&c), c.rejection), (100..=154, Some(DeckRejection::Steep)));
        for k in 140..=144 {
            r.mnt[k] = r.mns[k];
        }
        let apart = detect(&r);
        let mut all: Vec<_> = apart.accepted.iter().chain(&apart.rejected).map(span).collect();
        all.sort_by_key(|s| *s.start());
        assert_eq!(all, vec![100..=139, 145..=154]);
    }

    // ── Déterminisme ──

    #[test]
    fn detection_is_deterministic_and_from_surface_is_mns_minus_mnt() {
        let mut canopy = Route::new(220, 10.0);
        for k in (50..=89).chain(100..=139) {
            canopy.mns[k] = canopy.mnt[k] + 15.0 + crown(k);
        }
        let mut hole = canyon_bridge();
        hole.mns[62] = f64::NAN;
        let routes = [
            canyon_bridge(),
            cambered_deck(0.059, true),
            cambered_deck(0.12, false),
            canopy,
            two_spans(59),
            two_spans(60),
            short_deck(0.1, 2.0),
            deck_with_blip(100..=107, 115..=117, 125..=132),
            hole,
            bridge_then_field(21),
        ];
        for r in &routes {
            let a = format!("{:?}", detect(r));
            assert_eq!(a, format!("{:?}", detect(r)));
            assert_eq!(a, format!("{:?}", detect_profile(&r.mnh(), &r.mns, &r.mnt, &d())));
        }
    }

    // ── Aides ──

    #[test]
    fn rolling_max_clamps_and_propagates_nan() {
        let m = rolling_max(&[1.0, 3.0, 2.0, 0.0, 0.0, f64::NAN], 1);
        assert_eq!(&m[..4], &[3.0, 3.0, 3.0, 2.0]);
        assert!(m[4].is_nan() && m[5].is_nan());
    }

    #[test]
    fn clean_deck_slopes_interpolates_between_clean_stations_and_keeps_raw_ends() {
        let clean: Vec<bool> = (0..20).map(|k| (2..=5).contains(&k) || (12..=17).contains(&k)).collect();
        let mns: Vec<f64> = (0..20).map(|k| if clean[k] { 0.5 * k as f64 } else { 100.0 }).collect();
        let o = d();
        let w = o.windows().unwrap();
        let tall = vec![10.0; 20];
        let fraction = inner_clean_fraction(&clean, 0, 19, w.slope_half);
        close(fraction, 10.0 / 16.0, 1e-12);
        let slopes = |opts: &DeckDetectorOptions, mnh: &[f64]| {
            clean_deck_slopes(&mns, mnh, &clean, fraction, 0, 19, opts, &w)
        };
        let s = slopes(&o, &tall).unwrap();
        assert_eq!(s.len(), 16);
        for k in 6..=13 {
            close(s[k - 2], 0.1, 1e-12);
        }
        assert!(s[0] > 1.0 && s[15] > 1.0);
        assert!(slopes(&DeckDetectorOptions { min_clean_stations_count: 11, ..d() }, &tall).is_none());
        assert!(slopes(&DeckDetectorOptions { min_clean_fraction: 0.7, ..d() }, &tall).is_none());
        let mut grounded = tall.clone();
        grounded[9] = 0.0;
        assert!(slopes(&o, &grounded).unwrap()[4] > 1.0);
        let mut unknown = tall.clone();
        unknown[9] = f64::NAN;
        let u = slopes(&o, &unknown).unwrap();
        for k in 6..=13 {
            close(u[k - 2], 0.1, 1e-12);
        }
    }

    #[test]
    fn rolling_nan_median_clamps_skips_nan_and_averages_an_even_pair() {
        let n = f64::NAN;
        let m = rolling_nan_median(&[1.0, n, 3.0, 4.0, n, n, n, n, n], 2);
        close(m[0], 1.0, 1e-12);
        close(m[2], 3.0, 1e-12);
        close(m[3], 3.5, 1e-12);
        assert!(m[8].is_nan());
    }

    #[test]
    fn percentile_is_numpy_linear() {
        close(percentile_linear(&[4.0, 1.0, 3.0, 2.0], 0.95), 3.85, 1e-12);
        close(percentile_linear(&[7.0], 0.95), 7.0, 1e-12);
    }

    #[test]
    fn centred_slope_and_mean_clamp_at_the_edges() {
        let s = centred_slope(&[0.0, 1.0, 2.0, 3.0, 4.0], 2, 5.0);
        close(s[0], 2.0 / 20.0, 1e-12);
        close(s[2], 4.0 / 20.0, 1e-12);
        close(s[4], 2.0 / 20.0, 1e-12);
        close(rolling_mean(&[0.0, 3.0, 6.0], 1)[0], 1.0, 1e-12);
        assert!(rolling_mean(&[0.0, f64::NAN, 6.0], 1)[0].is_nan());
    }

    #[test]
    fn extend_steps_over_short_nan_islands_and_gives_up_past_max_steps() {
        let n = f64::NAN;
        let mnh = [0.0, 2.0, n, n, 2.0, 9.0, 9.0, 2.0, n, n, n, 2.0, 0.0];
        assert_eq!(extend(&mnh, 5, -1, 1.0, 2, 10), 1);
        assert_eq!(extend(&mnh, 6, 1, 1.0, 2, 10), 7);
        assert_eq!(extend(&mnh, 5, -1, 1.0, 2, 3), 5);
        assert_eq!(extend(&mnh, 5, -1, 1.0, 2, 4), 1);
    }

    #[test]
    fn close_holes_fills_bounded_holes_only() {
        let mut m = [false, true, false, true, false, false, false, true, false];
        close_holes(&mut m, (0, 8), 2, |_| true);
        assert_eq!(m, [false, true, true, true, false, false, false, true, false]);
        let mut n = [true, false, true];
        close_holes(&mut n, (0, 2), 2, |_| false);
        assert_eq!(n, [true, false, true]);
    }

    #[test]
    fn runs_are_inclusive_and_merge_strictly_below_the_gap() {
        let r = runs(&[true, true, false, false, true, false, true]);
        assert_eq!(r, vec![(0, 1), (4, 4), (6, 6)]);
        assert_eq!(merge_runs(&r, 2), vec![(0, 1), (4, 6)]);
        assert_eq!(merge_runs(&r, 3), vec![(0, 6)]);
    }
}
