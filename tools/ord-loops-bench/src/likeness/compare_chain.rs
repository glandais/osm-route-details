//! Comparaison des chaînes de recalé (note lot 4, §13.7) : les tables pas à pas, par équipe, les diagnostics, le coût
//! d'un profil par équipe appris au dépôt, la lecture. **Lettres et agrégats seulement.**

use std::fmt::Write as _;

use serde::Serialize;

use super::chain::ChainOwnerStep;
use super::compare::{base_index, class_rows, f3, o3, ratio_cell, Compare, CritRow, Done, Reading};
use super::taste::TasteTable;
use super::variants::OwnerGap;

/// Les `S_base` « toutes » du 4a (ROAD 0,25 · ROAD 1 · GRAVEL 0,25 · GRAVEL 1), à un millième près (§13.6).
pub const S_BASE_ALL_4A: [(&str, f64, f64); 4] =
    [("ROAD", 0.25, 0.614), ("ROAD", 1.0, 0.721), ("GRAVEL", 0.25, 0.735), ("GRAVEL", 1.0, 0.680)];

/// Les groupes d'un pas : type × curseur.
fn groups(cursors: &[f64]) -> Vec<(&'static str, f64)> {
    ["ROAD", "GRAVEL"].into_iter().flat_map(|t| cursors.iter().map(move |&c| (t, c))).collect()
}

fn group_name(g: &(&str, f64)) -> String {
    format!("{} {}", g.0, g.1)
}

// --- Contrôle « toutes » -----------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct AllCheckRow {
    pub ty: String,
    pub cursor: f64,
    pub measured: Option<f64>,
    pub expected: f64,
    pub ok: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct AllCheck {
    pub rows: Vec<AllCheckRow>,
    pub ok: bool,
}

/// Compare les `S_base` de la lecture « toutes », toutes équipes, à ceux du 4a, à un millième près.
pub fn all_check(crit: &[CritRow]) -> AllCheck {
    let rows: Vec<AllCheckRow> = S_BASE_ALL_4A
        .iter()
        .map(|&(ty, cursor, expected)| {
            let measured = crit
                .iter()
                .find(|r| {
                    r.team == "toutes" && r.reading == Reading::All.name() && r.ty == ty && r.cursor == cursor
                })
                .and_then(|r| r.s_base);
            let ok = measured.is_some_and(|m| (m - expected).abs() <= 0.0005 + 1e-9);
            AllCheckRow { ty: ty.to_string(), cursor, measured, expected, ok }
        })
        .collect();
    let ok = rows.iter().all(|r| r.ok);
    AllCheck { rows, ok }
}

// --- Chaînes -----------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct ChainSummary {
    pub stem: String,
    /// `rc` ou `rt`.
    pub source: String,
    pub dims: String,
    pub fam: Option<f32>,
    /// Les variantes de la chaîne, dans l'ordre des pas.
    pub steps: Vec<String>,
}

/// Les chaînes jouées, dans l'ordre de leur premier pas.
pub fn chains(done: &[Done]) -> Vec<ChainSummary> {
    let mut out: Vec<ChainSummary> = Vec::new();
    for d in done {
        let (Some(c), Some((src, _))) = (&d.variant.chain, d.variant.taste) else { continue };
        match out.iter_mut().find(|x| x.stem == c.stem) {
            Some(x) => x.steps.push(d.variant.name.clone()),
            None => out.push(ChainSummary {
                stem: c.stem.clone(),
                source: src.code().to_string(),
                dims: c.dims.code().to_string(),
                fam: d.variant.fam,
                steps: vec![d.variant.name.clone()],
            }),
        }
    }
    out
}

/// `S / S_base` d'une variante dans un groupe : `(rapport, n réel)`.
fn cell(
    rows: &[CritRow],
    team: &str,
    reading: Reading,
    g: &(&str, f64),
    variant: &str,
) -> Option<(f64, usize)> {
    let r = rows
        .iter()
        .find(|r| r.team == team && r.reading == reading.name() && r.ty == g.0 && r.cursor == g.1)?;
    let ratio = r.cells.iter().find(|c| c.variant == variant)?.ratio?;
    Some((ratio, r.n_real))
}

/// Le `n` réel d'un groupe, par équipe et lecture.
fn n_real(rows: &[CritRow], team: &str, reading: Reading, g: &(&str, f64)) -> Option<usize> {
    rows.iter()
        .find(|r| r.team == team && r.reading == reading.name() && r.ty == g.0 && r.cursor == g.1)
        .map(|r| r.n_real)
}

/// Les noms de variantes d'une chaîne, `base` en tête (`p0`).
fn steps_of<'a>(c: &'a ChainSummary, base: &'a str) -> Vec<(String, &'a str)> {
    std::iter::once(("p0".to_string(), base))
        .chain(c.steps.iter().enumerate().map(|(k, n)| (format!("p{}", k + 1), n.as_str())))
        .collect()
}

fn learned_from(done: &[Done], name: &str) -> String {
    done.iter()
        .find(|d| d.variant.name == name)
        .and_then(|d| d.meta.chain.as_ref())
        .map_or_else(|| "-".to_string(), |c| c.learned_from.clone())
}

fn table_head(cols: &[String]) -> String {
    format!(
        "| pas | variante | appris sur |{} groupes ≤ 0,5 |\n|---|---|---|{}---:|",
        cols.iter().map(|c| format!(" {c} |")).collect::<String>(),
        cols.iter().map(|_| "---:|").collect::<String>()
    )
}

// --- §3 bis, §5 bis ------------------------------------------------------------------------------------------------------

