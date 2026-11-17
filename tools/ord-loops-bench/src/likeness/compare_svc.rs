//! Section « 3b » de la comparaison (lot 4, §14.12.3) : le chemin du service contre le recalé par équipe de la seconde
//! sonde, la règle de la normalisation (§14.12.4) et les faits sur le chemin du service (§14.12.5). **Lettres et
//! agrégats seulement.**

use std::fmt::Write as _;

use serde::Serialize;

use super::compare::{Compare, CritRow, Done, Reading, ServiceRow};

/// Les variantes comparées, dans l'ordre.
pub const NAMES: [&str; 7] =
    ["rt05-p3", "common", "common-norm", "svc", "svc-norm", "svc-cap0", "svc-capt500"];

/// Les groupes : type × curseur.
const GROUPS: [(&str, f64); 4] = [("ROAD", 0.25), ("ROAD", 1.0), ("GRAVEL", 0.25), ("GRAVEL", 1.0)];

/// Rappel de la seconde sonde (constantes du relevé `ord-loops-lot4-sonde2.md`) : `S / S_base` toutes équipes.
pub const RECALL_RT_ALL: [f64; 4] = [0.244, 0.195, 0.371, 0.259];
pub const RECALL_RT_GAP: [f64; 4] = [0.203, 0.183, 0.379, 0.286];
pub const RECALL_RC_ALL: [f64; 4] = [0.255, 0.194, 0.377, 0.303];
/// B, ROAD, « toutes », aux deux curseurs, `rt05-p3` (seconde sonde).
pub const RECALL_B_ROAD: [f64; 2] = [0.80, 0.93];

/// Seuils de la règle de la normalisation (§14.12.4).
const NORM_MARGIN: f64 = 0.02;

fn row<'a>(rows: &'a [CritRow], team: &str, reading: Reading, g: (&str, f64)) -> Option<&'a CritRow> {
    rows.iter().find(|r| r.team == team && r.reading == reading.name() && r.ty == g.0 && r.cursor == g.1)
}

/// `S / S_base` d'une variante.
pub fn ratio(rows: &[CritRow], team: &str, reading: Reading, g: (&str, f64), variant: &str) -> Option<f64> {
    row(rows, team, reading, g)?.cells.iter().find(|c| c.variant == variant)?.ratio
}

fn has(done: &[Done], name: &str) -> bool {
    done.iter().any(|d| d.variant.name == name)
}

/// Le verdict de la règle de la normalisation pour une paire (`norm`, `brute`).
#[derive(Clone, Debug, Serialize)]
pub struct PairVerdict {
    pub norm: String,
    pub raw: String,
    /// Pire dégradation (`norm − brute`) sur les 4 groupes et les 2 lectures.
    pub worst_loss: Option<f64>,
    /// Meilleur gain (`brute − norm`).
    pub best_gain: Option<f64>,
    pub ok: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct NormRule {
    pub pairs: Vec<PairVerdict>,
    /// B ROAD `svc-norm` « toutes » sous 1,0 aux deux curseurs.
    pub b_road_below_one: Option<bool>,
    /// Service pas pire : P50 ≤ 1,10 × brute, part OK ≥ brute − 0,3 point, ni `PANIC` ni `WINDOW_ESCAPE`.
    pub service_ok: Option<bool>,
    /// Normalisée retenue : toutes les conditions réunies ; sinon brute.
    pub adopt_normalized: Option<bool>,
}

fn pair(rows: &[CritRow], norm: &str, raw: &str) -> PairVerdict {
    let (mut worst, mut best): (Option<f64>, Option<f64>) = (None, None);
    for reading in [Reading::All, Reading::Gap] {
        for g in GROUPS {
            if let (Some(n), Some(r)) =
                (ratio(rows, "toutes", reading, g, norm), ratio(rows, "toutes", reading, g, raw))
            {
                worst = Some(worst.map_or(n - r, |w: f64| w.max(n - r)));
                best = Some(best.map_or(r - n, |b: f64| b.max(r - n)));
            }
        }
    }
    let ok = worst.is_some_and(|w| w <= NORM_MARGIN) && best.is_some_and(|b| b >= NORM_MARGIN);
    PairVerdict { norm: norm.into(), raw: raw.into(), worst_loss: worst, best_gain: best, ok }
}

fn service<'a>(rows: &'a [ServiceRow], v: &str, c: f64) -> Option<&'a ServiceRow> {
    rows.iter().find(|r| r.variant == v && r.cursor == c)
}

