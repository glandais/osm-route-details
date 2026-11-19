//! Section « 4c » de la comparaison (lot 4, §15.11.4) : les figures organiques contre `svc` du même tirage — formes, goût,
//! cases à la borne, service, compteurs des triangles — et les critères du §15.12, chacun « tient » ou « ne tient pas »
//! avec ses chiffres. **Lettres et agrégats seulement.**

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;

use super::compare::{getter, group, type_iqrs, w1_over, Compare, Done, Reading};
use super::compare_svc::ratio;
use super::out::LoopRec;
use super::play::{GenRoute, PlayRec};
use super::shape::{median, quantile_sorted, sorted};

/// Les groupes : type × curseur.
const GROUPS: [(&str, f64); 4] = [("ROAD", 0.25), ("ROAD", 1.0), ("GRAVEL", 0.25), ("GRAVEL", 1.0)];
const TYPES: [&str; 2] = ["ROAD", "GRAVEL"];
/// Les mesures de forme lues (§15.11.4, point 1).
const FORM_MEASURES: [&str; 8] = [
    "r_over_d",
    "compactness",
    "vertices_10",
    "far_pos",
    "detour_far",
    "sinuosity_10",
    "turns_per_10km",
    "shared_share",
];
/// Les mesures du critère A4 (le reste de la forme) : ces quatre-ci en W1 / IQR ≤ `svc` + 0,10, et `shared_share` en
/// absolu (décision 2 du §9.8, réponse 3 du §16.14 : le partage de route seul).
const A4_W1_MEASURES: [&str; 4] = ["far_pos", "detour_far", "sinuosity_10", "turns_per_10km"];
/// A4 en absolu pour `shared_share` : médiane et P90 du premier parcours gardé.
const SHARED_MEDIAN_MAX: f64 = 0.05;
const SHARED_P90_MARGIN: f64 = 0.05;

/// A4, `shared_share` : la médiane de la variante ≤ 0,05 **et** son P90 ≤ P90 de `svc` + 0,05, sur les premiers parcours
/// gardés d'un type et d'un curseur. `None` si l'un des deux échantillons est vide.
fn shared_share_holds(cand: &[f64], svc: &[f64]) -> Option<bool> {
    let (c, s) = (sorted(cand), sorted(svc));
    let (median, p90, svc_p90) =
        (quantile_sorted(&c, 0.5)?, quantile_sorted(&c, 0.9)?, quantile_sorted(&s, 0.9)?);
    Some(median <= SHARED_MEDIAN_MAX && p90 <= svc_p90 + SHARED_P90_MARGIN)
}
const SCOPES: [Option<&str>; 3] = [None, Some("A"), Some("B")];

/// Les valeurs de classes à la borne de la table commune (`[1/4, 4]` de 256).
const BOUNDS: [u16; 2] = [64, 1024];

fn scope_name(s: Option<&str>) -> String {
    s.unwrap_or("toutes").to_string()
}

fn has(done: &[Done], name: &str) -> bool {
    done.iter().any(|d| d.variant.name == name)
}

fn find<'a>(done: &'a [Done], name: &str) -> Option<&'a Done> {
    done.iter().find(|d| d.variant.name == name)
}

fn f3(x: Option<f64>) -> String {
    x.map_or("—".to_string(), |v| format!("{v:.3}"))
}

// --- Mesures ------------------------------------------------------------------------------------------------------------

/// Les valeurs de la mesure `m` du **premier** parcours gardé du groupe.
fn gen_vals(d: &Done, ty: &str, scope: Option<&str>, reading: Reading, cursor: f64, m: &str) -> Vec<f64> {
    let get = getter(m);
    group(&d.loops, ty, scope, reading, cursor)
        .iter()
        .filter_map(|(_, r)| r.and_then(|r| get(&r.m)))
        .collect()
}

fn real_vals(base: &Done, ty: &str, scope: Option<&str>, reading: Reading, cursor: f64, m: &str) -> Vec<f64> {
    let get = getter(m);
    group(&base.loops, ty, scope, reading, cursor).iter().filter_map(|(l, _)| get(&l.real)).collect()
}

/// `W1 / IQR` d'une variante sur une mesure, IQR du réel de tout le type.
fn w1(
    base: &Done,
    d: &Done,
    iqrs: &BTreeMap<&'static str, f64>,
    (ty, cursor): (&str, f64),
    scope: Option<&str>,
    reading: Reading,
    m: &str,
) -> Option<f64> {
    w1_over(
        &real_vals(base, ty, scope, reading, cursor, m),
        &gen_vals(d, ty, scope, reading, cursor, m),
        iqrs.get(m).copied().unwrap_or(0.0),
    )
}

#[derive(Clone, Debug, Serialize)]
pub struct FormCell {
    pub variant: String,
    pub p50: Option<f64>,
    pub w1_over_iqr: Option<f64>,
}

