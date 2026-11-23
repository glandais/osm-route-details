//! Section « 5d » de la comparaison (lot 5, étape 7, §9) : la consigne de part de non-revêtu (`target:<t>`) contre la
//! même variante **sans** cible. Critères O1 à O5 (la part), E1 et E2 (les tronçons), nombre de pas et C1 réécrit (le
//! temps : médiane ≤ × 3,0 et P90 ≤ × 3,5 de la même requête sans cible). **Lettres et agrégats seulement.**
//!
//! Gravel seulement : la consigne n'a pas de sens sur ROAD (le banc y joue sans consigne), et le VTT compte trop peu de
//! boucles pour un seuil. Les seuils sont ceux de la note, avec le C1 de l'encadré des décisions du 23 novembre ; ils sont
//! écrits avant toute mesure et ce module les juge, il ne les règle pas. Les contrôles d'empreinte de référence ne
//! s'appliquent pas à ces variantes.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;

use super::compare::{group, Compare, Done, Reading};
use super::out::LoopRec;
use super::play::PlayRec;
use super::shape::{median, quantile_sorted, sorted};
use super::steer::within;

/// O1 : part des premières propositions dans la tolérance, au cran « Comme d'habitude » (0,47) et aux autres crans.
pub const O1_MIN_USUAL: f64 = 0.80;
pub const O1_MIN_OTHER: f64 = 0.70;
/// O2 : la médiane de (part − cible).
pub const O2_MAX_MEDIAN: f64 = 0.02;
/// O3 : aucune équipe de plus de 30 boucles gravel sous cette part d'O1, au cran « Comme d'habitude ».
pub const O3_MIN: f64 = 0.60;
pub const O3_MIN_LOOPS: usize = 30;
/// E1 : `run1k` médian généré au plus 0,10 sous le réel, et pas sous la variante sans cible.
pub const E1_MAX_BELOW: f64 = 0.10;
/// E2 : `switch_10km` médian généré ≤ 1,5 × le réel.
pub const E2_MAX_RATIO: f64 = 1.5;
/// C1 réécrit : médiane et P90 des temps de génération ≤ × 3,0 et × 3,5 de la même requête sans cible.
pub const C1_P50_MAX: f64 = 3.0;
pub const C1_P90_MAX: f64 = 3.5;
/// Au plus quatre pas (décision 2 du propriétaire).
pub const MAX_STEPS: u8 = 4;

const READINGS: [Reading; 2] = [Reading::All, Reading::Gap];
const TY: &str = "GRAVEL";

