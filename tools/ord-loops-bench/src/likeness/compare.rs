//! Comparaison des variantes de la sonde du goût (note lot 4, §12.5, §12.6 et §13.7) : `<label>-compare.json` et
//! `<label>-compare.md`. **Lettres et agrégats seulement** : jamais un départ, une coordonnée, un id.
//!
//! Le critère du 4b (décision 7 du §9.1) : `S(v) / S(base) ≤ 0,5` dans le même groupe et la même lecture, `S` étant la
//! moyenne des W1 / IQR des parts de classes, l'IQR étant celui du réel de **toutes** les boucles retenues du type (la
//! même normalisation pour toutes les lectures, équipes et variantes).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use super::compare_chain::{
    all_check, all_check_md, chains, chains_reading_md, diagnostics_md, steps_md, team_cost, teams_md,
    AllCheck, ChainSummary, TeamCostRow,
};
use super::out::{Getter, LoopRec, MEASURES};
use super::play::{GenRoute, PlayRec};
use super::shape::{iqr, median, quantile_sorted, sorted, wasserstein1};
use super::taste::{TasteParams, TasteTable};
use super::variants::{TeamWindowCounters, Variant, VariantMeta};

/// Les mesures de parts de classes que `S` moyenne, en ROAD (`way_path` et `surf_unpaved` exclus : IQR réel de 0,008
/// et 0,002, §4.1 du relevé du 4a) ; en GRAVEL, les mêmes plus ces deux-là.
pub const S_ROAD: [&str; 8] = [
    "way_major",
    "way_tertiary",
    "way_minor",
    "way_street",
    "way_cycle",
    "surf_paved",
    "surf_presumed",
    "marked",
];
pub const S_GRAVEL: [&str; 10] = [
    "way_major",
    "way_tertiary",
    "way_minor",
    "way_street",
    "way_cycle",
    "way_path",
    "surf_paved",
    "surf_presumed",
    "surf_unpaved",
    "marked",
];

const WAYS: [&str; 6] =
    ["grande route", "tertiaire", "petite route", "rue", "voie cyclable ou verte", "chemin"];
const SURFACES: [&str; 3] = ["revêtu connu", "présumé", "non revêtu connu"];

/// Ce qui est mesuré : les lectures du §12.6.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Reading {
    /// Jeux sur la surcouche complète, toutes les boucles retenues.
    All,
    /// Les boucles mises à l'écart, jouées sur la surcouche « écart », tables apprises sans elles : la lecture honnête.
    Gap,
    /// Les mêmes boucles, jouées sur la surcouche complète : l'écart avec `Gap` mesure la fuite de l'apprentissage.
    GapIn,
}

impl Reading {
    pub const ALL: [Reading; 3] = [Reading::All, Reading::Gap, Reading::GapIn];

    pub fn name(self) -> &'static str {
        match self {
            Self::All => "toutes",
            Self::Gap => "écart",
            Self::GapIn => "écart, dans l'échantillon",
        }
    }

    fn overlay(self) -> &'static str {
        match self {
            Self::All | Self::GapIn => "full",
            Self::Gap => "holdout",
        }
    }
}

/// Le résultat d'une variante, gardé en mémoire pour la comparaison.
pub struct Done {
    pub variant: Variant,
    pub meta: VariantMeta,
    pub fingerprint: String,
    pub loops: Vec<LoopRec>,
}

/// Conditions du banc.
#[derive(Clone, Debug, Serialize)]
pub struct Conditions {
    pub commit: String,
    pub cursors: Vec<f64>,
    pub params: TasteParams,
    /// Le banc tourne aux réglages du 4a : l'empreinte de `base` est comparable à celle du relevé.
    pub at_4a_defaults: bool,
    pub base_4a_fingerprint: String,
    /// Part des boucles mises à l'écart, par équipe.
    pub holdout_share: f64,
    /// Contrôle « pas 1 = `rc05` » (§13.6) : `None` s'il ne s'applique pas (réglages hors 4a ou pas de chaîne `rc05`).
    pub step1_check: Option<bool>,
    /// Hors des réglages du 4a, banc complet : les `S_base` « toutes » se comparent à ceux du 4a (§13.6).
    pub all_check_applies: bool,
}

// --- Mesures pures ---------------------------------------------------------------------------------------------------

/// `W1 / IQR` avec un IQR **imposé** (celui du réel de tout le type) : `None` si un échantillon est vide ou si l'IQR
/// n'est pas strictement positif. (`wasserstein1_normalised` prend l'IQR de son propre échantillon, ce qui rendrait
/// les petits groupes incomparables.)
pub fn w1_over(real: &[f64], generated: &[f64], iqr: f64) -> Option<f64> {
    (iqr > 0.0).then(|| wasserstein1(real, generated)).flatten().map(|w| w / iqr)
}

