//! Sorties de la livraison A (note : `ord-retouche-lot0.md` §2.6) : une ligne par trace (`-continuity.jsonl`), les
//! agrégats (`-continuity.json`) et le résumé recopiable (`-continuity.md`). **Aucun id, aucune coordonnée** : la
//! lettre de l'équipe, le rang dans l'ordre de l'export, des nombres.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;

use super::continuity::{Outcome, Shape, TrackRec, Q};

/// Quantile de la loi normale à 95 % unilatéral : l'intervalle de Wilson est à 90 %.
const Z90: f64 = 1.644_853_626_951_472_6;

/// Une part `k / n` et son intervalle de Wilson à 90 %.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct Share {
    pub k: usize,
    pub n: usize,
    pub p: f64,
    pub lo: f64,
    pub hi: f64,
}

impl Share {
    pub fn new(k: usize, n: usize) -> Self {
        if n == 0 {
            return Self { k, n, p: 0.0, lo: 0.0, hi: 1.0 };
        }
        let (kf, nf) = (k as f64, n as f64);
        let p = kf / nf;
        let z2 = Z90 * Z90;
        let denom = 1.0 + z2 / nf;
        let centre = (p + z2 / (2.0 * nf)) / denom;
        let half = Z90 * (p * (1.0 - p) / nf + z2 / (4.0 * nf * nf)).sqrt() / denom;
        Self { k, n, p, lo: (centre - half).max(0.0), hi: (centre + half).min(1.0) }
    }

    /// « tenu » si la borne basse atteint le seuil, « non tenu » si la borne haute est en dessous, sinon « à la limite ».
    pub fn verdict(&self, threshold: f64) -> &'static str {
        if self.n == 0 {
            "sans objet"
        } else if self.lo >= threshold {
            "tenu"
        } else if self.hi < threshold {
            "non tenu"
        } else {
            "à la limite"
        }
    }
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct Agg {
    pub n: usize,
    pub km: f64,
    pub continuous: Option<Share>,
    pub matched_share: Option<Q>,
    pub legs: Option<Q>,
    pub runs: Option<Q>,
    pub holes: Option<Q>,
    pub holes_per_100km: Option<Q>,
    /// Les trous et les plages eux-mêmes, toutes traces réunies.
    pub hole_chord_m: Option<Q>,
    pub run_m: Option<Q>,
    pub seams: Option<Q>,
    pub seams_per_km: Option<Q>,
    /// Traces sans aucune couture valide (distance infinie).
    pub no_seam: Option<Share>,
    /// Distance maximale à la couture, sur les traces qui en ont une.
    pub max_dist_to_seam_m: Option<Q>,
    pub within_500: Option<Share>,
    pub within_1000: Option<Share>,
    pub within_2000: Option<Share>,
    /// Même mesure sur `[5 %, 95 %]` (la vraie contrainte de `resize`).
    pub inner_within_1000: Option<Share>,
    /// Même mesure sur l'abscisse de marche : ce que coûte la corde d'un trou.
    pub walk_within_1000: Option<Share>,
    /// Distance de la dernière couture (marge avant) à l'arrivée (`move_end`).
    pub end_within_1000: Option<Share>,
    pub end_within_2000: Option<Share>,
    pub order_anomaly: Option<Share>,
    pub drift_over_2pct: Option<Share>,
    pub proj_p50_m: Option<Q>,
    pub proj_max_m: Option<Q>,
}

fn share(k: usize, n: usize) -> Option<Share> {
    (n > 0).then(|| Share::new(k, n))
}

