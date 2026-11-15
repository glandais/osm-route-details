//! Rapport : statistiques par équipe et par mode, parité du §8.4, gain du §8.5. Tout est calculé sur les
//! enregistrements ; rien n'est écrit à la main.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;

use crate::record::Rec;

pub fn percentile(sorted: &[f64], q: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
    Some(sorted[idx.min(sorted.len() - 1)])
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(f64::total_cmp);
    v
}

fn fmt_opt(v: Option<f64>, digits: usize) -> String {
    v.map_or_else(|| "-".to_string(), |x| format!("{x:.digits$}"))
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Counts {
    pub requests: usize,
    pub success: usize,
    pub interior: usize,
    pub interior_success: usize,
    pub edge: usize,
    pub edge_success: usize,
    pub java_success: usize,
    pub java_interior_success: usize,
    pub java_edge_success: usize,
    /// Réussites de Rust sur les requêtes que Java a ratées.
    pub gain_over_java: usize,
    pub java_failed: usize,
    pub panics: usize,
}

pub fn counts<'a>(recs: impl IntoIterator<Item = &'a Rec>) -> Counts {
    let mut c = Counts::default();
    for r in recs {
        c.requests += 1;
        let ok = r.success();
        c.success += usize::from(ok);
        if r.scope == "interior" {
            c.interior += 1;
            c.interior_success += usize::from(ok);
            c.java_interior_success += usize::from(r.java.ok);
        } else {
            c.edge += 1;
            c.edge_success += usize::from(ok);
            c.java_edge_success += usize::from(r.java.ok);
        }
        c.java_success += usize::from(r.java.ok);
        if !r.java.ok {
            c.java_failed += 1;
            c.gain_over_java += usize::from(ok);
        }
        c.panics += usize::from(r.outcome == "PANIC");
    }
    c
}

fn stats_line(v: Vec<f64>) -> String {
    let s = sorted(v);
    format!(
        "{} / {} / {}",
        fmt_opt(percentile(&s, 0.5), 0),
        fmt_opt(percentile(&s, 0.9), 0),
        fmt_opt(s.last().copied(), 0)
    )
}

pub struct Meta {
    pub lines: Vec<(String, String)>,
}

pub struct Autocheck {
    pub team: String,
    pub starts: usize,
    pub oneway_ok: usize,
    pub oneway_bad: usize,
}

pub struct SummaryInput<'a> {
    pub meta: &'a Meta,
    pub recs: &'a [Rec],
    /// Lettre → nombre de requêtes Java, dont hors région.
    pub java_total: &'a BTreeMap<String, (usize, usize)>,
    pub autocheck: &'a [Autocheck],
    pub modes: &'a [String],
    pub rounds: usize,
}