/// `S` : la moyenne des `W1 / IQR` sur les mesures dont les deux échantillons et l'IQR existent ; `None` si aucune.
pub fn criterion(parts: &[(Vec<f64>, Vec<f64>, f64)]) -> Option<f64> {
    let v: Vec<f64> = parts.iter().filter_map(|(r, g, q)| w1_over(r, g, *q)).collect();
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

/// `S(v) / S(base)`.
pub fn ratio(s: Option<f64>, s_base: Option<f64>) -> Option<f64> {
    match (s, s_base) {
        (Some(a), Some(b)) if b > 0.0 => Some(a / b),
        _ => None,
    }
}

fn pct_of(v: &[f64], p: f64) -> Option<f64> {
    quantile_sorted(&sorted(v), p)
}

pub(super) fn f3(x: f64) -> String {
    format!("{x:.3}")
}

pub(super) fn o3(x: Option<f64>) -> String {
    x.map_or_else(|| "-".into(), f3)
}

/// Un rapport en gras s'il atteint le critère.
pub(super) fn ratio_cell(r: Option<f64>) -> String {
    match r {
        Some(x) if x <= 0.5 => format!("**{x:.2}**"),
        Some(x) => format!("{x:.2}"),
        None => "-".into(),
    }
}

fn getter(name: &str) -> Getter {
    MEASURES.iter().find(|(n, _)| *n == name).expect("mesure connue").1
}

fn s_measures(ty: &str) -> &'static [&'static str] {
    if ty == "GRAVEL" {
        &S_GRAVEL
    } else {
        &S_ROAD
    }
}

/// Les boucles d'un groupe : type, équipe éventuelle, lecture ; avec leur jeu (curseur, surcouche de la lecture) et le
/// premier parcours gardé s'il est `OK`.
fn group<'a>(
    loops: &'a [LoopRec],
    ty: &str,
    team: Option<&str>,
    reading: Reading,
    cursor: f64,
) -> Vec<(&'a LoopRec, Option<&'a GenRoute>)> {
    loops
        .iter()
        .filter(|l| l.ty == ty && team.is_none_or(|t| l.team == t) && (reading == Reading::All || l.holdout))
        .map(|l| {
            let p: Option<&PlayRec> =
                l.plays.iter().find(|p| p.overlay == reading.overlay() && p.cursor == cursor);
            (l, p.filter(|p| p.outcome == "OK").and_then(|p| p.routes.first()))
        })
        .collect()
}

/// Les IQR du réel de **toutes** les boucles retenues du type, par mesure.
fn type_iqrs(loops: &[LoopRec], ty: &str) -> BTreeMap<&'static str, f64> {
    MEASURES
        .iter()
        .filter_map(|(n, g)| {
            let v: Vec<f64> = loops.iter().filter(|l| l.ty == ty).filter_map(|l| g(&l.real)).collect();
            iqr(&v).map(|q| (*n, q))
        })
        .collect()
}

/// Un groupe de mesures `(réel, généré, IQR du type)` pour des mesures nommées.
fn parts(
    g: &[(&LoopRec, Option<&GenRoute>)],
    names: &[&str],
    iqrs: &BTreeMap<&'static str, f64>,
) -> Vec<(Vec<f64>, Vec<f64>, f64)> {
    names
        .iter()
        .map(|n| {
            let get = getter(n);
            let real: Vec<f64> = g.iter().filter_map(|(l, _)| get(&l.real)).collect();
            let gen: Vec<f64> = g.iter().filter_map(|(_, r)| r.and_then(|r| get(&r.m))).collect();
            (real, gen, iqrs.get(n).copied().unwrap_or(0.0))
        })
        .collect()
}

// --- Lignes -------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct VarCell {
    pub variant: String,
    pub s: Option<f64>,
    pub ratio: Option<f64>,
    pub n_gen: usize,
}

/// Une ligne du critère : (lecture, équipe, type, curseur).
#[derive(Clone, Debug, Serialize)]
pub struct CritRow {
    pub reading: String,
    /// `toutes` ou la lettre.
    pub team: String,
    pub ty: String,
    pub cursor: f64,
    pub n_real: usize,
    pub s_base: Option<f64>,
    pub cells: Vec<VarCell>,
}