#[derive(Clone, Debug, Serialize)]
pub struct TimeStats {
    /// Requêtes jouées des deux côtés (issue `OK`), temps de génération seul (`gen_ms`).
    pub n: usize,
    pub p50_ms: Option<f64>,
    pub p90_ms: Option<f64>,
    pub paired_p50_ms: Option<f64>,
    pub paired_p90_ms: Option<f64>,
    /// Rapports des quantiles (le critère) et quantiles des rapports requête par requête (pour lire la queue).
    pub p50_ratio: Option<f64>,
    pub p90_ratio: Option<f64>,
    pub per_request_p50: Option<f64>,
    pub per_request_p90: Option<f64>,
    pub c1_ok: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Disc {
    /// Premières propositions qui ne tiennent pas la cible (ou jeux sans parcours gardé).
    pub missed: usize,
    /// Parmi elles, celles où le disque D / 2 offrait moins que la cible moins la tolérance : un secteur sans chemins.
    pub no_paths: usize,
    pub avail_p50: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SteerRow {
    pub variant: String,
    /// La variante sans cible qui sert de référence (temps, `run1k`) ; `None` si elle n'a pas été jouée.
    pub paired: Option<String>,
    pub reading: String,
    pub cursor: f64,
    pub target: f64,
    /// Requêtes gravel jouées, et celles qui ont un premier parcours gardé.
    pub n: usize,
    pub n_first: usize,
    pub o1: Option<f64>,
    pub o1_min: f64,
    pub o1_ok: Option<bool>,
    pub o2_median: Option<f64>,
    pub o2_ok: Option<bool>,
    /// O4 : part où `reached` du moteur dit la même chose que la mesure du banc sur le premier parcours gardé. Le
    /// critère complet (100 % sur la réponse du service, contrôleur compris) se juge au service (étape 8).
    pub o4_agree: Option<f64>,
    pub disc: Disc,
    pub steps_p50: Option<f64>,
    pub steps_p90: Option<f64>,
    pub steps_max: Option<u8>,
    pub steps_ok: Option<bool>,
    /// Part des requêtes réglées au pas 0 (rapportée, sans seuil depuis la décision 3).
    pub one_step_share: Option<f64>,
    pub e1_gen_p50: Option<f64>,
    pub e1_real_p50: Option<f64>,
    pub e1_paired_p50: Option<f64>,
    pub e1_ok: Option<bool>,
    pub e2_gen_p50: Option<f64>,
    pub e2_real_p50: Option<f64>,
    pub e2_ok: Option<bool>,
    pub time: TimeStats,
}

#[derive(Clone, Debug, Serialize)]
pub struct TeamRow {
    pub variant: String,
    pub team: String,
    pub reading: String,
    pub cursor: f64,
    pub loops: usize,
    pub o1: Option<f64>,
    pub ok: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SteerSection {
    pub rows: Vec<SteerRow>,
    /// O3, au cran 0,47, sur la lecture « toutes ».
    pub teams: Vec<TeamRow>,
}

/// La variante de même spécification **sans** cible (nom mis à part), si elle a été jouée.
fn paired<'a>(done: &'a [Done], d: &Done) -> Option<&'a Done> {
    done.iter().find(|x| {
        let mut v = d.variant.clone();
        v.target_milli = None;
        v.name = x.variant.name.clone();
        x.variant.target_milli.is_none() && x.variant == v
    })
}

fn play_of(l: &LoopRec, reading: Reading, cursor: f64) -> Option<&PlayRec> {
    l.plays.iter().find(|p| p.overlay == overlay_of(reading) && p.cursor == cursor)
}

fn overlay_of(r: Reading) -> &'static str {
    match r {
        Reading::Gap => "holdout",
        _ => "full",
    }
}

fn q(v: &[f64], p: f64) -> Option<f64> {
    quantile_sorted(&sorted(v), p)
}

fn ratio(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) if b > 0.0 => Some(a / b),
        _ => None,
    }
}

fn o1_min(target: f64) -> f64 {
    if (target - 0.47).abs() < 1e-9 {
        O1_MIN_USUAL
    } else {
        O1_MIN_OTHER
    }
}

fn time_stats(d: &Done, p: Option<&Done>, reading: Reading, cursor: f64) -> TimeStats {
    let (mut own, mut other, mut per_req) = (Vec::new(), Vec::new(), Vec::new());
    if let Some(p) = p {
        for (l, lp) in d.loops.iter().zip(&p.loops) {
            if (&l.team, l.i) != (&lp.team, lp.i) || l.ty != TY || !reading.keeps(l.holdout) {
                continue;
            }
            let (Some(a), Some(b)) = (play_of(l, reading, cursor), play_of(lp, reading, cursor)) else {
                continue;
            };
            if a.outcome != "OK" || b.outcome != "OK" {
                continue;
            }
            if let (Some(x), Some(y)) = (a.gen_ms, b.gen_ms) {
                own.push(x);
                other.push(y);
                if y > 0.0 {
                    per_req.push(x / y);
                }
            }
        }
    }
    let (p50, p90) = (q(&own, 0.5), q(&own, 0.9));
    let (b50, b90) = (q(&other, 0.5), q(&other, 0.9));
    let (r50, r90) = (ratio(p50, b50), ratio(p90, b90));
    TimeStats {
        n: own.len(),
        p50_ms: p50,
        p90_ms: p90,
        paired_p50_ms: b50,
        paired_p90_ms: b90,
        p50_ratio: r50,
        p90_ratio: r90,
        per_request_p50: q(&per_req, 0.5),
        per_request_p90: q(&per_req, 0.9),
        c1_ok: r50.zip(r90).map(|(a, b)| a <= C1_P50_MAX && b <= C1_P90_MAX),
    }
}

