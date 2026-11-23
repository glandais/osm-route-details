//! Section « 4d » de la comparaison (lot 4, §17.11) : la part des points lointains sur un attracteur OSM, le réel, `svc-org`
//! (1.12) et les variantes `attr`, puis les critères du §17.12, chacun « tient » ou « ne tient pas » avec ses chiffres.
//! **Lettres, masques de catégories et agrégats seulement : aucune position.**

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use ord_graph::util::SplitMix64;
use ord_loops::RideType;
use serde::Serialize;

use super::compare::{Compare, Conditions, Done, Reading};
use super::compare_org::{all_hold, f3, find, has, verdict, w1, Check, GROUPS, TYPES};
use super::compare_svc::ratio;
use super::out::LoopRec;
use super::play::GenRoute;

/// Variante de référence : le service 1.12.
const REFERENCE: &str = "svc-org";

/// Groupes de catégories (bits du masque : café 0, boulangerie 1, point de vue 2, bourg 3, village 4, col 5, sommet 6).
const COMMERCE: u8 = 0b11;
const VIEWPOINT: u8 = 0b100;
const BOURG: u8 = 0b1_1000;
const PASS: u8 = 0b10_0000;
const PEAK: u8 = 0b100_0000;
/// Les catégories à poids non nul des poids généraux, par type.
const RETAINED_ROAD: u8 = COMMERCE | VIEWPOINT | PASS | PEAK;
const RETAINED_GRAVEL: u8 = VIEWPOINT | PASS | PEAK;