fn step_tables(s: &mut String, cmp: &Compare, done: &[Done], team: &str, readings: &[Reading], show_n: bool) {
    let base = &done[base_index(done)].variant.name;
    let gs = groups(&cmp.conditions.cursors);
    for c in &cmp.chains {
        for &reading in readings {
            let _ = writeln!(s, "### {} — lecture « {} »\n", c.stem, reading.name());
            let cols: Vec<String> = gs
                .iter()
                .map(|g| match (show_n, n_real(&cmp.criterion, team, reading, g)) {
                    (true, Some(n)) => {
                        format!("{} (n = {n}{})", group_name(g), if n < 30 { ", indicatif" } else { "" })
                    }
                    _ => group_name(g),
                })
                .collect();
            let _ = writeln!(s, "{}", table_head(&cols));
            for (label, name) in steps_of(c, base) {
                let learned = if label == "p0" { "-".to_string() } else { learned_from(done, name) };
                let cells: Vec<Option<(f64, usize)>> =
                    gs.iter().map(|g| cell(&cmp.criterion, team, reading, g, name)).collect();
                let halves = cells.iter().filter(|x| x.is_some_and(|(r, _)| r <= 0.5)).count();
                let _ = writeln!(
                    s,
                    "| {label} | {name} | {learned} |{} {halves} |",
                    cells.iter().map(|x| format!(" {} |", ratio_cell(x.map(|v| v.0)))).collect::<String>()
                );
            }
            s.push('\n');
        }
    }
}

/// La fuite de la surcouche par pas et par groupe : `S/S_base` « écart, dans l'échantillon » moins « écart ».
fn leak(cmp: &Compare, g: &(&str, f64), variant: &str) -> Option<f64> {
    let a = cell(&cmp.criterion, "toutes", Reading::GapIn, g, variant)?.0;
    let b = cell(&cmp.criterion, "toutes", Reading::Gap, g, variant)?.0;
    Some(a - b)
}

/// Le surapprentissage par pas et par groupe : `S/S_base` « écart, dans l'échantillon » (boucles mises à l'écart) moins
/// « apprentissage » (boucles sur lesquelles la table apprend), même surcouche complète, même table. Positif : les boucles
/// qui n'ont pas servi se lisent moins bien que celles qui ont servi. `p0` vaut 0 par construction.
fn overfit(cmp: &Compare, team: &str, g: &(&str, f64), variant: &str) -> Option<f64> {
    let a = cell(&cmp.criterion, team, Reading::GapIn, g, variant)?.0;
    let b = cell(&cmp.criterion, team, Reading::Train, g, variant)?.0;
    Some(a - b)
}

/// Une différence de lectures pour un groupe et une variante.
type DiffFn<'a> = &'a dyn Fn(&(&str, f64), &str) -> Option<f64>;

/// Une table « pas × groupe » d'une différence de lectures.
fn diff_table(s: &mut String, c: &ChainSummary, base: &str, gs: &[(&str, f64)], f: DiffFn<'_>) {
    let _ = writeln!(
        s,
        "| pas | variante |{}\n|---|---|{}",
        gs.iter().map(|g| format!(" {} |", group_name(g))).collect::<String>(),
        gs.iter().map(|_| "---:|").collect::<String>()
    );
    for (label, name) in steps_of(c, base) {
        let _ = writeln!(
            s,
            "| {label} | {name} |{}",
            gs.iter()
                .map(|g| format!(" {} |", f(g, name).map_or_else(|| "-".to_string(), |x| format!("{x:+.2}"))))
                .collect::<String>()
        );
    }
    s.push('\n');
}

fn leak_md(s: &mut String, cmp: &Compare, done: &[Done]) {
    let base = &done[base_index(done)].variant.name;
    let gs = groups(&cmp.conditions.cursors);
    let _ = writeln!(
        s,
        "La fuite de la surcouche, par pas : `S/S_base` en lecture « écart, dans l'échantillon » moins « écart » (§13.4 ; \
         négatif : les boucles mises à l'écart se lisent mieux quand leurs propres traces sont dans la surcouche). Elle ne \
         mesure **pas** le surapprentissage des tables : aucune table n'apprend sur ces boucles.\n"
    );
    for c in &cmp.chains {
        let _ = writeln!(s, "**{}**\n", c.stem);
        diff_table(s, c, base, &gs, &|g, n| leak(cmp, g, n));
    }
    let _ = writeln!(
        s,
        "Le surapprentissage, par pas : `S/S_base` en lecture « écart, dans l'échantillon » moins « apprentissage » (même \
         surcouche complète, même table ; positif : les boucles mises à l'écart se lisent moins bien que celles sur lesquelles \
         la table apprend ; au-delà de 0,10 au dernier pas, la chaîne surapprend, §13.11 point 3 lu ainsi, §13.13). Toutes \
         équipes, puis A et B (indicatif : leurs boucles mises à l'écart sont peu nombreuses).\n"
    );
    for c in &cmp.chains {
        for team in ["toutes", "A", "B"] {
            if team != "toutes" && cmp.criterion.iter().all(|r| r.team != team) {
                continue;
            }
            let _ = writeln!(s, "**{}, {team}**\n", c.stem);
            diff_table(s, c, base, &gs, &|g, n| overfit(cmp, team, g, n));
        }
    }
}

// --- §3 ter, diagnostics --------------------------------------------------------------------------------------------------

/// Une ligne de diagnostic : un propriétaire (ou « autres ») et un type.
struct DiagRow {
    owner: String,
    ty: String,
    loops: u32,
    pairs: u32,
    weight: Option<f64>,
    l1_gap: f64,
    l1_after: Option<f64>,
    mean_mult: f64,
    halved: usize,
    gamma_min: f64,
    at_bound: usize,
}

