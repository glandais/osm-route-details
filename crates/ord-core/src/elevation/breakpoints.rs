//! Sélection des points de rupture (T04 §6) du mode `input_with_breakpoints` : tous les points
//! d'entrée, jamais déplacés, plus des points insérés aux événements du profil corrigé et des
//! segments typés, chacun avec ses raisons (doc 02 §4.2).
//!
//! Événements candidats (§6.1) : classe A (bornes d'ouvrage, de trou, de terrain, croisement d'un
//! pont qui enjambe la trace), B (changement
//! de revêtement), C (extrema confirmés par l'accumulateur du D+, ruptures de pente). Puis (§6.2) :
//! fusion à `merge_m`, rattachement à un point d'entrée à `snap_m`, espacement des C à
//! `min_spacing_m`, plafond par km glissant et total (seuls des C sont retirés).
//!
//! Déterminisme : `f64` séquentiel, tris stables avec égalités tranchées par distance puis par
//! raison, aucune table de hachage.

use ord_domain::route::PreparedRoute;
use serde::Serialize;

use super::output::{point_at, OutputPoint, PointOrigin};
use super::ElevationProfile;
use crate::classification::StructureClass;
use crate::rules::BreakpointRules;
use crate::typology::TypedSegment;

/// Raison d'insertion d'un point (doc 02 §4.2) ; l'ordre de déclaration est l'ordre de sortie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakpointReason {
    LocalMax,
    LocalMin,
    GradeChange,
    StructureStart,
    StructureEnd,
    SurfaceChange,
    TerrainSourceChange,
    ElevationGapStart,
    ElevationGapEnd,
    /// Croisement d'un pont qui enjambe la trace au sol (règles 2026.11.8, api 1.4).
    OverpassCrossing,
}

