//! Sorties de la livraison B (note : `ord-retouche-lot0.md` §7.5 et §9) : les lignes de `move_end` et de `resize`, les
//! agrégats et les critères K2 à K4. **Aucun id, aucune coordonnée** : la lettre de l'équipe, le rang dans l'ordre de
//! l'export, des nombres.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::{json, Value};

use super::continuity::Q;
use super::move_end::MeLine;
use super::out::Share;
use super::resize::RsLine;

fn q(v: &[f64]) -> Value {
    serde_json::to_value(Q::of(v)).unwrap_or(Value::Null)
}

fn share(k: usize, n: usize) -> Value {
    serde_json::to_value(Share::new(k, n)).unwrap_or(Value::Null)
}

fn counts<'a>(it: impl Iterator<Item = &'a str>) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for x in it {
        *m.entry(x.to_string()).or_default() += 1;
    }
    m
}

pub fn jsonl<T: serde::Serialize>(lines: &[T]) -> Result<String, serde_json::Error> {
    let mut s = String::new();
    for l in lines {
        s.push_str(&serde_json::to_string(l)?);
        s.push('\n');
    }
    Ok(s)
}

/// Essais de `move_end` jugés par K2 : `E′` s'accroche (hors `end_too_far`), pas de fermeture en boucle.
fn k2_eligible(l: &MeLine) -> bool {
    l.outcome != "end_too_far" && l.outcome != "no_seam" && l.kind != "close_loop"
}

/// Un essai dont `move_end_in` a tourné au-delà de l'accroche (ni `no_seam` ni `end_too_far`) : la population de K4.
fn me_played(l: &MeLine) -> bool {
    l.outcome != "no_seam" && l.outcome != "end_too_far"
}

pub fn me_agg(lines: &[&MeLine]) -> Value {
    let ok: Vec<&&MeLine> = lines.iter().filter(|l| l.outcome == "ok").collect();
    let el: Vec<&&MeLine> = lines.iter().filter(|l| k2_eligible(l)).collect();
    let acc = el.iter().filter(|l| l.accepted).count();
    let f = |g: &dyn Fn(&MeLine) -> Option<f64>| -> Vec<f64> { ok.iter().filter_map(|l| g(l)).collect() };
    let codes = |sel: &dyn Fn(&MeLine) -> &Vec<String>| {
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        for l in &ok {
            for c in sel(l) {
                *m.entry(c.clone()).or_default() += 1;
            }
        }
        m
    };
    // K4 : tous les appels de l'essai, échecs et reprises compris ; les lignes non jouées n'y entrent pas.
    let k4: Vec<f64> = lines.iter().filter(|l| me_played(l)).map(|l| l.call_ms + l.ctl_ms).collect();
    json!({
        "n": lines.len(),
        "outcomes": counts(lines.iter().map(|l| l.outcome.as_str())),
        "k2_eligible": el.len(),
        "k2_accepted": share(acc, el.len()),
        "truncated": share(el.iter().filter(|l| l.outcome == "truncated").count(), el.len()),
        "widened": share(ok.iter().filter(|l| l.widened).count(), ok.len()),
        "retried_passes": share(ok.iter().filter(|l| l.passes > 1).count(), ok.len()),
        "prefix_retry": share(lines.iter().filter(|l| l.prefix_retry).count(), lines.len()),
        "c1_codes": codes(&|l| &l.c1),
        "c2_codes": codes(&|l| &l.c2),
        "c3_outside": share(ok.iter().filter(|l| l.c3_outside_m > 0.0).count(), ok.len()),
        "abandon_m": q(&f(&|l| l.best.as_ref().map(|b| b.abandon_m))),
        "raccord_m": q(&f(&|l| l.best.as_ref().map(|b| b.raccord_m))),
        "raccord_over_crow": q(&f(&|l| l.best.as_ref().map(|b| b.raccord_over_crow))),
        "length_ratio": q(&f(&|l| l.best.as_ref().map(|b| b.length_ratio))),
        "team_share": q(&f(&|l| l.team_share)),
        "angle_deg": q(&f(&|l| l.angle_deg)),
        "unchecked_m": q(&f(&|l| Some(l.unchecked_m))),
        "masked_m": q(&f(&|l| Some(l.masked_m))),
        "gen_ms": q(&lines.iter().filter(|l| l.outcome == "ok").map(|l| l.gen_ms).collect::<Vec<_>>()),
        "k4_ms": q(&k4),
        "win_ms": q(&lines.iter().map(|l| l.win_ms).collect::<Vec<_>>()),
        "keep_all_differs": share(ok.iter().filter(|l| l.keep_all.is_some()).count(), ok.len()),
    })
}