fn row(d: &Done, p: Option<&Done>, reading: Reading, cursor: f64) -> Option<SteerRow> {
    let target = f64::from(d.variant.target_milli?) / 1000.0;
    let g = group(&d.loops, TY, None, reading, cursor);
    if g.is_empty() {
        return None;
    }
    // Les jeux : (boucle, jeu).
    let plays: Vec<(&LoopRec, &PlayRec)> =
        g.iter().filter_map(|(l, _)| play_of(l, reading, cursor).map(|p| (*l, p))).collect();
    let firsts: Vec<(&LoopRec, &super::play::GenRoute)> =
        g.iter().filter_map(|(l, r)| r.map(|r| (*l, r))).collect();
    let n_first = firsts.len();
    let gaps: Vec<f64> = firsts.iter().map(|(_, r)| r.m.offroad - target).collect();
    let o1 = (n_first > 0)
        .then(|| firsts.iter().filter(|(_, r)| within(r.m.offroad, target)).count() as f64 / n_first as f64);
    let min = o1_min(target);
    let o2 = median(&gaps);
    // O4 (banc) : `reached` du moteur contre la mesure du banc sur le premier parcours gardé.
    let steer: Vec<&super::steer::SteerRec> = plays.iter().filter_map(|(_, p)| p.steer.as_ref()).collect();
    let agree: Vec<bool> =
        steer.iter().filter_map(|s| s.kept_share.map(|x| s.reached_engine == within(x, target))).collect();
    // O5 : les jeux dont la première proposition ne tient pas la cible.
    let missed: Vec<&&super::steer::SteerRec> =
        steer.iter().filter(|s| !s.kept_share.is_some_and(|x| within(x, target))).collect();
    let avail: Vec<f64> = missed.iter().filter_map(|s| s.disc.map(|d| d.0)).collect();
    let no_paths =
        avail.iter().filter(|a| **a < target - f64::from(super::steer::TOLERANCE_MILLI) / 1000.0).count();
    let steps: Vec<f64> = steer.iter().map(|s| f64::from(s.steps)).collect();
    let steps_max = steer.iter().map(|s| s.steps).max();
    // E1, E2 : les premiers parcours gardés contre le réel des mêmes boucles et contre la variante sans cible.
    let gen_run: Vec<f64> = firsts.iter().filter_map(|(_, r)| r.m.run1k).collect();
    let real_run: Vec<f64> = firsts.iter().filter_map(|(l, _)| l.real.run1k).collect();
    let paired_run: Vec<f64> = p
        .map(|p| {
            group(&p.loops, TY, None, reading, cursor)
                .iter()
                .filter_map(|(_, r)| r.and_then(|r| r.m.run1k))
                .collect()
        })
        .unwrap_or_default();
    let (gr, rr, pr) = (median(&gen_run), median(&real_run), median(&paired_run));
    let gen_sw: Vec<f64> = firsts.iter().map(|(_, r)| r.m.switch_10km).collect();
    let real_sw: Vec<f64> = firsts.iter().map(|(l, _)| l.real.switch_10km).collect();
    let (gs, rs) = (median(&gen_sw), median(&real_sw));
    Some(SteerRow {
        variant: d.variant.name.clone(),
        paired: p.map(|p| p.variant.name.clone()),
        reading: reading.name().to_string(),
        cursor,
        target,
        n: plays.len(),
        n_first,
        o1,
        o1_min: min,
        o1_ok: o1.map(|x| x >= min),
        o2_median: o2,
        o2_ok: o2.map(|x| x.abs() <= O2_MAX_MEDIAN),
        o4_agree: (!agree.is_empty())
            .then(|| agree.iter().filter(|a| **a).count() as f64 / agree.len() as f64),
        disc: Disc { missed: missed.len(), no_paths, avail_p50: median(&avail) },
        steps_p50: q(&steps, 0.5),
        steps_p90: q(&steps, 0.9),
        steps_max,
        steps_ok: steps_max.map(|m| m <= MAX_STEPS),
        one_step_share: (!steps.is_empty())
            .then(|| steps.iter().filter(|s| **s <= 1.0).count() as f64 / steps.len() as f64),
        e1_gen_p50: gr,
        e1_real_p50: rr,
        e1_paired_p50: pr,
        e1_ok: gr.zip(rr).map(|(g, r)| g >= r - E1_MAX_BELOW && pr.is_none_or(|p| g >= p)),
        e2_gen_p50: gs,
        e2_real_p50: rs,
        e2_ok: gs.zip(rs).map(|(g, r)| g <= E2_MAX_RATIO * r),
        time: time_stats(d, p, reading, cursor),
    })
}