pub fn summary(inp: &SummaryInput<'_>) -> String {
    let mut s = String::new();
    let recs = inp.recs;
    let teams: Vec<String> = {
        let mut t: Vec<String> = recs.iter().map(|r| r.team.clone()).collect();
        t.sort();
        t.dedup();
        t
    };
    let sel = |team: Option<&str>, mode: &str| -> Vec<&Rec> {
        recs.iter().filter(|r| r.mode == mode && team.is_none_or(|t| r.team == t)).collect()
    };
    let _ = writeln!(s, "# Banc du moteur `ord-loops` — résumé\n");
    for (k, v) in &inp.meta.lines {
        let _ = writeln!(s, "- {k} : {v}");
    }
    let _ = writeln!(s, "- tours chauds par requête : {} (+ un tour froid)\n", inp.rounds);

    let _ = writeln!(s, "## 1. Périmètre et autocontrôle\n");
    let _ = writeln!(s, "| Équipe | requêtes Java | rejouées (périmètre) | hors région | départs | `oneway` : `km` = ⌈1,4 × vol d'oiseau⌉ (ok / écart) |");
    let _ = writeln!(s, "|---|---|---|---|---|---|");
    for t in &teams {
        let (all, inside) = inp.java_total.get(t).copied().unwrap_or((0, 0));
        let a = inp.autocheck.iter().find(|a| &a.team == t);
        let _ = writeln!(
            s,
            "| {t} | {all} | {inside} | {} | {} | {} / {} |",
            all - inside,
            a.map_or(0, |a| a.starts),
            a.map_or(0, |a| a.oneway_ok),
            a.map_or(0, |a| a.oneway_bad)
        );
    }

    for mode in inp.modes {
        let _ = writeln!(s, "\n## Mode `{mode}`\n");
        let _ = writeln!(s, "| Équipe | requêtes | réussites Rust | dont intérieures | de bord | réussites Java | dont intérieures | de bord | Rust sur les ratées Java | refus du contrôleur (parcours) | paniques |");
        let _ = writeln!(s, "|---|---|---|---|---|---|---|---|---|---|---|");
        let mut all = Counts::default();
        for t in &teams {
            let c = counts(sel(Some(t), mode));
            let _ = writeln!(
                s,
                "| {t} | {} | {} | {}/{} | {}/{} | {} | {}/{} | {}/{} | {}/{} | {} | {} |",
                c.requests,
                c.success,
                c.interior_success,
                c.interior,
                c.edge_success,
                c.edge,
                c.java_success,
                c.java_interior_success,
                c.interior,
                c.java_edge_success,
                c.edge,
                c.gain_over_java,
                c.java_failed,
                violating_routes(&sel(Some(t), mode)),
                c.panics
            );
            all.requests += c.requests;
            all.success += c.success;
            all.interior += c.interior;
            all.interior_success += c.interior_success;
            all.edge += c.edge;
            all.edge_success += c.edge_success;
            all.java_success += c.java_success;
            all.java_interior_success += c.java_interior_success;
            all.java_edge_success += c.java_edge_success;
            all.gain_over_java += c.gain_over_java;
            all.java_failed += c.java_failed;
            all.panics += c.panics;
        }
        let _ = writeln!(
            s,
            "| **total** | {} | {} | {}/{} | {}/{} | {} | {}/{} | {}/{} | {}/{} | {} | {} |",
            all.requests,
            all.success,
            all.interior_success,
            all.interior,
            all.edge_success,
            all.edge,
            all.java_success,
            all.java_interior_success,
            all.interior,
            all.java_edge_success,
            all.edge,
            all.gain_over_java,
            all.java_failed,
            violating_routes(&sel(None, mode)),
            all.panics
        );

        // temps
        let rs = sel(None, mode);
        let ms = |f: &dyn Fn(&Rec) -> bool| stats_line(rs.iter().filter(|r| f(r)).map(|r| r.ms).collect());
        let _ =
            writeln!(s, "\nTemps de génération, ms (médiane / p90 / maximum), un fil, `Engine` chargé :\n");
        let _ = writeln!(s, "| Sélection | n | ms |");
        let _ = writeln!(s, "|---|---|---|");
        let n = |f: &dyn Fn(&Rec) -> bool| rs.iter().filter(|r| f(r)).count();
        let le100 = |r: &Rec| r.km <= 100.0;
        type Sel<'a> = (&'a str, &'a dyn Fn(&Rec) -> bool);
        let rows: [Sel<'_>; 5] = [
            ("toutes", &|_| true),
            ("D ≤ 100 km", &le100),
            ("D ≤ 100 km, intérieures", &|r| le100(r) && r.scope == "interior"),
            ("D ≤ 100 km, réussies", &|r| le100(r) && r.success()),
            ("D > 100 km", &|r| !le100(r)),
        ];
        for (name, f) in rows {
            let _ = writeln!(s, "| {name} | {} | {} |", n(f), ms(f));
        }
        let cold = stats_line(rs.iter().filter(|r| r.km <= 100.0).map(|r| r.cold_ms).collect());
        let _ = writeln!(s, "| D ≤ 100 km, tour froid | {} | {cold} |", n(&le100));

        // issues
        let mut codes: BTreeMap<String, usize> = BTreeMap::new();
        for r in &rs {
            *codes.entry(r.outcome.clone()).or_default() += 1;
        }
        let _ = writeln!(s, "\nIssues : {}", join_counts(&codes));
        let mut viol: BTreeMap<String, usize> = BTreeMap::new();
        for r in &rs {
            for (k, v) in &r.violations {
                *viol.entry(k.clone()).or_default() += *v as usize;
            }
        }
        let _ = writeln!(
            s,
            "\nViolations du contrôleur indépendant, par code : {}",
            if viol.is_empty() { "aucune".into() } else { join_counts(&viol) }
        );
        let dis: u32 = rs.iter().map(|r| r.disagreements).sum();
        let _ = writeln!(s, "\nParcours refusés par le contrôleur alors que le contrôle interne du moteur les avait acceptés : {dis}");

        concordance(&mut s, &rs);

        // qualité des parcours acceptés
        let ok_routes: Vec<&crate::record::RouteRec> =
            rs.iter().flat_map(|r| r.routes.iter()).filter(|r| r.violations.is_empty()).collect();
        let team_share = sorted(ok_routes.iter().map(|r| r.team_share).collect());
        let bridged = sorted(ok_routes.iter().map(|r| r.bridged_m).collect());
        let over = sorted(ok_routes.iter().map(|r| r.over_target).collect());
        let _ = writeln!(
            s,
            "\nParcours acceptés : {} ; part sur les arêtes d'équipe, médiane {} / p10 {} ; comblé (m) médiane {} / p90 {} ; L / D p10 {} / médiane {} / p90 {}",
            ok_routes.len(),
            fmt_opt(percentile(&team_share, 0.5), 3),
            fmt_opt(percentile(&team_share, 0.1), 3),
            fmt_opt(percentile(&bridged, 0.5), 0),
            fmt_opt(percentile(&bridged, 0.9), 0),
            fmt_opt(percentile(&over, 0.1), 3),
            fmt_opt(percentile(&over, 0.5), 3),
            fmt_opt(percentile(&over, 0.9), 3),
        );
    }

    if inp.modes.iter().any(|m| m == "strict") {
        strict_parity(&mut s, recs, &teams);
    }
    gain(&mut s, recs, &teams, inp.modes);
    s
}

