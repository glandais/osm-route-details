//! Sorties (§11.10 de la note) : le rapport JSON détaillé et le résumé `.md`, recopiable tel quel dans le rapport
//! commité. **Aucun départ, aucune coordonnée, aucun id d'arête, de trace ou d'équipe** : des lettres, des rangs et
//! des agrégats.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;
use xxhash_rust::xxh3::xxh3_64;

use super::export::RetentionCounts;
use super::holdout::OverlayCheck;
use super::measure::Measures;
use super::play::{GenRoute, PlayRec, Recovered, WindowCounters};
use super::shape::{median, quantiles5, wasserstein1, wasserstein1_normalised};
use super::variants::VariantMeta;

pub const TYPE_NAMES: [&str; 3] = ["ROAD", "GRAVEL", "MTB"];

/// Part des mètres d'une boucle mise à l'écart qu'une autre boucle de l'équipe doit prendre pour en être la jumelle.
pub const TWIN_SHARE: f64 = 0.9;

/// Une boucle réelle retenue et ses jeux. Désignée par `(team, i)`.
#[derive(Clone, Debug, Serialize)]
pub struct LoopRec {
    pub team: String,
    pub i: u32,
    pub ty: String,
    pub d_km: f64,
    pub matched_share: f64,
    pub contiguous: bool,
    pub holdout: bool,
    /// Boucle mise à l'écart : la plus grande part de ses mètres (`E(R)`) qu'une **autre** boucle retenue de
    /// l'équipe, restée dans la surcouche « écart », prend aussi (`holdout::twin_shares`). `None` sinon.
    pub twin_share: Option<f64>,
    /// `|l_poly − d_graph| / d_graph` de la marche réelle.
    pub poly_vs_graph: f64,
    pub real: Measures,
    /// Codes du contrôleur sur la marche continue ; `None` si elle ne l'est pas.
    pub real_check: Option<Vec<String>>,
    pub plays: Vec<PlayRec>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Phase {
    pub name: String,
    pub secs: f64,
    pub peak_anon_mb: f64,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub meta: Vec<(String, String)>,
    pub retention: BTreeMap<String, RetentionCounts>,
    /// Les lignes de `shapes.py` (`raw_legacy`).
    pub calibration: Vec<String>,
    /// Histogrammes de sommets sur les boucles retenues : `type -> définition -> {sommets: boucles}`.
    pub vertex_histograms: BTreeMap<String, BTreeMap<String, BTreeMap<u32, usize>>>,
    pub overlays: Vec<OverlayCheck>,
    pub phases: Vec<Phase>,
    pub windows: WindowCounters,
    /// Sonde du goût : ce que la variante a joué ; absent au banc du 4a.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<VariantMeta>,
    pub cursors: Vec<f64>,
    pub fingerprint: String,
    pub loops: Vec<LoopRec>,
}

/// Les mesures comparées, dans l'ordre (§11.5).
pub type Getter = fn(&Measures) -> Option<f64>;
pub const MEASURES: [(&str, Getter); 31] = [
    ("r_over_d", |m| Some(m.r_over_d)),
    ("far_pos", |m| Some(m.far_pos)),
    ("compactness", |m| Some(m.compactness)),
    ("vertices", |m| Some(f64::from(m.vertices))),
    ("vertices_10", |m| Some(f64::from(m.vertices_10))),
    ("vertices_legacy", |m| Some(f64::from(m.vertices_legacy))),
    ("turns_per_10km", |m| Some(m.turns_per_10km)),
    ("detour_far", |m| m.detour_far),
    ("sinuosity_10", |m| m.sinuosity_10),
    ("shared_share", |m| Some(m.shared_share)),
    ("crossings", |m| m.crossings.map(f64::from)),
    ("way_major", |m| Some(m.way[0])),
    ("way_tertiary", |m| Some(m.way[1])),
    ("way_minor", |m| Some(m.way[2])),
    ("way_street", |m| Some(m.way[3])),
    ("way_cycle", |m| Some(m.way[4])),
    ("way_path", |m| Some(m.way[5])),
    ("surf_paved", |m| Some(m.surface[0])),
    ("surf_presumed", |m| Some(m.surface[1])),
    ("surf_unpaved", |m| Some(m.surface[2])),
    ("marked", |m| Some(m.marked)),
    ("destination", |m| Some(m.destination)),
    // Lot 5, §3.1 : rapportées à part, hors de `S_GRAVEL`.
    ("offroad", |m| Some(m.offroad)),
    ("offroad_sc", |m| m.offroad_sc),
    ("run_p50_m", |m| m.run_p50_m),
    ("run1k", |m| m.run1k),
    ("switch_10km", |m| Some(m.switch_10km)),
    ("banned", |m| Some(m.banned)),
    ("banned_team", |m| Some(m.banned_team)),
    ("smooth_known", |m| Some(m.smooth_known)),
    ("mtb_known", |m| Some(m.mtb_known)),
];

/// Empreinte de résultat : xxh3 des lignes triées `lettre i curseur surcouche issue route_xxh3…` (sans aucun temps).
pub fn fingerprint(loops: &[LoopRec]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for l in loops {
        for p in &l.plays {
            let routes: Vec<&str> = p.routes.iter().map(|r| r.route_xxh3.as_str()).collect();
            lines.push(format!(
                "{} {} {} {} {} {}",
                l.team,
                l.i,
                p.cursor,
                p.overlay,
                p.outcome,
                routes.join(" ")
            ));
        }
    }
    lines.sort();
    format!("{:016x}", xxh3_64(lines.join("\n").as_bytes()))
}

fn f3(x: f64) -> String {
    format!("{x:.3}")
}

fn opt3(x: Option<f64>) -> String {
    x.map_or_else(|| "-".into(), f3)
}

fn q5(v: &[f64]) -> String {
    quantiles5(v).map_or_else(|| "-".into(), |q| q.iter().map(|x| f3(*x)).collect::<Vec<_>>().join(" · "))
}

fn pct(n: usize, d: usize) -> String {
    if d == 0 {
        "-".into()
    } else {
        format!("{:.1} %", 100.0 * n as f64 / d as f64)
    }
}

fn w1n(real: &[f64], gen: &[f64]) -> String {
    match wasserstein1_normalised(real, gen) {
        Some(x) => f3(x),
        None => match wasserstein1(real, gen) {
            Some(w) => format!("IQR nul (W1 {})", f3(w)),
            None => "-".into(),
        },
    }
}

/// Le jeu d'un curseur sur la surcouche complète.
fn play_of(l: &LoopRec, cursor: f64) -> Option<&PlayRec> {
    l.plays.iter().find(|p| p.overlay == "full" && p.cursor == cursor)
}

fn first(p: &PlayRec) -> Option<&GenRoute> {
    p.routes.first()
}

fn best_shape(p: &PlayRec) -> Option<&GenRoute> {
    p.best_shape.map(|k| &p.routes[k])
}

fn best_recovered(p: &PlayRec) -> Option<&GenRoute> {
    p.best_recovered.map(|k| &p.routes[k])
}

fn table_head(out: &mut String) {
    out.push_str(
        "| mesure | réel P10 · P25 · P50 · P75 · P90 | premier, idem | W1 / IQR | Δ médian apparié | `best_shape` P50 | W1 / IQR |\n|---|---|---|---:|---:|---:|---:|\n",
    );
}

/// La table d'un groupe (type × curseur) : `loops` = boucles du type.
fn group_table(out: &mut String, loops: &[&LoopRec], cursor: f64) {
    let plays: Vec<(&LoopRec, &PlayRec)> =
        loops.iter().filter_map(|l| play_of(l, cursor).map(|p| (*l, p))).collect();
    let ok: Vec<(&LoopRec, &PlayRec)> = plays.iter().filter(|(_, p)| p.outcome == "OK").copied().collect();
    let _ = writeln!(
        out,
        "n réel = {}, jeux = {}, `OK` = {} ({})\n",
        loops.len(),
        plays.len(),
        ok.len(),
        pct(ok.len(), plays.len())
    );
    table_head(out);
    for (name, get) in MEASURES {
        let real: Vec<f64> = loops.iter().filter_map(|l| get(&l.real)).collect();
        // Le généré des seules boucles dont le réel est mesuré : `crossings` n'est connu que des marches réelles
        // continues, et le comparer à tous les jeux mêlerait deux populations. Sans effet sur les autres mesures.
        let ok: Vec<(&LoopRec, &PlayRec)> =
            ok.iter().filter(|(l, _)| get(&l.real).is_some()).copied().collect();
        let g1: Vec<f64> = ok.iter().filter_map(|(_, p)| first(p).and_then(|r| get(&r.m))).collect();
        let gb: Vec<f64> = ok.iter().filter_map(|(_, p)| best_shape(p).and_then(|r| get(&r.m))).collect();
        let deltas: Vec<f64> =
            ok.iter().filter_map(|(l, p)| Some(get(&first(p)?.m)? - get(&l.real)?)).collect();
        let _ = writeln!(
            out,
            "| {name} | {} | {} | {} | {} | {} | {} |",
            q5(&real),
            q5(&g1),
            w1n(&real, &g1),
            opt3(median(&deltas)),
            opt3(median(&gb)),
            w1n(&real, &gb)
        );
    }
    out.push('\n');
}

fn by_type<'a>(loops: &'a [LoopRec], ty: &str) -> Vec<&'a LoopRec> {
    loops.iter().filter(|l| l.ty == ty).collect()
}