#[derive(Clone, Debug, Serialize)]
pub struct NamedRow {
    pub ty: String,
    pub cursor: f64,
    pub measure: String,
    pub real_p50: Option<f64>,
    /// `(variante, P50, W1 / IQR)`.
    pub variants: Vec<(String, Option<f64>, Option<f64>)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ServiceRow {
    pub variant: String,
    pub cursor: f64,
    pub plays: usize,
    pub ok_share: f64,
    pub outcomes: BTreeMap<String, usize>,
    pub ms_p50: Option<f64>,
    pub ms_p90: Option<f64>,
    pub ms_p50_d100: Option<f64>,
    pub ms_p90_d100: Option<f64>,
    pub team_share_p50: Option<f64>,
    pub length_over_target_p10: Option<f64>,
    pub length_over_target_p90: Option<f64>,
    /// `ms_p50` contre celui de `base` au même curseur (`× base`, §13.7 point 5).
    pub ms_p50_x_base: Option<f64>,
    /// Signaux du §12.6 contre `base` au même curseur.
    pub signals: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VariantSummary {
    pub name: String,
    pub fingerprint: String,
    pub taste_xxh3: String,
    pub team_windows: TeamWindowCounters,
    pub secs: f64,
    pub peak_anon_mb: f64,
    /// Groupes (types × curseurs × lectures `toutes` et `écart`, toutes équipes) où `S / S_base ≤ 0,5`.
    pub groups_at_half: usize,
    pub groups_total: usize,
}

#[derive(Debug, Serialize)]
pub struct Compare {
    pub conditions: Conditions,
    pub variants: Vec<VariantSummary>,
    pub criterion: Vec<CritRow>,
    pub named: Vec<NamedRow>,
    pub shapes: Vec<NamedRow>,
    pub service: Vec<ServiceRow>,
    /// Les tables apprises (parts, g, q8) par variante.
    pub tables: Vec<(String, Vec<TasteTable>)>,
    /// Les chaînes de recalé, pas à pas (§13.7).
    pub chains: Vec<ChainSummary>,
    /// Coût d'un profil par équipe appris au dépôt, par chaîne `rt` (§13.7 point 6).
    pub team_cost: Vec<TeamCostRow>,
    /// Contrôle « toutes » (§13.6) : les `S_base` « toutes » contre ceux du 4a ; `None` s'il ne s'applique pas.
    pub all_check: Option<AllCheck>,
}

const NAMED: [(&str, &[&str]); 2] =
    [("ROAD", &["way_major", "way_cycle"]), ("GRAVEL", &["way_path", "surf_unpaved", "way_tertiary"])];
const SHAPES: [&str; 7] =
    ["r_over_d", "compactness", "far_pos", "detour_far", "sinuosity_10", "turns_per_10km", "vertices_10"];
const TYPES: [&str; 2] = ["ROAD", "GRAVEL"];

pub(super) fn base_index(done: &[Done]) -> usize {
    done.iter().position(|d| d.variant.is_base()).unwrap_or(0)
}

fn criterion_rows(done: &[Done], cursors: &[f64]) -> Vec<CritRow> {
    let b = base_index(done);
    let mut rows = Vec::new();
    let iqrs: Vec<(&str, BTreeMap<&'static str, f64>)> =
        TYPES.iter().map(|t| (*t, type_iqrs(&done[b].loops, t))).collect();
    let mut teams: Vec<String> = done[b].loops.iter().map(|l| l.team.clone()).collect();
    teams.sort();
    teams.dedup();
    let teams_of_interest: Vec<Option<&str>> = std::iter::once(None)
        .chain(["A", "B"].into_iter().filter(|t| teams.iter().any(|x| x == t)).map(Some))
        .collect();
    for team in teams_of_interest {
        for reading in Reading::ALL {
            // Par équipe : « toutes » et « écart » seulement (le tableau 5 du §12.5).
            if team.is_some() && reading == Reading::GapIn {
                continue;
            }
            for (ty, iq) in &iqrs {
                for &cursor in cursors {
                    let s_of = |d: &Done| {
                        let g = group(&d.loops, ty, team, reading, cursor);
                        (
                            criterion(&parts(&g, s_measures(ty), iq)),
                            g.iter().filter(|(_, r)| r.is_some()).count(),
                        )
                    };
                    let n_real = group(&done[b].loops, ty, team, reading, cursor).len();
                    if n_real == 0 {
                        continue;
                    }
                    let s_base = s_of(&done[b]).0;
                    let cells = done
                        .iter()
                        .map(|d| {
                            let (s, n_gen) = s_of(d);
                            VarCell { variant: d.variant.name.clone(), s, ratio: ratio(s, s_base), n_gen }
                        })
                        .collect();
                    rows.push(CritRow {
                        reading: reading.name().into(),
                        team: team.unwrap_or("toutes").into(),
                        ty: (*ty).into(),
                        cursor,
                        n_real,
                        s_base,
                        cells,
                    });
                }
            }
        }
    }
    rows
}

fn named_rows(done: &[Done], cursors: &[f64], names: &[(&str, &[&str])], shapes: bool) -> Vec<NamedRow> {
    let b = base_index(done);
    let mut rows = Vec::new();
    let list: Vec<(&str, Vec<&str>)> = if shapes {
        TYPES.iter().map(|t| (*t, SHAPES.to_vec())).collect()
    } else {
        names.iter().map(|(t, m)| (*t, m.to_vec())).collect()
    };
    for (ty, measures) in list {
        let iqrs = type_iqrs(&done[b].loops, ty);
        for &cursor in cursors {
            for m in &measures {
                let get = getter(m);
                let g0 = group(&done[b].loops, ty, None, Reading::All, cursor);
                if g0.is_empty() {
                    continue;
                }
                let real: Vec<f64> = g0.iter().filter_map(|(l, _)| get(&l.real)).collect();
                let variants = done
                    .iter()
                    .map(|d| {
                        let g = group(&d.loops, ty, None, Reading::All, cursor);
                        let gen: Vec<f64> = g.iter().filter_map(|(_, r)| r.and_then(|r| get(&r.m))).collect();
                        (
                            d.variant.name.clone(),
                            median(&gen),
                            w1_over(&real, &gen, iqrs.get(m).copied().unwrap_or(0.0)),
                        )
                    })
                    .collect();
                rows.push(NamedRow {
                    ty: ty.into(),
                    cursor,
                    measure: (*m).into(),
                    real_p50: median(&real),
                    variants,
                });
            }
        }
    }
    rows
}

fn service_rows(done: &[Done], cursors: &[f64]) -> Vec<ServiceRow> {
    let b = base_index(done);
    let mut rows: Vec<ServiceRow> = Vec::new();
    for d in done {
        for &c in cursors {
            let plays: Vec<(&LoopRec, &PlayRec)> = d
                .loops
                .iter()
                .filter_map(|l| l.plays.iter().find(|p| p.overlay == "full" && p.cursor == c).map(|p| (l, p)))
                .collect();
            let mut outcomes: BTreeMap<String, usize> = BTreeMap::new();
            for (_, p) in &plays {
                *outcomes.entry(p.outcome.clone()).or_default() += 1;
            }
            let ok = outcomes.get("OK").copied().unwrap_or(0);
            let ms: Vec<f64> = plays.iter().map(|(_, p)| p.ms).collect();
            let ms100: Vec<f64> = plays.iter().filter(|(l, _)| l.d_km >= 100.0).map(|(_, p)| p.ms).collect();
            let first: Vec<&GenRoute> = plays
                .iter()
                .filter(|(_, p)| p.outcome == "OK")
                .filter_map(|(_, p)| p.routes.first())
                .collect();
            let share: Vec<f64> = first.iter().map(|r| r.team_share).collect();
            let lot: Vec<f64> = first.iter().map(|r| r.length_over_target).collect();
            rows.push(ServiceRow {
                variant: d.variant.name.clone(),
                cursor: c,
                plays: plays.len(),
                ok_share: if plays.is_empty() { 0.0 } else { ok as f64 / plays.len() as f64 },
                outcomes,
                ms_p50: pct_of(&ms, 0.5),
                ms_p90: pct_of(&ms, 0.9),
                ms_p50_d100: pct_of(&ms100, 0.5),
                ms_p90_d100: pct_of(&ms100, 0.9),
                team_share_p50: median(&share),
                length_over_target_p10: pct_of(&lot, 0.1),
                length_over_target_p90: pct_of(&lot, 0.9),
                ms_p50_x_base: None,
                signals: Vec::new(),
            });
        }
    }
    // Les signaux du §12.6, contre `base` au même curseur.
    let base_name = done[b].variant.name.clone();
    let base_rows: Vec<ServiceRow> = rows.iter().filter(|r| r.variant == base_name).cloned().collect();
    for r in rows.iter_mut() {
        let Some(bs) = base_rows.iter().find(|x| x.cursor == r.cursor) else { continue };
        r.ms_p50_x_base = match (r.ms_p50, bs.ms_p50) {
            (Some(a), Some(b)) if b > 0.0 => Some(a / b),
            _ => None,
        };
        if r.variant == base_name {
            continue;
        }
        if bs.ok_share - r.ok_share > 0.01 {
            r.signals
                .push(format!("part `OK` en baisse de {:.1} points", 100.0 * (bs.ok_share - r.ok_share)));
        }
        for code in ["PANIC", "WINDOW_ESCAPE"] {
            if let Some(n) = r.outcomes.get(code).filter(|&&n| n > 0) {
                r.signals.push(format!("{n} {code}"));
            }
        }
        if let (Some(a), Some(b)) = (r.ms_p50, bs.ms_p50) {
            if a > 1.25 * b {
                r.signals.push(format!("ms P50 {a:.0} au-delà de 1,25 × {b:.0} (indicatif)"));
            }
        }
        if let (Some(a), Some(b)) = (r.ms_p90, bs.ms_p90) {
            if a > 1.5 * b {
                r.signals.push(format!("ms P90 {a:.0} au-delà de 1,5 × {b:.0} (indicatif)"));
            }
        }
    }
    rows
}

/// Construit la comparaison.
pub fn compare(conds: &Conditions, done: &[Done]) -> Compare {
    let crit = criterion_rows(done, &conds.cursors);
    let all_check = conds.all_check_applies.then(|| all_check(&crit));
    let variants = done
        .iter()
        .map(|d| {
            // Les groupes du §12.5 point 8 : types × curseurs × lectures `toutes` et `écart`, toutes équipes.
            let own: Vec<&VarCell> = crit
                .iter()
                .filter(|r| r.team == "toutes" && (r.reading == "toutes" || r.reading == "écart"))
                .filter_map(|r| r.cells.iter().find(|c| c.variant == d.variant.name))
                .collect();
            VariantSummary {
                name: d.variant.name.clone(),
                fingerprint: d.fingerprint.clone(),
                taste_xxh3: d.meta.taste_xxh3.clone(),
                team_windows: d.meta.team_windows,
                secs: d.meta.secs,
                peak_anon_mb: d.meta.peak_anon_mb,
                groups_at_half: own.iter().filter(|c| c.ratio.is_some_and(|r| r <= 0.5)).count(),
                groups_total: own.iter().filter(|c| c.ratio.is_some()).count(),
            }
        })
        .collect();
    Compare {
        conditions: conds.clone(),
        variants,
        criterion: crit,
        named: named_rows(done, &conds.cursors, &NAMED, false),
        shapes: named_rows(done, &conds.cursors, &NAMED, true),
        service: service_rows(done, &conds.cursors),
        tables: done.iter().map(|d| (d.variant.name.clone(), d.meta.tables.clone())).collect(),
        chains: chains(done),
        team_cost: team_cost(done, &conds.cursors),
        all_check,
    }
}

// --- Rendu ----------------------------------------------------------------------------------------------------------------

/// Agrège les 72 classes en lignes : une par voie (6), par revêtement (3) et « balisé » : `(Σ p_n, Σ p_d, g moyen
/// pondéré par p_d, multiplicateur moyen pondéré par p_d)`.
/// Une ligne agrégée : nom, Σ p_n, Σ p_d, g moyen, multiplicateur moyen.
pub(super) type ClassRow = (String, f64, f64, f64, f64);

/// Axe d'agrégation : l'indice de voie, de revêtement ou de balisage d'une classe de goût.
type Axis = fn(usize) -> usize;

pub(super) fn class_rows(t: &TasteTable) -> Vec<ClassRow> {
    // Les 10 lignes : 6 voies, 3 revêtements, balisé ; chacune est un prédicat sur l'indice de classe.
    let sel: Vec<(String, Axis, usize)> = (0..6)
        .map(|w| (WAYS[w].to_string(), (|c| c / 12) as Axis, w))
        .chain((0..3).map(|s| (format!("revêtement : {}", SURFACES[s]), (|c| (c / 4) % 3) as Axis, s)))
        .chain(std::iter::once(("balisé".to_string(), (|c| (c / 2) % 2) as Axis, 1)))
        .collect();
    sel.into_iter()
        .map(|(name, axis, want)| {
            let cs: Vec<usize> = (0..72).filter(|&c| axis(c) == want).collect();
            let pn: f64 = cs.iter().map(|&c| t.pn[c]).sum();
            let pd: f64 = cs.iter().map(|&c| t.pd[c]).sum();
            let wavg = |v: &dyn Fn(usize) -> f64| {
                if pd > 0.0 {
                    cs.iter().map(|&c| t.pd[c] * v(c)).sum::<f64>() / pd
                } else {
                    1.0
                }
            };
            (name, pn, pd, wavg(&|c| t.g[c]), wavg(&|c| f64::from(t.q8[c]) / 256.0))
        })
        .collect()
}

fn learned_md(s: &mut String, cmp: &Compare) {
    let _ = writeln!(s, "## 2. Les tables apprises (surcouches complètes)\n");
    let _ = writeln!(
        s,
        "Par type et par source : Σ p_n (part roulée), Σ p_d (part disponible autour du réseau), g moyen (pondéré par p_d) ; \
         puis, par variante qui joue cette source, le multiplicateur moyen `Σ p_d · q8 / 256 / Σ p_d` (1 = neutre, < 1 : la classe \
         coûte moins cher).\n"
    );
    for ride in TYPES {
        let mut sources: Vec<String> = Vec::new();
        for (_, ts) in &cmp.tables {
            for t in ts.iter().filter(|t| t.ride == ride && t.overlay == "full") {
                if !sources.contains(&t.source) {
                    sources.push(t.source.clone());
                }
            }
        }
        sources.sort();
        if sources.is_empty() {
            continue;
        }
        let _ = writeln!(s, "### {ride}\n");
        for src in &sources {
            let users: Vec<(&String, &TasteTable)> = cmp
                .tables
                .iter()
                .filter_map(|(v, ts)| {
                    ts.iter()
                        .find(|t| t.source == *src && t.ride == ride && t.overlay == "full")
                        .map(|t| (v, t))
                })
                .collect();
            let Some((_, first)) = users.first() else { continue };
            let _ = writeln!(
                s,
                "Source **{src}** ({:.0} km-passages ou km de traces{}) :\n",
                first.num_km,
                if first.neutral { ", table neutre : trop peu de données" } else { "" }
            );
            let _ = writeln!(
                s,
                "| classe | Σ p_n | Σ p_d | g moyen |{}\n|---|---:|---:|---:|{}",
                users.iter().map(|(v, _)| format!(" ×{v} |")).collect::<String>(),
                users.iter().map(|_| "---:|").collect::<String>()
            );
            let all: Vec<Vec<ClassRow>> = users.iter().map(|(_, t)| class_rows(t)).collect();
            for (k, (name, pn, pd, g, _)) in all[0].iter().enumerate() {
                let _ = writeln!(
                    s,
                    "| {name} | {pn:.3} | {pd:.3} | {g:.2} |{}",
                    all.iter().map(|r| format!(" {:.2} |", r[k].4)).collect::<String>()
                );
            }
            s.push('\n');
        }
    }
}

fn criterion_md(s: &mut String, cmp: &Compare) {
    let names: Vec<&str> =
        cmp.variants.iter().filter(|v| v.name != "base").map(|v| v.name.as_str()).collect();
    let _ = writeln!(
        s,
        "## 3. Le critère : `S`, moyenne des W1 / IQR des parts de classes (ROAD : 8 mesures, GRAVEL : 10), IQR du réel de \
         tout le type ; `S / S_base` en gras s'il est ≤ 0,5\n"
    );
    for reading in Reading::ALL {
        let _ = writeln!(s, "### Lecture « {} »\n", reading.name());
        let _ = writeln!(
            s,
            "| type | curseur | n réel | S base |{}\n|---|---:|---:|---:|{}",
            names.iter().map(|n| format!(" {n} S | {n} S/Sb |")).collect::<String>(),
            names.iter().map(|_| "---:|---:|").collect::<String>()
        );
        for r in cmp.criterion.iter().filter(|r| r.team == "toutes" && r.reading == reading.name()) {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} |{}",
                r.ty,
                r.cursor,
                r.n_real,
                o3(r.s_base),
                r.cells
                    .iter()
                    .filter(|c| c.variant != "base")
                    .map(|c| format!(" {} | {} |", o3(c.s), ratio_cell(c.ratio)))
                    .collect::<String>()
            );
        }
        s.push('\n');
    }
}

fn team_md(s: &mut String, cmp: &Compare) {
    let names: Vec<&str> =
        cmp.variants.iter().filter(|v| v.name != "base").map(|v| v.name.as_str()).collect();
    let _ = writeln!(s, "## 5. Par équipe (A et B), le tableau 3 restreint à l'équipe\n");
    for team in ["A", "B"] {
        for reading in [Reading::All, Reading::Gap] {
            let rows: Vec<&CritRow> =
                cmp.criterion.iter().filter(|r| r.team == team && r.reading == reading.name()).collect();
            if rows.is_empty() {
                continue;
            }
            let _ = writeln!(s, "### Équipe {team}, lecture « {} »\n", reading.name());
            let _ = writeln!(
                s,
                "| type | curseur | n réel | S base |{}\n|---|---:|---:|---:|{}",
                names.iter().map(|n| format!(" {n} S | {n} S/Sb |")).collect::<String>(),
                names.iter().map(|_| "---:|---:|").collect::<String>()
            );
            for r in rows {
                let note = if r.n_real < 30 { " (indicatif : < 30 boucles)" } else { "" };
                let _ = writeln!(
                    s,
                    "| {} | {} | {}{note} | {} |{}",
                    r.ty,
                    r.cursor,
                    r.n_real,
                    o3(r.s_base),
                    r.cells
                        .iter()
                        .filter(|c| c.variant != "base")
                        .map(|c| format!(" {} | {} |", o3(c.s), ratio_cell(c.ratio)))
                        .collect::<String>()
                );
            }
            s.push('\n');
        }
    }
}

fn named_md(s: &mut String, title: &str, rows: &[NamedRow]) {
    let _ = writeln!(s, "{title}\n");
    let Some(first) = rows.first() else { return };
    let names: Vec<&str> = first.variants.iter().map(|v| v.0.as_str()).collect();
    let _ = writeln!(
        s,
        "| type | curseur | mesure | réel P50 |{}\n|---|---:|---|---:|{}",
        names.iter().map(|n| format!(" {n} P50 | {n} W1/IQR |")).collect::<String>(),
        names.iter().map(|_| "---:|---:|").collect::<String>()
    );
    for r in rows {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} |{}",
            r.ty,
            r.cursor,
            r.measure,
            o3(r.real_p50),
            r.variants.iter().map(|(_, p, w)| format!(" {} | {} |", o3(*p), o3(*w))).collect::<String>()
        );
    }
    s.push('\n');
}