/// Les grandeurs que le moteur et le contrôleur mesurent chacun de leur côté : où ils divergent, l'un des deux a tort.
fn concordance(s: &mut String, rs: &[&Rec]) {
    let routes: Vec<(&Rec, &crate::record::RouteRec)> =
        rs.iter().flat_map(|r| r.routes.iter().map(move |x| (*r, x))).collect();
    let len = routes.iter().filter(|(r, x)| (x.over_target - x.length_km / r.km).abs() > 0.005).count();
    let cross = routes.iter().filter(|(_, x)| x.crossings != x.checker_crossings).count();
    let shared = routes.iter().filter(|(_, x)| (x.shared_m - x.checker_shared_m).abs() > 5.0).count();
    let team = routes
        .iter()
        .filter(|(r, x)| (x.engine_team_m - x.checker_team_m).abs() > 0.01 * r.km * 1000.0)
        .count();
    let _ = writeln!(
        s,
        "\nConcordance moteur / contrôleur sur {} parcours rendus : longueur à plus de 0,5 % de D près : {len} écarts ; croisements différents : {cross} ; longueur recouverte à plus de 5 m près : {shared} écarts ; part d'équipe à plus de 1 % de D près : {team} écarts",
        routes.len()
    );
}

fn violating_routes(rs: &[&Rec]) -> usize {
    rs.iter().map(|r| r.routes.iter().filter(|x| !x.violations.is_empty()).count()).sum()
}

fn join_counts(m: &BTreeMap<String, usize>) -> String {
    let mut v: Vec<_> = m.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    v.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ")
}