/// Une ligne des formes : (portée, lecture, type, curseur, mesure).
#[derive(Clone, Debug, Serialize)]
pub struct FormRow {
    pub scope: String,
    pub reading: String,
    pub ty: String,
    pub cursor: f64,
    pub measure: String,
    pub real_p50: Option<f64>,
    pub cells: Vec<FormCell>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShareCell {
    pub variant: String,
    pub n_first: usize,
    /// Part des premiers parcours à `vertices_10 = 2`, et part des premiers de famille `Triangle`.
    pub v2_share: Option<f64>,
    pub triangle_share: Option<f64>,
    /// r / D et compacité médians par famille du premier parcours.
    pub rd_p50_triangle: Option<f64>,
    pub rd_p50_lens: Option<f64>,
    pub comp_p50_triangle: Option<f64>,
    pub comp_p50_lens: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShareRow {
    pub scope: String,
    pub reading: String,
    pub ty: String,
    pub cursor: f64,
    /// Part des boucles réelles à `vertices_10 = 2`.
    pub real_v2_share: Option<f64>,
    pub cells: Vec<ShareCell>,
}

fn share_cell(d: &Done, g: &[(&LoopRec, Option<&GenRoute>)]) -> ShareCell {
    let firsts: Vec<&GenRoute> = g.iter().filter_map(|(_, r)| *r).collect();
    let n = firsts.len();
    let part = |k: usize| (n > 0).then(|| k as f64 / n as f64);
    let v2 = firsts.iter().filter(|r| r.m.vertices_10 == 2).count();
    let tri = |t: bool| firsts.iter().filter(move |r| (r.family == "Triangle") == t);
    let tri_n = tri(true).count();
    let med = |it: &mut dyn Iterator<Item = f64>| median(&it.collect::<Vec<f64>>());
    ShareCell {
        variant: d.variant.name.clone(),
        n_first: n,
        v2_share: part(v2),
        triangle_share: part(tri_n),
        rd_p50_triangle: med(&mut tri(true).map(|r| r.m.r_over_d)),
        rd_p50_lens: med(&mut tri(false).map(|r| r.m.r_over_d)),
        comp_p50_triangle: med(&mut tri(true).map(|r| r.m.compactness)),
        comp_p50_lens: med(&mut tri(false).map(|r| r.m.compactness)),
    }
}

// --- Cases à la borne ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct BorneRow {
    pub ty: String,
    pub class: usize,
    /// Valeur de `COMMON_Q8` : 64 ou 1 024.
    pub common_q8: u16,
    pub real_share: f64,
    pub svc_share: Option<f64>,
    pub org_share: Option<f64>,
    /// L'écart du généré au réel sous la variante dépasse max(0,02 ; 0,5 × réel) et celui de `svc` (B3).
    pub flagged: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BorneProfile {
    pub variant: String,
    pub team: String,
    /// Classes dont le multiplicateur du profil est à la borne cumulée `[1/4, 4]`, tous types.
    pub at_bound: usize,
}

fn pooled_shares(d: &Done, ride: &str) -> Option<(Vec<f64>, Vec<f64>)> {
    let t = d.meta.class_meters.pooled.iter().find(|t| t.ride == ride)?;
    let norm = |v: &[f64]| {
        let s: f64 = v.iter().sum();
        (s > 0.0).then(|| v.iter().map(|x| x / s).collect::<Vec<f64>>())
    };
    Some((norm(&t.real_m)?, norm(&t.gen_m)?))
}

fn borne_rows(done: &[Done], org: &str) -> Vec<BorneRow> {
    let (Some(svc), Some(cand)) = (find(done, "svc"), find(done, org)) else { return Vec::new() };
    let common = ord_loops::Taste::common();
    let mut out = Vec::new();
    for (ti, ty) in TYPES.iter().enumerate() {
        let (Some((real, gs)), Some((_, go))) = (pooled_shares(svc, ty), pooled_shares(cand, ty)) else {
            continue;
        };
        for (c, &q) in common.q8[ti].iter().enumerate() {
            if !BOUNDS.contains(&q) {
                continue;
            }
            let tol = (0.5 * real[c]).max(0.02);
            let (ds, dorg) = ((gs[c] - real[c]).abs(), (go[c] - real[c]).abs());
            out.push(BorneRow {
                ty: (*ty).into(),
                class: c,
                common_q8: q,
                real_share: real[c],
                svc_share: Some(gs[c]),
                org_share: Some(go[c]),
                flagged: dorg > tol && dorg > ds,
            });
        }
    }
    out
}

fn bound_profiles(done: &[Done]) -> Vec<BorneProfile> {
    let mut out = Vec::new();
    for d in done.iter().filter(|d| d.variant.svc.is_some_and(|s| s.kind == super::variants::SvcKind::Svc)) {
        for team in ["A", "B"] {
            let n: usize = d
                .meta
                .tables
                .iter()
                .filter(|t| t.source == team && t.overlay == "full")
                .map(|t| t.q8.iter().filter(|&&q| q <= BOUNDS[0] || q >= BOUNDS[1]).count())
                .sum();
            if d.meta.tables.iter().any(|t| t.source == team) {
                out.push(BorneProfile { variant: d.variant.name.clone(), team: team.into(), at_bound: n });
            }
        }
    }
    out
}

// --- Service ------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct OrgServiceRow {
    pub variant: String,
    pub cursor: f64,
    pub ms_p50: Option<f64>,
    pub ms_p90: Option<f64>,
    /// Contre `svc` du même tirage.
    pub p50_x_svc: Option<f64>,
    pub p90_x_svc: Option<f64>,
    pub ok_share: f64,
    pub panic: usize,
    pub window_escape: usize,
    pub violation: usize,
}

fn service_rows(cmp: &Compare, done: &[Done]) -> Vec<OrgServiceRow> {
    let reference = |c: f64| cmp.service.iter().find(|r| r.variant == "svc" && r.cursor == c);
    let mut out = Vec::new();
    for d in done.iter().filter(|d| d.variant.org.is_some() || d.variant.name == "svc") {
        for r in cmp.service.iter().filter(|r| r.variant == d.variant.name) {
            let svc = reference(r.cursor);
            let x = |a: Option<f64>, b: Option<f64>| match (a, b) {
                (Some(a), Some(b)) if b > 0.0 => Some(a / b),
                _ => None,
            };
            let n = |k: &str| r.outcomes.get(k).copied().unwrap_or(0);
            out.push(OrgServiceRow {
                variant: r.variant.clone(),
                cursor: r.cursor,
                ms_p50: r.ms_p50,
                ms_p90: r.ms_p90,
                p50_x_svc: x(r.ms_p50, svc.and_then(|s| s.ms_p50)),
                p90_x_svc: x(r.ms_p90, svc.and_then(|s| s.ms_p90)),
                ok_share: r.ok_share,
                panic: n("PANIC"),
                window_escape: n("WINDOW_ESCAPE"),
                violation: n("VIOLATION"),
            });
        }
    }
    out
}

// --- Compteurs des triangles --------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct TriRow {
    pub variant: String,
    pub ty: String,
    /// Jeux `full` où la génération organique a joué.
    pub requests: usize,
    pub tri_first_share: f64,
    /// Par requête : paires admissibles (moyenne), paires cherchées (moyenne), jambes 2 trouvées, abandonnées au plafond.
    pub admissible_mean: f64,
    pub searched_mean: f64,
    /// Dont les reprises d'une paire refusée pour un virage serré (moyenne par requête).
    pub retried_mean: f64,
    pub leg2_found_mean: f64,
    pub leg2_capped_mean: f64,
    /// Jambes 3 : chemin T2 gardé, A* d'arrivée, échec (totaux).
    pub leg3_t2: u64,
    pub leg3_astar: u64,
    pub leg3_failed: u64,
    /// Triangles complets, à ±10 % de D, gardés (notés) : totaux et rendement par requête.
    pub complete: u64,
    pub within_tol: u64,
    pub scored: u64,
    pub with_triangle_share: f64,
    /// Refus du contrôle interne par code (totaux).
    pub rejected: BTreeMap<String, u64>,
    pub spikes: u64,
    pub spikes_repaired: u64,
    pub spikes_dropped: u64,
    pub spikes_kept: u64,
    /// États fixés par requête (moyenne) : jambes 2 et jambes 3.
    pub p2p_settled_mean: f64,
    pub leg3_settled_mean: f64,
    /// β final : P10 · P50 · P90.
    pub beta: [Option<f64>; 3],
    /// Par requête (moyenne) : relaxations refusées par le modèle des virages, évaluées hors voie rapide (le coût), allers-retours
    /// refusés.
    pub turn_cut_mean: f64,
    pub turn_evals_mean: f64,
    pub reuse_cut_mean: f64,
}