fn service_md(s: &mut String, rows: &[ServiceRow], cmp: &Compare) {
    let _ = writeln!(s, "## 7. Service\n");
    let _ = writeln!(
        s,
        "Jeux sur la surcouche complète, toutes les boucles. `D ≥ 100` : boucles de 100 km et plus.\n\n| variante | curseur | jeux | `OK` | issues | ms P50 / P90 | × base (P50) | ms P50 / P90, D ≥ 100 | `team_share` P50 | L/D P10 · P90 | signaux |\n|---|---:|---:|---:|---|---|---:|---|---:|---|---|"
    );
    for r in rows {
        let outcomes = r.outcomes.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ");
        let _ = writeln!(
            s,
            "| {} | {} | {} | {:.1} % | {outcomes} | {} / {} | {} | {} / {} | {} | {} · {} | {} |",
            r.variant,
            r.cursor,
            r.plays,
            100.0 * r.ok_share,
            o3(r.ms_p50),
            o3(r.ms_p90),
            r.ms_p50_x_base.map_or_else(|| "-".to_string(), |x| format!("{x:.2}")),
            o3(r.ms_p50_d100),
            o3(r.ms_p90_d100),
            o3(r.team_share_p50),
            o3(r.length_over_target_p10),
            o3(r.length_over_target_p90),
            if r.signals.is_empty() { "-".to_string() } else { r.signals.join(" ; ") }
        );
    }
    let _ = writeln!(s, "\nConstructions de `WindowTeam` par variante :\n");
    let _ = writeln!(s, "| variante | constructions | ms au total | plus gros (Mo) | durée (s) | pic anonyme (Mo) |\n|---|---:|---:|---:|---:|---:|");
    for v in &cmp.variants {
        let w = &v.team_windows;
        let _ = writeln!(
            s,
            "| {} | {} | {:.0} | {:.1} | {:.0} | {:.0} |",
            v.name,
            w.builds,
            w.ms,
            w.max_bytes as f64 / 1e6,
            v.secs,
            v.peak_anon_mb
        );
    }
    s.push('\n');
}