fn cat_groups(ty: &str) -> [(&'static str, u8); 6] {
    [
        ("commerce", COMMERCE),
        ("viewpoint", VIEWPOINT),
        ("bourg", BOURG),
        ("mountain_pass", PASS),
        ("peak", PEAK),
        ("retenus", if ty == "ROAD" { RETAINED_ROAD } else { RETAINED_GRAVEL }),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    All,
    A,
    NotA,
}

impl Scope {
    const ALL: [Scope; 3] = [Scope::All, Scope::A, Scope::NotA];
    fn name(self) -> &'static str {
        match self {
            Self::All => "toutes",
            Self::A => "A",
            Self::NotA => "hors A",
        }
    }
    fn keeps(self, team: &str) -> bool {
        match self {
            Self::All => true,
            Self::A => team == "A",
            Self::NotA => team != "A",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Radius {
    /// À `reach_m` du fichier (300 m).
    M300,
    /// À 1 km.
    Km1,
}

impl Radius {
    fn name(self) -> &'static str {
        match self {
            Self::M300 => "300 m",
            Self::Km1 => "1 km",
        }
    }
}

type Key = (String, u32);

fn first_route(l: &LoopRec, cursor: f64) -> Option<&GenRoute> {
    l.plays
        .iter()
        .find(|p| p.overlay == "full" && p.cursor == cursor && p.outcome == "OK")
        .and_then(|p| p.routes.first())
}

fn far_mask(m: &super::measure::Measures, r: Radius) -> Option<u8> {
    m.attr.as_ref().map(|a| if r == Radius::M300 { a.far } else { a.far_1k })
}

/// Les masques du point lointain des premiers parcours gardés d'une variante : `(boucle, masque)`.
fn gen_masks(d: &Done, ty: &str, scope: Scope, cursor: f64, r: Radius) -> Vec<(Key, u8)> {
    d.loops
        .iter()
        .filter(|l| l.ty == ty && scope.keeps(&l.team))
        .filter_map(|l| {
            let m = far_mask(&first_route(l, cursor)?.m, r)?;
            Some(((l.team.clone(), l.i), m))
        })
        .collect()
}

/// Les masques du point lointain des boucles réelles.
fn real_masks(base: &Done, ty: &str, scope: Scope, r: Radius) -> Vec<u8> {
    base.loops
        .iter()
        .filter(|l| l.ty == ty && scope.keeps(&l.team))
        .filter_map(|l| far_mask(&l.real, r))
        .collect()
}

fn share(masks: &[u8], cat: u8) -> Option<f64> {
    (!masks.is_empty()).then(|| masks.iter().filter(|&&m| m & cat != 0).count() as f64 / masks.len() as f64)
}

fn share_keyed(v: &[(Key, u8)], cat: u8) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().filter(|(_, m)| m & cat != 0).count() as f64 / v.len() as f64)
}

/// Écart apparié (variante − référence) sur les requêtes qui ont un premier parcours dans les deux, et son intervalle à 90 %
/// par rééchantillonnage des requêtes (300 tirages, graine fixe).
fn paired_delta(cand: &[(Key, u8)], reference: &[(Key, u8)], cat: u8, salt: u64) -> Option<(f64, [f64; 2])> {
    let refs: HashMap<&Key, u8> = reference.iter().map(|(k, m)| (k, *m)).collect();
    let pairs: Vec<f64> = cand
        .iter()
        .filter_map(|(k, m)| {
            let r = *refs.get(k)?;
            Some(f64::from(u8::from(m & cat != 0)) - f64::from(u8::from(r & cat != 0)))
        })
        .collect();
    let n = pairs.len();
    if n < 10 {
        return None;
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let mut rng = SplitMix64(0xA77D_4D00 ^ salt);
    let mut draws: Vec<f64> = (0..300)
        .map(|_| {
            let s: f64 = (0..n).map(|_| pairs[rng.below(n)]).sum();
            s / n as f64
        })
        .collect();
    draws.sort_by(f64::total_cmp);
    Some((mean(&pairs), [draws[14], draws[284]]))
}

// --- Parts ------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct ShareCell {
    pub variant: String,
    pub n: usize,
    pub share: Option<f64>,
    /// Écart à `svc-org` (apparié) et son intervalle à 90 %.
    pub delta: Option<f64>,
    pub ci90: Option<[f64; 2]>,
    /// Part de l'écart au réel fermée : `(variante − svc-org) / (réel − svc-org)` quand `réel − svc-org > 0,005`.
    pub closed: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShareRow {
    pub scope: String,
    pub ty: String,
    pub cursor: f64,
    pub radius: String,
    pub group: String,
    pub n_real: usize,
    pub real: Option<f64>,
    pub cells: Vec<ShareCell>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VertexCell {
    pub variant: String,
    pub vertices: usize,
    pub share: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VertexRow {
    pub scope: String,
    pub ty: String,
    pub cursor: f64,
    pub group: String,
    pub real_vertices: usize,
    pub real: Option<f64>,
    pub cells: Vec<VertexCell>,
}

fn carries_attr(d: &Done) -> bool {
    d.loops.iter().any(|l| l.plays.iter().any(|p| p.routes.first().is_some_and(|r| r.m.attr.is_some())))
}

fn share_rows(done: &[Done], base: &Done) -> Vec<ShareRow> {
    let reference = find(done, REFERENCE);
    let shown: Vec<&Done> = done.iter().filter(|d| carries_attr(d)).collect();
    let mut out = Vec::new();
    for scope in Scope::ALL {
        for ty in TYPES {
            for radius in [Radius::M300, Radius::Km1] {
                let real = real_masks(base, ty, scope, radius);
                for (group, cat) in cat_groups(ty) {
                    for cursor in [0.25, 1.0] {
                        let ref_masks = reference.map(|r| gen_masks(r, ty, scope, cursor, radius));
                        let ref_share = ref_masks.as_ref().and_then(|m| share_keyed(m, cat));
                        let real_share = share(&real, cat);
                        let cells = shown
                            .iter()
                            .map(|d| {
                                let m = gen_masks(d, ty, scope, cursor, radius);
                                let s = share_keyed(&m, cat);
                                let salt = (cat as u64) << 8 | (cursor * 100.0) as u64;
                                let pd = ref_masks
                                    .as_ref()
                                    .filter(|_| d.variant.name != REFERENCE)
                                    .and_then(|r| paired_delta(&m, r, cat, salt));
                                let closed = match (s, ref_share, real_share) {
                                    (Some(s), Some(r), Some(x))
                                        if x - r > 0.005 && d.variant.name != REFERENCE =>
                                    {
                                        Some((s - r) / (x - r))
                                    }
                                    _ => None,
                                };
                                ShareCell {
                                    variant: d.variant.name.clone(),
                                    n: m.len(),
                                    share: s,
                                    delta: pd.map(|p| p.0),
                                    ci90: pd.map(|p| p.1),
                                    closed,
                                }
                            })
                            .collect();
                        out.push(ShareRow {
                            scope: scope.name().into(),
                            ty: ty.into(),
                            cursor,
                            radius: radius.name().into(),
                            group: group.into(),
                            n_real: real.len(),
                            real: real_share,
                            cells,
                        });
                    }
                }
            }
        }
    }
    out
}

/// Les masques des sommets intérieurs à 10 % de D (départ et arrivée exclus).
fn vertex_masks_real(base: &Done, ty: &str, scope: Scope) -> Vec<u8> {
    base.loops
        .iter()
        .filter(|l| l.ty == ty && scope.keeps(&l.team))
        .filter_map(|l| l.real.attr.as_ref())
        .flat_map(|a| a.v10.iter().copied())
        .collect()
}

fn vertex_masks_gen(d: &Done, ty: &str, scope: Scope, cursor: f64) -> Vec<u8> {
    d.loops
        .iter()
        .filter(|l| l.ty == ty && scope.keeps(&l.team))
        .filter_map(|l| first_route(l, cursor)?.m.attr.as_ref())
        .flat_map(|a| a.v10.iter().copied())
        .collect()
}

fn vertex_rows(done: &[Done], base: &Done) -> Vec<VertexRow> {
    let shown: Vec<&Done> = done.iter().filter(|d| carries_attr(d)).collect();
    let mut out = Vec::new();
    for scope in [Scope::All, Scope::A, Scope::NotA] {
        for ty in TYPES {
            let real = vertex_masks_real(base, ty, scope);
            for (group, cat) in cat_groups(ty) {
                for cursor in [0.25, 1.0] {
                    out.push(VertexRow {
                        scope: scope.name().into(),
                        ty: ty.into(),
                        cursor,
                        group: group.into(),
                        real_vertices: real.len(),
                        real: share(&real, cat),
                        cells: shown
                            .iter()
                            .map(|d| {
                                let m = vertex_masks_gen(d, ty, scope, cursor);
                                VertexCell {
                                    variant: d.variant.name.clone(),
                                    vertices: m.len(),
                                    share: share(&m, cat),
                                }
                            })
                            .collect(),
                    });
                }
            }
        }
    }
    out
}

// --- Compteurs, service -----------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct CounterRow {
    pub variant: String,
    pub ty: String,
    pub requests: usize,
    /// Par requête (moyenne) : sommets candidats ajoutés, paires de poids non nul cherchées, demi-tours remplacés.
    pub candidates_mean: f64,
    pub pairs_searched_mean: f64,
    pub lens_picks_mean: f64,
    /// Part des requêtes où l'un des trois compteurs est non nul.
    pub touched_share: f64,
}

fn counter_rows(done: &[Done]) -> Vec<CounterRow> {
    let mut out = Vec::new();
    for d in done.iter().filter(|d| d.variant.org.is_some_and(|o| o.attr.is_some_and(|a| !a.zero))) {
        for ty in TYPES {
            let figs: Vec<&super::play::FigAgg> = d
                .loops
                .iter()
                .filter(|l| l.ty == ty)
                .flat_map(|l| l.plays.iter().filter(|p| p.overlay == "full"))
                .filter_map(|p| p.fig.as_ref())
                .collect();
            let n = figs.len();
            if n == 0 {
                continue;
            }
            let mean = |f: &dyn Fn(&super::play::FigAgg) -> u32| {
                figs.iter().map(|x| f64::from(f(x))).sum::<f64>() / n as f64
            };
            out.push(CounterRow {
                variant: d.variant.name.clone(),
                ty: ty.into(),
                requests: n,
                candidates_mean: mean(&|f| f.attr_candidates),
                pairs_searched_mean: mean(&|f| f.attr_pairs_searched),
                lens_picks_mean: mean(&|f| f.attr_lens_picks),
                touched_share: figs
                    .iter()
                    .filter(|f| f.attr_candidates + f.attr_pairs_searched + f.attr_lens_picks > 0)
                    .count() as f64
                    / n as f64,
            });
        }
    }
    out
}

#[derive(Clone, Debug, Serialize)]
pub struct ServiceRow {
    pub variant: String,
    pub cursor: f64,
    pub ms_p50: Option<f64>,
    pub ms_p90: Option<f64>,
    /// Contre `svc-org` du même tirage (le seuil de C1) et contre `svc` (rapporté).
    pub p50_x_org: Option<f64>,
    pub p90_x_org: Option<f64>,
    pub p50_x_svc: Option<f64>,
    pub p90_x_svc: Option<f64>,
    pub ok_share: f64,
    pub panic: usize,
    pub window_escape: usize,
    pub violation: usize,
}

fn service_rows(cmp: &Compare, done: &[Done]) -> Vec<ServiceRow> {
    let pick = |v: &str, c: f64| cmp.service.iter().find(|r| r.variant == v && r.cursor == c);
    let x = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) if b > 0.0 => Some(a / b),
        _ => None,
    };
    let mut out = Vec::new();
    for d in done.iter().filter(|d| {
        d.variant.name == "svc"
            || d.variant.name == REFERENCE
            || d.variant.org.is_some_and(|o| o.attr.is_some())
    }) {
        for r in cmp.service.iter().filter(|r| r.variant == d.variant.name) {
            let (org, svc) = (pick(REFERENCE, r.cursor), pick("svc", r.cursor));
            let n = |k: &str| r.outcomes.get(k).copied().unwrap_or(0);
            out.push(ServiceRow {
                variant: r.variant.clone(),
                cursor: r.cursor,
                ms_p50: r.ms_p50,
                ms_p90: r.ms_p90,
                p50_x_org: x(r.ms_p50, org.and_then(|o| o.ms_p50)),
                p90_x_org: x(r.ms_p90, org.and_then(|o| o.ms_p90)),
                p50_x_svc: x(r.ms_p50, svc.and_then(|o| o.ms_p50)),
                p90_x_svc: x(r.ms_p90, svc.and_then(|o| o.ms_p90)),
                ok_share: r.ok_share,
                panic: n("PANIC"),
                window_escape: n("WINDOW_ESCAPE"),
                violation: n("VIOLATION"),
            });
        }
    }
    out
}

/// Parcours rendus par le moteur et refusés par le contrôleur indépendant, tous jeux d'une variante.
fn controller_refusals(d: &Done) -> (u64, u64) {
    let plays = d.loops.iter().flat_map(|l| l.plays.iter());
    plays.fold((0, 0), |(r, k), p| (r + p.n_routes as u64, k + (p.n_routes - p.n_kept) as u64))
}

// --- Contrôles --------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct Controls {
    pub base: Option<Check>,
    pub common: Option<Check>,
    pub svc: Option<Check>,
    pub i5_org: Option<Check>,
    pub i5_attr0: Option<Check>,
    /// Métrique contre la sonde (§17.11 point 4), ROAD, toutes équipes.
    pub sonde: Vec<Check>,
    /// La campagne s'arrête : un contrôle manque.
    pub stop: Vec<String>,
}