fn gap_of(l1: &[OwnerGap], owner: &str, ty: &str) -> Option<f64> {
    l1.iter().find(|g| g.owner == owner && g.ty == ty).map(|g| g.l1)
}

/// Les lignes d'un pas : `général` (rc) ; pour `rt` : `commun`, A, B, puis « autres » (sommes pour les boucles, les paires
/// et les paramètres ; moyennes simples pour les écarts, le poids et le multiplicateur moyen).
fn diag_rows(diag: &[ChainOwnerStep], l1: &[OwnerGap]) -> Vec<DiagRow> {
    let mk = |d: &ChainOwnerStep| DiagRow {
        owner: d.owner.clone(),
        ty: d.ty.clone(),
        loops: d.loops,
        pairs: d.pairs,
        weight: d.weight,
        l1_gap: d.l1_gap,
        l1_after: gap_of(l1, &d.owner, &d.ty),
        mean_mult: d.mean_mult,
        halved: d.halved,
        gamma_min: d.gamma_min,
        at_bound: d.at_bound,
    };
    let mut rows: Vec<DiagRow> = Vec::new();
    let named = |o: &str| matches!(o, "général" | "commun" | "A" | "B");
    for d in diag.iter().filter(|d| named(&d.owner) && d.pairs > 0) {
        rows.push(mk(d));
    }
    for ty in ["ROAD", "GRAVEL", "MTB"] {
        let others: Vec<&ChainOwnerStep> =
            diag.iter().filter(|d| !named(&d.owner) && d.ty == ty && d.pairs > 0).collect();
        if others.is_empty() {
            continue;
        }
        let n = others.len() as f64;
        let mean = |f: &dyn Fn(&ChainOwnerStep) -> f64| others.iter().map(|d| f(d)).sum::<f64>() / n;
        let afters: Vec<f64> = others.iter().filter_map(|d| gap_of(l1, &d.owner, &d.ty)).collect();
        rows.push(DiagRow {
            owner: format!("autres ({})", others.len()),
            ty: ty.to_string(),
            loops: others.iter().map(|d| d.loops).sum(),
            pairs: others.iter().map(|d| d.pairs).sum(),
            weight: others[0].weight.map(|_| mean(&|d| d.weight.unwrap_or(0.0))),
            l1_gap: mean(&|d| d.l1_gap),
            l1_after: (!afters.is_empty()).then(|| afters.iter().sum::<f64>() / afters.len() as f64),
            mean_mult: mean(&|d| d.mean_mult),
            halved: others.iter().map(|d| d.halved).sum(),
            gamma_min: others.iter().map(|d| d.gamma_min).fold(f64::INFINITY, f64::min),
            at_bound: others.iter().map(|d| d.at_bound).sum(),
        });
    }
    // Ordre : le type, puis le propriétaire tel qu'il vient.
    let ord = |t: &str| ["ROAD", "GRAVEL", "MTB"].iter().position(|x| *x == t).unwrap_or(3);
    rows.sort_by_key(|r| ord(&r.ty));
    rows
}

fn diag_md(s: &mut String, cmp: &Compare, done: &[Done]) {
    let _ = writeln!(
        s,
        "## 3 ter. Diagnostics des chaînes\n\n`l1 avant` : l'écart L1 des parts de classes que le pas corrige (les parcours du pas \
         précédent) ; `l1 après` : celui que les parcours du pas laissent ; `mult. moyen` : `Σ p_d · mult` (dérive d'échelle, \
         1 = neutre) ; `γ divisé` : paramètres dont γ a été divisé à ce pas ; `à la borne` : classes à `1/B` ou `B`. « autres » : \
         sommes pour les boucles, les paires, les paramètres et les classes, moyennes simples pour le reste.\n"
    );
    for c in &cmp.chains {
        let _ = writeln!(s, "### {}\n", c.stem);
        let _ = writeln!(
            s,
            "| pas | propriétaire | type | boucles | paires | poids | l1 avant | l1 après | mult. moyen | γ divisé | γ min | à la borne |\n|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|"
        );
        for (k, name) in c.steps.iter().enumerate() {
            let Some(d) = done.iter().find(|d| &d.variant.name == name) else { continue };
            let Some(ch) = &d.meta.chain else { continue };
            for r in diag_rows(&ch.diag, &d.meta.l1_after) {
                let _ = writeln!(
                    s,
                    "| p{} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.3} | {} |",
                    k + 1,
                    r.owner,
                    r.ty,
                    r.loops,
                    r.pairs,
                    o3(r.weight),
                    f3(r.l1_gap),
                    o3(r.l1_after),
                    f3(r.mean_mult),
                    r.halved,
                    r.gamma_min,
                    r.at_bound
                );
            }
        }
        s.push('\n');
        // Les multiplicateurs cumulés, agrégés : le tableau 2 de la comparaison, une colonne par pas.
        let sources: Vec<String> = if c.source == "rt" { vec!["A".into(), "B".into()] } else { vec![] };
        let owners: Vec<(String, Vec<&Done>)> = if sources.is_empty() {
            vec![("général".to_string(), c.steps.iter().filter_map(|n| find(done, n)).collect())]
        } else {
            sources
                .into_iter()
                .map(|o| (o, c.steps.iter().filter_map(|n| find(done, n)).collect::<Vec<_>>()))
                .collect()
        };
        for ride in ["ROAD", "GRAVEL"] {
            for (owner, ds) in &owners {
                let tables: Vec<Option<&super::taste::TasteTable>> = ds
                    .iter()
                    .map(|d| {
                        d.meta.tables.iter().find(|t| {
                            t.ride == ride
                                && t.overlay == "full"
                                && (t.source == *owner
                                    || (owner == "général" && t.source.starts_with("recalé")))
                        })
                    })
                    .collect();
                let Some(first) = tables.iter().flatten().next() else { continue };
                let _ = writeln!(
                    s,
                    "Multiplicateurs cumulés ({ride}, {owner}), moyenne pondérée par la part générée du pas 1 :\n\n| classe | Σ p_n (p1) | Σ p_d (p1) |{}\n|---|---:|---:|{}",
                    tables.iter().enumerate().map(|(k, _)| format!(" × p{} |", k + 1)).collect::<String>(),
                    tables.iter().map(|_| "---:|").collect::<String>()
                );
                // Mêmes poids pour toutes les colonnes : la part générée du pas 1.
                let rows: Vec<Vec<super::compare::ClassRow>> = tables
                    .iter()
                    .map(|t| {
                        t.map(|t| class_rows(&TasteTable { pd: first.pd.clone(), ..t.clone() }))
                            .unwrap_or_default()
                    })
                    .collect();
                for (i, (name, pn, pd, _, _)) in class_rows(first).iter().enumerate() {
                    let _ = writeln!(
                        s,
                        "| {name} | {pn:.3} | {pd:.3} |{}",
                        rows.iter()
                            .map(|r| r
                                .get(i)
                                .map_or_else(|| " - |".to_string(), |x| format!(" {:.2} |", x.4)))
                            .collect::<String>()
                    );
                }
                s.push('\n');
            }
        }
    }
}