/// Le rapport `.md`.
pub fn markdown(cmp: &Compare, done: &[Done]) -> String {
    let mut s = String::new();
    let c = &cmp.conditions;
    let _ = writeln!(s, "# Sonde du goût : comparaison des variantes (lot 4, §12 et §13)\n");
    let _ = writeln!(s, "## 1. Conditions\n");
    let _ = writeln!(s, "- commit : {}", c.commit);
    let _ = writeln!(s, "- curseurs : {:?}", c.cursors);
    let _ = writeln!(s, "- paramètres du goût : {:?}", c.params);
    let _ = writeln!(
        s,
        "- recalé par équipe : `n0` = {} boucles ; borne du multiplicateur cumulé : {} ; part mise à l'écart : {}",
        c.params.team_n0, c.params.mult_bound, c.holdout_share
    );
    let _ =
        writeln!(s, "\n| variante | empreinte de résultat | `taste_xxh3` | durée (s) |\n|---|---|---|---:|");
    for v in &cmp.variants {
        let _ = writeln!(s, "| {} | `{}` | `{}` | {:.0} |", v.name, v.fingerprint, v.taste_xxh3, v.secs);
    }
    let base = cmp.variants.iter().find(|v| v.name == "base");
    let check = match (c.at_4a_defaults, base) {
        (true, Some(b)) => (if b.fingerprint == c.base_4a_fingerprint { "oui" } else { "NON" }).to_string(),
        (false, _) => "sans objet (banc partiel ou réglages hors défaut)".to_string(),
        (true, None) => "sans objet (pas de variante `base`)".to_string(),
    };
    let _ = writeln!(s, "\nContrôle « `base` = 4a : `{}` » : **{check}**\n", c.base_4a_fingerprint);
    let step1 = match c.step1_check {
        Some(true) => "oui",
        Some(false) => "NON",
        None => "sans objet (réglages hors 4a, ou pas de chaîne `rc05`)",
    };
    let _ = writeln!(
        s,
        "Contrôle « pas 1 = `rc05` : `{}` · `{}` » : **{step1}**\n",
        super::RC05_FINGERPRINT,
        super::RC05_TASTE_XXH3
    );
    all_check_md(&mut s, &cmp.all_check);
    s.push('\n');
    learned_md(&mut s, cmp);
    criterion_md(&mut s, cmp);
    steps_md(&mut s, cmp, done);
    diagnostics_md(&mut s, cmp, done);
    named_md(
        &mut s,
        "## 4. Les écarts nommés du relevé (lecture « toutes », toutes équipes)\n\nP50 du réel, P50 de chaque variante, W1 / IQR de chaque variante.",
        &cmp.named,
    );
    team_md(&mut s, cmp);
    teams_md(&mut s, cmp, done);
    named_md(
        &mut s,
        "## 6. Forme (lecture « toutes »)\n\nP50 du réel, P50 de chaque variante et W1 / IQR (IQR du réel du type), par type et curseur.",
        &cmp.shapes,
    );
    service_md(&mut s, &cmp.service, cmp);
    let _ = writeln!(
        s,
        "## 8. Lecture (des faits, sans décision)\n\nGroupes où `S / S_base ≤ 0,5` : types × curseurs × lectures « toutes » et « écart », toutes équipes.\n\n| variante | groupes à ≤ 0,5 | signaux de service |\n|---|---:|---|"
    );
    for v in &cmp.variants {
        let sig: Vec<String> = cmp
            .service
            .iter()
            .filter(|r| r.variant == v.name && !r.signals.is_empty())
            .map(|r| format!("curseur {} : {}", r.cursor, r.signals.join(", ")))
            .collect();
        let _ = writeln!(
            s,
            "| {} | {} sur {} | {} |",
            v.name,
            v.groups_at_half,
            v.groups_total,
            if sig.is_empty() { "aucun".to_string() } else { sig.join(" ; ") }
        );
    }
    s.push('\n');
    chains_reading_md(&mut s, cmp, done);
    s
}