pub fn aggregate(recs: &[&TrackRec]) -> Agg {
    let n = recs.len();
    let v = |f: &dyn Fn(&TrackRec) -> f64| -> Vec<f64> { recs.iter().map(|r| f(r)).collect() };
    let finite =
        |f: &dyn Fn(&TrackRec) -> Option<f64>| -> Vec<f64> { recs.iter().filter_map(|r| f(r)).collect() };
    let count = |f: &dyn Fn(&TrackRec) -> bool| recs.iter().filter(|r| f(r)).count();
    let within = |f: &dyn Fn(&TrackRec) -> Option<f64>, m: f64| count(&|r| f(r).is_some_and(|d| d <= m));
    let all_holes: Vec<f64> = recs.iter().flat_map(|r| r.hole_chords.iter().copied()).collect();
    let all_runs: Vec<f64> = recs.iter().flat_map(|r| r.run_lens.iter().copied()).collect();
    let dist = |r: &TrackRec| r.max_dist_to_seam_m;
    let inner = |r: &TrackRec| r.max_dist_inner_m;
    let walk = |r: &TrackRec| r.max_dist_to_seam_walk_m;
    let from_end = |r: &TrackRec| r.last_seam_from_end_m;
    Agg {
        n,
        km: recs.iter().map(|r| r.doc_m).sum::<f64>() / 1000.0,
        continuous: share(count(&|r| r.continuous), n),
        matched_share: Q::of(&v(&|r| r.matched_share)),
        legs: Q::of(&v(&|r| r.legs as f64)),
        runs: Q::of(&v(&|r| r.runs as f64)),
        holes: Q::of(&v(&|r| r.holes as f64)),
        holes_per_100km: Q::of(&v(&|r| r.holes_per_100km)),
        hole_chord_m: Q::of(&all_holes),
        run_m: Q::of(&all_runs),
        seams: Q::of(&v(&|r| r.seams as f64)),
        seams_per_km: Q::of(&v(&|r| r.seams_per_km)),
        no_seam: share(count(&|r| r.max_dist_to_seam_m.is_none()), n),
        max_dist_to_seam_m: Q::of(&finite(&dist)),
        within_500: share(within(&dist, 500.0), n),
        within_1000: share(within(&dist, 1000.0), n),
        within_2000: share(within(&dist, 2000.0), n),
        inner_within_1000: share(within(&inner, 1000.0), n),
        walk_within_1000: share(within(&walk, 1000.0), n),
        end_within_1000: share(within(&from_end, 1000.0), n),
        end_within_2000: share(within(&from_end, 2000.0), n),
        order_anomaly: share(count(&|r| r.order_anomalies > 0), n),
        drift_over_2pct: share(count(&|r| r.drift > 0.02), n),
        proj_p50_m: Q::of(&finite(&|r| r.proj_p50_m)),
        proj_max_m: Q::of(&finite(&|r| r.proj_max_m)),
    }
}

/// Comptes de la population (avant toute mesure).
#[derive(Clone, Debug, Default, Serialize)]
pub struct Population {
    pub rows: usize,
    pub unreadable: usize,
    pub empty: usize,
    /// Kilomètres annoncés des traces vides.
    pub empty_km: f64,
    pub measured: usize,
    pub excluded_measured: usize,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub meta: Vec<(String, String)>,
    pub population: BTreeMap<String, Population>,
    pub groups: BTreeMap<String, Agg>,
    /// Par cause de refus du banc : traces, km, et les mesures de couture des seules traces écartées sont dans
    /// `groups["refused/<cause>"]`.
    pub refusals: BTreeMap<String, (usize, f64)>,
    pub cut: bool,
}

fn team_group(t: &str) -> &str {
    if t == "A" || t == "B" {
        t
    } else {
        "autres"
    }
}

/// Les groupes nommés d'une trace (elle compte dans chacun).
fn groups_of(r: &TrackRec) -> Vec<String> {
    let mut g = vec![
        "total".to_string(),
        format!("stratum/{}/{}", r.ty, if r.shape == Shape::Loop { "loop" } else { "oneway" }),
        format!("team/{}", team_group(&r.team)),
    ];
    if r.ty != "MIXED" {
        g.push("k1".into());
        if r.retained {
            g.push("k1/retained_loop".into());
        }
        if r.retained_any {
            g.push("k1/retained_any".into());
        }
        g.push(format!("k1/{}", r.ty));
    }
    match r.refusal {
        Some(c) => g.push(format!("refused/{c}")),
        None => g.push("retained".into()),
    }
    g
}