/// O3 : par équipe de plus de 30 boucles gravel, O1 au cran 0,47, lecture « toutes ».
fn team_rows(d: &Done, cursor: f64) -> Vec<TeamRow> {
    let Some(milli) = d.variant.target_milli else { return Vec::new() };
    let target = f64::from(milli) / 1000.0;
    if (target - 0.47).abs() > 1e-9 {
        return Vec::new();
    }
    let g = group(&d.loops, TY, None, Reading::All, cursor);
    let mut by: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    for (l, r) in &g {
        let e = by.entry(l.team.as_str()).or_default();
        e.0 += 1;
        if let Some(r) = r {
            e.1 += 1;
            e.2 += usize::from(within(r.m.offroad, target));
        }
    }
    by.into_iter()
        .map(|(team, (loops, firsts, hit))| {
            let o1 = (firsts > 0).then(|| hit as f64 / firsts as f64);
            TeamRow {
                variant: d.variant.name.clone(),
                team: team.to_string(),
                reading: Reading::All.name().to_string(),
                cursor,
                loops,
                o1,
                ok: (loops > O3_MIN_LOOPS).then(|| o1.is_some_and(|x| x >= O3_MIN)),
            }
        })
        .collect()
}

/// La section ; `None` sans variante `target`.
pub fn steer_section(cmp: &Compare, done: &[Done]) -> Option<SteerSection> {
    let steered: Vec<&Done> = done.iter().filter(|d| d.variant.target_milli.is_some()).collect();
    if steered.is_empty() {
        return None;
    }
    let (mut rows, mut teams) = (Vec::new(), Vec::new());
    for d in steered {
        let p = paired(done, d);
        for &c in &cmp.conditions.cursors {
            for r in READINGS {
                rows.extend(row(d, p, r, c));
            }
            teams.extend(team_rows(d, c));
        }
    }
    Some(SteerSection { rows, teams })
}

fn pct(x: Option<f64>) -> String {
    x.map_or_else(|| "-".into(), |x| format!("{:.0} %", x * 100.0))
}

fn f2(x: Option<f64>) -> String {
    x.map_or_else(|| "-".into(), |x| format!("{x:.2}"))
}

fn f3(x: Option<f64>) -> String {
    x.map_or_else(|| "-".into(), |x| format!("{x:.3}"))
}

fn ok(x: Option<bool>) -> &'static str {
    x.map_or("sans objet", |b| if b { "tient" } else { "NE TIENT PAS" })
}