fn find<'a>(done: &'a [Done], name: &str) -> Option<&'a Done> {
    done.iter().find(|d| d.variant.name == name)
}

// --- §9, coût d'un profil par équipe -----------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct GenCost {
    pub variant: String,
    pub plays_full: usize,
    pub cpu_s_full: f64,
    pub wall_s: f64,
    pub build_s: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TeamCostRow {
    pub chain: String,
    /// `A`, `B` ou `autres` (la somme des autres lettres).
    pub team: String,
    pub steps: u8,
    /// Les générations nécessaires à `steps` pas : `base`, p1, …, p(steps − 1).
    pub generations: Vec<GenCost>,
    pub cpu_s_total: f64,
    /// Le temps estimé sur 1, 4, 8 et 16 fils : `total / fils`.
    pub est_s: Vec<(usize, f64)>,
    /// Le même calcul sur les seuls jeux du premier curseur (l'allègement « un seul curseur »).
    pub cursor: f64,
    pub cpu_s_total_one_cursor: f64,
    pub est_s_one_cursor: Vec<(usize, f64)>,
}

pub const THREADS: [usize; 4] = [1, 4, 8, 16];

/// Pour chaque chaîne `rt`, par équipe A, B et « autres » : ce que coûte l'apprentissage d'un profil de `steps` pas.
pub fn team_cost(done: &[Done], cursors: &[f64]) -> Vec<TeamCostRow> {
    let cursor = cursors.first().copied().unwrap_or(0.25);
    let mut out = Vec::new();
    let b = base_index(done);
    for c in chains(done).into_iter().filter(|c| c.source == "rt") {
        let steps = c.steps.len() as u8;
        // Les générations : `base` puis les pas qui ont un successeur.
        let gens: Vec<&Done> = std::iter::once(&done[b])
            .chain(c.steps.iter().take(c.steps.len().saturating_sub(1)).filter_map(|n| find(done, n)))
            .collect();
        for team in ["A", "B", "autres"] {
            let pick = |t: &str| -> bool {
                if team == "autres" {
                    t != "A" && t != "B"
                } else {
                    t == team
                }
            };
            let mut generations: Vec<GenCost> = Vec::new();
            let (mut total, mut total_c) = (0.0, 0.0);
            let mut seen = false;
            for d in &gens {
                let mine: Vec<_> = d.meta.team_times.iter().filter(|t| pick(&t.team)).collect();
                seen |= !mine.is_empty();
                generations.push(GenCost {
                    variant: d.variant.name.clone(),
                    plays_full: mine.iter().map(|t| t.plays_full).sum(),
                    cpu_s_full: mine.iter().map(|t| t.cpu_s_full).sum(),
                    wall_s: mine.iter().map(|t| t.wall_s).sum(),
                    build_s: mine.iter().map(|t| t.build_s).sum(),
                });
                total += mine.iter().map(|t| t.cpu_s_full).sum::<f64>();
                total_c += mine
                    .iter()
                    .flat_map(|t| {
                        t.cpu_s_full_by_cursor.iter().filter(|(k, _)| *k == cursor).map(|(_, s)| *s)
                    })
                    .sum::<f64>();
            }
            if !seen {
                continue;
            }
            let est = |x: f64| THREADS.iter().map(|&n| (n, x / n as f64)).collect::<Vec<_>>();
            out.push(TeamCostRow {
                chain: c.stem.clone(),
                team: team.to_string(),
                steps,
                generations,
                cpu_s_total: total,
                est_s: est(total),
                cursor,
                cpu_s_total_one_cursor: total_c,
                est_s_one_cursor: est(total_c),
            });
        }
    }
    out
}

fn mins(s: f64) -> String {
    if s >= 90.0 {
        format!("{:.1} min", s / 60.0)
    } else {
        format!("{s:.0} s")
    }
}