fn eq_check(id: &str, label: &str, d: Option<&Done>, want: &str) -> Option<Check> {
    let d = d?;
    Some(Check {
        id: id.into(),
        label: label.into(),
        holds: Some(d.fingerprint == want),
        detail: format!("`{}` contre `{want}`", d.fingerprint),
    })
}

fn controls(conds: &Conditions, done: &[Done], base: &Done) -> Controls {
    let full = conds.all_check_applies && (conds.holdout_share - 0.25).abs() < 1e-9;
    let i5 = if full { Some(super::I5_FULL_FINGERPRINT) } else { None };
    let mut c = Controls {
        base: full
            .then(|| eq_check("I1", "`base` = sonde 4c", Some(base), super::SONDE_4C_BASE_FINGERPRINT))
            .flatten(),
        common: full
            .then(|| {
                eq_check(
                    "I1",
                    "`common` = sonde 4c",
                    find(done, "common"),
                    super::SONDE_4C_COMMON_FINGERPRINT,
                )
            })
            .flatten(),
        svc: full
            .then(|| eq_check("I1", "`svc` = sonde 4c", find(done, "svc"), super::SONDE_4C_SVC_FINGERPRINT))
            .flatten(),
        i5_org: i5.and_then(|w| eq_check("I5", "`svc-org` = 1.12", find(done, "svc-org"), w)),
        i5_attr0: i5.and_then(|w| eq_check("I5", "`svc-org-attr0` = 1.12", find(done, "svc-org-attr0"), w)),
        sonde: Vec::new(),
        stop: Vec::new(),
    };
    // Métrique contre la sonde : ROAD, toutes équipes, point lointain à 300 m.
    if full {
        let real = real_masks(base, "ROAD", Scope::All, Radius::M300);
        let mut push = |label: &str, got: Option<f64>, want: f64| {
            c.sonde.push(Check {
                id: "sonde".into(),
                label: label.into(),
                holds: got.map(|g| (g - want).abs() <= 0.006),
                detail: format!("{} contre {want:.3} (±0,006)", f3(got)),
            });
        };
        push("réel, commerce", share(&real, COMMERCE), 0.199);
        push("réel, point de vue", share(&real, VIEWPOINT), 0.046);
        if let Some(r) = find(done, REFERENCE) {
            let m = gen_masks(r, "ROAD", Scope::All, 0.25, Radius::M300);
            push("`svc-org` à 0,25, commerce", share_keyed(&m, COMMERCE), 0.146);
            push("`svc-org` à 0,25, point de vue", share_keyed(&m, VIEWPOINT), 0.033);
        }
    }
    let all: Vec<&Check> = [&c.base, &c.common, &c.svc, &c.i5_org, &c.i5_attr0]
        .into_iter()
        .flatten()
        .chain(c.sonde.iter())
        .collect();
    c.stop = all
        .iter()
        .filter(|k| k.holds == Some(false))
        .map(|k| format!("{} : {} ({})", k.id, k.label, k.detail))
        .collect();
    c
}

// --- Critères du §17.12 -------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct AttrVerdict {
    pub candidate: String,
    pub checks: Vec<Check>,
    /// E1 à E4, A1 à A5, B1, B2, C1 et C2 tiennent tous (D et C3 se lisent hors du banc).
    pub switch_criteria_hold: Option<bool>,
    /// Part de l'écart fermée sur E1 et E2 (plafonnée à 1), moyenne : le critère de choix du réglage.
    pub mean_closed: Option<f64>,
}