/// Les refus du **contrôleur indépendant** sur les parcours rendus, par code, pour une variante et un type (tous les jeux) :
/// ce que le contrôle interne du moteur a laissé passer.
#[derive(Clone, Debug, Serialize)]
pub struct CtrlRow {
    pub variant: String,
    pub ty: String,
    /// Parcours rendus par le moteur, et refusés par le contrôleur (au moins un code).
    pub rendered: u64,
    pub refused: u64,
    pub by_code: BTreeMap<String, u64>,
}

fn ctrl_rows(done: &[Done]) -> Vec<CtrlRow> {
    let mut out = Vec::new();
    for d in done {
        for ty in TYPES {
            let plays: Vec<&PlayRec> =
                d.loops.iter().filter(|l| l.ty == ty).flat_map(|l| l.plays.iter()).collect();
            if plays.is_empty() {
                continue;
            }
            let mut by_code: BTreeMap<String, u64> = BTreeMap::new();
            for p in &plays {
                for (k, v) in &p.dropped {
                    *by_code.entry(k.clone()).or_default() += u64::from(*v);
                }
            }
            out.push(CtrlRow {
                variant: d.variant.name.clone(),
                ty: ty.into(),
                rendered: plays.iter().map(|p| p.n_routes as u64).sum(),
                refused: plays.iter().map(|p| (p.n_routes - p.n_kept) as u64).sum(),
                by_code,
            });
        }
    }
    out
}

fn tri_rows(done: &[Done]) -> Vec<TriRow> {
    let mut out = Vec::new();
    for d in done.iter().filter(|d| d.variant.org.is_some()) {
        for ty in TYPES {
            let plays: Vec<&PlayRec> = d
                .loops
                .iter()
                .filter(|l| l.ty == ty)
                .flat_map(|l| l.plays.iter().filter(|p| p.overlay == "full"))
                .filter(|p| p.fig.is_some())
                .collect();
            let n = plays.len();
            if n == 0 {
                continue;
            }
            let figs: Vec<&super::play::FigAgg> = plays.iter().filter_map(|p| p.fig.as_ref()).collect();
            let mean =
                |f: &dyn Fn(&super::play::FigAgg) -> f64| figs.iter().map(|x| f(x)).sum::<f64>() / n as f64;
            let sum =
                |f: &dyn Fn(&super::play::FigAgg) -> u32| figs.iter().map(|x| u64::from(f(x))).sum::<u64>();
            let mut rejected: BTreeMap<String, u64> = BTreeMap::new();
            for f in &figs {
                for (k, v) in &f.tri_rejected {
                    *rejected.entry(k.clone()).or_default() += u64::from(*v);
                }
            }
            let betas: Vec<f64> = figs.iter().filter(|f| f.searched > 0).map(|f| f.beta_final).collect();
            let sb = sorted(&betas);
            out.push(TriRow {
                variant: d.variant.name.clone(),
                ty: ty.into(),
                requests: n,
                tri_first_share: figs.iter().filter(|f| f.tri_first).count() as f64 / n as f64,
                admissible_mean: mean(&|f| f64::from(f.pairs_admissible)),
                searched_mean: mean(&|f| f64::from(f.searched)),
                retried_mean: mean(&|f| f64::from(f.retried)),
                leg2_found_mean: mean(&|f| f64::from(f.leg2_found)),
                leg2_capped_mean: mean(&|f| f64::from(f.leg2_capped)),
                leg3_t2: sum(&|f| f.leg3_t2),
                leg3_astar: sum(&|f| f.leg3_astar),
                leg3_failed: sum(&|f| f.leg3_failed),
                complete: sum(&|f| f.complete),
                within_tol: sum(&|f| f.within_tol),
                scored: sum(&|f| f.tri_scored),
                with_triangle_share: figs.iter().filter(|f| f.tri_scored > 0).count() as f64 / n as f64,
                rejected,
                spikes: sum(&|f| f.spikes),
                spikes_repaired: sum(&|f| f.spikes_repaired),
                spikes_dropped: sum(&|f| f.spikes_dropped),
                spikes_kept: sum(&|f| f.spikes_kept),
                p2p_settled_mean: mean(&|f| f.p2p_settled as f64),
                leg3_settled_mean: mean(&|f| f.leg3_settled as f64),
                beta: [quantile_sorted(&sb, 0.1), quantile_sorted(&sb, 0.5), quantile_sorted(&sb, 0.9)],
                turn_cut_mean: mean(&|f| f.turn_cut as f64),
                turn_evals_mean: mean(&|f| f.turn_evals as f64),
                reuse_cut_mean: mean(&|f| f.reuse_cut as f64),
            });
        }
    }
    out
}

// --- Critères du §15.12 -------------------------------------------------------------------------------------------------