fn cost_md(s: &mut String, cmp: &Compare) {
    let _ = writeln!(
        s,
        "## 9. Coût d'un profil par équipe appris au dépôt [estimé]\n\nApprendre un profil de `n` pas demande de regénérer les \
         boucles de l'équipe à chaque pas : `base`, p1, …, p(n − 1) (le dernier pas n'a pas besoin d'être joué). Temps cumulés \
         des jeux sur la surcouche complète (`Σ PlayRec::ms`), aux deux curseurs ; le temps sur `k` fils est `total / k`. \
         « autres » : la somme des autres lettres. **Mesuré à 16 fils sur 8 cœurs, machine pleine ; un cœur dédié est plus \
         rapide, non mesuré.**\n"
    );
    if cmp.team_cost.is_empty() {
        let _ = writeln!(s, "Aucune chaîne `rt` jouée.\n");
        return;
    }
    for r in &cmp.team_cost {
        let _ = writeln!(s, "**{}, équipe {}**\n", r.chain, r.team);
        let _ = writeln!(
            s,
            "| génération | jeux full | cpu (s) | mur (s) | construction (s) |\n|---|---:|---:|---:|---:|"
        );
        for g in &r.generations {
            let _ = writeln!(
                s,
                "| {} | {} | {:.1} | {:.1} | {:.1} |",
                g.variant, g.plays_full, g.cpu_s_full, g.wall_s, g.build_s
            );
        }
        let line = |label: &str, total: f64, est: &[(usize, f64)]| {
            format!(
                "| {label} | {} | {} |",
                mins(total),
                est.iter().map(|(n, x)| format!("{n} fils : {}", mins(*x))).collect::<Vec<_>>().join(" · ")
            )
        };
        let _ = writeln!(s, "\n| calcul | total (cpu) | estimé |\n|---|---:|---|");
        let _ = writeln!(s, "{}", line("deux curseurs", r.cpu_s_total, &r.est_s));
        let _ = writeln!(
            s,
            "{}\n",
            line(&format!("curseur {} seul", r.cursor), r.cpu_s_total_one_cursor, &r.est_s_one_cursor)
        );
    }
}

// --- §8, lecture ----------------------------------------------------------------------------------------------------------

/// Le premier pas (1-indexé) où `S/S_base ≤ 0,5` dans le groupe, ou `None`.
fn first_half(
    cmp: &Compare,
    c: &ChainSummary,
    team: &str,
    reading: Reading,
    g: &(&str, f64),
) -> Option<usize> {
    c.steps
        .iter()
        .position(|n| cell(&cmp.criterion, team, reading, g, n).is_some_and(|(r, _)| r <= 0.5))
        .map(|k| k + 1)
}

fn reading_md(s: &mut String, cmp: &Compare, done: &[Done]) {
    let gs = groups(&cmp.conditions.cursors);
    let _ = writeln!(s, "### 8 bis. Chaînes (des faits, sans décision)\n");
    let _ = writeln!(
        s,
        "**Premier pas où `S/S_base ≤ 0,5`**, toutes équipes (« jamais » : aucun pas de la chaîne) :\n\n| chaîne | lecture |{}\n|---|---|{}",
        gs.iter().map(|g| format!(" {} |", group_name(g))).collect::<String>(),
        gs.iter().map(|_| "---:|").collect::<String>()
    );
    for c in &cmp.chains {
        for reading in [Reading::All, Reading::Gap] {
            let _ = writeln!(
                s,
                "| {} | {} |{}",
                c.stem,
                reading.name(),
                gs.iter()
                    .map(|g| format!(
                        " {} |",
                        first_half(cmp, c, "toutes", reading, g)
                            .map_or_else(|| "jamais".to_string(), |k| format!("p{k}"))
                    ))
                    .collect::<String>()
            );
        }
    }
    s.push('\n');
    // `rt` contre `rc` du même pas et de la même forme : A et B, lectures « toutes » et « écart ».
    let pairs: Vec<(&ChainSummary, &ChainSummary)> = cmp
        .chains
        .iter()
        .filter(|c| c.source == "rt")
        .filter_map(|rt| {
            let want = rt.stem.replacen("rt", "rc", 1);
            cmp.chains.iter().find(|rc| rc.source == "rc" && rc.stem == want).map(|rc| (rt, rc))
        })
        .collect();
    for (rt, rc) in pairs {
        let _ = writeln!(
            s,
            "**`{}` contre `{}`** (`S/S_base` : rt, rc, rt − rc), même pas :\n",
            rt.stem, rc.stem
        );
        let _ = writeln!(
            s,
            "| équipe | lecture | pas |{}\n|---|---|---|{}",
            gs.iter().map(|g| format!(" {} |", group_name(g))).collect::<String>(),
            gs.iter().map(|_| "---|").collect::<String>()
        );
        for team in ["A", "B"].into_iter().filter(|t| cmp.criterion.iter().any(|r| r.team == *t)) {
            for reading in [Reading::All, Reading::Gap] {
                for k in 0..rt.steps.len().min(rc.steps.len()) {
                    let cells: Vec<String> = gs
                        .iter()
                        .map(|g| {
                            let a = cell(&cmp.criterion, team, reading, g, &rt.steps[k]);
                            let b = cell(&cmp.criterion, team, reading, g, &rc.steps[k]);
                            match (a, b) {
                                (Some((a, _)), Some((b, _))) => format!(" {a:.2} · {b:.2} · {:+.2} |", a - b),
                                _ => " - |".to_string(),
                            }
                        })
                        .collect();
                    let _ = writeln!(s, "| {team} | {} | p{} |{}", reading.name(), k + 1, cells.concat());
                }
            }
        }
        s.push('\n');
    }
    // Service par pas.
    let _ = writeln!(s, "**Signaux de service par pas** :\n");
    for c in &cmp.chains {
        let sig: Vec<String> = c
            .steps
            .iter()
            .enumerate()
            .filter_map(|(k, n)| {
                let v: Vec<String> = cmp
                    .service
                    .iter()
                    .filter(|r| &r.variant == n && !r.signals.is_empty())
                    .map(|r| format!("curseur {} : {}", r.cursor, r.signals.join(", ")))
                    .collect();
                (!v.is_empty()).then(|| format!("p{} ({})", k + 1, v.join(" ; ")))
            })
            .collect();
        let _ = writeln!(
            s,
            "- {} : {}",
            c.stem,
            if sig.is_empty() { "aucun".to_string() } else { sig.join(" ; ") }
        );
    }
    // La plus grande fuite et la dérive d'échelle.
    let _ = writeln!(
        s,
        "\n**Fuite de la surcouche la plus grande (en valeur absolue), surapprentissage le plus fort (toutes équipes), dérive \
         d'échelle la plus forte** :\n"
    );
    for c in &cmp.chains {
        let mut worst: Option<(f64, usize, String)> = None;
        for (k, n) in c.steps.iter().enumerate() {
            for g in &gs {
                if let Some(x) = leak(cmp, g, n) {
                    if worst.as_ref().is_none_or(|w| x.abs() > w.0.abs()) {
                        worst = Some((x, k + 1, group_name(g)));
                    }
                }
            }
        }
        let mut over: Option<(f64, usize, String)> = None;
        for (k, n) in c.steps.iter().enumerate() {
            for g in &gs {
                if let Some(x) = overfit(cmp, "toutes", g, n) {
                    if over.as_ref().is_none_or(|w| x > w.0) {
                        over = Some((x, k + 1, group_name(g)));
                    }
                }
            }
        }
        let mut far: Option<(f64, usize, String)> = None;
        for (k, n) in c.steps.iter().enumerate() {
            let Some(ch) = find(done, n).and_then(|d| d.meta.chain.as_ref()) else { continue };
            for d in ch.diag.iter().filter(|d| d.pairs > 0 && (d.ty == "ROAD" || d.ty == "GRAVEL")) {
                if far.as_ref().is_none_or(|f| (d.mean_mult - 1.0).abs() > (f.0 - 1.0).abs()) {
                    far = Some((d.mean_mult, k + 1, format!("{} {}", d.owner, d.ty)));
                }
            }
        }
        let _ = writeln!(
            s,
            "- {} : fuite de la surcouche {} ; surapprentissage {} ; `mult. moyen` {}",
            c.stem,
            worst.map_or_else(|| "-".to_string(), |(x, k, g)| format!("{x:+.2} (p{k}, {g})")),
            over.map_or_else(|| "-".to_string(), |(x, k, g)| format!("{x:+.2} (p{k}, {g})")),
            far.map_or_else(|| "-".to_string(), |(x, k, o)| format!("{x:.3} (p{k}, {o})"))
        );
    }
    s.push('\n');
}