/// Applique la règle de la normalisation sur ce qui a été joué.
pub fn norm_rule(cmp: &Compare, done: &[Done]) -> Option<NormRule> {
    let pairs: Vec<PairVerdict> = [("common-norm", "common"), ("svc-norm", "svc")]
        .into_iter()
        .filter(|(n, r)| has(done, n) && has(done, r))
        .map(|(n, r)| pair(&cmp.criterion, n, r))
        .collect();
    if pairs.is_empty() {
        return None;
    }
    let b_road: Vec<Option<f64>> = [0.25, 1.0]
        .iter()
        .map(|&c| ratio(&cmp.criterion, "B", Reading::All, ("ROAD", c), "svc-norm"))
        .collect();
    let b_road_below_one = (has(done, "svc-norm") && b_road.iter().all(Option::is_some))
        .then(|| b_road.iter().flatten().all(|&x| x < 1.0));
    let service_ok = (has(done, "svc-norm") && has(done, "svc")).then(|| {
        [0.25, 1.0].iter().all(|&c| {
            let (Some(n), Some(r)) = (service(&cmp.service, "svc-norm", c), service(&cmp.service, "svc", c))
            else {
                return true;
            };
            let p50 = match (n.ms_p50, r.ms_p50) {
                (Some(a), Some(b)) => a <= 1.10 * b,
                _ => true,
            };
            let bad = |row: &ServiceRow| {
                row.outcomes.get("PANIC").copied().unwrap_or(0)
                    + row.outcomes.get("WINDOW_ESCAPE").copied().unwrap_or(0)
            };
            p50 && n.ok_share >= r.ok_share - 0.003 && bad(n) == 0
        })
    });
    let adopt = pairs.iter().all(|p| p.ok) && b_road_below_one != Some(false) && service_ok != Some(false);
    Some(NormRule { pairs, b_road_below_one, service_ok, adopt_normalized: Some(adopt) })
}

/// Un fait de la lecture (§14.12.5), avec son résultat.
#[derive(Clone, Debug, Serialize)]
pub struct Fact {
    pub label: String,
    pub holds: Option<bool>,
    pub detail: String,
}

fn f3(x: Option<f64>) -> String {
    x.map_or("—".to_string(), |v| format!("{v:.3}"))
}

/// Les faits (a) à (d) du §14.12.5 ; (e) est la ligne de service du tableau.
pub fn facts(cmp: &Compare, done: &[Done]) -> Vec<Fact> {
    let mut out = Vec::new();
    let rows = &cmp.criterion;
    for v in ["common", "svc"].into_iter().filter(|v| has(done, v)) {
        let mut worst = (0.0f64, String::new());
        for reading in [Reading::All, Reading::Gap] {
            for g in GROUPS {
                if let Some(r) = ratio(rows, "toutes", reading, g, v) {
                    if r > worst.0 {
                        worst = (r, format!("{} {} ({})", g.0, g.1, reading.name()));
                    }
                }
            }
        }
        out.push(Fact {
            label: format!("(a) `{v}` : les 4 groupes ≤ 0,5, « toutes » et « écart »"),
            holds: (worst.0 > 0.0).then_some(worst.0 <= 0.5),
            detail: format!("plus haut : {:.3} en {}", worst.0, worst.1),
        });
    }
    if has(done, "svc") {
        let b: Vec<Option<f64>> =
            [0.25, 1.0].iter().map(|&c| ratio(rows, "B", Reading::All, ("ROAD", c), "svc")).collect();
        out.push(Fact {
            label: "(b) B ROAD « toutes » < 1,0 aux deux curseurs (`svc`)".into(),
            holds: b.iter().all(Option::is_some).then(|| b.iter().flatten().all(|&x| x < 1.0)),
            detail: format!(
                "{} · {} (rappel `rt05-p3` : {} · {})",
                f3(b[0]),
                f3(b[1]),
                RECALL_B_ROAD[0],
                RECALL_B_ROAD[1]
            ),
        });
        if has(done, "rt05-p3") {
            let mut worst = (f64::MIN, String::new());
            for g in GROUPS {
                if let (Some(s), Some(r)) = (
                    ratio(rows, "toutes", Reading::All, g, "svc"),
                    ratio(rows, "toutes", Reading::All, g, "rt05-p3"),
                ) {
                    if s - r > worst.0 {
                        worst = (s - r, format!("{} {}", g.0, g.1));
                    }
                }
            }
            out.push(Fact {
                label: "(c) `svc` ≤ `rt05-p3` + 0,05 par groupe, « toutes »".into(),
                holds: (worst.0 > f64::MIN).then_some(worst.0 <= 0.05),
                detail: format!("écart le plus grand : {:+.3} en {}", worst.0, worst.1),
            });
        }
        if has(done, "common") {
            let d: Vec<Option<f64>> = [0.25, 1.0]
                .iter()
                .map(|&c| {
                    Some(
                        ratio(rows, "A", Reading::All, ("ROAD", c), "svc")?
                            - ratio(rows, "A", Reading::All, ("ROAD", c), "common")?,
                    )
                })
                .collect();
            out.push(Fact {
                label: "(d) A : `svc − common` ≤ +0,03 en ROAD, « toutes »".into(),
                holds: d.iter().all(Option::is_some).then(|| d.iter().flatten().all(|&x| x <= 0.03)),
                detail: format!("{} · {}", f3(d[0]), f3(d[1])),
            });
        }
    }
    out
}