/// Histogramme d'une liste de comptes, plafonné à 6.
pub fn histogram(counts: impl Iterator<Item = u32>) -> BTreeMap<u32, usize> {
    let mut h = BTreeMap::new();
    for c in counts {
        *h.entry(c.min(6)).or_default() += 1;
    }
    h
}

fn hist_text(h: &BTreeMap<u32, usize>) -> String {
    h.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(", ")
}

/// Somme et médiane par trace d'un rapport de mètres, sur une liste de `Recovered`.
fn recon_stats(
    rs: &[Recovered],
    f: fn(&Recovered) -> (f64, f64),
    per: fn(&Recovered) -> Option<f64>,
) -> String {
    let (num, den) = rs.iter().map(f).fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
    let med: Vec<f64> = rs.iter().filter_map(per).collect();
    format!("{} / {}", if den > 0.0 { f3(num / den) } else { "-".into() }, opt3(median(&med)))
}

/// Les lignes que la variante ajoute en tête de son résumé (sonde du goût, §12.5).
fn variant_head(s: &mut String, v: &VariantMeta, r: &Report) {
    let _ = writeln!(s, "# Sonde du goût, variante `{}`\n", v.name);
    let taste = match (&v.taste, v.gamma) {
        (Some(t), Some(g)) => format!("goût `{t}`, γ = {g}"),
        _ => "pas de goût".into(),
    };
    let fam = v.fam.map_or("familiarité du curseur".to_string(), |f| format!("familiarité imposée {f}"));
    let _ = writeln!(s, "- variante : {taste} ; {fam}");
    let _ = writeln!(s, "- paramètres du goût : {:?}", v.params);
    let _ = writeln!(s, "- empreinte xxh3 des tables de goût (`taste_xxh3`) : `{}`", v.taste_xxh3);
    let _ = writeln!(
        s,
        "- tables jouées : {} ({} non neutres)",
        v.tables.len(),
        v.tables.iter().filter(|t| !t.neutral).count()
    );
    let w = &v.team_windows;
    let _ = writeln!(
        s,
        "- constructions de `WindowTeam` : {}, {:.0} ms au total, plus gros {:.1} Mo",
        w.builds,
        w.ms,
        w.max_bytes as f64 / 1e6
    );
    let _ = writeln!(s, "- durée de la variante : {:.0} s, pic anonyme {:.0} Mo", v.secs, v.peak_anon_mb);
    let _ = writeln!(s, "- empreinte de résultat : `{}`\n", r.fingerprint);
}