/// Écrit `<label>-compare.json` et `<label>-compare.md`.
pub fn write(dir: &Path, label: &str, conds: &Conditions, done: &[Done]) -> Result<()> {
    if done.is_empty() {
        return Ok(());
    }
    let cmp = compare(conds, done);
    std::fs::create_dir_all(dir)?;
    let (json, md) = (dir.join(format!("{label}-compare.json")), dir.join(format!("{label}-compare.md")));
    std::fs::write(&json, serde_json::to_vec_pretty(&cmp)?)?;
    std::fs::write(&md, markdown(&cmp, done))?;
    eprintln!("comparaison : {}\ncomparaison : {}", json.display(), md.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lin(n: usize, shift: f64) -> Vec<f64> {
        (0..n).map(|i| i as f64 / (n - 1) as f64 + shift).collect()
    }

    #[test]
    fn w1_over_uses_the_imposed_iqr_not_the_samples() {
        let real = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let gen: Vec<f64> = real.iter().map(|x| x + 1.0).collect();
        // W1 = 1 ; l'IQR du réel est 2, mais on impose 4.
        assert!((w1_over(&real, &gen, 4.0).unwrap() - 0.25).abs() < 1e-12);
        assert_eq!(w1_over(&real, &gen, 0.0), None);
        assert_eq!(w1_over(&real, &[], 4.0), None);
    }

    #[test]
    fn halving_the_gap_gives_exactly_one_half() {
        // Trois mesures : le généré de `base` est décalé de d, celui de la variante de d / 2.
        let real = lin(101, 0.0);
        let mk = |shift: f64| -> Vec<(Vec<f64>, Vec<f64>, f64)> {
            [0.5, 0.25, 1.0].iter().map(|iqr| (real.clone(), lin(101, shift * iqr), *iqr)).collect()
        };
        let (base, half) = (criterion(&mk(0.4)), criterion(&mk(0.2)));
        let (b, h) = (base.unwrap(), half.unwrap());
        assert!((ratio(half, base).unwrap() - 0.5).abs() < 1e-9, "{b} {h}");
        // Chaque mesure : W1 = décalage × IQR… puis / IQR : le décalage ; la moyenne vaut donc le décalage.
        assert!((b - 0.4).abs() < 1e-9 && (h - 0.2).abs() < 1e-9);
        // Une mesure sans échantillon ou sans IQR est ignorée.
        let mut p = mk(0.4);
        p.push((vec![], vec![0.0], 1.0));
        p.push((real.clone(), real.clone(), 0.0));
        assert!((criterion(&p).unwrap() - 0.4).abs() < 1e-9);
        assert_eq!(criterion(&[]), None);
        assert_eq!(ratio(Some(0.1), Some(0.0)), None);
    }

    #[test]
    fn the_4a_values_of_s_use_the_listed_measures() {
        assert_eq!(S_ROAD.len(), 8);
        assert_eq!(S_GRAVEL.len(), 10);
        for n in S_GRAVEL {
            assert!(MEASURES.iter().any(|(m, _)| *m == n), "{n}");
        }
        assert!(!S_ROAD.contains(&"way_path") && !S_ROAD.contains(&"surf_unpaved"));
    }

    #[test]
    fn class_rows_cover_every_class_exactly_once_per_axis() {
        let n = 72;
        let t = TasteTable {
            source: "A".into(),
            overlay: "full".into(),
            ride: "ROAD".into(),
            num_km: 1.0,
            neutral: false,
            pn: vec![1.0 / n as f64; n],
            pd: vec![1.0 / n as f64; n],
            g: vec![1.0; n],
            q8: vec![256; n],
        };
        let rows = class_rows(&t);
        assert_eq!(rows.len(), 10);
        let ways: f64 = rows[..6].iter().map(|r| r.2).sum();
        let surfs: f64 = rows[6..9].iter().map(|r| r.2).sum();
        assert!((ways - 1.0).abs() < 1e-9 && (surfs - 1.0).abs() < 1e-9);
        assert!((rows[9].2 - 0.5).abs() < 1e-9 && rows.iter().all(|r| (r.4 - 1.0).abs() < 1e-9));
        // Une voie fait 12 classes sur 72.
        assert!((rows[0].2 - 12.0 / 72.0).abs() < 1e-9);
    }
}