fn mean(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

fn attr_verdict(cmp: &Compare, done: &[Done], base: &Done, cand: &Done) -> AttrVerdict {
    let name = cand.variant.name.clone();
    let mut checks: Vec<Check> = Vec::new();
    let Some(org) = find(done, REFERENCE) else {
        return AttrVerdict { candidate: name, checks, switch_criteria_hold: None, mean_closed: None };
    };
    let sh = |d: &Done, ty: &str, cursor: f64, cat: u8| {
        share_keyed(&gen_masks(d, ty, Scope::All, cursor, Radius::M300), cat)
    };
    let real = |ty: &str, cat: u8| share(&real_masks(base, ty, Scope::All, Radius::M300), cat);
    let mut push = |id: &str, label: &str, holds: Option<bool>, detail: String| {
        checks.push(Check { id: id.into(), label: label.into(), holds, detail });
    };
    let mut closed: Vec<f64> = Vec::new();
    // E1 : ROAD, commerce, chaque curseur.
    let (mut e1, mut d1) = (Vec::new(), Vec::new());
    for c in [0.25, 1.0] {
        let (s, r, x) = (sh(cand, "ROAD", c, COMMERCE), sh(org, "ROAD", c, COMMERCE), real("ROAD", COMMERCE));
        let need = r.zip(x).map(|(r, x)| r + 0.5 * (x - r));
        e1.push(s.zip(need).map(|(s, n)| s >= n));
        if let (Some(s), Some(r), Some(x)) = (s, r, x) {
            if x - r > 0.005 {
                closed.push(((s - r) / (x - r)).min(1.0));
            }
        }
        d1.push(format!(
            "curseur {c} : {} (seuil {} ; `svc-org` {}, réel {})",
            f3(s),
            f3(need),
            f3(r),
            f3(x)
        ));
    }
    push(
        "E1",
        "ROAD, commerce : part ≥ `svc-org` + la moitié de l'écart au réel, à chaque curseur",
        all_hold(&e1),
        d1.join(" ; "),
    );
    // E2 : ROAD, point de vue, moyenne des deux curseurs.
    let cur = [0.25, 1.0];
    let vals = |d: &Done, ty: &str, cat: u8| -> Option<f64> {
        let v: Vec<f64> = cur.iter().filter_map(|&c| sh(d, ty, c, cat)).collect();
        (v.len() == 2).then(|| mean(&v)).flatten()
    };
    let (s2, r2, x2) = (vals(cand, "ROAD", VIEWPOINT), vals(org, "ROAD", VIEWPOINT), real("ROAD", VIEWPOINT));
    let need2 = r2.zip(x2).map(|(r, x)| r + 0.5 * (x - r));
    if let (Some(s), Some(r), Some(x)) = (s2, r2, x2) {
        if x - r > 0.005 {
            closed.push(((s - r) / (x - r)).min(1.0));
        }
    }
    let km1 = |d: &Done, c: f64| share_keyed(&gen_masks(d, "ROAD", Scope::All, c, Radius::Km1), VIEWPOINT);
    let e2 = s2.zip(need2).map(|(s, n)| s >= n);
    push(
        "E2",
        "ROAD, point de vue : moyenne des deux curseurs ≥ `svc-org` + la moitié de l'écart au réel",
        e2,
        format!(
            "{} (seuil {} ; `svc-org` {}, réel {}) ; à 1 km : {} · {} contre `svc-org` {} · {}",
            f3(s2),
            f3(need2),
            f3(r2),
            f3(x2),
            f3(km1(cand, 0.25)),
            f3(km1(cand, 1.0)),
            f3(km1(org, 0.25)),
            f3(km1(org, 1.0))
        ),
    );
    // E3 : pas de dépassement, ROAD.
    let (mut e3, mut d3) = (Vec::new(), Vec::new());
    for (g, cat) in [
        ("commerce", COMMERCE),
        ("viewpoint", VIEWPOINT),
        ("mountain_pass", PASS),
        ("peak", PEAK),
        ("bourg", BOURG),
    ] {
        for c in cur {
            let (s, x) = (sh(cand, "ROAD", c, cat), real("ROAD", cat));
            e3.push(s.zip(x).map(|(s, x)| s <= x + 0.03));
            d3.push(format!("{g} {c} : {} (plafond {})", f3(s), f3(x.map(|x| x + 0.03))));
        }
    }
    push(
        "E3",
        "ROAD : chaque groupe à poids non nul et `bourg` ≤ réel + 0,03, à chaque curseur",
        all_hold(&e3),
        d3.join(" ; "),
    );
    // E4 : GRAVEL.
    let (s4, r4, x4) =
        (vals(cand, "GRAVEL", VIEWPOINT), vals(org, "GRAVEL", VIEWPOINT), real("GRAVEL", VIEWPOINT));
    let need4 = r4.zip(x4).map(|(r, x)| r + 0.5 * (x - r));
    let mut e4 = vec![s4.zip(need4).map(|(s, n)| s >= n)];
    let mut d4 = vec![format!(
        "point de vue : {} (seuil {} ; `svc-org` {}, réel {})",
        f3(s4),
        f3(need4),
        f3(r4),
        f3(x4)
    )];
    for c in cur {
        let (s, r) = (sh(cand, "GRAVEL", c, COMMERCE), sh(org, "GRAVEL", c, COMMERCE));
        e4.push(s.zip(r).map(|(s, r)| s <= r + 0.005));
        d4.push(format!("commerce {c} : {} (plafond {})", f3(s), f3(r.map(|r| r + 0.005))));
    }
    push(
        "E4",
        "GRAVEL : point de vue ≥ la moitié de l'écart (moyenne des curseurs), commerce ≤ `svc-org` + 0,005",
        all_hold(&e4),
        d4.join(" ; "),
    );
    // A1 à A4 : contre `svc`, aux seuils du §16.11 (les contrôles de l'étape précédente).
    let old = verdict(cmp, done, cand);
    let mut a_all: Vec<Option<bool>> = Vec::new();
    for id in ["A1", "A2", "A3", "A4"] {
        if let Some(k) = old.checks.iter().find(|k| k.id == id) {
            a_all.push(k.holds);
            push(id, &k.label, k.holds, k.detail.clone());
        }
    }
    // A5 : la forme ne bouge pas contre `svc-org`.
    let iq: BTreeMap<&str, BTreeMap<&'static str, f64>> =
        TYPES.iter().map(|t| (*t, super::compare::type_iqrs(&base.loops, t))).collect();
    let (mut a5, mut d5) = (Vec::new(), Vec::new());
    for g in GROUPS {
        for m in ["r_over_d", "compactness"] {
            let (c, r) = (
                w1(base, cand, &iq[g.0], g, None, Reading::All, m),
                w1(base, org, &iq[g.0], g, None, Reading::All, m),
            );
            a5.push(c.zip(r).map(|(c, r)| c <= r + 0.03));
            d5.push(format!("{m} {} {} : {} contre {}", g.0, g.1, f3(c), f3(r)));
        }
        let gr = |d: &Done| {
            let grp = super::compare::group(&d.loops, g.0, None, Reading::All, g.1);
            super::compare_org::share_cell(d, &grp).v2_share
        };
        let (c, r) = (gr(cand), gr(org));
        a5.push(c.zip(r).map(|(c, r)| (c - r).abs() <= 0.03));
        d5.push(format!("part à 2 sommets {} {} : {} contre {}", g.0, g.1, f3(c), f3(r)));
    }
    push(
        "A5",
        "garde des formes : W1 / IQR de r / D et de la compacité ≤ `svc-org` + 0,03 ; part des premiers à 2 sommets à ±0,03",
        all_hold(&a5),
        d5.join(" ; "),
    );
    // B1 : goût, contre `svc-org`.
    let rows = &cmp.criterion;
    let b1_of = |variant: &str| -> Option<bool> {
        let mut v = Vec::new();
        for reading in [Reading::All, Reading::Gap] {
            for g in GROUPS {
                let (c, r) = (
                    ratio(rows, "toutes", reading, g, variant),
                    ratio(rows, "toutes", reading, g, REFERENCE),
                );
                v.push(c.zip(r).map(|(c, r)| c <= r + 0.02 && c <= 0.5));
            }
        }
        all_hold(&v)
    };
    let mut d1b = Vec::new();
    for reading in [Reading::All, Reading::Gap] {
        for g in GROUPS {
            d1b.push(format!(
                "{} {} ({}) : {} contre {}",
                g.0,
                g.1,
                reading.name(),
                f3(ratio(rows, "toutes", reading, g, &name)),
                f3(ratio(rows, "toutes", reading, g, REFERENCE))
            ));
        }
    }
    let b1 = b1_of(&name);
    push(
        "B1",
        "`S / S_base` ≤ `svc-org` + 0,02 et ≤ 0,5, quatre groupes, « toutes » et « écart »",
        b1,
        d1b.join(" ; "),
    );
    // B2 : réserve du fait (d), avec le partenaire `common` du même réglage.
    let partner = name.replacen("svc", "common", 1);
    let b2_of = |variant: &str, with: &str| -> (Option<bool>, String) {
        let d: Vec<Option<f64>> = cur
            .iter()
            .map(|&c| {
                Some(
                    ratio(rows, "A", Reading::All, ("ROAD", c), variant)?
                        - ratio(rows, "A", Reading::All, ("ROAD", c), with)?,
                )
            })
            .collect();
        (
            d.iter().all(Option::is_some).then(|| d.iter().flatten().all(|&x| x <= 0.054)),
            format!("{} · {}", f3(d[0]), f3(d[1])),
        )
    };
    let (b2, d2b) = b2_of(&name, &partner);
    push(
        "B2",
        "réserve du fait (d) : A ROAD « toutes », `svc-org-attr − common-org-attr` ≤ +0,054 aux deux curseurs",
        b2,
        format!("{d2b} (partenaire `{partner}`{})", if has(done, &partner) { "" } else { ", absent" }),
    );
    // C1, C2 : service, contre `svc-org`.
    let srv = service_rows(cmp, done);
    let pick = |v: &str, c: f64| srv.iter().find(|r| r.variant == v && r.cursor == c);
    let (mut c1, mut dc1, mut c2, mut dc2) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for c in cur {
        let (Some(r), Some(s)) = (pick(&name, c), pick(REFERENCE, c)) else { continue };
        c1.push(r.p50_x_org.map(|x| x <= 1.05).zip(r.p90_x_org.map(|x| x <= 1.10)).map(|(a, b)| a && b));
        dc1.push(format!(
            "curseur {c} : P50 {} × `svc-org` (≤ 1,05), P90 {} × (≤ 1,10) ; contre `svc` : {} · {}",
            f3(r.p50_x_org),
            f3(r.p90_x_org),
            f3(r.p50_x_svc),
            f3(r.p90_x_svc)
        ));
        c2.push(Some(
            r.ok_share >= s.ok_share - 0.005
                && r.panic == 0
                && r.window_escape == 0
                && r.violation <= s.violation,
        ));
        dc2.push(format!(
            "curseur {c} : `OK` {:.1} % contre {:.1} %, PANIC {}, WINDOW_ESCAPE {}, VIOLATION {} contre {}",
            100.0 * r.ok_share,
            100.0 * s.ok_share,
            r.panic,
            r.window_escape,
            r.violation,
            s.violation
        ));
    }
    let c1h = (!c1.is_empty()).then(|| all_hold(&c1)).flatten();
    let (rendered, refused) = controller_refusals(cand);
    let c2h = (!c2.is_empty()).then(|| all_hold(&c2)).flatten().map(|h| h && refused == 0);
    push(
        "C1",
        "temps au banc : P50 ≤ 1,05 × `svc-org`, P90 ≤ 1,10 × `svc-org` (à chaque curseur)",
        c1h,
        dc1.join(" ; "),
    );
    dc2.push(format!(
        "refus du contrôleur indépendant : {refused} sur {rendered} parcours rendus (attendu 0)"
    ));
    push(
        "C2",
        "issues : `OK` ≥ `svc-org` − 0,5 point, ni PANIC ni WINDOW_ESCAPE, VIOLATION ≤ `svc-org`, aucun refus du contrôleur",
        c2h,
        dc2.join(" ; "),
    );
    push(
        "C3",
        "conteneur de 3 Gio, après la bascule : ≤ 1,05 × une 1.12 de la même séance",
        None,
        "séance en conteneur, hors banc".into(),
    );
    push(
        "D",
        "déterminisme : mêmes empreintes à 1 et 16 fils (B et C)",
        None,
        "campagne 2 : comparer les empreintes à 1 et 16 fils".into(),
    );
    let switch = all_hold(&[
        all_hold(&e1),
        e2,
        all_hold(&e3),
        all_hold(&e4),
        all_hold(&a_all),
        all_hold(&a5),
        b1,
        b2,
        c1h,
        c2h,
    ]);
    AttrVerdict { candidate: name, checks, switch_criteria_hold: switch, mean_closed: mean(&closed) }
}

/// Règle L′ (§17.9) : le candidat appris sans attracteurs tient B1 et B2 → `attractors = false` reste ; sinon l'apprenant
/// `la` les tient → `true` ; sinon pas de bascule.
fn rule_l_prime(verdicts: &[AttrVerdict]) -> Option<Check> {
    let find_v = |suffix: &str| verdicts.iter().find(|v| v.candidate.ends_with(suffix));
    let la = verdicts.iter().find(|v| v.candidate.ends_with("-la"))?;
    let stem = la.candidate.trim_end_matches("-la");
    let plain = verdicts.iter().find(|v| v.candidate == stem).or_else(|| find_v("-attr"))?;
    let holds = |v: &AttrVerdict| {
        let get = |id: &str| v.checks.iter().find(|c| c.id == id).and_then(|c| c.holds);
        get("B1").zip(get("B2")).map(|(a, b)| a && b)
    };
    let (p, l) = (holds(plain), holds(la));
    let decision = match (p, l) {
        (Some(true), _) => "`attractors = false` reste (aucun redépôt)",
        (Some(false), Some(true)) => {
            "`attractors = true` : tous les profils `incompatible`, une nuit de redépôt"
        }
        (Some(false), Some(false)) => "pas de bascule",
        _ => "indéterminée",
    };
    Some(Check {
        id: "L′".into(),
        label: "règle L′ : B1 et B2 du candidat appris sans attracteurs, puis de `-la`".into(),
        holds: p.or(l),
        detail: format!(
            "`{}` : {} ; `{}` : {} ; décision : {decision}",
            plain.candidate,
            f3b(p),
            la.candidate,
            f3b(l)
        ),
    })
}

fn f3b(b: Option<bool>) -> &'static str {
    match b {
        Some(true) => "tient",
        Some(false) => "ne tient pas",
        None => "sans objet",
    }
}