/// Parité du §8.4 en mode strict, chaque critère avec la valeur mesurée et son verdict.
fn strict_parity(s: &mut String, recs: &[Rec], teams: &[String]) {
    let strict: Vec<&Rec> = recs.iter().filter(|r| r.mode == "strict").collect();
    let _ = writeln!(s, "\n## Parité (§8.4), mode strict\n");
    let mut all_ge = true;
    let _ = writeln!(
        s,
        "**(1) Réussites Rust ≥ réussites Java `head`, par équipe et au total, et sur les intérieures**\n"
    );
    let _ = writeln!(s, "| Équipe | Rust | Java | verdict | Rust intérieures | Java intérieures | verdict |");
    let _ = writeln!(s, "|---|---|---|---|---|---|---|");
    for t in teams {
        let c = counts(strict.iter().copied().filter(|r| &r.team == t));
        let ok = c.success >= c.java_success;
        all_ge &= ok;
        let _ = writeln!(
            s,
            "| {t} | {} | {} | {} | {} | {} | {} |",
            c.success,
            c.java_success,
            verdict(ok),
            c.interior_success,
            c.java_interior_success,
            verdict(c.interior_success >= c.java_interior_success)
        );
    }
    let c = counts(strict.iter().copied());
    let total_ok = c.success >= c.java_success;
    let int_ok = c.interior_success >= c.java_interior_success;
    let _ = writeln!(
        s,
        "| **total** | {} | {} | {} | {} | {} | {} |",
        c.success,
        c.java_success,
        verdict(total_ok),
        c.interior_success,
        c.java_interior_success,
        verdict(int_ok)
    );
    let _ = writeln!(s, "\nCritère (1) : **{}**\n", verdict(all_ge && total_ok && int_ok));

    // head-alone
    let alone: Vec<&&Rec> = strict.iter().filter(|r| r.java.alone_outcome.is_some()).collect();
    if !alone.is_empty() {
        let java = alone.iter().filter(|r| r.java.alone_outcome.as_deref() == Some("OK")).count();
        let rust = alone.iter().filter(|r| r.success()).count();
        let _ = writeln!(
            s,
            "Écart à `head-alone` (plus exigeant), sur les {} requêtes qu'il a jouées dans le périmètre : Java {java}, Rust {rust}.\n",
            alone.len()
        );
    }

    let violations: usize =
        strict.iter().map(|r| r.violations.values().map(|v| *v as usize).sum::<usize>()).sum();
    let _ = writeln!(s, "**(2) Zéro violation du contrôleur indépendant** : {violations} violation(s) sur {} parcours rendus : **{}**\n", strict.iter().map(|r| r.n_routes).sum::<usize>(), verdict(violations == 0));

    // (3) Le temps se juge sur les générations qui rendent un parcours : un échec rapide (une milliseconde) ne dit rien
    // de la vitesse à laquelle un parcours est produit. Les deux populations sont données, pour chaque mode.
    let time_line = |mode: &str| -> (String, bool) {
        let rs: Vec<&Rec> =
            recs.iter().filter(|r| r.mode == mode && r.km <= 100.0 && r.scope == "interior").collect();
        let all = sorted(rs.iter().map(|r| r.ms).collect());
        let ok = sorted(rs.iter().filter(|r| r.success()).map(|r| r.ms).collect());
        let line = format!(
            "{mode} : réussies ({}) médiane {} / p90 {} / max {} ms ; toutes les requêtes ({}) médiane {} / p90 {} / max {} ms",
            ok.len(),
            fmt_opt(percentile(&ok, 0.5), 0),
            fmt_opt(percentile(&ok, 0.9), 0),
            fmt_opt(ok.last().copied(), 0),
            all.len(),
            fmt_opt(percentile(&all, 0.5), 0),
            fmt_opt(percentile(&all, 0.9), 0),
            fmt_opt(all.last().copied(), 0),
        );
        (line, percentile(&ok, 0.5).is_none_or(|m| m < 500.0))
    };
    let (strict_line, strict_ok) = time_line("strict");
    let _ = writeln!(
        s,
        "**(3) Médiane de génération < 500 ms pour D ≤ 100 km (intérieures), jugée sur les générations réussies** : {strict_line} : **{}**\n",
        verdict(strict_ok)
    );
    for mode in ["team", "discovery"] {
        if recs.iter().any(|r| r.mode == mode) {
            let _ = writeln!(s, "  - {}", time_line(mode).0);
        }
    }
    let _ = writeln!(s);

    let _ = writeln!(s, "**Requêtes intérieures réussies par Java et pas par Rust**, classées d'après les diagnostics du moteur (réseau atteint, chemin de retour, refus du contrôle ; un GeoJSON par requête : `--geojson <lettre>:<i>`) :\n");
    let mut lost = 0;
    let mut by_class: BTreeMap<&'static str, usize> = BTreeMap::new();
    for r in strict.iter().filter(|r| r.scope == "interior" && r.java.ok && !r.success()) {
        lost += 1;
        let class = lost_class(r);
        *by_class.entry(class).or_default() += 1;
        let _ = writeln!(
            s,
            "- {} #{} {} {} km {} : {} (réseau atteint {:.0} km, qui ramène au départ {:.0} km, refus {:?}) : {class}",
            r.team,
            r.i,
            r.kind,
            r.km,
            r.ride,
            r.outcome,
            r.reached_m / 1000.0,
            r.way_back_m / 1000.0,
            r.rejected
        );
    }
    if lost == 0 {
        let _ = writeln!(s, "- aucune");
    } else {
        let _ = writeln!(
            s,
            "\nPar classe : {}",
            join_counts(&by_class.iter().map(|(k, v)| ((*k).to_string(), *v)).collect())
        );
    }
    let lost_edge = strict.iter().filter(|r| r.scope == "edge" && r.java.ok && !r.success()).count();
    let _ = writeln!(s, "\nDe bord, réussies par Java et pas par Rust (classe `bord`) : {lost_edge}");
}