impl BreakpointReason {
    pub const ALL: [Self; 10] = [
        Self::LocalMax,
        Self::LocalMin,
        Self::GradeChange,
        Self::StructureStart,
        Self::StructureEnd,
        Self::SurfaceChange,
        Self::TerrainSourceChange,
        Self::ElevationGapStart,
        Self::ElevationGapEnd,
        Self::OverpassCrossing,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalMax => "local_max",
            Self::LocalMin => "local_min",
            Self::GradeChange => "grade_change",
            Self::StructureStart => "structure_start",
            Self::StructureEnd => "structure_end",
            Self::SurfaceChange => "surface_change",
            Self::TerrainSourceChange => "terrain_source_change",
            Self::ElevationGapStart => "elevation_gap_start",
            Self::ElevationGapEnd => "elevation_gap_end",
            Self::OverpassCrossing => "overpass_crossing",
        }
    }

    /// Priorité de position dans une fusion (§6.2) : ouvrage > trou > terrain > pont au-dessus >
    /// revêtement > extremum > pente.
    fn priority(self) -> u8 {
        match self {
            Self::StructureStart | Self::StructureEnd => 0,
            Self::ElevationGapStart | Self::ElevationGapEnd => 1,
            Self::TerrainSourceChange => 2,
            Self::OverpassCrossing => 3,
            Self::SurfaceChange => 4,
            Self::LocalMax | Self::LocalMin => 5,
            Self::GradeChange => 6,
        }
    }

    /// A : fait (ouvrage, trou, terrain, pont au-dessus) ; B : revêtement ; C : extremum, pente.
    fn class(self) -> Class {
        match self.priority() {
            0..=3 => Class::A,
            4 => Class::B,
            _ => Class::C,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    A,
    B,
    C,
}

#[derive(Clone, Copy, Debug)]
struct Event {
    d: f64,
    reason: BreakpointReason,
    /// Poids d'un C : amplitude de la jambe (extremum) ou |Δpente| (rupture de pente).
    weight: f64,
}

/// Événements fusionnés en un point candidat.
#[derive(Clone, Debug)]
struct Cluster {
    d: f64,
    reasons: Vec<BreakpointReason>,
    class: Class,
    weight: f64,
}

impl Cluster {
    /// Rang de retrait au plafond : ruptures de pente seules d'abord, puis extrema.
    fn removal_group(&self) -> u8 {
        u8::from(
            self.reasons.iter().any(|r| matches!(r, BreakpointReason::LocalMax | BreakpointReason::LocalMin)),
        )
    }
}

/// Événements candidats (§6.1), non triés.
fn events(
    route: &PreparedRoute,
    p: &ElevationProfile,
    segs: &[TypedSegment],
    r: &BreakpointRules,
) -> Vec<Event> {
    let length = route.length_m;
    let n = p.d.len();
    let mut out = Vec::new();
    let mut push = |d: f64, reason: BreakpointReason, weight: f64| {
        if d.is_finite() {
            out.push(Event { d: d.clamp(0.0, length), reason, weight });
        }
    };
    // A : bornes de chaque entrée de `summary.structures[]`, coupées au parcours.
    for o in &p.structures {
        push(o.from_m, BreakpointReason::StructureStart, 0.0);
        push(o.to_m, BreakpointReason::StructureEnd, 0.0);
    }
    // Deux ouvrages contigus (deux segments d'éléments différents) gardent leur borne commune.
    let is_structure =
        |s: &TypedSegment| matches!(s.structure, StructureClass::Bridge | StructureClass::Tunnel);
    for w in segs.windows(2) {
        if is_structure(&w[0])
            && is_structure(&w[1])
            && (w[0].element != w[1].element || w[0].structure != w[1].structure)
        {
            push(w[0].to_m, BreakpointReason::StructureEnd, 0.0);
            push(w[1].from_m, BreakpointReason::StructureStart, 0.0);
        }
    }
    // A : bornes des trous restés après comblement, portées par la dernière (première) station
    // lue qui les encadre, pour que le point ait une altitude ; à défaut, la borne elle-même.
    for m in &p.missing {
        let first_nan = p.d.partition_point(|&x| x < m.from_m);
        let start =
            if first_nan > 0 && !p.ele[first_nan - 1].is_nan() { p.d[first_nan - 1] } else { m.from_m };
        let first_after = p.d.partition_point(|&x| x <= m.to_m);
        let end = if first_after < n && !p.ele[first_after].is_nan() { p.d[first_after] } else { m.to_m };
        push(start, BreakpointReason::ElevationGapStart, 0.0);
        push(end, BreakpointReason::ElevationGapEnd, 0.0);
    }
    // A : changement d'origine du terrain, et bornes des plages GLO-30 retenues (§5.4).
    for w in p.terrain_ranges.windows(2) {
        if w[0].origin != w[1].origin {
            push(w[1].from_m, BreakpointReason::TerrainSourceChange, 0.0);
        }
    }
    for a in &p.gain.params.adapted {
        for d in [a.from_m, a.to_m] {
            if d > 0.0 && d < length {
                push(d, BreakpointReason::TerrainSourceChange, 0.0);
            }
        }
    }
    // A : croisement d'un pont qui enjambe la trace au sol (règles 2026.11.8) : le point porte
    // l'altitude du profil à cet endroit, celle du mode `distance`, au lieu de la droite entre les
    // points d'entrée qui l'encadrent. Sans altitude au croisement, aucun point.
    for &d in &p.overpass_crossings {
        if p.at(d).ele.is_some() {
            push(d, BreakpointReason::OverpassCrossing, 0.0);
        }
    }
    // B : changement de classe de revêtement entre deux segments.
    for w in segs.windows(2) {
        if w[0].surface_class != w[1].surface_class {
            push(w[1].from_m, BreakpointReason::SurfaceChange, 0.0);
        }
    }
    // C : extrema confirmés par l'accumulateur du D+, recalés (dans `gain_turns_on_runs`) sur
    // l'argmax (argmin) de `ele` non lissé à ± lissage du preset, sans quitter la suite lue. Les
    // bouts de suite (départ, arrivée, bords de trou) sont déjà des points.
    let turns = &p.gain.turning_points;
    let mut nan_before = vec![0usize; n + 1];
    for i in 0..n {
        nan_before[i + 1] = nan_before[i] + usize::from(p.ele[i].is_nan());
    }
    let same_run = |a: usize, b: usize| nan_before[b.max(a) + 1] == nan_before[a.min(b)];
    for (k, t) in turns.iter().enumerate() {
        let i = t.station;
        if i == 0 || i + 1 >= n || p.ele[i - 1].is_nan() || p.ele[i + 1].is_nan() {
            continue;
        }
        let leg =
            |j: usize| if same_run(i, turns[j].station) { (t.value - turns[j].value).abs() } else { 0.0 };
        let weight = leg(k.saturating_sub(1)).max(if k + 1 < turns.len() { leg(k + 1) } else { 0.0 });
        let reason = if t.max { BreakpointReason::LocalMax } else { BreakpointReason::LocalMin };
        push(t.peak_d_m, reason, weight);
    }
    // C : ruptures de pente, Douglas-Peucker vertical entre points fixes consécutifs (points
    // d'entrée, événements A et B, extrema).
    let mut fixed: Vec<f64> = route.input_pos.iter().map(|&i| route.samples[i as usize].d_m).collect();
    fixed.extend(out.iter().map(|e| e.d));
    fixed.sort_by(f64::total_cmp);
    fixed.dedup();
    let mut grades = Vec::new();
    for w in fixed.windows(2) {
        grade_changes(p, w[0], w[1], r, &mut grades);
    }
    out.extend(grades);
    out
}

/// Douglas-Peucker vertical sur `ele` entre deux points fixes (§6.1, « grade_change ») : le
/// sommet le plus éloigné de la corde est retenu s'il s'en écarte de plus de la tolérance, si les
/// deux pièces font au moins `grade_min_run_m` et si leurs pentes diffèrent d'au moins
/// `grade_min_delta_pct` ; sinon la plage est laissée telle quelle. Récursion de gauche à droite.
fn grade_changes(p: &ElevationProfile, a: f64, b: f64, r: &BreakpointRules, out: &mut Vec<Event>) {
    if b - a < 2.0 * r.grade_min_run_m {
        return;
    }
    let (Some(ea), Some(eb)) = (p.at(a).ele, p.at(b).ele) else { return };
    let lo = p.d.partition_point(|&x| x <= a);
    let hi = p.d.partition_point(|&x| x < b);
    if lo >= hi || p.ele[lo..hi].iter().any(|e| e.is_nan()) {
        return;
    }
    split(p, (a, ea), (b, eb), lo, hi, r, out);
}

fn split(
    p: &ElevationProfile,
    (a, ea): (f64, f64),
    (b, eb): (f64, f64),
    lo: usize,
    hi: usize,
    r: &BreakpointRules,
    out: &mut Vec<Event>,
) {
    if lo >= hi || b - a < 2.0 * r.grade_min_run_m {
        return;
    }
    let chord = |x: f64| ea + (eb - ea) * (x - a) / (b - a);
    let mut best = lo;
    let mut err = -1.0;
    for k in lo..hi {
        let e = (p.ele[k] - chord(p.d[k])).abs();
        if e > err {
            err = e;
            best = k;
        }
    }
    // Aucune altitude absente dans la plage (`grade_changes`) : comparaisons sans `NaN`.
    if err <= r.grade_tolerance_m {
        return;
    }
    let (dk, ek) = (p.d[best], p.ele[best]);
    let (run1, run2) = (dk - a, b - dk);
    if run1 < r.grade_min_run_m || run2 < r.grade_min_run_m {
        return;
    }
    let delta = (100.0 * (ek - ea) / run1 - 100.0 * (eb - ek) / run2).abs();
    if delta < r.grade_min_delta_pct {
        return;
    }
    split(p, (a, ea), (dk, ek), lo, best, r, out);
    out.push(Event { d: dk, reason: BreakpointReason::GradeChange, weight: delta });
    split(p, (dk, ek), (b, eb), best + 1, hi, r, out);
}

/// Fusion (§6.2, 1) : événements à moins de `merge_m` du premier d'un groupe ; position de
/// l'événement prioritaire (puis de plus petite distance), union des raisons.
fn merge(mut ev: Vec<Event>, merge_m: f64) -> Vec<Cluster> {
    ev.sort_by(|x, y| {
        x.d.total_cmp(&y.d).then(x.reason.priority().cmp(&y.reason.priority())).then(x.reason.cmp(&y.reason))
    });
    let mut out: Vec<Cluster> = Vec::new();
    let mut i = 0;
    while i < ev.len() {
        let anchor = ev[i].d;
        let mut j = i;
        while j < ev.len() && ev[j].d - anchor < merge_m {
            j += 1;
        }
        let group = &ev[i..j];
        let lead = group
            .iter()
            .min_by(|x, y| x.reason.priority().cmp(&y.reason.priority()).then(x.d.total_cmp(&y.d)))
            .copied()
            .unwrap_or(ev[i]);
        let mut reasons: Vec<BreakpointReason> = group.iter().map(|e| e.reason).collect();
        reasons.sort();
        reasons.dedup();
        out.push(Cluster {
            d: lead.d,
            class: group.iter().map(|e| e.reason.class()).min().unwrap_or(Class::C),
            weight: group.iter().map(|e| e.weight).fold(0.0, f64::max),
            reasons,
        });
        i = j;
    }
    out
}

/// Point d'entrée le plus proche de `x` à `snap_m` au plus (le précédent en cas d'égalité).
fn snap(input_d: &[f64], x: f64, snap_m: f64) -> Option<usize> {
    let i = input_d.partition_point(|&v| v < x);
    let mut best: Option<(usize, f64)> = None;
    for k in [i.checked_sub(1), (i < input_d.len()).then_some(i)].into_iter().flatten() {
        let dist = (input_d[k] - x).abs();
        if best.is_none_or(|(_, b)| dist < b) {
            best = Some((k, dist));
        }
    }
    best.filter(|&(_, dist)| dist <= snap_m).map(|(k, _)| k)
}

/// Plafond (§6.2, 4) : au plus `max_inserted_per_km` points insérés sur toute fenêtre d'un km et
/// `max_inserted_total` en tout ; on retire des C, ruptures de pente puis extrema, par poids
/// croissant puis distance décroissante, tant qu'ils sont dans une fenêtre trop pleine.
fn cap(points: &mut Vec<Cluster>, r: &BreakpointRules) {
    const KM: f64 = 1_000.0;
    let pos: Vec<f64> = points.iter().map(|c| c.d).collect();
    let mut alive = vec![true; points.len()];
    let mut order: Vec<usize> = (0..points.len()).filter(|&i| points[i].class == Class::C).collect();
    order.sort_by(|&x, &y| {
        let (a, b) = (&points[x], &points[y]);
        a.removal_group()
            .cmp(&b.removal_group())
            .then(a.weight.total_cmp(&b.weight))
            .then(b.d.total_cmp(&a.d))
    });
    let count = |alive: &[bool], from: f64| -> usize {
        let lo = pos.partition_point(|&v| v < from);
        let hi = pos.partition_point(|&v| v < from + KM);
        alive[lo..hi].iter().filter(|a| **a).count()
    };
    for &c in &order {
        let x = pos[c];
        // Fenêtres maximales qui contiennent `x` : celles qui commencent à un point vivant.
        let lo = pos.partition_point(|&v| v <= x - KM);
        let hi = pos.partition_point(|&v| v <= x);
        let full = (lo..hi).any(|s| alive[s] && count(&alive, pos[s]) > r.max_inserted_per_km);
        if full {
            alive[c] = false;
        }
    }
    let mut total = alive.iter().filter(|a| **a).count();
    for &c in &order {
        if total <= r.max_inserted_total {
            break;
        }
        if alive[c] {
            alive[c] = false;
            total -= 1;
        }
    }
    let mut k = 0;
    points.retain(|_| {
        k += 1;
        alive[k - 1]
    });
}

/// Points du mode `input_with_breakpoints` (T04 §6), triés par distance : chaque point d'entrée
/// (raisons des événements qui lui sont rattachés), plus les points insérés.
pub fn select_breakpoints(
    route: &PreparedRoute,
    p: &ElevationProfile,
    segs: &[TypedSegment],
    r: &BreakpointRules,
) -> Vec<OutputPoint> {
    let mut points = super::output::input_points(route, p);
    let input_d: Vec<f64> = points.iter().map(|q| q.d_m).collect();
    let clusters = merge(events(route, p, segs, r), r.merge_m);
    // Rattachement aux points d'entrée.
    let mut free: Vec<Cluster> = Vec::new();
    for c in clusters {
        match snap(&input_d, c.d, r.snap_m) {
            Some(k) => {
                let reasons = &mut points[k].reasons;
                reasons.extend(c.reasons);
                reasons.sort();
                reasons.dedup();
            }
            None => free.push(c),
        }
    }
    // Espacement : un C trop proche d'un point retenu (entrée, A ou B) est abandonné.
    let mut kept: Vec<f64> = input_d.clone();
    kept.extend(free.iter().filter(|c| c.class != Class::C).map(|c| c.d));
    kept.sort_by(f64::total_cmp);
    free.retain(|c| {
        if c.class != Class::C {
            return true;
        }
        let i = kept.partition_point(|&v| v < c.d);
        let near = |k: usize| (kept[k] - c.d).abs() < r.min_spacing_m;
        !(i < kept.len() && near(i) || i > 0 && near(i - 1))
    });
    cap(&mut free, r);
    points.extend(free.into_iter().map(|c| {
        let mut q = point_at(route, p, c.d);
        q.origin = Some(PointOrigin::Inserted);
        q.reasons = c.reasons;
        q
    }));
    points.sort_by(|a, b| a.d_m.total_cmp(&b.d_m));
    points
}

#[cfg(test)]
mod tests;