/// Un critère : « tient » ou « ne tient pas », avec ses chiffres.
#[derive(Clone, Debug, Serialize)]
pub struct Check {
    pub id: String,
    pub label: String,
    pub holds: Option<bool>,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Verdict {
    pub candidate: String,
    pub checks: Vec<Check>,
    /// Les critères de la bascule (A1 à A4, B1, B2, C1, C2) tiennent tous ; D se lit à la campagne 3 (empreintes).
    pub switch_criteria_hold: Option<bool>,
    /// B2 : réserve levée (≤ +0,03 aux deux curseurs), maintenue (≤ +0,054), ou non tenue.
    pub reserve_d: Option<String>,
}

fn all_hold(v: &[Option<bool>]) -> Option<bool> {
    if v.iter().any(Option::is_none) {
        return None;
    }
    Some(v.iter().all(|x| *x == Some(true)))
}

fn verdict(cmp: &Compare, done: &[Done], cand: &Done) -> Verdict {
    let b = super::compare::base_index(done);
    let (base, svc) = (&done[b], find(done, "svc"));
    let name = cand.variant.name.clone();
    let mut checks: Vec<Check> = Vec::new();
    let Some(svc) = svc else {
        return Verdict { candidate: name, checks, switch_criteria_hold: None, reserve_d: None };
    };
    let iq: BTreeMap<&str, BTreeMap<&'static str, f64>> =
        TYPES.iter().map(|t| (*t, type_iqrs(&base.loops, t))).collect();
    let w = |d: &Done, g: (&str, f64), reading: Reading, m: &str| w1(base, d, &iq[g.0], g, None, reading, m);
    let mut push = |id: &str, label: &str, holds: Option<bool>, detail: String| {
        checks.push(Check { id: id.into(), label: label.into(), holds, detail });
    };
    // A1 : r / D.
    let mut a1: Vec<Option<bool>> = Vec::new();
    let mut d1 = Vec::new();
    for g in GROUPS {
        let (c, s) = (w(cand, g, Reading::All, "r_over_d"), w(svc, g, Reading::All, "r_over_d"));
        let max = if g.0 == "ROAD" { 0.75 } else { 0.85 };
        a1.push(c.zip(s).map(|(c, s)| c <= max * s));
        d1.push(format!(
            "{} {} : {} contre {} ({} ×, seuil {max})",
            g.0,
            g.1,
            f3(c),
            f3(s),
            f3(c.zip(s).map(|(c, s)| c / s))
        ));
    }
    push("A1", "r / D : W1 / IQR ≤ 0,75 × `svc` (ROAD), ≤ 0,85 × (GRAVEL)", all_hold(&a1), d1.join(" ; "));
    // A2 : figure.
    let (mut a2, mut d2) = (Vec::new(), Vec::new());
    for g in GROUPS {
        let gr = group(&cand.loops, g.0, None, Reading::All, g.1);
        let share = share_cell(cand, &gr).v2_share;
        let (lo, hi) = if g.0 == "ROAD" { (0.40, 0.58) } else { (0.32, 0.50) };
        let (c, s) = (w(cand, g, Reading::All, "vertices_10"), w(svc, g, Reading::All, "vertices_10"));
        a2.push(share.map(|x| (lo..=hi).contains(&x)));
        a2.push(c.zip(s).map(|(c, s)| c <= 0.5 * s));
        d2.push(format!(
            "{} {} : part à 2 sommets {} (borne [{lo} ; {hi}]), W1/IQR {} contre {}",
            g.0,
            g.1,
            f3(share),
            f3(c),
            f3(s)
        ));
    }
    push(
        "A2",
        "figure : part à `vertices_10 = 2` dans l'intervalle et W1 / IQR de `vertices_10` ≤ 0,5 × `svc`",
        all_hold(&a2),
        d2.join(" ; "),
    );
    // A3 : compacité.
    let (mut a3, mut d3) = (Vec::new(), Vec::new());
    for g in GROUPS {
        let (c, s) = (w(cand, g, Reading::All, "compactness"), w(svc, g, Reading::All, "compactness"));
        a3.push(c.zip(s).map(|(c, s)| if g.0 == "ROAD" { c <= 0.80 * s } else { c <= s + 0.05 }));
        d3.push(format!("{} {} : {} contre {}", g.0, g.1, f3(c), f3(s)));
    }
    let a3_road = all_hold(&[a3[0], a3[1]]);
    push("A3", "compacité : ROAD ≤ 0,80 × `svc`, GRAVEL ≤ `svc` + 0,05", all_hold(&a3), d3.join(" ; "));
    // A4 : le reste. Quatre mesures en W1 / IQR ≤ `svc` + 0,10, `shared_share` en absolu.
    let (mut a4, mut d4) = (Vec::new(), Vec::new());
    for m in A4_W1_MEASURES {
        for g in GROUPS {
            let (c, s) = (w(cand, g, Reading::All, m), w(svc, g, Reading::All, m));
            let ok = c.zip(s).map(|(c, s)| c <= s + 0.10);
            if ok != Some(true) {
                d4.push(format!("{m} {} {} : {} contre {}", g.0, g.1, f3(c), f3(s)));
            }
            a4.push(ok);
        }
    }
    for g in GROUPS {
        let vals = |d: &Done| gen_vals(d, g.0, None, Reading::All, g.1, "shared_share");
        let (c, s) = (vals(cand), vals(svc));
        let ok = shared_share_holds(&c, &s);
        if ok != Some(true) {
            d4.push(format!(
                "shared_share {} {} : médiane {} (≤ {SHARED_MEDIAN_MAX}), P90 {} contre `svc` {} (+ {SHARED_P90_MARGIN})",
                g.0,
                g.1,
                f3(median(&c)),
                f3(quantile_sorted(&sorted(&c), 0.9)),
                f3(quantile_sorted(&sorted(&s), 0.9)),
            ));
        }
        a4.push(ok);
    }
    push(
        "A4",
        "le reste de la forme : quatre mesures ≤ `svc` + 0,10 (W1 / IQR) ; `shared_share` en absolu, médiane ≤ 0,05 et P90 ≤ P90 de `svc` + 0,05 (quatre groupes)",
        all_hold(&a4),
        if d4.is_empty() { "toutes dans la marge".into() } else { d4.join(" ; ") },
    );
    // B1 : goût.
    let rows = &cmp.criterion;
    let (mut b1, mut d1b) = (Vec::new(), Vec::new());
    for reading in [Reading::All, Reading::Gap] {
        for g in GROUPS {
            let (c, s) = (ratio(rows, "toutes", reading, g, &name), ratio(rows, "toutes", reading, g, "svc"));
            let ok = c.zip(s).map(|(c, s)| c <= s + 0.03 && c <= 0.5);
            b1.push(ok);
            d1b.push(format!("{} {} ({}) : {} contre {}", g.0, g.1, reading.name(), f3(c), f3(s)));
        }
    }
    push(
        "B1",
        "`S / S_base` ≤ `svc` + 0,03 et ≤ 0,5, quatre groupes, « toutes » et « écart »",
        all_hold(&b1),
        d1b.join(" ; "),
    );
    // B2 : réserve du fait (d).
    let partner = "common-org";
    let diff = |form: &str, with: &str, c: f64| {
        Some(
            ratio(rows, "A", Reading::All, ("ROAD", c), form)?
                - ratio(rows, "A", Reading::All, ("ROAD", c), with)?,
        )
    };
    let d: Vec<Option<f64>> = [0.25, 1.0].iter().map(|&c| diff(&name, partner, c)).collect();
    let d_classic: Vec<Option<f64>> = [0.25, 1.0].iter().map(|&c| diff("svc", "common", c)).collect();
    let b2 = d.iter().all(Option::is_some).then(|| d.iter().flatten().all(|&x| x <= 0.054));
    let reserve = d.iter().all(Option::is_some).then(|| {
        if d.iter().flatten().all(|&x| x <= 0.03) {
            "levée".to_string()
        } else if b2 == Some(true) {
            "maintenue".to_string()
        } else {
            "non tenue : pas de bascule, la table commune organique est proposée au propriétaire".to_string()
        }
    });
    push(
        "B2",
        "réserve du fait (d) : A ROAD « toutes », `svc-org − common-org` ≤ +0,054 aux deux curseurs (levée ≤ +0,03)",
        b2,
        format!(
            "{} · {} (à côté de `svc − common` : {} · {}) : réserve {}",
            f3(d[0]),
            f3(d[1]),
            f3(d_classic[0]),
            f3(d_classic[1]),
            reserve.clone().unwrap_or_else(|| "sans objet (`common-org` absent)".into())
        ),
    );
    // C1, C2 : service.
    let srv = service_rows(cmp, done);
    let pick = |v: &str, c: f64| srv.iter().find(|r| r.variant == v && r.cursor == c);
    let (mut c1, mut dc1, mut c2, mut dc2) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for c in [0.25, 1.0] {
        let (Some(r), Some(s)) = (pick(&name, c), pick("svc", c)) else { continue };
        c1.push(r.p50_x_svc.map(|x| x <= 1.15).zip(r.p90_x_svc.map(|x| x <= 1.25)).map(|(a, b)| a && b));
        dc1.push(format!(
            "curseur {c} : P50 {} × (≤ 1,15), P90 {} × (≤ 1,25)",
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
    let c2h = (!c2.is_empty()).then(|| all_hold(&c2)).flatten();
    push(
        "C1",
        "temps au banc : P50 ≤ 1,15 × `svc`, P90 ≤ 1,25 × `svc` (à chaque curseur)",
        c1h,
        dc1.join(" ; "),
    );
    push(
        "C2",
        "issues : `OK` ≥ `svc` − 0,5 point, ni PANIC ni WINDOW_ESCAPE, VIOLATION ≤ `svc`",
        c2h,
        dc2.join(" ; "),
    );
    push("C3", "conteneur de 3 Gio, après la bascule", None, "séance en conteneur, hors banc".into());
    push(
        "D",
        "déterminisme : mêmes empreintes à 1 et 16 fils (B et C)",
        None,
        "campagne 3 : comparer les empreintes de `svc-org` à 1 et 16 fils".into(),
    );
    let bascule =
        all_hold(&[all_hold(&a1), all_hold(&a2), a3_road, all_hold(&a4), all_hold(&b1), b2, c1h, c2h]);
    Verdict { candidate: name, checks, switch_criteria_hold: bascule, reserve_d: reserve }
}

/// Règle L (apprentissage) : `svc-org-lc` contre `svc-org`.
fn rule_l(cmp: &Compare, done: &[Done]) -> Option<Check> {
    if !(has(done, "svc-org-lc") && has(done, "svc-org")) {
        return None;
    }
    let rows = &cmp.criterion;
    let mut worst = f64::MIN;
    let mut ok = Vec::new();
    for g in GROUPS {
        let (a, b) = (
            ratio(rows, "toutes", Reading::All, g, "svc-org-lc"),
            ratio(rows, "toutes", Reading::All, g, "svc-org"),
        );
        if let (Some(a), Some(b)) = (a, b) {
            worst = worst.max(a - b);
            ok.push(Some(a <= b + 0.02));
        } else {
            ok.push(None);
        }
    }
    let d: Vec<Option<f64>> = [0.25, 1.0]
        .iter()
        .map(|&c| {
            Some(
                ratio(rows, "A", Reading::All, ("ROAD", c), "svc-org-lc")?
                    - ratio(rows, "A", Reading::All, ("ROAD", c), "common-org")?,
            )
        })
        .collect();
    let b2 = d.iter().all(Option::is_some).then(|| d.iter().flatten().all(|&x| x <= 0.054));
    let holds = all_hold(&ok).zip(b2).map(|(a, b)| a && b);
    Some(Check {
        id: "L".into(),
        label: "règle L : `svc-org-lc` ≤ `svc-org` + 0,02 (quatre groupes « toutes ») et B2 tient pour `lc` : l'apprentissage reste `classic`".into(),
        holds,
        detail: format!("pire écart {:+.3} ; B2 de `lc` : {} · {}", worst, f3(d[0]), f3(d[1])),
    })
}

// --- La section ---------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct Neutrality {
    pub svc_fingerprint: Option<String>,
    pub neutral_variant: Option<String>,
    pub neutral_fingerprint: Option<String>,
    /// `svc+org+tri:0+w:0+turns:0+reuse:0` rend l'empreinte de `svc` (I2 au banc).
    pub equal: Option<bool>,
    /// `svc+org+turns:0+reuse:0` rend l'organique d'avant l'étape « virages » (I2′) : sa variante, son empreinte, et si elle est
    /// l'une des deux relevées (toutes équipes, ou l'essai B et C ; la portée exacte est contrôlée à l'exécution).
    pub i2p_variant: Option<String>,
    pub i2p_fingerprint: Option<String>,
    pub i2p_equal: Option<bool>,
    pub base_fingerprint: Option<String>,
    pub base_is_sonde: Option<bool>,
    pub svc_is_sonde: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OrgSection {
    pub candidates: Vec<String>,
    pub neutrality: Neutrality,
    pub forms: Vec<FormRow>,
    pub shares: Vec<ShareRow>,
    pub service: Vec<OrgServiceRow>,
    pub borne: Vec<BorneRow>,
    pub bound_profiles: Vec<BorneProfile>,
    pub triangles: Vec<TriRow>,
    /// Refus du contrôleur indépendant sur les parcours rendus (attendu : aucun `SHARP_TURN` en organique).
    pub controller: Vec<CtrlRow>,
    pub verdicts: Vec<Verdict>,
    pub rule_l: Option<Check>,
}

fn neutrality(done: &[Done]) -> Neutrality {
    let fp = |n: &str| find(done, n).map(|d| d.fingerprint.clone());
    let neutral = done
        .iter()
        .find(|d| d.variant.org.is_some_and(|o| o.tri0 && o.w_milli == Some(0) && o.turns0 && o.reuse0));
    let i2p = done.iter().find(|d| {
        d.variant.org.is_some_and(|o| o.turns0 && o.reuse0 && !o.tri0 && o.w_milli.is_none() && !o.lc)
            && d.variant.svc.is_some_and(|s| s.kind == super::variants::SvcKind::Svc)
    });
    let svc = fp("svc");
    Neutrality {
        i2p_variant: i2p.map(|d| d.variant.name.clone()),
        i2p_fingerprint: i2p.map(|d| d.fingerprint.clone()),
        i2p_equal: i2p.map(|d| {
            d.fingerprint == super::I2P_FULL_FINGERPRINT || d.fingerprint == super::I2P_TRIAL_FINGERPRINT
        }),
        equal: neutral.zip(svc.as_ref()).map(|(n, s)| &n.fingerprint == s),
        neutral_variant: neutral.map(|d| d.variant.name.clone()),
        neutral_fingerprint: neutral.map(|d| d.fingerprint.clone()),
        base_is_sonde: fp("base").map(|f| f == super::SONDE_4C_BASE_FINGERPRINT),
        base_fingerprint: fp("base"),
        svc_is_sonde: svc.as_ref().map(|f| f == super::SONDE_4C_SVC_FINGERPRINT),
        svc_fingerprint: svc,
    }
}

/// La section du 4c : `None` sans variante `org`.
pub fn org_section(cmp: &Compare, done: &[Done]) -> Option<OrgSection> {
    if !done.iter().any(|d| d.variant.org.is_some()) {
        return None;
    }
    let b = super::compare::base_index(done);
    let base = &done[b];
    let iq: BTreeMap<&str, BTreeMap<&'static str, f64>> =
        TYPES.iter().map(|t| (*t, type_iqrs(&base.loops, t))).collect();
    let mut forms = Vec::new();
    let mut shares = Vec::new();
    for scope in SCOPES {
        for reading in [Reading::All, Reading::Gap] {
            for g in GROUPS {
                let g0 = group(&base.loops, g.0, scope, reading, g.1);
                if g0.is_empty() {
                    continue;
                }
                for m in FORM_MEASURES {
                    forms.push(FormRow {
                        scope: scope_name(scope),
                        reading: reading.name().into(),
                        ty: g.0.into(),
                        cursor: g.1,
                        measure: m.into(),
                        real_p50: median(&real_vals(base, g.0, scope, reading, g.1, m)),
                        cells: done
                            .iter()
                            .map(|d| FormCell {
                                variant: d.variant.name.clone(),
                                p50: median(&gen_vals(d, g.0, scope, reading, g.1, m)),
                                w1_over_iqr: w1(base, d, &iq[g.0], g, scope, reading, m),
                            })
                            .collect(),
                    });
                }
                let real_v2 = {
                    let v = real_vals(base, g.0, scope, reading, g.1, "vertices_10");
                    (!v.is_empty()).then(|| v.iter().filter(|&&x| x == 2.0).count() as f64 / v.len() as f64)
                };
                shares.push(ShareRow {
                    scope: scope_name(scope),
                    reading: reading.name().into(),
                    ty: g.0.into(),
                    cursor: g.1,
                    real_v2_share: real_v2,
                    cells: done
                        .iter()
                        .map(|d| share_cell(d, &group(&d.loops, g.0, scope, reading, g.1)))
                        .collect(),
                });
            }
        }
    }
    let candidates: Vec<String> = done
        .iter()
        .filter(|d| {
            d.variant.org.is_some() && d.variant.svc.is_some_and(|s| s.kind == super::variants::SvcKind::Svc)
        })
        .map(|d| d.variant.name.clone())
        .collect();
    let verdicts =
        candidates.iter().filter_map(|n| find(done, n)).map(|c| verdict(cmp, done, c)).collect::<Vec<_>>();
    let borne = candidates.first().map(|c| borne_rows(done, c)).unwrap_or_default();
    Some(OrgSection {
        neutrality: neutrality(done),
        forms,
        shares,
        service: service_rows(cmp, done),
        borne,
        bound_profiles: bound_profiles(done),
        triangles: tri_rows(done),
        controller: ctrl_rows(done),
        rule_l: rule_l(cmp, done),
        verdicts,
        candidates,
    })
}

// --- Rendu --------------------------------------------------------------------------------------------------------------

fn yes(h: Option<bool>) -> &'static str {
    match h {
        Some(true) => "tient",
        Some(false) => "NE TIENT PAS",
        None => "non évalué",
    }
}

/// La section « 4c » du résumé.
pub fn section_md(s: &mut String, cmp: &Compare) {
    let Some(o) = &cmp.org else { return };
    let names: Vec<&str> = cmp.variants.iter().map(|v| v.name.as_str()).collect();
    let _ = writeln!(s, "## 10. 4c : figures organiques (lot 4, §15.11.4)\n");
    let n = &o.neutrality;
    let _ = writeln!(s, "### Contrôles de neutralité\n");
    let _ = writeln!(
        s,
        "- `base` = sonde 4c (`{}`) : {} ; `svc` = sonde 4c (`{}`) : {}",
        super::SONDE_4C_BASE_FINGERPRINT,
        n.base_is_sonde.map_or("sans objet", |b| if b { "oui" } else { "NON" }),
        super::SONDE_4C_SVC_FINGERPRINT,
        n.svc_is_sonde.map_or("sans objet", |b| if b { "oui" } else { "NON" }),
    );
    let _ = writeln!(
        s,
        "- I2 : `{}` (`svc+org+tri:0+w:0+turns:0+reuse:0`) rend l'empreinte de `svc` : {} (`{}` contre `{}`)",
        n.neutral_variant.as_deref().unwrap_or("absent"),
        n.equal.map_or("sans objet", |b| if b { "oui" } else { "NON" }),
        n.neutral_fingerprint.as_deref().unwrap_or("-"),
        n.svc_fingerprint.as_deref().unwrap_or("-"),
    );
    let _ = writeln!(
        s,
        "- I2′ : `{}` (`svc+org+turns:0+reuse:0`) rend l'organique d'avant l'étape, `{}` (toutes équipes) ou `{}` (essai B et C) : {} (`{}`)\n",
        n.i2p_variant.as_deref().unwrap_or("absent"),
        super::I2P_FULL_FINGERPRINT,
        super::I2P_TRIAL_FINGERPRINT,
        n.i2p_equal.map_or("sans objet", |b| if b { "oui" } else { "NON" }),
        n.i2p_fingerprint.as_deref().unwrap_or("-"),
    );
    // 1. Formes.
    let _ = writeln!(
        s,
        "### 1. Formes : P50 et W1 / IQR (IQR du réel du type), premier parcours gardé\n\nChaque cellule : `P50 · W1/IQR`. Lectures « toutes » et « écart », toutes équipes puis A et B.\n"
    );
    for scope in ["toutes", "A", "B"] {
        for reading in ["toutes", "écart"] {
            let rows: Vec<&FormRow> =
                o.forms.iter().filter(|r| r.scope == scope && r.reading == reading).collect();
            if rows.is_empty() {
                continue;
            }
            let _ = writeln!(s, "#### Équipes : {scope}, lecture « {reading} »\n");
            let _ = writeln!(
                s,
                "| type | curseur | mesure | réel P50 |{}\n|---|---:|---|---:|{}",
                names.iter().map(|n| format!(" {n} |")).collect::<String>(),
                names.iter().map(|_| "---:|").collect::<String>()
            );
            for r in rows {
                let _ = writeln!(
                    s,
                    "| {} | {} | {} | {} |{}",
                    r.ty,
                    r.cursor,
                    r.measure,
                    f3(r.real_p50),
                    r.cells
                        .iter()
                        .map(|c| format!(" {} · {} |", f3(c.p50), f3(c.w1_over_iqr)))
                        .collect::<String>()
                );
            }
            s.push('\n');
        }
    }
    let _ = writeln!(
        s,
        "#### Parts des figures et médianes par famille\n\n`v2` : part des premiers à `vertices_10 = 2` ; `tri` : part des premiers de famille `Triangle` ; `r/D tri · lentille` et `compacité tri · lentille` : médianes par famille du premier parcours.\n"
    );
    let _ = writeln!(
        s,
        "| équipes | lecture | type | curseur | réel v2 |{}\n|---|---|---|---:|---:|{}",
        names
            .iter()
            .map(|n| format!(" {n} : v2 · tri · r/D tri · r/D lentille · comp. tri · comp. lentille |"))
            .collect::<String>(),
        names.iter().map(|_| "---|").collect::<String>()
    );
    for r in &o.shares {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} |{}",
            r.scope,
            r.reading,
            r.ty,
            r.cursor,
            f3(r.real_v2_share),
            r.cells
                .iter()
                .map(|c| format!(
                    " {} · {} · {} · {} · {} · {} |",
                    f3(c.v2_share),
                    f3(c.triangle_share),
                    f3(c.rd_p50_triangle),
                    f3(c.rd_p50_lens),
                    f3(c.comp_p50_triangle),
                    f3(c.comp_p50_lens)
                ))
                .collect::<String>()
        );
    }
    s.push('\n');
    // 2. Goût.
    let _ = writeln!(s, "### 2. Goût : `S / S_base`, fait (d) organique, cases à la borne\n");
    let _ = writeln!(s, "`S / S_base` : voir les sections 3 et 5 (toutes les variantes y figurent) ; le fait (d) est lu en B2 ci-dessous.\n");
    if !o.borne.is_empty() {
        let _ = writeln!(s, "Cases à la borne de la table commune (`COMMON_Q8` vaut 64 ou 1 024), part de la longueur, toutes équipes :\n\n| type | classe | `COMMON_Q8` | réel | `svc` | candidate | signalée (B3) |\n|---|---:|---:|---:|---:|---:|---|");
        for r in &o.borne {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {:.3} | {} | {} | {} |",
                r.ty,
                r.class,
                r.common_q8,
                r.real_share,
                f3(r.svc_share),
                f3(r.org_share),
                if r.flagged { "**oui**" } else { "non" }
            );
        }
        s.push('\n');
    }
    if !o.bound_profiles.is_empty() {
        let _ = writeln!(s, "Classes à la borne cumulée `[1/4, 4]` des profils appris, tous types :\n");
        for p in &o.bound_profiles {
            let _ = writeln!(s, "- `{}`, équipe {} : {} classes", p.variant, p.team, p.at_bound);
        }
        s.push('\n');
    }
    // 3. Service.
    let _ = writeln!(s, "### 3. Service : temps et issues contre `svc` du même tirage\n\n| variante | curseur | ms P50 / P90 | P50 × `svc` | P90 × `svc` | `OK` | PANIC | WINDOW_ESCAPE | VIOLATION |\n|---|---:|---|---:|---:|---:|---:|---:|---:|");
    for r in &o.service {
        let _ = writeln!(
            s,
            "| {} | {} | {} / {} | {} | {} | {:.1} % | {} | {} | {} |",
            r.variant,
            r.cursor,
            f3(r.ms_p50),
            f3(r.ms_p90),
            f3(r.p50_x_svc),
            f3(r.p90_x_svc),
            100.0 * r.ok_share,
            r.panic,
            r.window_escape,
            r.violation
        );
    }
    s.push('\n');
    // 4. Triangles.
    let _ = writeln!(s, "### 4. Triangles : compteurs par requête\n");
    let _ = writeln!(
        s,
        "| variante | type | requêtes | `tri_first` | paires admissibles | cherchées (reprises) | jambes 2 trouvées · au plafond | jambes 3 : T2 · A* · échec | complets | à ±10 % | gardés | requêtes avec triangle | refus | épis · réparés · jetés · gardés | états jambe 2 · jambe 3 | β P10 · P50 · P90 | virages refusés · évalués · allers-retours refusés |\n|---|---|---:|---:|---:|---:|---|---|---:|---:|---:|---:|---|---|---|---|---|"
    );
    for t in &o.triangles {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {:.3} | {:.2} | {:.2} ({:.2}) | {:.2} · {:.2} | {} · {} · {} | {} | {} | {} | {:.3} | {} | {} · {} · {} · {} | {:.0} · {:.0} | {} · {} · {} | {:.1} · {:.1} · {:.2} |",
            t.variant,
            t.ty,
            t.requests,
            t.tri_first_share,
            t.admissible_mean,
            t.searched_mean,
            t.retried_mean,
            t.leg2_found_mean,
            t.leg2_capped_mean,
            t.leg3_t2,
            t.leg3_astar,
            t.leg3_failed,
            t.complete,
            t.within_tol,
            t.scored,
            t.with_triangle_share,
            if t.rejected.is_empty() {
                "-".to_string()
            } else {
                t.rejected.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")
            },
            t.spikes,
            t.spikes_repaired,
            t.spikes_dropped,
            t.spikes_kept,
            t.p2p_settled_mean,
            t.leg3_settled_mean,
            f3(t.beta[0]),
            f3(t.beta[1]),
            f3(t.beta[2]),
            t.turn_cut_mean,
            t.turn_evals_mean,
            t.reuse_cut_mean,
        );
    }
    s.push('\n');
    // 4 bis. Contrôleur indépendant.
    let _ = writeln!(
        s,
        "#### Refus du contrôleur indépendant sur les parcours rendus, par code (tous les jeux)\n\n| variante | type | rendus | refusés | par code |\n|---|---|---:|---:|---|"
    );
    for r in &o.controller {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} |",
            r.variant,
            r.ty,
            r.rendered,
            r.refused,
            if r.by_code.is_empty() {
                "-".to_string()
            } else {
                r.by_code.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")
            }
        );
    }
    s.push('\n');
    // 5. Lecture, en faits.
    let _ = writeln!(s, "### 5. Lecture, en faits (critères du §15.12)\n");
    for v in &o.verdicts {
        let _ = writeln!(s, "#### `{}` contre `svc`\n", v.candidate);
        for c in &v.checks {
            let _ = writeln!(s, "- **{}** {} : **{}** — {}", c.id, c.label, yes(c.holds), c.detail);
        }
        let _ = writeln!(
            s,
            "- **Critères de la bascule** (A1 à A4, B1, B2, C1, C2 ; D à lire à la campagne 3, C3 en conteneur) : **{}** ; réserve du fait (d) : {}\n",
            yes(v.switch_criteria_hold),
            v.reserve_d.as_deref().unwrap_or("sans objet")
        );
    }
    if let Some(c) = &o.rule_l {
        let _ = writeln!(s, "- **{}** {} : **{}** — {}\n", c.id, c.label, yes(c.holds), c.detail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_criterion_needs_every_part_evaluated_and_holding() {
        assert_eq!(all_hold(&[Some(true), Some(true)]), Some(true));
        assert_eq!(all_hold(&[Some(true), Some(false)]), Some(false));
        assert_eq!(all_hold(&[Some(true), None]), None, "une partie non évaluée : pas de verdict");
        assert_eq!(all_hold(&[]), Some(true));
        assert_eq!((yes(Some(true)), yes(Some(false)), yes(None)), ("tient", "NE TIENT PAS", "non évalué"));
    }

    /// Test 13 du §16.7 : A4 en absolu pour `shared_share` (médiane ≤ 0,05 et P90 ≤ P90 de `svc` + 0,05).
    #[test]
    fn the_shared_share_is_judged_in_absolute_terms() {
        // 20 valeurs : médiane 0,02, P90 vers 0,08.
        let svc: Vec<f64> = (0..20).map(|i| if i < 18 { 0.02 } else { 0.10 }).collect();
        let cand = |lo: f64, hi: f64, n_hi: usize| -> Vec<f64> {
            (0..20).map(|i| if i < 20 - n_hi { lo } else { hi }).collect()
        };
        // Médiane et P90 dans les bornes.
        assert_eq!(shared_share_holds(&cand(0.03, 0.12, 2), &svc), Some(true));
        // Médiane à 0,06 : trop de partage, même si `svc` en a autant.
        assert_eq!(shared_share_holds(&cand(0.06, 0.06, 0), &svc), Some(false));
        // Médiane de 0,05 exactement : tient (borne incluse).
        assert_eq!(shared_share_holds(&cand(0.05, 0.05, 0), &svc), Some(true));
        // P90 au-delà de celui de `svc` + 0,05 (0,10 + 0,05 = 0,15).
        assert_eq!(shared_share_holds(&cand(0.01, 0.30, 4), &svc), Some(false));
        // Un échantillon vide : non évalué.
        assert_eq!(shared_share_holds(&[], &svc), None);
        assert_eq!(shared_share_holds(&svc, &[]), None);
        // L'absolu ne dépend pas de l'IQR : une variante au partage nul tient toujours.
        assert_eq!(shared_share_holds(&[0.0; 20], &[0.0; 20]), Some(true));
    }

    #[test]
    fn the_bounds_are_the_cumulated_quarter_and_four() {
        assert_eq!(BOUNDS, [256 / 4, 256 * 4]);
        assert_eq!(SCOPES.len(), 3);
        for m in FORM_MEASURES.iter().chain(&A4_W1_MEASURES) {
            assert!(super::super::out::MEASURES.iter().any(|(n, _)| n == m), "{m}");
        }
    }
}