// --- La section ---------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct AttrSection {
    pub candidates: Vec<String>,
    pub controls: Controls,
    pub shares: Vec<ShareRow>,
    pub vertices: Vec<VertexRow>,
    pub counters: Vec<CounterRow>,
    pub service: Vec<ServiceRow>,
    pub verdicts: Vec<AttrVerdict>,
    pub rule_l: Option<Check>,
    /// Parmi les candidats qui tiennent A, B1, C1, C2 et E3, le réglage qui ferme la plus grande part moyenne de l'écart.
    pub chosen: Option<String>,
}

/// La douceur d'un réglage (§17.15) : la somme de ses poids ROAD et GRAVEL, puis sa tolérance de lentille ; plus petit, plus
/// doux. `rank:0` (aucune clé de rang) passe devant à poids et tolérance égaux.
fn softness(d: &Done) -> (u32, u16, bool) {
    let Some(a) = d.variant.org.and_then(|o| o.attr) else { return (u32::MAX, u16::MAX, true) };
    let sum = [RideType::Road, RideType::Gravel]
        .into_iter()
        .map(|r| a.weights(r).w.iter().map(|&x| u32::from(x)).sum::<u32>())
        .sum();
    let ltol = a.ltol_milli.unwrap_or(ord_loops::OrganicTune::default().attr_lens_tol_milli);
    (sum, ltol, !a.rank0)
}