pub fn section_md(s: &mut String, cmp: &Compare) {
    let Some(sec) = &cmp.steer else { return };
    let _ = writeln!(s, "## 11. 5d : la consigne de part de non-revêtu (lot 5, étape 7, §9)\n");
    let _ = writeln!(
        s,
        "Gravel seulement (ROAD joué sans consigne). Tolérance ±0,05, au plus {MAX_STEPS} pas. Temps : génération seule \
         (`gen_ms`), requêtes jouées des deux côtés ; référence : la même variante sans cible. Les contrôles d'empreinte de \
         référence ne s'appliquent pas à ces variantes.\n"
    );
    for r in &sec.rows {
        let _ = writeln!(
            s,
            "### `{}`, lecture « {} », curseur {} (cible {:.2}, référence : {})\n",
            r.variant,
            r.reading,
            r.cursor,
            r.target,
            r.paired.as_deref().map_or("absente".to_string(), |p| format!("`{p}`"))
        );
        let _ = writeln!(s, "| critère | valeur | seuil | verdict |\n|---|---|---|---|");
        let _ = writeln!(
            s,
            "| O1 : première proposition à ±0,05 ({} sur {} jeux) | {} | ≥ {:.0} % | {} |",
            r.n_first,
            r.n,
            pct(r.o1),
            r.o1_min * 100.0,
            ok(r.o1_ok)
        );
        let _ = writeln!(
            s,
            "| O2 : médiane de (part − cible) | {} | ±{O2_MAX_MEDIAN} | {} |",
            f3(r.o2_median),
            ok(r.o2_ok)
        );
        let _ = writeln!(
            s,
            "| O4 (banc) : `reached` du moteur = mesure du banc | {} | 100 % au service | rapporté |",
            pct(r.o4_agree)
        );
        let _ = writeln!(
            s,
            "| O5 : cible manquée | {} jeux, dont {} où le disque D / 2 offre moins que la cible (secteur sans chemins), part disponible médiane {} | aucun seuil | rapporté |",
            r.disc.missed,
            r.disc.no_paths,
            pct(r.disc.avail_p50)
        );
        let _ = writeln!(
            s,
            "| pas : P50 / P90 / max, réglés au pas 0 | {} / {} / {} ; {} | ≤ {MAX_STEPS} | {} |",
            f2(r.steps_p50),
            f2(r.steps_p90),
            r.steps_max.map_or("-".into(), |m| m.to_string()),
            pct(r.one_step_share),
            ok(r.steps_ok)
        );
        let _ = writeln!(
            s,
            "| E1 : `run1k` médian généré / réel / sans cible | {} / {} / {} | réel − {E1_MAX_BELOW} et pas sous la référence | {} |",
            f3(r.e1_gen_p50),
            f3(r.e1_real_p50),
            f3(r.e1_paired_p50),
            ok(r.e1_ok)
        );
        let _ = writeln!(
            s,
            "| E2 : `switch_10km` médian généré / réel | {} / {} | ≤ {E2_MAX_RATIO} × réel | {} |",
            f2(r.e2_gen_p50),
            f2(r.e2_real_p50),
            ok(r.e2_ok)
        );
        let t = &r.time;
        let _ = writeln!(
            s,
            "| C1 : temps P50 / P90 avec cible, sans cible, rapports (sur {} requêtes) | {} / {} ms, {} / {} ms, × {} / × {} (par requête : × {} / × {}) | ≤ × {C1_P50_MAX} / × {C1_P90_MAX} | {} |\n",
            t.n,
            f2(t.p50_ms),
            f2(t.p90_ms),
            f2(t.paired_p50_ms),
            f2(t.paired_p90_ms),
            f2(t.p50_ratio),
            f2(t.p90_ratio),
            f2(t.per_request_p50),
            f2(t.per_request_p90),
            ok(t.c1_ok)
        );
    }
    if !sec.teams.is_empty() {
        let _ = writeln!(
            s,
            "### O3 : O1 par équipe au cran 0,47 (lecture « toutes »)\n\n| variante | curseur | équipe | boucles gravel | O1 | verdict (> {O3_MIN_LOOPS} boucles, ≥ {:.0} %) |\n|---|---|---|---|---|---|",
            O3_MIN * 100.0
        );
        for t in &sec.teams {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} | {} | {} |",
                t.variant,
                t.cursor,
                t.team,
                t.loops,
                pct(t.o1),
                ok(t.ok)
            );
        }
        s.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::measure::Measures;
    use super::super::play::{GenRoute, Recovered};
    use super::super::steer::SteerRec;
    use super::super::taste::TasteParams;
    use super::super::variants::{parse_variants, ClassMetersOut, TeamWindowCounters, VariantMeta};

    fn route(offroad: f64, run1k: f64) -> GenRoute {
        GenRoute {
            m: Measures { offroad, run1k: Some(run1k), switch_10km: 2.0, ..Measures::default() },
            length_over_target: 1.0,
            team_share: 0.5,
            family: "Triangle".into(),
            recovered: Recovered::default(),
            shared_vs_route_m: 0.0,
            poly_vs_graph: 0.0,
            route_xxh3: "0".into(),
        }
    }

    fn play(gen_ms: f64, routes: Vec<GenRoute>, steer: Option<SteerRec>) -> PlayRec {
        PlayRec {
            cursor: 0.25,
            overlay: "full".into(),
            outcome: "OK".into(),
            n_routes: routes.len(),
            n_kept: routes.len(),
            dropped: Default::default(),
            ms: gen_ms + 5.0,
            gen_ms: Some(gen_ms),
            steer,
            routes,
            best_shape: None,
            best_recovered: None,
            first_class_m: None,
            fig: None,
        }
    }

    fn done(spec: &str, plays: &dyn Fn(usize) -> PlayRec) -> Done {
        let variant =
            parse_variants(&["base".to_string(), spec.to_string()]).unwrap().into_iter().last().unwrap();
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
            class_meters: ClassMetersOut { pooled: Vec::new() },
            svc: None,
            team_times: Vec::new(),
        };
        let loops = (0..40)
            .map(|i| LoopRec {
                team: "A".into(),
                i: i as u32,
                ty: "GRAVEL".into(),
                d_km: 50.0,
                matched_share: 1.0,
                contiguous: true,
                holdout: false,
                twin_share: None,
                poly_vs_graph: 0.0,
                real: Measures { run1k: Some(0.5), switch_10km: 2.0, ..Measures::default() },
                real_check: None,
                plays: vec![plays(i)],
            })
            .collect();
        Done { variant, meta, fingerprint: "f".into(), loops }
    }

    /// 40 boucles : 34 premières propositions dans la tolérance (85 %), 6 trop basses ; temps × 2,5 ; un pas sur trois.
    #[test]
    fn a_steered_variant_is_judged_against_its_untargeted_twin() {
        let plain = done("svc", &|_| play(100.0, vec![route(0.20, 0.3)], None));
        let steered = done("svc+target:0.47", &|i| {
            let share = if i < 34 { 0.46 } else { 0.30 };
            let rec = SteerRec {
                target: 0.47,
                steps: if i % 3 == 0 { 2 } else { 1 },
                mu: 0.5,
                reached_engine: i < 34,
                kept_share: Some(share),
                disc: (i >= 34).then_some((if i == 34 { 0.1 } else { 0.6 }, 40.0)),
            };
            play(250.0, vec![route(share, 0.4)], Some(rec))
        });
        let done = [plain, steered];
        let p = paired(&done, &done[1]).expect("la jumelle sans cible");
        assert_eq!(p.variant.name, "svc");
        let r = row(&done[1], Some(p), Reading::All, 0.25).unwrap();
        assert_eq!((r.n, r.n_first), (40, 40));
        assert!((r.o1.unwrap() - 0.85).abs() < 1e-9 && r.o1_ok == Some(true));
        // La médiane de (part − cible) est −0,01 : dans ±0,02.
        assert!((r.o2_median.unwrap() + 0.01).abs() < 1e-9 && r.o2_ok == Some(true));
        assert_eq!(r.o4_agree, Some(1.0));
        assert_eq!((r.disc.missed, r.disc.no_paths), (6, 1));
        assert_eq!((r.steps_max, r.steps_ok), (Some(2), Some(true)));
        assert!((r.one_step_share.unwrap() - 26.0 / 40.0).abs() < 1e-9);
        // E1 : 0,4 contre un réel à 0,5 (≥ 0,4) et contre la jumelle à 0,3 ; E2 : 2 ≤ 1,5 × 2.
        assert_eq!((r.e1_ok, r.e2_ok), (Some(true), Some(true)));
        assert!((r.time.p50_ratio.unwrap() - 2.5).abs() < 1e-9 && r.time.c1_ok == Some(true));
        // Sans jumelle, ni temps ni critère C1.
        let r = row(&done[1], None, Reading::All, 0.25).unwrap();
        assert!(r.time.c1_ok.is_none() && r.paired.is_none());
        // Au cran « Comme d'habitude » : O3 pour l'équipe de 40 boucles.
        let t = team_rows(&done[1], 0.25);
        assert_eq!((t.len(), t[0].loops, t[0].ok), (1, 40, Some(true)));
    }

    #[test]
    fn a_slow_search_fails_c1() {
        let plain = done("svc", &|_| play(100.0, vec![route(0.20, 0.3)], None));
        let steered = done("svc+target:0.47", &|_| {
            let rec = SteerRec {
                target: 0.47,
                steps: 4,
                mu: 1.0,
                reached_engine: true,
                kept_share: Some(0.47),
                disc: None,
            };
            play(360.0, vec![route(0.47, 0.4)], Some(rec))
        });
        let done = [plain, steered];
        let r = row(&done[1], paired(&done, &done[1]), Reading::All, 0.25).unwrap();
        assert_eq!(r.time.c1_ok, Some(false));
        assert!((r.time.p50_ratio.unwrap() - 3.6).abs() < 1e-9);
    }

    #[test]
    fn ratios_and_thresholds() {
        assert_eq!(ratio(Some(3.0), Some(1.0)), Some(3.0));
        assert_eq!(ratio(Some(3.0), Some(0.0)), None);
        assert_eq!(o1_min(0.47), 0.80);
        assert_eq!(o1_min(0.25), 0.70);
        assert_eq!(o1_min(0.60), 0.70);
    }
}