/// Le résumé `.md`.
pub fn summary(r: &Report) -> String {
    let mut s = String::new();
    let cursors = &r.cursors;
    if let Some(v) = &r.variant {
        variant_head(&mut s, v, r);
    }
    let _ = writeln!(s, "# Banc de vraisemblance (lot 4, 4a) : résumé\n");
    // 1. Conditions
    let _ = writeln!(s, "## 1. Conditions\n");
    for (k, v) in &r.meta {
        let _ = writeln!(s, "- {k} : {v}");
    }
    let _ = writeln!(s, "\n### Phases\n\n| phase | durée (s) | pic anonyme (Mo) |\n|---|---:|---:|");
    for p in &r.phases {
        let _ = writeln!(s, "| {} | {:.1} | {:.0} |", p.name, p.secs, p.peak_anon_mb);
    }
    // 2. Retenue
    let _ = writeln!(
        s,
        "\n## 2. Retenue par équipe\n\n| équipe | lignes | exclues | illisibles | type | part appariée | vide | trou | < min km | pas une boucle | > max km | retenues | gardées | continues | à l'écart |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"
    );
    let mut tot = RetentionCounts::default();
    for (t, c) in &r.retention {
        let _ = writeln!(
            s,
            "| {t} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            c.rows,
            c.excluded,
            c.unreadable,
            c.type_refused,
            c.matched_share_below,
            c.empty,
            c.hole_too_long,
            c.below_min_km,
            c.not_a_loop,
            c.above_max_km,
            c.retained,
            c.kept,
            c.contiguous,
            c.holdout
        );
        tot.rows += c.rows;
        tot.retained += c.retained;
        tot.kept += c.kept;
        tot.contiguous += c.contiguous;
        tot.holdout += c.holdout;
    }
    let _ = writeln!(
        s,
        "| total | {} | | | | | | | | | | {} | {} | {} | {} |",
        tot.rows, tot.retained, tot.kept, tot.contiguous, tot.holdout
    );
    // 3. Étalonnage
    let _ =
        writeln!(s, "\n## 3. Étalonnage\n\n`raw_legacy` : `shapes.py` en entier sur l'export brut.\n\n```");
    for l in &r.calibration {
        let _ = writeln!(s, "{l}");
    }
    let _ = writeln!(
        s,
        "```\n\nSommets des boucles réelles retenues, par type et par définition (plafonnés à 6) :\n"
    );
    for (ty, defs) in &r.vertex_histograms {
        let _ = writeln!(s, "- **{ty}**");
        for (d, h) in defs {
            let _ = writeln!(s, "  - `{d}` : {{{}}}", hist_text(h));
        }
    }
    // 4. Surcouches
    let _ = writeln!(
        s,
        "\n## 4. Surcouches reconstruites contre les fichiers du lot 6\n\n| équipe | arêtes | km comptés | arêtes (écart) | contre le fichier |\n|---|---:|---:|---:|---|"
    );
    for o in &r.overlays {
        let cmp = match o.differing_edges {
            None => "fichier absent".to_string(),
            Some(0) => "identique".to_string(),
            Some(n) => format!("{n} arêtes différentes"),
        };
        let _ = writeln!(
            s,
            "| {} | {} | {:.1} | {} | {cmp} |",
            o.team,
            o.edges,
            o.km_counted,
            o.holdout_edges.map_or("-".into(), |n| n.to_string())
        );
    }
    // 5. Par type et curseur
    let _ = writeln!(s, "\n## 5. Distributions, réel contre généré, par type et par curseur\n");
    for ty in TYPE_NAMES {
        let loops = by_type(&r.loops, ty);
        if loops.is_empty() {
            continue;
        }
        for &c in cursors {
            let note = if loops.len() < 30 { " (indicatif : moins de 30 boucles)" } else { "" };
            let _ = writeln!(s, "### {ty}, curseur {c}{note}\n");
            group_table(&mut s, &loops, c);
        }
    }
    // 6. Par équipe
    let _ = writeln!(s, "## 6. Par équipe\n");
    let per_team_measures = ["r_over_d", "compactness", "vertices", "vertices_10", "turns_per_10km"];
    for ty in TYPE_NAMES {
        let loops = by_type(&r.loops, ty);
        if loops.is_empty() {
            continue;
        }
        let mut groups: BTreeMap<String, Vec<&LoopRec>> = BTreeMap::new();
        for l in &loops {
            groups.entry(l.team.clone()).or_default().push(l);
        }
        let (big, small): (Vec<_>, Vec<_>) = groups.into_iter().partition(|(_, v)| v.len() >= 30);
        let mut rows: Vec<(String, Vec<&LoopRec>)> = big;
        let others: Vec<&LoopRec> = small.into_iter().flat_map(|(_, v)| v).collect();
        if !others.is_empty() {
            rows.push(("autres".into(), others));
        }
        for &c in cursors {
            let _ = writeln!(
                s,
                "### {ty}, curseur {c}\n\nPar mesure : réel P50 / premier P50 / W1 / IQR.\n\n| équipe | n | {} |\n|---|---:|{}",
                per_team_measures.join(" | "),
                per_team_measures.iter().map(|_| "---|").collect::<String>()
            );
            for (team, ls) in &rows {
                let mut cells = Vec::new();
                for name in per_team_measures {
                    let get = MEASURES.iter().find(|(n, _)| *n == name).expect("mesure connue").1;
                    let real: Vec<f64> = ls.iter().filter_map(|l| get(&l.real)).collect();
                    let gen: Vec<f64> = ls
                        .iter()
                        .filter_map(|l| play_of(l, c))
                        .filter(|p| p.outcome == "OK")
                        .filter_map(|p| first(p).and_then(|g| get(&g.m)))
                        .collect();
                    cells.push(format!(
                        "{} / {} / {}",
                        opt3(median(&real)),
                        opt3(median(&gen)),
                        w1n(&real, &gen)
                    ));
                }
                let _ = writeln!(s, "| {team} | {} | {} |", ls.len(), cells.join(" | "));
            }
            s.push('\n');
        }
    }
    // 7. Contrôleur sur le réel
    let _ = writeln!(s, "## 7. Le contrôleur sur les boucles réelles continues\n");
    let n = r.loops.len();
    let cont: Vec<&LoopRec> = r.loops.iter().filter(|l| l.real_check.is_some()).collect();
    let _ =
        writeln!(s, "Boucles retenues : {n} ; marche continue : {} ({}).\n", cont.len(), pct(cont.len(), n));
    let mut codes: BTreeMap<String, usize> = BTreeMap::new();
    for l in &cont {
        for c in l.real_check.iter().flatten() {
            *codes.entry(c.clone()).or_default() += 1;
        }
    }
    let _ = writeln!(s, "| code | boucles | part des continues |\n|---|---:|---:|");
    for (c, k) in codes.iter().filter(|(c, _)| *c != "WRONG_END") {
        let _ = writeln!(s, "| {c} | {k} | {} |", pct(*k, cont.len()));
    }
    if let Some(k) = codes.get("WRONG_END") {
        let _ = writeln!(
            s,
            "\n`WRONG_END` (attendu : départ et arrivée réels ne sont pas la même position) : {k} ({}).",
            pct(*k, cont.len())
        );
    }
    // 8. Issues
    let _ = writeln!(s, "\n## 8. Issues des jeux\n");
    let mut outcomes: BTreeMap<(String, String), BTreeMap<String, usize>> = BTreeMap::new();
    for l in &r.loops {
        for p in &l.plays {
            *outcomes
                .entry((format!("{}", p.cursor), p.overlay.clone()))
                .or_default()
                .entry(p.outcome.clone())
                .or_default() += 1;
        }
    }
    let _ = writeln!(s, "| curseur | surcouche | jeux | issues |\n|---:|---|---:|---|");
    for ((c, o), m) in &outcomes {
        let total: usize = m.values().sum();
        let list = m.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ");
        let _ = writeln!(s, "| {c} | {o} | {total} | {list} |");
    }
    let w = &r.windows;
    let _ = writeln!(
        s,
        "\nFenêtres : {} construites, {} servies du cache, {} insérées au-delà du budget, {} reprises après échappement.",
        w.built, w.hits, w.overflow, w.escapes
    );
    // 9. Reconstruction
    let _ = writeln!(s, "\n## 9. Reconstruction (boucles mises à l'écart)\n");
    let _ = writeln!(
        s,
        "Chaque cellule : part agrégée (Σ mètres retrouvés / Σ mètres réels) / médiane par trace. « Hors échantillon » : surcouche sans la boucle ; « dans l'échantillon » : surcouche complète, mêmes boucles. « Jumelle » : une autre boucle retenue de l'équipe, restée dans la surcouche, prend au moins 90 % des mètres de la boucle mise à l'écart (sortie de groupe enregistrée deux fois, même parcours roulé deux fois) ; « sans les jumelles » : la part agrégée totale des autres.\n"
    );
    let teams: Vec<&String> = r.retention.keys().collect();
    for &c in cursors {
        for (which, pick) in [
            ("premier", first as fn(&PlayRec) -> Option<&GenRoute>),
            ("meilleur des trois (retrouvé)", best_recovered),
        ] {
            let _ = writeln!(
                s,
                "### Curseur {c}, {which}\n\n| équipe | à l'écart | `OK` | total | arêtes d'équipe | arêtes qu'aucune autre trace ne prend | part de nouveau dans la trace (médiane) | total, dans l'échantillon | nouveau, dans l'échantillon | jumelle ≥ 0,9 | total, sans les jumelles |\n|---|---:|---:|---|---|---|---:|---|---|---:|---|"
            );
            for t in &teams {
                let held: Vec<&LoopRec> = r.loops.iter().filter(|l| &l.team == *t && l.holdout).collect();
                if held.is_empty() {
                    let _ = writeln!(s, "| {t} | 0 | | trop peu de boucles | | | | | | | |");
                    continue;
                }
                let mut out_rs: Vec<Recovered> = Vec::new();
                let mut in_rs: Vec<Recovered> = Vec::new();
                let mut lone_rs: Vec<Recovered> = Vec::new();
                let mut ok = 0;
                let is_twin = |l: &LoopRec| l.twin_share.is_some_and(|x| x >= TWIN_SHARE);
                let twins = held.iter().filter(|l| is_twin(l)).count();
                for l in &held {
                    let ho = l.plays.iter().find(|p| p.overlay == "holdout" && p.cursor == c);
                    let full = play_of(l, c);
                    if let Some(g) = ho.filter(|p| p.outcome == "OK").and_then(pick) {
                        ok += 1;
                        out_rs.push(g.recovered);
                        if !is_twin(l) {
                            lone_rs.push(g.recovered);
                        }
                        if let Some(f) = full.filter(|p| p.outcome == "OK").and_then(pick) {
                            in_rs.push(f.recovered);
                        }
                    }
                }
                let _ = writeln!(
                    s,
                    "| {t} | {} | {ok} | {} | {} | {} | {} | {} | {} | {twins} | {} |",
                    held.len(),
                    recon_stats(&out_rs, |r| (r.found_m, r.real_m), Recovered::total),
                    recon_stats(&out_rs, |r| (r.team_found_m, r.team_real_m), Recovered::team),
                    recon_stats(&out_rs, |r| (r.fresh_found_m, r.fresh_real_m), Recovered::fresh),
                    opt3(median(&out_rs.iter().filter_map(Recovered::fresh_real_share).collect::<Vec<_>>())),
                    recon_stats(&in_rs, |r| (r.found_m, r.real_m), Recovered::total),
                    recon_stats(&in_rs, |r| (r.fresh_found_m, r.fresh_real_m), Recovered::fresh),
                    recon_stats(&lone_rs, |r| (r.found_m, r.real_m), Recovered::total),
                );
            }
            s.push('\n');
        }
    }
    // 10. Cohérence
    let _ = writeln!(s, "## 10. Cohérence des mesures\n");
    let real_pg: Vec<f64> = r.loops.iter().map(|l| l.poly_vs_graph).collect();
    let mut gen_pg: Vec<f64> = Vec::new();
    let mut gen_sh: Vec<f64> = Vec::new();
    for l in &r.loops {
        for p in l.plays.iter().filter(|p| p.overlay == "full") {
            for g in &p.routes {
                gen_pg.push(g.poly_vs_graph);
                gen_sh.push(g.shared_vs_route_m.abs());
            }
        }
    }
    let med_p90 = |v: &[f64]| match (median(v), quantiles5(v)) {
        (Some(m), Some(q)) => format!("médiane {}, P90 {}", f3(m), f3(q[4])),
        _ => "-".into(),
    };
    let _ = writeln!(s, "- `|l_poly − d_graph| / d_graph`, réel : {}", med_p90(&real_pg));
    let _ = writeln!(s, "- `|l_poly − d_graph| / d_graph`, généré : {}", med_p90(&gen_pg));
    let _ = writeln!(s, "- `|recouvrement mesuré − Route::shared_m|`, m, généré : {}", med_p90(&gen_sh));
    // 11. Lecture du §2.4
    let _ = writeln!(s, "\n## 11. Lecture du §2.4 (des faits, sans décision)\n");
    let _ = writeln!(s, "| type | curseur | r / D généré P50 (réel P25–P75) | dans | compacité générée P50 (réel P25–P75) | dans | compacité générée P50 contre réel P25 |\n|---|---:|---|---|---|---|---|");
    for ty in TYPE_NAMES {
        let loops = by_type(&r.loops, ty);
        if loops.is_empty() {
            continue;
        }
        for &c in cursors {
            let g = |get: fn(&Measures) -> f64| -> (Option<f64>, Option<[f64; 5]>) {
                let real: Vec<f64> = loops.iter().map(|l| get(&l.real)).collect();
                let gen: Vec<f64> = loops
                    .iter()
                    .filter_map(|l| play_of(l, c))
                    .filter(|p| p.outcome == "OK")
                    .filter_map(|p| first(p).map(|x| get(&x.m)))
                    .collect();
                (median(&gen), quantiles5(&real))
            };
            let (rg, rr) = g(|m| m.r_over_d);
            let (cg, cr) = g(|m| m.compactness);
            let inside = |g: Option<f64>, q: Option<[f64; 5]>| match (g, q) {
                (Some(g), Some(q)) => (q[1]..=q[3]).contains(&g).to_string(),
                _ => "-".into(),
            };
            let _ = writeln!(
                s,
                "| {ty} | {c} | {} ({}–{}) | {} | {} ({}–{}) | {} | {} contre {} |",
                opt3(rg),
                opt3(rr.map(|q| q[1])),
                opt3(rr.map(|q| q[3])),
                inside(rg, rr),
                opt3(cg),
                opt3(cr.map(|q| q[1])),
                opt3(cr.map(|q| q[3])),
                inside(cg, cr),
                opt3(cg),
                opt3(cr.map(|q| q[1]))
            );
        }
    }
    if let Some(&c) = cursors.iter().find(|&&c| c >= 1.0) {
        let _ = writeln!(
            s,
            "\nPart « qu'aucune autre trace ne prend » retrouvée (agrégée), meilleur des trois en reconstruction, curseur {c} :\n"
        );
        let _ = writeln!(s, "| équipe | à l'écart | part retrouvée |\n|---|---:|---|");
        for t in &teams {
            let rs: Vec<Recovered> = r
                .loops
                .iter()
                .filter(|l| &l.team == *t && l.holdout)
                .filter_map(|l| l.plays.iter().find(|p| p.overlay == "holdout" && p.cursor == c))
                .filter(|p| p.outcome == "OK")
                .filter_map(best_recovered)
                .map(|g| g.recovered)
                .collect();
            if !rs.is_empty() {
                let _ = writeln!(
                    s,
                    "| {t} | {} | {} |",
                    rs.len(),
                    recon_stats(&rs, |r| (r.fresh_found_m, r.fresh_real_m), Recovered::fresh)
                );
            }
        }
    }
    // 12. Empreinte
    let _ = writeln!(s, "\n## 12. Empreinte de résultat\n\n`{}`", r.fingerprint);
    s
}