/// Le choix du réglage (§17.15) : parmi les verdicts qui tiennent `ok`, ceux dont la part fermée moyenne est à 0,05 au plus
/// de la plus grande ; parmi eux, le plus doux (poids plus petits, puis `ltol` plus petit), puis l'ordre de la liste.
fn choose(verdicts: &[AttrVerdict], done: &[Done], ok: impl Fn(&AttrVerdict) -> bool) -> Option<String> {
    let held: Vec<(&AttrVerdict, f64)> =
        verdicts.iter().filter(|v| ok(v)).map(|v| (v, v.mean_closed.unwrap_or(0.0))).collect();
    let best = held.iter().map(|(_, m)| *m).fold(f64::NEG_INFINITY, f64::max);
    held.iter()
        .filter(|(_, m)| *m >= best - 0.05)
        .enumerate()
        .min_by_key(|(i, (v, _))| (find(done, &v.candidate).map(softness), *i))
        .map(|(_, (v, _))| v.candidate.clone())
}

pub fn attr_section(cmp: &Compare, done: &[Done]) -> Option<AttrSection> {
    let any = done.iter().any(|d| d.variant.org.is_some_and(|o| o.attr.is_some()));
    if !any && !has(done, REFERENCE) {
        return None;
    }
    let base = &done[super::compare::base_index(done)];
    let candidates: Vec<String> = done
        .iter()
        .filter(|d| {
            d.variant.org.is_some_and(|o| o.attr.is_some_and(|a| !a.zero))
                && d.variant.svc.is_some_and(|s| s.kind == super::variants::SvcKind::Svc)
        })
        .map(|d| d.variant.name.clone())
        .collect();
    let verdicts: Vec<AttrVerdict> =
        candidates.iter().filter_map(|n| find(done, n)).map(|d| attr_verdict(cmp, done, base, d)).collect();
    // Le choix du réglage (§17.15) : parmi ceux qui tiennent A, B1, C1, C2 et E3, la plus grande part fermée ; à 0,05 près, le
    // premier de la liste (la plus douce y est jouée avant).
    let ok = |v: &AttrVerdict| {
        ["A1", "A2", "A3", "A4", "A5", "B1", "C1", "C2", "E3"]
            .iter()
            .all(|id| v.checks.iter().find(|c| c.id == *id).and_then(|c| c.holds) == Some(true))
    };
    let chosen = choose(&verdicts, done, ok);
    Some(AttrSection {
        candidates,
        controls: controls(&cmp.conditions, done, base),
        shares: share_rows(done, base),
        vertices: vertex_rows(done, base),
        counters: counter_rows(done),
        service: service_rows(cmp, done),
        rule_l: rule_l_prime(&verdicts),
        verdicts,
        chosen,
    })
}

// --- Rendu --------------------------------------------------------------------------------------------------------------