/// Classe d'une requête perdue (§8.4), d'après ce que le moteur a mesuré.
fn lost_class(r: &Rec) -> &'static str {
    let sharp_only = !r.rejected.is_empty() && r.rejected.keys().all(|k| k == "SharpTurn");
    match r.outcome.as_str() {
        "NETWORK_TOO_SMALL" if r.reached_m < 1000.0 => {
            "poche : le départ ne mène qu'à un bout de réseau (trou ou sens unique non comblé)"
        }
        "NETWORK_TOO_SMALL" | "NO_ROUTE:NO_WAY_BACK" => {
            "poche orientée : le réseau atteint ne ramène pas au départ (trou ou sens unique non comblé)"
        }
        "NO_ROUTE:END_UNREACHABLE" => "arrivée hors du réseau strict atteint depuis le départ",
        "NO_ROUTE:NO_CANDIDATE" if sharp_only => {
            "virage de 150° ou plus que Java exempte (nœud roulé par l'équipe)"
        }
        "NO_ROUTE:NO_CANDIDATE" => "recherche : aucun retour disjoint dans la tolérance",
        _ => "autre",
    }
}

fn verdict(ok: bool) -> &'static str {
    if ok {
        "tenu"
    } else {
        "non tenu"
    }
}

/// Gain du §8.5 : réussite par mode et par équipe, surtout là où Java ne rendait rien.
fn gain(s: &mut String, recs: &[Rec], teams: &[String], modes: &[String]) {
    let _ = writeln!(s, "\n## Gain (§8.5)\n");
    let _ = writeln!(s, "Réussites par équipe sur les requêtes que Java a **ratées** (n = requêtes ratées par Java dans le périmètre) :\n");
    let mut head = String::from("| Équipe | n |");
    let mut sep = String::from("|---|---|");
    for m in modes {
        let _ = write!(head, " {m} |");
        sep.push_str("---|");
    }
    let _ = writeln!(s, "{head}\n{sep}");
    for t in teams {
        let n = recs.iter().filter(|r| &r.team == t && r.mode == modes[0] && !r.java.ok).count();
        let mut row = format!("| {t} | {n} |");
        for m in modes {
            let ok =
                recs.iter().filter(|r| &r.team == t && &r.mode == m && !r.java.ok && r.success()).count();
            let _ = write!(row, " {ok} |");
        }
        let _ = writeln!(s, "{row}");
    }
}