// --- Rendu -----------------------------------------------------------------------------------------------------------------

/// §3 bis : les chaînes pas à pas, toutes équipes, et la fuite par pas ; vide si aucune chaîne n'a été jouée.
pub fn steps_md(s: &mut String, cmp: &Compare, done: &[Done]) {
    if cmp.chains.is_empty() {
        return;
    }
    let _ = writeln!(
        s,
        "## 3 bis. Les chaînes, pas à pas\n\n`S / S_base` par groupe, toutes équipes ; `p0` est `base`. Chaque pas apprend sur les \
         jeux du précédent (colonne « appris sur »). En gras : ≤ 0,5.\n"
    );
    step_tables(s, cmp, done, "toutes", &Reading::ALL, false);
    leak_md(s, cmp, done);
}

/// §5 bis : A et B pas à pas, lectures « toutes » et « écart ».
pub fn teams_md(s: &mut String, cmp: &Compare, done: &[Done]) {
    if cmp.chains.is_empty() {
        return;
    }
    let _ = writeln!(s, "## 5 bis. Par équipe et par pas (A et B)\n");
    for team in ["A", "B"] {
        if cmp.criterion.iter().all(|r| r.team != team) {
            continue;
        }
        let _ = writeln!(s, "#### Équipe {team}\n");
        step_tables(s, cmp, done, team, &[Reading::All, Reading::Gap], true);
    }
}

/// §3 ter : diagnostics des chaînes.
pub fn diagnostics_md(s: &mut String, cmp: &Compare, done: &[Done]) {
    if !cmp.chains.is_empty() {
        diag_md(s, cmp, done);
    }
}

/// §8 bis et §9 : la lecture des chaînes, et le coût d'un profil par équipe.
pub fn chains_reading_md(s: &mut String, cmp: &Compare, done: &[Done]) {
    if cmp.chains.is_empty() {
        return;
    }
    reading_md(s, cmp, done);
    cost_md(s, cmp);
}