pub fn section_md(s: &mut String, cmp: &Compare) {
    let Some(a) = &cmp.attr else { return };
    let names: Vec<&str> =
        a.shares.first().map_or(Vec::new(), |r| r.cells.iter().map(|c| c.variant.as_str()).collect());
    let _ = writeln!(s, "## 11. 4d : attracteurs OSM (lot 4, §17.11)\n");
    let _ = writeln!(s, "### Contrôles (arrêt de la campagne si l'un manque)\n");
    let c = &a.controls;
    for k in [&c.base, &c.common, &c.svc, &c.i5_org, &c.i5_attr0].into_iter().flatten().chain(c.sonde.iter())
    {
        let _ = writeln!(s, "- {} : {} : **{}** ({})", k.id, k.label, f3b(k.holds), k.detail);
    }
    if c.stop.is_empty() {
        let _ = writeln!(s, "- aucun contrôle ne manque\n");
    } else {
        let _ = writeln!(s, "- **ARRÊT** : {}\n", c.stop.join(" ; "));
    }
    let _ = writeln!(
        s,
        "### 1. Part des points lointains sur un attracteur\n\nPremier parcours gardé, lecture « toutes ». Cellule : `part · Δ à svc-org [IC 90 %] · part de l'écart fermée`. Réel : les boucles réelles du groupe.\n"
    );
    for scope in ["toutes", "A", "hors A"] {
        for radius in ["300 m", "1 km"] {
            let rows: Vec<&ShareRow> =
                a.shares.iter().filter(|r| r.scope == scope && r.radius == radius).collect();
            if rows.is_empty() {
                continue;
            }
            let _ = writeln!(s, "#### Équipes : {scope}, rayon {radius}\n");
            let _ = writeln!(
                s,
                "| type | curseur | groupe | réel (n) |{}\n|---|---:|---|---:|{}",
                names.iter().map(|n| format!(" {n} |")).collect::<String>(),
                names.iter().map(|_| "---:|").collect::<String>()
            );
            for r in rows {
                let _ = writeln!(
                    s,
                    "| {} | {} | {} | {} ({}) |{}",
                    r.ty,
                    r.cursor,
                    r.group,
                    f3(r.real),
                    r.n_real,
                    r.cells
                        .iter()
                        .map(|c| format!(
                            " {}{}{} |",
                            f3(c.share),
                            c.delta.map_or(String::new(), |d| format!(
                                " · {d:+.3}{}",
                                c.ci90.map_or(String::new(), |i| format!(" [{:+.3} ; {:+.3}]", i[0], i[1]))
                            )),
                            c.closed.map_or(String::new(), |x| format!(" · {x:.2}"))
                        ))
                        .collect::<String>()
                );
            }
            let _ = writeln!(s);
        }
    }
    let _ = writeln!(s, "### 2. Sommets à 10 % de D sur un attracteur (300 m), part des sommets\n");
    for scope in ["toutes", "A", "hors A"] {
        let rows: Vec<&VertexRow> = a.vertices.iter().filter(|r| r.scope == scope).collect();
        if rows.is_empty() {
            continue;
        }
        let _ = writeln!(s, "#### Équipes : {scope}\n");
        let _ = writeln!(
            s,
            "| type | curseur | groupe | réel (sommets) |{}\n|---|---:|---|---:|{}",
            names.iter().map(|n| format!(" {n} |")).collect::<String>(),
            names.iter().map(|_| "---:|").collect::<String>()
        );
        for r in rows {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} ({}) |{}",
                r.ty,
                r.cursor,
                r.group,
                f3(r.real),
                r.real_vertices,
                r.cells.iter().map(|c| format!(" {} ({}) |", f3(c.share), c.vertices)).collect::<String>()
            );
        }
        let _ = writeln!(s);
    }
    let _ = writeln!(
        s,
        "### 3. Compteurs par requête\n\n| variante | type | requêtes | candidats ajoutés | paires pesantes cherchées | demi-tours remplacés | requêtes touchées |\n|---|---|---:|---:|---:|---:|---:|"
    );
    for r in &a.counters {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {:.2} | {:.2} | {:.2} | {:.1} % |",
            r.variant,
            r.ty,
            r.requests,
            r.candidates_mean,
            r.pairs_searched_mean,
            r.lens_picks_mean,
            100.0 * r.touched_share
        );
    }
    let _ = writeln!(
        s,
        "\n### 4. Service\n\n| variante | curseur | P50 ms | P90 ms | P50 × svc-org | P90 × svc-org | P50 × svc | P90 × svc | OK | PANIC | ESCAPE | VIOLATION |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"
    );
    for r in &a.service {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {:.1} % | {} | {} | {} |",
            r.variant,
            r.cursor,
            f3(r.ms_p50),
            f3(r.ms_p90),
            f3(r.p50_x_org),
            f3(r.p90_x_org),
            f3(r.p50_x_svc),
            f3(r.p90_x_svc),
            100.0 * r.ok_share,
            r.panic,
            r.window_escape,
            r.violation
        );
    }
    let _ = writeln!(s, "\n### 5. Critères du §17.12\n");
    for v in &a.verdicts {
        let _ = writeln!(
            s,
            "#### `{}` : critères de la bascule {} ; part fermée moyenne (E1, E2) {}\n",
            v.candidate,
            match v.switch_criteria_hold {
                Some(true) => "**tenus**",
                Some(false) => "**non tenus**",
                None => "indéterminés",
            },
            f3(v.mean_closed)
        );
        for k in &v.checks {
            let _ = writeln!(s, "- **{}** {} : **{}** — {}", k.id, k.label, f3b(k.holds), k.detail);
        }
        let _ = writeln!(s);
    }
    if let Some(k) = &a.rule_l {
        let _ = writeln!(s, "- **{}** {} : {}\n", k.id, k.label, k.detail);
    }
    let _ = writeln!(s, "Réglage retenu (§17.15) : {}\n", a.chosen.as_deref().unwrap_or("aucun"));
}

#[cfg(test)]
mod tests {
    use super::super::compare::{compare, markdown};
    use super::super::measure::{AttrMasks, Measures};
    use super::super::play::{PlayRec, Recovered};
    use super::super::taste::TasteParams;
    use super::super::variants::{parse_variants, TeamTime, TeamWindowCounters, VariantMeta};
    use super::*;

    /// Un parcours dont le point lointain porte `far` (masque d'attracteurs).
    fn route(far: u8) -> GenRoute {
        GenRoute {
            m: Measures {
                attr: Some(AttrMasks { far, far_1k: far, v10: vec![far, 0] }),
                ..Measures::default()
            },
            length_over_target: 1.0,
            team_share: 0.5,
            family: "Triangle".into(),
            recovered: Recovered::default(),
            shared_vs_route_m: 0.0,
            poly_vs_graph: 0.0,
            route_xxh3: "0".into(),
        }
    }

    fn play(cursor: f64, far: u8) -> PlayRec {
        PlayRec {
            cursor,
            overlay: "full".into(),
            outcome: "OK".into(),
            n_routes: 1,
            n_kept: 1,
            dropped: Default::default(),
            ms: 100.0,
            gen_ms: None,
            steer: None,
            routes: vec![route(far)],
            best_shape: None,
            best_recovered: None,
            first_class_m: None,
            fig: None,
        }
    }

    /// 100 boucles ROAD (80 de l'équipe A) ; la boucle `i` a un point lointain de masque `mask(i)`.
    fn loops(real: &dyn Fn(usize) -> u8, gen: &dyn Fn(usize) -> u8) -> Vec<LoopRec> {
        (0..100)
            .map(|i| LoopRec {
                team: if i < 80 { "A".into() } else { "B".into() },
                i: i as u32,
                ty: "ROAD".into(),
                d_km: 50.0,
                matched_share: 1.0,
                contiguous: true,
                holdout: false,
                twin_share: None,
                poly_vs_graph: 0.0,
                real: Measures {
                    attr: Some(AttrMasks { far: real(i), far_1k: real(i), v10: vec![real(i)] }),
                    ..Measures::default()
                },
                real_check: None,
                plays: vec![play(0.25, gen(i)), play(1.0, gen(i))],
            })
            .collect()
    }

    fn done(name: &str, fingerprint: &str, loops: Vec<LoopRec>) -> Done {
        let variant =
            parse_variants(&["base".to_string(), name.to_string()]).unwrap().into_iter().last().unwrap();
        let meta = VariantMeta {
            name: variant.name.clone(),
            fam: None,
            fam_road_only: false,
            taste: None,
            gamma: None,
            params: TasteParams::default(),
            tables: Vec::new(),
            taste_xxh3: "0".into(),
            team_windows: TeamWindowCounters::default(),
            secs: 1.0,
            peak_anon_mb: 1.0,
            chain: None,
            l1_after: Vec::new(),
            class_meters: crate::likeness::variants::ClassMetersOut { pooled: Vec::new() },
            svc: None,
            team_times: vec![TeamTime {
                team: "A".into(),
                loops: 10,
                plays_full: 20,
                cpu_s_full: 1.0,
                cpu_s_full_by_cursor: vec![(0.25, 0.5), (1.0, 0.5)],
                build_s: 1.0,
                wall_s: 2.0,
            }],
        };
        Done { variant, meta, fingerprint: fingerprint.into(), loops }
    }

    fn base_done(real: &dyn Fn(usize) -> u8) -> Done {
        let mut d = done("svc", "b", loops(real, &|_| 0));
        d.variant = super::super::variants::Variant::base();
        d
    }

    fn conds(full: bool) -> Conditions {
        Conditions {
            commit: "t".into(),
            cursors: vec![0.25, 1.0],
            params: TasteParams::default(),
            at_4a_defaults: false,
            base_4a_fingerprint: "x".into(),
            holdout_share: 0.25,
            step1_check: None,
            all_check_applies: full,
        }
    }