fn grouped<'a, T>(items: &'a [T], key: &dyn Fn(&T) -> String) -> BTreeMap<String, Vec<&'a T>> {
    let mut m: BTreeMap<String, Vec<&'a T>> = BTreeMap::new();
    for x in items {
        m.entry(key(x)).or_default().push(x);
    }
    m
}

pub fn rs_agg(lines: &[&RsLine]) -> Value {
    let ok: Vec<&&RsLine> = lines.iter().filter(|l| l.outcome == "ok").collect();
    let acc: Vec<&&RsLine> = lines.iter().filter(|l| l.best.is_some()).collect();
    let f = |g: &dyn Fn(&RsLine, &super::resize::Best) -> Option<f64>| -> Vec<f64> {
        acc.iter().filter_map(|l| l.best.as_ref().and_then(|b| g(l, b))).collect()
    };
    let wins: Vec<&super::resize::WinLine> = lines.iter().flat_map(|l| l.windows.iter()).collect();
    let mut refused: BTreeMap<String, usize> = BTreeMap::new();
    for l in lines {
        for (c, n) in &l.refused_codes {
            *refused.entry(c.clone()).or_default() += *n as usize;
        }
    }
    let shortening: Vec<&&RsLine> = lines.iter().filter(|l| l.delta < 0.0).collect();
    let shorten = shortening.len();
    let too_short = shortening.iter().filter(|l| l.fail.as_deref() == Some("DISTANCE_TOO_SHORT")).count();
    let short_wins: Vec<&super::resize::WinLine> = shortening.iter().flat_map(|l| l.windows.iter()).collect();
    let short_wins_too_short = short_wins.iter().filter(|w| w.outcome == "DISTANCE_TOO_SHORT").count();
    let hit_some =
        shortening.iter().filter(|l| l.windows.iter().any(|w| w.outcome == "DISTANCE_TOO_SHORT")).count();
    let wsum: f64 = lines.iter().map(|l| l.weight).sum();
    let wok: f64 = ok.iter().map(|l| l.weight).sum();
    json!({
        "n": lines.len(),
        "outcomes": counts(lines.iter().map(|l| l.outcome.as_str())),
        "ok": share(ok.len(), lines.len()),
        "ok_weighted": if wsum > 0.0 { Value::from(wok / wsum) } else { Value::Null },
        "proposals": q(&lines.iter().map(|l| l.proposals as f64).collect::<Vec<_>>()),
        "refused_codes": refused,
        "reuse_refused": lines.iter().map(|l| l.reuse_refused).sum::<usize>(),
        "engine_fail_dominant": counts(lines.iter().filter_map(|l| l.fail.as_deref())),
        "distance_too_short_share_of_shortening": share(too_short, shorten),
        "distance_too_short_some_window_of_shortening": share(hit_some, shorten),
        "distance_too_short_share_of_shortening_windows": share(short_wins_too_short, short_wins.len()),
        "controls_capped": share(lines.iter().filter(|l| l.capped).count(), lines.iter().filter(|l| l.controls_run > 0).count()),
        "windows": wins.len(),
        "window_yield": share(wins.iter().filter(|w| w.routes > 0).count(), wins.len()),
        "window_outcomes": counts(wins.iter().map(|w| w.outcome.as_str())),
        "best_from_first_family": share(acc.iter().filter(|l| l.best.as_ref().is_some_and(|b| b.family == 0)).count(), acc.len()),
        "err": q(&f(&|_, b| Some(b.err))),
        "kept_share": q(&f(&|_, b| Some(b.kept_share))),
        "changed_over_target": q(&f(&|l, b| Some(b.changed_m / l.target_m.max(1.0)))),
        "team_share": q(&f(&|_, b| b.team_share)),
        "angle_in_deg": q(&f(&|_, b| b.angle_in_deg)),
        "angle_out_deg": q(&f(&|_, b| b.angle_out_deg)),
        "unchecked_m": q(&f(&|_, b| Some(b.unchecked_m))),
        "total_ms": q(&lines.iter().filter(|l| l.outcome != "no_window").map(|l| l.total_ms).collect::<Vec<_>>()),
        "first_ok_ms": q(&lines.iter().filter_map(|l| l.first_ok_ms).collect::<Vec<_>>()),
        "ctl_ms": q(&lines.iter().map(|l| l.ctl_ms).collect::<Vec<_>>()),
    })
}

pub struct PlaySummary {
    pub json: Value,
    pub md: String,
}