/// Les `S_base` du contrôle « toutes », pour la section des conditions.
pub fn all_check_md(s: &mut String, c: &Option<AllCheck>) {
    match c {
        None => {
            let _ = writeln!(
                s,
                "Contrôle « `S_base` toutes = 4a » : sans objet (réglages du 4a, ou banc partiel)."
            );
        }
        Some(c) => {
            let _ = writeln!(
                s,
                "Contrôle « `S_base` toutes = 0,614 · 0,721 · 0,735 · 0,680 » (à un millième) : **{}** (mesuré : {})",
                if c.ok { "oui" } else { "NON" },
                c.rows.iter().map(|r| o3(r.measured)).collect::<Vec<_>>().join(" · ")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::compare::{compare, markdown, Conditions, Done};
    use super::super::measure::Measures;
    use super::super::out::LoopRec;
    use super::super::play::{GenRoute, PlayRec, Recovered};
    use super::super::taste::TasteParams;
    use super::super::variants::{
        Chain, ChainMeta, Dims, TasteSource, TeamTime, TeamWindowCounters, Variant, VariantMeta,
    };
    use super::*;

    /// Un parcours dont toutes les mesures de parts valent `v`.
    fn route(v: f64) -> GenRoute {
        let m = Measures { way: [v; 6], surface: [v; 3], marked: v, ..Measures::default() };
        GenRoute {
            m,
            length_over_target: 1.0,
            team_share: 0.5,
            family: "X".into(),
            recovered: Recovered::default(),
            shared_vs_route_m: 0.0,
            poly_vs_graph: 0.0,
            route_xxh3: "0".into(),
        }
    }

    fn play(cursor: f64, overlay: &str, v: f64) -> PlayRec {
        PlayRec {
            cursor,
            overlay: overlay.into(),
            outcome: "OK".into(),
            n_routes: 1,
            n_kept: 1,
            dropped: Default::default(),
            ms: 100.0,
            routes: vec![route(v)],
            best_shape: None,
            best_recovered: None,
            first_class_m: None,
        }
    }

    /// 20 boucles ROAD de l'équipe A, une sur quatre mise à l'écart ; le réel vaut `i / 19`, le généré `i / 19 + shift`
    /// (`leak` ajouté en lecture « écart, dans l'échantillon » pour tester la fuite).
    fn loops(shift: f64, leak: f64) -> Vec<LoopRec> {
        (0..20)
            .map(|i| {
                let real = i as f64 / 19.0;
                let held = i % 4 == 0;
                let mut plays = vec![
                    play(0.25, "full", real + shift + if held { leak } else { 0.0 }),
                    play(1.0, "full", real + shift),
                ];
                if held {
                    plays.push(play(0.25, "holdout", real + shift));
                    plays.push(play(1.0, "holdout", real + shift));
                }
                LoopRec {
                    team: "A".into(),
                    i,
                    ty: "ROAD".into(),
                    d_km: 50.0,
                    matched_share: 1.0,
                    contiguous: true,
                    holdout: held,
                    twin_share: None,
                    poly_vs_graph: 0.0,
                    real: Measures {
                        way: [real; 6],
                        surface: [real; 3],
                        marked: real,
                        ..Measures::default()
                    },
                    real_check: None,
                    plays,
                }
            })
            .collect()
    }

    fn meta(name: &str, chain: Option<ChainMeta>, secs: f64) -> VariantMeta {
        VariantMeta {
            name: name.into(),
            fam: None,
            fam_road_only: false,
            taste: chain.as_ref().map(|_| "rt".into()),
            gamma: None,
            params: TasteParams::default(),
            tables: Vec::new(),
            taste_xxh3: "0".into(),
            team_windows: TeamWindowCounters::default(),
            secs: 1.0,
            peak_anon_mb: 1.0,
            chain,
            l1_after: Vec::new(),
            class_meters: crate::likeness::variants::ClassMetersOut { pooled: Vec::new() },
            team_times: ["A", "B", "C"]
                .iter()
                .map(|t| TeamTime {
                    team: (*t).into(),
                    loops: 10,
                    plays_full: 20,
                    cpu_s_full: secs,
                    cpu_s_full_by_cursor: vec![(0.25, secs * 0.6), (1.0, secs * 0.4)],
                    build_s: 1.0,
                    wall_s: secs + 5.0,
                })
                .collect(),
        }
    }

    fn done_base(shift: f64) -> Done {
        Done {
            variant: Variant::base(),
            meta: meta("base", None, 100.0),
            fingerprint: "b".into(),
            loops: loops(shift, 0.0),
        }
    }

    fn done_step(k: u8, shift: f64, leak: f64) -> Done {
        let name = format!("rt05-p{k}");
        let variant = Variant {
            name: name.clone(),
            fam: None,
            fam_road_only: false,
            taste: Some((TasteSource::RecalTeam, 0.5)),
            chain: Some(Chain { stem: "rt05".into(), step: k, steps: 3, dims: Dims::D72 }),
        };
        let cm = ChainMeta {
            stem: "rt05".into(),
            step: k,
            steps: 3,
            learned_from: if k == 1 { "base".into() } else { format!("rt05-p{}", k - 1) },
            dims: "72".into(),
            team_n0: 25.0,
            mult_bound: 4.0,
            diag: Vec::new(),
        };
        Done {
            variant,
            meta: meta(&name, Some(cm), 100.0 * f64::from(k + 1)),
            fingerprint: "x".into(),
            loops: loops(shift, leak),
        }
    }

    fn conds() -> Conditions {
        Conditions {
            commit: "t".into(),
            cursors: vec![0.25, 1.0],
            params: TasteParams::default(),
            at_4a_defaults: false,
            base_4a_fingerprint: "0".into(),
            holdout_share: 0.25,
            step1_check: None,
            all_check_applies: false,
        }
    }

    #[test]
    fn the_step_table_the_first_half_and_the_cost_of_a_profile_are_exact() {
        // Écart du généré : 0,40 (base), 0,18 (p1), 0,10 (p2), 0,30 (p3) ; la fuite de p3 vaut 0,10 en `S / S_base`.
        let done =
            vec![done_base(0.40), done_step(1, 0.18, 0.0), done_step(2, 0.10, 0.0), done_step(3, 0.30, 0.04)];
        let cmp = compare(&conds(), &done);
        assert_eq!(cmp.chains.len(), 1);
        assert_eq!(cmp.chains[0].steps, ["rt05-p1", "rt05-p2", "rt05-p3"]);
        let g = ("ROAD", 0.25);
        let ratio = |reading: Reading, v: &str| cell(&cmp.criterion, "toutes", reading, &g, v).unwrap().0;
        // p0 = 1,00 ; les autres : décalage / décalage de base (W1 d'un décalage constant).
        assert!((ratio(Reading::All, "base") - 1.0).abs() < 1e-12);
        assert!((ratio(Reading::All, "rt05-p1") - 0.45).abs() < 1e-9);
        assert!((ratio(Reading::All, "rt05-p2") - 0.25).abs() < 1e-9);
        assert!((ratio(Reading::Gap, "rt05-p3") - 0.75).abs() < 1e-9);
        assert_eq!(first_half(&cmp, &cmp.chains[0], "toutes", Reading::All, &g), Some(1));
        assert_eq!(first_half(&cmp, &cmp.chains[0], "toutes", Reading::Gap, &("ROAD", 1.0)), Some(1));
        // GRAVEL : aucun jeu, aucune ligne : « jamais ».
        assert_eq!(first_half(&cmp, &cmp.chains[0], "toutes", Reading::All, &("GRAVEL", 0.25)), None);
        // La fuite de p3 : (S_in − S_gap) / S_base, 0,04 / 0,40 = 0,10 au curseur 0,25 seulement.
        assert!((leak(&cmp, &g, "rt05-p3").unwrap() - 0.10).abs() < 1e-9);
        assert!(leak(&cmp, &("ROAD", 1.0), "rt05-p3").unwrap().abs() < 1e-9);
        // Le surapprentissage de p3 : mises à l'écart (0,34 / 0,40) contre apprentissage (0,30 / 0,40), même surcouche ;
        // nul ailleurs, et nul à p0 par construction ; par équipe aussi.
        assert!((overfit(&cmp, "toutes", &g, "rt05-p3").unwrap() - 0.10).abs() < 1e-9);
        assert!((overfit(&cmp, "A", &g, "rt05-p3").unwrap() - 0.10).abs() < 1e-9);
        assert!(overfit(&cmp, "toutes", &("ROAD", 1.0), "rt05-p3").unwrap().abs() < 1e-9);
        assert!(overfit(&cmp, "toutes", &g, "rt05-p1").unwrap().abs() < 1e-9);
        assert!(overfit(&cmp, "toutes", &g, "base").unwrap().abs() < 1e-12);
        // Le rendu : l'ordre des pas, p0 = 1.00 d'abord.
        let md = markdown(&cmp, &done);
        let t = md.find("### rt05 — lecture « toutes »").expect("table de chaîne");
        let body = &md[t..];
        let (p0, p1, p2, p3) = (
            body.find("| p0 | base |").unwrap(),
            body.find("| p1 | rt05-p1 | base |").unwrap(),
            body.find("| p2 | rt05-p2 | rt05-p1 |").unwrap(),
            body.find("| p3 | rt05-p3 | rt05-p2 |").unwrap(),
        );
        assert!(p0 < p1 && p1 < p2 && p2 < p3);
        assert!(body[p0..p1].contains("1.00"));
        assert!(body[p1..p2].contains("**0.45**"));
        // Le coût d'un profil de 3 pas : les générations `base`, p1, p2 (pas p3) ; 100 + 200 + 300 s cpu par équipe.
        let a = cmp.team_cost.iter().find(|r| r.team == "A").unwrap();
        assert_eq!(a.steps, 3);
        assert_eq!(
            a.generations.iter().map(|g| g.variant.as_str()).collect::<Vec<_>>(),
            ["base", "rt05-p1", "rt05-p2"]
        );
        assert!((a.cpu_s_total - 600.0).abs() < 1e-9);
        assert_eq!(a.est_s, vec![(1, 600.0), (4, 150.0), (8, 75.0), (16, 37.5)]);
        // Un seul curseur : 60 % des temps.
        assert!((a.cpu_s_total_one_cursor - 360.0).abs() < 1e-9 && a.cursor == 0.25);
        assert_eq!(a.est_s_one_cursor[1], (4, 90.0));
        // « autres » : la somme des autres lettres (ici C seule).
        let o = cmp.team_cost.iter().find(|r| r.team == "autres").unwrap();
        assert!((o.cpu_s_total - 600.0).abs() < 1e-9);
        assert_eq!(cmp.team_cost.len(), 3);
        assert!(md.contains("## 9. Coût d'un profil par équipe"));
        assert!(md.contains("Mesuré à 16 fils sur 8 cœurs"));
        // Le × base du service : P50 identique à base (100 ms), donc 1,00.
        assert!(cmp.service.iter().all(|r| r.ms_p50_x_base.is_some_and(|x| (x - 1.0).abs() < 1e-9)));
    }

    #[test]
    fn the_all_check_compares_s_base_to_the_4a_to_a_thousandth() {
        let rows = |s: [f64; 4]| -> Vec<CritRow> {
            S_BASE_ALL_4A
                .iter()
                .zip(s)
                .map(|(&(ty, cursor, _), x)| CritRow {
                    reading: Reading::All.name().into(),
                    team: "toutes".into(),
                    ty: ty.into(),
                    cursor,
                    n_real: 100,
                    s_base: Some(x),
                    cells: Vec::new(),
                })
                .collect()
        };
        assert!(all_check(&rows([0.614, 0.7214, 0.735, 0.6804])).ok);
        assert!(!all_check(&rows([0.614, 0.7214, 0.735, 0.682])).ok);
        assert!(!all_check(&[]).ok);
    }
}