    /// Réel : commerce (bit 0) pour 20 boucles, point de vue (bit 2) pour 5 ; `svc-org` : 10 et 2 ; la variante : 20 et 5.
    fn real_mask(i: usize) -> u8 {
        if i.is_multiple_of(5) {
            0b1
        } else if i % 20 == 1 {
            0b100
        } else {
            0
        }
    }
    fn org_mask(i: usize) -> u8 {
        if i.is_multiple_of(10) {
            0b1
        } else if i % 50 == 1 {
            0b100
        } else {
            0
        }
    }

    /// Test 20 du 4d : parts, part de l'écart fermée, écart apparié et son intervalle (déterministe), arrêt sur un contrôle
    /// manqué.
    #[test]
    fn the_4d_section_reads_shares_the_closed_part_and_stops_on_a_missing_control() {
        let all = vec![
            base_done(&real_mask),
            done("svc+org", "o", loops(&real_mask, &org_mask)),
            done("svc+org+attr", "a", loops(&real_mask, &real_mask)),
        ];
        let cmp = compare(&conds(false), &all);
        let a = cmp.attr.as_ref().expect("section 4d");
        assert_eq!(a.candidates, vec!["svc-org-attr".to_string()]);
        let row = a
            .shares
            .iter()
            .find(|r| {
                (r.scope.as_str(), r.ty.as_str(), r.group.as_str(), r.radius.as_str(), r.cursor)
                    == ("toutes", "ROAD", "commerce", "300 m", 0.25)
            })
            .expect("ligne ROAD commerce");
        assert_eq!((row.real, row.n_real), (Some(0.2), 100));
        let cell = |v: &str| row.cells.iter().find(|c| c.variant == v).unwrap();
        assert_eq!(cell("svc-org").share, Some(0.1));
        let cand = cell("svc-org-attr");
        assert_eq!(cand.share, Some(0.2));
        // Écart apparié : +0,10, et la part fermée de l'écart au réel : (0,2 − 0,1) / (0,2 − 0,1) = 1.
        assert!((cand.delta.unwrap() - 0.10).abs() < 1e-9, "{:?}", cand.delta);
        assert!((cand.closed.unwrap() - 1.0).abs() < 1e-9);
        let ci = cand.ci90.unwrap();
        assert!(ci[0] < 0.10 && 0.10 < ci[1], "{ci:?}");
        // La référence n'a ni écart ni part fermée.
        assert!(cell("svc-org").delta.is_none() && cell("svc-org").closed.is_none());
        // Déterministe : la même comparaison redonne le même intervalle.
        let again = compare(&conds(false), &all);
        let ci2 = again
            .attr
            .unwrap()
            .shares
            .iter()
            .find(|r| {
                r.group == "commerce"
                    && r.ty == "ROAD"
                    && r.scope == "toutes"
                    && r.radius == "300 m"
                    && r.cursor == 0.25
            })
            .unwrap()
            .cells[2]
            .ci90
            .unwrap();
        assert_eq!(ci, ci2);
        // Sans l'équipe A : 20 boucles de B (indices 80 à 99).
        let b_only = a
            .shares
            .iter()
            .find(|r| {
                r.scope == "hors A"
                    && r.ty == "ROAD"
                    && r.group == "commerce"
                    && r.radius == "300 m"
                    && r.cursor == 0.25
            })
            .unwrap();
        assert_eq!(b_only.n_real, 20);
        // Les sommets : un masque à 300 m par sommet (un sur deux porte le masque du point lointain pour les parcours générés).
        let v = a
            .vertices
            .iter()
            .find(|r| r.scope == "toutes" && r.ty == "ROAD" && r.group == "commerce" && r.cursor == 1.0)
            .unwrap();
        assert_eq!(v.real_vertices, 100);
        assert_eq!(v.cells.iter().find(|c| c.variant == "svc-org-attr").unwrap().vertices, 200);
        // E1 tient (0,20 ≥ 0,10 + 0,5 × 0,10), E3 tient (0,20 ≤ 0,20 + 0,03).
        let verdict = &a.verdicts[0];
        let hold = |id: &str| verdict.checks.iter().find(|c| c.id == id).and_then(|c| c.holds);
        assert_eq!(hold("E1"), Some(true));
        assert_eq!(hold("E3"), Some(true));
        // Le rendu cite la section.
        assert!(markdown(&cmp, &all).contains("## 11. 4d : attracteurs OSM"));
        // Contrôles : à portée complète, une empreinte qui n'est pas celle de la sonde arrête la campagne.
        let full = compare(&conds(true), &all);
        let stop = full.attr.unwrap().controls.stop;
        assert!(!stop.is_empty(), "base, svc-org et la sonde ne sont pas ceux du relevé");
        // Sans portée complète, aucun contrôle.
        assert!(cmp.attr.as_ref().unwrap().controls.stop.is_empty());
    }

    /// Le choix du réglage (§17.15) : la plus grande part fermée, et à 0,05 près le plus doux (poids, puis `ltol`), quel que
    /// soit l'ordre de la liste.
    #[test]
    fn the_chosen_setting_is_the_softest_within_0_05_of_the_best() {
        let all = vec![
            base_done(&real_mask),
            done("svc+org+attr", "a", Vec::new()),
            done("svc+org+attr+ltol:20", "b", Vec::new()),
            done("svc+org+attr+rcom:2", "c", Vec::new()),
        ];
        let v = |name: &str, m: f64| AttrVerdict {
            candidate: name.into(),
            checks: Vec::new(),
            switch_criteria_hold: None,
            mean_closed: Some(m),
        };
        let pick = |ms: [f64; 3]| {
            let vs =
                [v("svc-org-attr", ms[0]), v("svc-org-attr-ltol20", ms[1]), v("svc-org-attr-rcom2", ms[2])];
            choose(&vs, &all, |_| true)
        };
        // `rcom:2` ferme nettement plus : il l'emporte, si fort soit-il.
        assert_eq!(pick([0.60, 0.57, 0.70]).as_deref(), Some("svc-org-attr-rcom2"));
        // Tous à 0,05 du meilleur : le plus doux, `ltol:20` (mêmes poids que `attr`, tolérance plus petite), joué après `attr`.
        assert_eq!(pick([0.60, 0.57, 0.62]).as_deref(), Some("svc-org-attr-ltol20"));
        // `ltol:20` trop loin : `attr` (poids plus petits que `rcom:2`).
        assert_eq!(pick([0.60, 0.50, 0.62]).as_deref(), Some("svc-org-attr"));
        // Aucun verdict ne tient : aucun choix.
        let vs = [v("svc-org-attr", 0.9)];
        assert_eq!(choose(&vs, &all, |_| false), None);
    }
}