fn qp(v: &Value, k: &str) -> String {
    match v.get(k).filter(|x| !x.is_null()) {
        Some(x) => {
            format!("P10 {:.0} / P50 {:.0} / P90 {:.0} / max {:.0}", x["p10"], x["p50"], x["p90"], x["max"])
        }
        None => "n/a".into(),
    }
}

fn sh_md(v: &Value) -> String {
    match v.as_object() {
        Some(o) if !o.is_empty() => format!(
            "{}/{} = {:.1} % [{:.1} ; {:.1}]",
            o["k"],
            o["n"],
            o["p"].as_f64().unwrap_or(0.0) * 100.0,
            o["lo"].as_f64().unwrap_or(0.0) * 100.0,
            o["hi"].as_f64().unwrap_or(0.0) * 100.0
        ),
        _ => "n/a".into(),
    }
}

pub fn summarise(meta: Vec<(String, String)>, me: &[MeLine], rs: &[RsLine], cut: bool) -> PlaySummary {
    let me_refs: Vec<&MeLine> = me.iter().collect();
    let rs_refs: Vec<&RsLine> = rs.iter().collect();
    let me_total = me_agg(&me_refs);
    let rs_total = rs_agg(&rs_refs);
    let by = |m: BTreeMap<String, Vec<&MeLine>>| -> BTreeMap<String, Value> {
        m.into_iter().map(|(k, v)| (k, me_agg(&v))).collect()
    };
    let by_rs = |m: BTreeMap<String, Vec<&RsLine>>| -> BTreeMap<String, Value> {
        m.into_iter().map(|(k, v)| (k, rs_agg(&v))).collect()
    };
    let strat_me = |l: &MeLine| format!("{}/{}", l.ty, l.shape);
    let strat_rs = |l: &RsLine| format!("{}/{}", l.ty, l.shape);
    let me_json = json!({
        "total": me_total,
        "by_stratum": by(grouped(me, &strat_me)),
        "by_cursor": by(grouped(me, &|l| format!("{}", l.cursor))),
        "by_kind": by(grouped(me, &|l| super::move_end::kind_group_of(&l.kind).to_string())),
        "by_team": by(grouped(me, &|l| l.team.clone())),
    });
    let rs_json = json!({
        "total": rs_total,
        "by_stratum": by_rs(grouped(rs, &strat_rs)),
        "by_cursor": by_rs(grouped(rs, &|l| format!("{}", l.cursor))),
        "by_delta": by_rs(grouped(rs, &|l| format!("{:+.0}", l.delta * 100.0))),
        "by_team": by_rs(grouped(rs, &|l| l.team.clone())),
    });
    // Critères.
    let k3: Vec<&RsLine> = rs.iter().filter(|l| l.delta.abs() <= 0.25 + 1e-9).collect();
    let k3_ok = k3.iter().filter(|l| l.outcome == "ok").count();
    let k3_raw = k3.iter().filter(|l| l.first_ok_ms.is_some()).count();
    let k3_w: f64 = k3.iter().map(|l| l.weight).sum();
    let k3_wok: f64 = k3.iter().filter(|l| l.outcome == "ok").map(|l| l.weight).sum();
    let k2: Vec<&MeLine> = me.iter().filter(|l| k2_eligible(l)).collect();
    let k2_ok = k2.iter().filter(|l| l.accepted).count();
    let k2s = Share::new(k2_ok, k2.len());
    // Le verdict de K3 porte sur la part pondérée par strate (effectif de Kish) ; la part brute est la seconde ligne.
    let k3s = Share::weighted(k3.iter().map(|l| (l.weight, l.outcome == "ok")));
    let k3_unw = Share::new(k3_ok, k3.len());
    // Le raccourcissement seul : la moitié que le moteur du lot 7a a refaite (arbre d'itinéraire au lieu de la lentille).
    let k3_short: Vec<&&RsLine> = k3.iter().filter(|l| l.delta < 0.0).collect();
    let k3_short_s = Share::new(k3_short.iter().filter(|l| l.outcome == "ok").count(), k3_short.len());
    let k3_long: Vec<&&RsLine> = k3.iter().filter(|l| l.delta > 0.0).collect();
    let k3_long_s = Share::new(k3_long.iter().filter(|l| l.outcome == "ok").count(), k3_long.len());
    let k4_rs: Vec<f64> = rs.iter().filter(|l| l.outcome != "no_window").map(|l| l.total_ms).collect();
    let k4_me: Vec<f64> = me.iter().filter(|l| me_played(l)).map(|l| l.call_ms + l.ctl_ms).collect();
    let (q4r, q4m) = (Q::of(&k4_rs), Q::of(&k4_me));
    let k4_verdict = |q: &Option<Q>| match q {
        Some(q) if q.p90 < 2000.0 => "tenu",
        Some(_) => "non tenu",
        None => "sans objet",
    };
    let criteria = json!({
        "k2": {"share": k2s, "verdict": k2s.verdict(0.95), "seuil": 0.95},
        "k3": {"share": k3s, "unweighted": k3_unw, "shortening_only": k3_short_s, "lengthening_only": k3_long_s, "before_controls": Share::new(k3_raw, k3.len()), "weighted_p": if k3_w > 0.0 { Value::from(k3_wok / k3_w) } else { Value::Null },
               "verdict": k3s.verdict(0.90), "seuil": 0.90},
        "k4_resize_p90_ms": q4r.map(|x| x.p90), "k4_resize_verdict": k4_verdict(&q4r),
        "k4_move_end_p90_ms": q4m.map(|x| x.p90), "k4_move_end_verdict": k4_verdict(&q4m),
    });
    // Courbe de succès de K3 en fonction de δ.
    let curve: BTreeMap<String, Value> = grouped(rs, &|l| format!("{:+.0}", l.delta * 100.0))
        .into_iter()
        .map(|(k, v)| {
            let ok = v.iter().filter(|l| l.outcome == "ok").count();
            (k, serde_json::to_value(Share::new(ok, v.len())).unwrap_or(Value::Null))
        })
        .collect();
    let json = json!({
        "meta": meta.iter().cloned().collect::<BTreeMap<_, _>>(),
        "cut": cut,
        "criteria": criteria,
        "success_by_delta": curve,
        "move_end": me_json,
        "resize": rs_json,
    });

    let mut md = String::from("# Retouche : `move_end` et `resize` (agrégats ; lot 0 B ou moteur du lot 7a, voir « mise en œuvre »)\n\n");
    for (k, v) in &meta {
        let _ = writeln!(md, "- {k} : {v}");
    }
    if cut {
        md.push_str("- **campagne coupée** (`--max-minutes`)\n");
    }
    md.push_str("\n## Critères\n\n| # | critère | mesure | verdict |\n|---|---|---|---|\n");
    let _ = writeln!(
        md,
        "| K2 | `move_end` sans refus du contrôleur, 95 % | {} | {} |",
        sh_md(&criteria["k2"]["share"]),
        k2s.verdict(0.95)
    );
    let _ = writeln!(
        md,
        "| K3 | distance à ±3 %, 90 %, \\|δ\\| ≤ 25 % | pondéré (n effectif de Kish) {} ; non pondéré {} | {} |",
        sh_md(&criteria["k3"]["share"]),
        sh_md(&criteria["k3"]["unweighted"]),
        k3s.verdict(0.90)
    );
    let _ = writeln!(
        md,
        "| K4 | P90 sous 2 s | `resize` P90 {} ms ({}) ; `move_end` P90 {} ms ({}) | |",
        q4r.map_or("n/a".into(), |x| format!("{:.0}", x.p90)),
        k4_verdict(&q4r),
        q4m.map_or("n/a".into(), |x| format!("{:.0}", x.p90)),
        k4_verdict(&q4m)
    );
    md.push_str("\n## `move_end`\n\n");
    let t = &me_json["total"];
    let _ = writeln!(md, "- essais : {} ; issues : {}", t["n"], t["outcomes"]);
    let _ = writeln!(md, "- K2 : {}", sh_md(&t["k2_accepted"]));
    let _ = writeln!(md, "- tronqués (`E′` sur le parcours) : {}", sh_md(&t["truncated"]));
    let _ = writeln!(
        md,
        "- rayon élargi : {} ; rejeu du préfixe : {} ; reprises après un virage serré : {}",
        sh_md(&t["widened"]),
        sh_md(&t["prefix_retry"]),
        sh_md(&t["retried_passes"])
    );
    let _ = writeln!(
        md,
        "- codes C1 : {} ; codes C2 : {} ; réutilisation hors zone : {}",
        t["c1_codes"],
        t["c2_codes"],
        sh_md(&t["c3_outside"])
    );
    let _ = writeln!(md, "- mètres abandonnés : {}", qp(t, "abandon_m"));
    let _ = writeln!(md, "- raccord, m : {}", qp(t, "raccord_m"));
    let _ = writeln!(md, "- raccord / vol d'oiseau : {}", qp(t, "raccord_over_crow"));
    let _ = writeln!(md, "- L′ / L (x1000) : voir le json");
    let _ = writeln!(md, "- temps `move_end_in`+contrôles, ms : {}", qp(t, "k4_ms"));
    let _ = writeln!(md, "- mètres sans contrôle près des coutures : {}", qp(t, "unchecked_m"));
    md.push_str("\n### Par rayon et par lieu\n\n| groupe | essais | K2 |\n|---|---|---|\n");
    for (k, v) in me_json["by_kind"].as_object().into_iter().flatten() {
        let _ = writeln!(md, "| {k} | {} | {} |", v["n"], sh_md(&v["k2_accepted"]));
    }
    md.push_str("\n### Par curseur\n\n| curseur | essais | K2 |\n|---|---|---|\n");
    for (k, v) in me_json["by_cursor"].as_object().into_iter().flatten() {
        let _ = writeln!(md, "| {k} | {} | {} |", v["n"], sh_md(&v["k2_accepted"]));
    }
    md.push_str("\n## `resize`\n\n");
    let t = &rs_json["total"];
    let _ = writeln!(md, "- demandes : {} ; issues : {}", t["n"], t["outcomes"]);
    let _ = writeln!(
        md,
        "- K3 (|δ| ≤ 25 %), pondéré : {} ; non pondéré : {}",
        sh_md(&criteria["k3"]["share"]),
        sh_md(&criteria["k3"]["unweighted"])
    );
    let _ = writeln!(
        md,
        "- K3, raccourcissement seul : {} ; allongement seul : {}",
        sh_md(&criteria["k3"]["shortening_only"]),
        sh_md(&criteria["k3"]["lengthening_only"])
    );
    let _ = writeln!(
        md,
        "- K3 avant les contrôles (un candidat à 3 % au moins existe) : {}",
        sh_md(&criteria["k3"]["before_controls"])
    );
    let _ = writeln!(md, "- succès toutes valeurs de δ : {}", sh_md(&t["ok"]));
    let _ = writeln!(
        md,
        "- rendement des fenêtres : {} ; issues : {}",
        sh_md(&t["window_yield"]),
        t["window_outcomes"]
    );
    let _ = writeln!(md, "- échec dominant du moteur : {}", t["engine_fail_dominant"]);
    let _ = writeln!(
        md,
        "- `DISTANCE_TOO_SHORT` comme échec dominant parmi les raccourcissements : {} ; au moins une fenêtre : {} ; parmi les fenêtres de raccourcissement : {}",
        sh_md(&t["distance_too_short_share_of_shortening"]),
        sh_md(&t["distance_too_short_some_window_of_shortening"]),
        sh_md(&t["distance_too_short_share_of_shortening_windows"])
    );
    let _ = writeln!(
        md,
        "- plafond de contrôles atteint (parmi les demandes contrôlées) : {}",
        sh_md(&t["controls_capped"])
    );
    let _ = writeln!(md, "- codes de refus (C1, C2, C3) : {}", t["refused_codes"]);
    let _ = writeln!(
        md,
        "- part gardée (x1000) : voir le json ; angle d'entrée, deg : {}",
        qp(t, "angle_in_deg")
    );
    let _ = writeln!(md, "- temps de la demande, ms : {}", qp(t, "total_ms"));
    let _ = writeln!(md, "- temps jusqu'à la première proposition à 3 %, ms : {}", qp(t, "first_ok_ms"));
    md.push_str("\n### Succès en fonction de δ\n\n| δ % | demandes | succès |\n|---|---|---|\n");
    for (k, v) in &curve {
        let _ = writeln!(md, "| {k} | {} | {} |", v["n"], sh_md(v));
    }
    md.push_str("\n### Par curseur\n\n| curseur | demandes | succès | part d'équipe du tronçon (P50) |\n|---|---|---|---|\n");
    for (k, v) in rs_json["by_cursor"].as_object().into_iter().flatten() {
        let _ = writeln!(md, "| {k} | {} | {} | {} |", v["n"], sh_md(&v["ok"]), v["team_share"]["p50"]);
    }
    md.push_str("\n### Par δ : part gardée\n\n| δ % | P10 | P50 |\n|---|---|---|\n");
    for (k, v) in rs_json["by_delta"].as_object().into_iter().flatten() {
        let _ = writeln!(
            md,
            "| {k} | {:.3} | {:.3} |",
            v["kept_share"]["p10"].as_f64().unwrap_or(f64::NAN),
            v["kept_share"]["p50"].as_f64().unwrap_or(f64::NAN)
        );
    }
    PlaySummary { json, md }
}