pub fn summarise(meta: Vec<(String, String)>, outcomes: &[(String, Outcome)], cut: bool) -> Summary {
    let mut population: BTreeMap<String, Population> = BTreeMap::new();
    let mut by_group: BTreeMap<String, Vec<&TrackRec>> = BTreeMap::new();
    let mut refusals: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    for (team, o) in outcomes {
        let (ty, kind) = match o {
            Outcome::Unreadable { ty } => (*ty, 0),
            Outcome::Empty { ty, .. } => (*ty, 1),
            Outcome::Measured(r) => (r.ty, 2),
        };
        for key in [format!("type/{ty}"), "type/total".to_string(), format!("team/{}", team_group(team))] {
            let p = population.entry(key).or_default();
            p.rows += 1;
            match kind {
                0 => p.unreadable += 1,
                1 => {
                    p.empty += 1;
                    if let Outcome::Empty { doc_m, .. } = o {
                        p.empty_km += doc_m / 1000.0;
                    }
                }
                _ => {
                    p.measured += 1;
                    if let Outcome::Measured(r) = o {
                        p.excluded_measured += usize::from(r.excluded);
                    }
                }
            }
        }
        if let Outcome::Measured(r) = o {
            for g in groups_of(r) {
                by_group.entry(g).or_default().push(r);
            }
            if let Some(c) = r.refusal {
                let e = refusals.entry(c.to_string()).or_default();
                e.0 += 1;
                e.1 += r.doc_m / 1000.0;
            }
        }
    }
    let groups = by_group.iter().map(|(k, v)| (k.clone(), aggregate(v))).collect();
    Summary { meta, population, groups, refusals, cut }
}

pub fn jsonl(outcomes: &[(String, Outcome)]) -> Result<String, serde_json::Error> {
    let mut s = String::new();
    for (_, o) in outcomes {
        if let Outcome::Measured(r) = o {
            s.push_str(&serde_json::to_string(r)?);
            s.push('\n');
        }
    }
    Ok(s)
}

fn pct(s: &Option<Share>) -> String {
    s.as_ref()
        .map_or("-".into(), |s| format!("{:.1} % [{:.1} ; {:.1}]", s.p * 100.0, s.lo * 100.0, s.hi * 100.0))
}

fn q(q: &Option<Q>) -> String {
    q.as_ref().map_or("-".into(), |q| format!("{:.0} / {:.0} / {:.0} / {:.0}", q.p10, q.p50, q.p90, q.max))
}

fn q2(q: &Option<Q>) -> String {
    q.as_ref().map_or("-".into(), |q| format!("{:.2} / {:.2} / {:.2} / {:.2}", q.p10, q.p50, q.p90, q.max))
}