/// Ce que le banc ajoute à `Compare` pour le 3b.
#[derive(Clone, Debug, Serialize)]
pub struct ThreeB {
    pub norm_rule: Option<NormRule>,
    pub facts: Vec<Fact>,
}

pub fn three_b(cmp: &Compare, done: &[Done]) -> Option<ThreeB> {
    done.iter()
        .any(|d| d.variant.svc.is_some())
        .then(|| ThreeB { norm_rule: norm_rule(cmp, done), facts: facts(cmp, done) })
}

fn names_present(done: &[Done]) -> Vec<&'static str> {
    NAMES.iter().copied().filter(|n| has(done, n)).collect()
}

fn table(s: &mut String, rows: &[CritRow], team: &str, readings: &[Reading], names: &[&str]) {
    let _ = write!(s, "| lecture · groupe |");
    for n in names {
        let _ = write!(s, " {n} |");
    }
    let _ = writeln!(s, "\n|---|{}", "---:|".repeat(names.len()));
    for &reading in readings {
        for g in GROUPS {
            let _ = write!(s, "| {} · {} {} |", reading.name(), g.0, g.1);
            for n in names {
                let _ = write!(s, " {} |", f3(ratio(rows, team, reading, g, n)));
            }
            let _ = writeln!(s);
        }
    }
    let _ = writeln!(s);
}