/// Le résumé recopiable : agrégats seulement.
pub fn markdown(s: &Summary) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "# Retouche, lot 0, A : continuité et coutures\n");
    for (k, v) in &s.meta {
        let _ = writeln!(o, "- {k} : {v}");
    }
    if s.cut {
        let _ = writeln!(o, "\n**Campagne coupée** (`--max-minutes`).");
    }
    let _ = writeln!(o, "\n## Population\n\n| groupe | lignes | illisibles | vides | km des vides | mesurées | dont exclues |\n|---|---|---|---|---|---|---|");
    for (k, p) in &s.population {
        let _ = writeln!(
            o,
            "| {k} | {} | {} | {} | {:.0} | {} | {} |",
            p.rows, p.unreadable, p.empty, p.empty_km, p.measured, p.excluded_measured
        );
    }
    if let Some(k1) = s.groups.get("k1") {
        let sh = k1.within_1000;
        let _ = writeln!(o, "\n## K1 : couture à 1 km ou moins pour 95 % des parcours\n");
        if let Some(sh) = sh {
            let _ = writeln!(o, "Toutes traces ROAD, GRAVEL, MTB lisibles et non vides, exclues comprises : {} sur {} soit {}, verdict **{}** (Wilson 90 %).", sh.k, sh.n, pct(&Some(sh)), sh.verdict(0.95));
        }
    }
    let _ = writeln!(o, "\n## Groupes\n\nQuantiles `P10 / P50 / P90 / max`. Distances en m.\n");
    let _ = writeln!(o, "| groupe | n | km | continues | sans couture | <= 500 m | <= 1 km | <= 2 km | <= 1 km sur [5 %, 95 %] | <= 1 km sur la marche | dernière couture <= 1 km de l'arrivée |\n|---|---|---|---|---|---|---|---|---|---|---|");
    for (k, a) in &s.groups {
        let _ = writeln!(
            o,
            "| {k} | {} | {:.0} | {} | {} | {} | {} | {} | {} | {} | {} |",
            a.n,
            a.km,
            pct(&a.continuous),
            pct(&a.no_seam),
            pct(&a.within_500),
            pct(&a.within_1000),
            pct(&a.within_2000),
            pct(&a.inner_within_1000),
            pct(&a.walk_within_1000),
            pct(&a.end_within_1000),
        );
    }
    let _ = writeln!(o, "\n## Continuité et coutures par groupe\n");
    let _ = writeln!(o, "| groupe | part appariée | jambes | plages | trous | trous / 100 km | corde des trous (m) | plages (m) | coutures | distance max à la couture (m) |\n|---|---|---|---|---|---|---|---|---|---|");
    for (k, a) in &s.groups {
        let _ = writeln!(
            o,
            "| {k} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            q2(&a.matched_share),
            q(&a.legs),
            q(&a.runs),
            q(&a.holes),
            q2(&a.holes_per_100km),
            q(&a.hole_chord_m),
            q(&a.run_m),
            q(&a.seams),
            q(&a.max_dist_to_seam_m),
        );
    }
    let _ = writeln!(o, "\n## Ordre des jambes (critère : moins de 1 % des traces avec une anomalie)\n");
    let _ = writeln!(o, "| groupe | traces avec anomalie d'ordre | dérive > 2 % | distance de projection P10 / P50 / P90 / max des P50 | idem des max |\n|---|---|---|---|---|");
    for (k, a) in &s.groups {
        let _ = writeln!(
            o,
            "| {k} | {} | {} | {} | {} |",
            pct(&a.order_anomaly),
            pct(&a.drift_over_2pct),
            q(&a.proj_p50_m),
            q(&a.proj_max_m)
        );
    }
    let _ = writeln!(o, "\n## Ce que les filtres du banc écartent\n\n| cause | traces | km |\n|---|---|---|");
    for (c, (n, km)) in &s.refusals {
        let _ = writeln!(o, "| {c} | {n} | {km:.0} |");
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_matches_known_values() {
        // 95 sur 100 à 90 % : environ [0,897 ; 0,974].
        let s = Share::new(95, 100);
        assert!((s.p - 0.95).abs() < 1e-12);
        assert!((s.lo - 0.8987).abs() < 0.003 && (s.hi - 0.9763).abs() < 0.003, "{s:?}");
        // Cas limites : tout et rien.
        let all = Share::new(50, 50);
        assert!((all.hi - 1.0).abs() < 1e-9 && all.lo > 0.94);
        let none = Share::new(0, 50);
        assert!(none.lo.abs() < 1e-9 && none.hi < 0.06);
        assert_eq!(Share::new(0, 0).verdict(0.95), "sans objet");
    }

    #[test]
    fn verdicts_follow_the_interval() {
        assert_eq!(Share::new(990, 1000).verdict(0.95), "tenu");
        assert_eq!(Share::new(900, 1000).verdict(0.95), "non tenu");
        assert_eq!(Share::new(19, 20).verdict(0.95), "à la limite");
    }
}