/// La section « 3b » du résumé : vide si le banc n'a joué ni `common` ni `svc`.
pub fn section_md(s: &mut String, cmp: &Compare, done: &[Done]) {
    let Some(tb) = &cmp.three_b else { return };
    let names = names_present(done);
    let _ = writeln!(
        s,
        "## 9. 3b : le chemin du service (lot 4, §14.12)\n\n`S / S_base`, toutes équipes. Rappel de la seconde sonde (constantes) : `rt05-p3` toutes {:?}, écart {:?} ; `rc05-p3` toutes {:?} (ROAD 0,25 · ROAD 1 · GRAVEL 0,25 · GRAVEL 1).\n",
        RECALL_RT_ALL, RECALL_RT_GAP, RECALL_RC_ALL
    );
    table(s, &cmp.criterion, "toutes", &[Reading::All, Reading::Gap], &names);
    for team in ["A", "B"] {
        let _ = writeln!(s, "### Équipe {team}\n");
        table(s, &cmp.criterion, team, &[Reading::All, Reading::Gap], &names);
    }
    let _ = writeln!(
        s,
        "(B ROAD « toutes », rappel `rt05-p3` de la seconde sonde : {} · {}.)\n",
        RECALL_B_ROAD[0], RECALL_B_ROAD[1]
    );
    let _ = writeln!(s, "### Temps d'apprentissage par équipe (`svc`)\n");
    let _ = writeln!(s, "| variante | équipe | boucles (R, G, M) | pas | jeux | mur (s) | CPU (s) | estimé sur 4 fils (s) | budget 720 s |\n|---|---|---|---:|---:|---:|---:|---:|---|");
    for d in done {
        let Some(m) = &d.meta.svc else { continue };
        for t in &m.teams {
            let _ = writeln!(
                s,
                "| {} | {} | {:?} | {} | {} | {:.0} | {:.0} | {:.0} | {} |",
                d.variant.name,
                t.team,
                t.loops,
                t.steps_done,
                t.plays,
                t.learn_wall_s,
                t.learn_cpu_s,
                t.est_4_threads_s,
                if t.est_4_threads_s <= 720.0 { "tient" } else { "dépasse" }
            );
        }
    }
    let _ = writeln!(s, "\n### `mean_mult` au dernier pas (équipes jouées, par type)\n\n| variante | équipe | type | mean_mult | poids N/(N+n0) | écart L1 |\n|---|---|---|---:|---:|---:|");
    for d in done {
        let Some(m) = &d.meta.svc else { continue };
        for t in &m.teams {
            let last = t.steps.iter().map(|x| x.step).max();
            for x in t.steps.iter().filter(|x| Some(x.step) == last) {
                let _ = writeln!(
                    s,
                    "| {} | {} | {} | {:.3} | {:.3} | {:.3} |",
                    d.variant.name, t.team, x.ty, x.mean_mult, x.weight, x.l1_gap
                );
            }
        }
    }
    let _ = writeln!(s, "\n### Normalisation (règle du §14.12.4)\n");
    match &tb.norm_rule {
        None => {
            let _ = writeln!(s, "Non jouée (`common+norm` ou `svc+norm` absent) : brute.\n");
        }
        Some(r) => {
            for p in &r.pairs {
                let _ = writeln!(
                    s,
                    "- `{}` contre `{}` : pire perte {} (seuil +0,02), meilleur gain {} (seuil 0,02) : **{}**",
                    p.norm,
                    p.raw,
                    f3(p.worst_loss),
                    f3(p.best_gain),
                    if p.ok { "satisfait" } else { "non satisfait" }
                );
            }
            let _ = writeln!(
                s,
                "- B ROAD `svc-norm` « toutes » < 1,0 : {:?} ; service pas pire : {:?}\n- **Décision de la règle : {}**\n",
                r.b_road_below_one,
                r.service_ok,
                if r.adopt_normalized == Some(true) { "normalisée" } else { "brute" }
            );
        }
    }
    let _ = writeln!(s, "### Faits sur le chemin du service (§14.12.5)\n");
    for f in &tb.facts {
        let v = match f.holds {
            Some(true) => "oui",
            Some(false) => "NON",
            None => "sans objet",
        };
        let _ = writeln!(s, "- {} : **{v}** — {}", f.label, f.detail);
    }
    let _ =
        writeln!(s, "- (e) signaux de service contre `base` : voir la section 7 (une ligne par variante).\n");
}

#[cfg(test)]
mod tests {
    use super::super::compare::VarCell;
    use super::*;

    fn rows(norm: f64, raw: f64, gain_group: usize) -> Vec<CritRow> {
        let mut out = Vec::new();
        for reading in [Reading::All, Reading::Gap] {
            for (k, g) in GROUPS.iter().enumerate() {
                let n = if k == gain_group { norm - 0.03 } else { norm };
                out.push(CritRow {
                    reading: reading.name().into(),
                    team: "toutes".into(),
                    ty: g.0.into(),
                    cursor: g.1,
                    n_real: 10,
                    s_base: Some(1.0),
                    cells: vec![
                        VarCell { variant: "svc-norm".into(), s: Some(n), ratio: Some(n), n_gen: 10 },
                        VarCell { variant: "svc".into(), s: Some(raw), ratio: Some(raw), n_gen: 10 },
                    ],
                });
            }
        }
        out
    }

    #[test]
    fn the_normalisation_rule_needs_a_gain_somewhere_and_no_loss_anywhere() {
        // Égale partout : pas de gain, donc non satisfaite.
        let p = pair(&rows(0.30, 0.30, 99), "svc-norm", "svc");
        assert!(!p.ok);
        assert_eq!(p.best_gain.map(|x| (x * 1000.0).round()), Some(0.0));
        // Un gain de 0,03 dans un groupe, rien ailleurs : satisfaite.
        let p = pair(&rows(0.30, 0.30, 2), "svc-norm", "svc");
        assert!(p.ok, "{p:?}");
        // Une perte de 0,03 partout (sauf un groupe à +0,00) : non satisfaite.
        let p = pair(&rows(0.33, 0.30, 2), "svc-norm", "svc");
        assert!(!p.ok);
        assert!(p.worst_loss.unwrap() > 0.02);
    }

    #[test]
    fn missing_variants_make_missing_facts_not_failed_ones() {
        assert_eq!(pair(&[], "a", "b").worst_loss, None);
        assert!(!pair(&[], "a", "b").ok);
    }
}
