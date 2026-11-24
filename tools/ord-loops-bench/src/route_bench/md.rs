//! Le Markdown du banc d'itinéraire : agrégats seulement (lettres d'équipes et comptes, aucune position ni id).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;

use super::agg::Stats;
use super::{Group, Tuned, CASES, CURSOR_U, LADDER, SWEEP};

/// Les comptes de la population.
#[derive(Debug, Serialize)]
pub struct Population {
    pub rows_read: usize,
    pub posed: usize,
    pub anomaly: usize,
    pub kept: usize,
    pub tracks_with_seg: usize,
    pub segs: usize,
    /// Tronçons routés dans la seconde zone (sans chemin dans la première).
    pub zone1: usize,
    /// Échecs communs à toutes les variantes, par raison.
    pub common_fail: BTreeMap<&'static str, usize>,
    pub by_ride_band_half: BTreeMap<String, usize>,
    /// Part des kilomètres de référence sur des arêtes hors de la vue du type.
    pub ref_off_view_share: f64,
    /// Lignes routées dans la fenêtre élargie (échappement).
    pub enlarged: usize,
    pub lines: usize,
}

fn fr(x: f64, d: usize) -> String {
    if x.is_nan() {
        "—".into()
    } else {
        format!("{x:.d$}").replace('.', ",")
    }
}

fn pct(x: f64) -> String {
    if x.is_nan() {
        "—".into()
    } else {
        format!("{} %", fr(100.0 * x, 1))
    }
}

/// Un Jaccard en points (× 100), avec son intervalle.
fn pts_ci(x: f64, ci: [f64; 2]) -> String {
    format!("{} [{} ; {}]", fr(100.0 * x, 1), fr(100.0 * ci[0], 1), fr(100.0 * ci[1], 1))
}

fn signed(x: f64, ci: [f64; 2]) -> String {
    let s = |v: f64| if v.is_nan() { "—".into() } else { format!("{:+.1}", 100.0 * v).replace('.', ",") };
    format!("{} [{} ; {}]", s(x), s(ci[0]), s(ci[1]))
}

fn mult(q: u16) -> String {
    fr(f64::from(q) / 256.0, 2)
}

pub fn markdown(meta: &[(String, String)], pop: &Population, tuned: &[Tuned], groups: &[Group]) -> String {
    let mut idx: BTreeMap<(&str, &str, char, &str), &Stats> = BTreeMap::new();
    for g in groups {
        idx.insert((g.key.ride, g.key.band, g.key.half, g.key.variant.as_str()), &g.stats);
    }
    let get = |r: &str, b: &str, h: char, v: &str| idx.get(&(r, b, h, v)).copied();
    let mut o = String::new();
    let _ = writeln!(o, "# Banc d'itinéraire sur tronçons réels (lot 1 bis)\n");
    let _ = writeln!(o, "Agrégats seulement. Jaccard en points (× 100), échecs comptés à 0 ; IC à 95 % par bootstrap des traces.\n");
    for (k, v) in meta {
        let _ = writeln!(o, "- {k} : {v}");
    }
    let _ = writeln!(o, "\n## Population\n");
    let _ = writeln!(
        o,
        "- lignes lues {}, traces posées {}, écartées pour anomalie d'ordre {}, gardées {}, avec au moins un tronçon {}",
        pop.rows_read, pop.posed, pop.anomaly, pop.kept, pop.tracks_with_seg
    );
    let _ = writeln!(
        o,
        "- tronçons {} ; seconde zone {} ; échecs communs {:?} ; lignes {} dont {} en fenêtre élargie",
        pop.segs, pop.zone1, pop.common_fail, pop.lines, pop.enlarged
    );
    let _ =
        writeln!(o, "- part des km de référence hors de la vue du type : {}", pct(pop.ref_off_view_share));
    let _ = writeln!(o, "\n| type/bande/moitié | tronçons |\n|---|---|");
    for (k, n) in &pop.by_ride_band_half {
        let _ = writeln!(o, "| {k} | {n} |");
    }
    let rides: Vec<&str> = {
        let mut v: Vec<&str> = groups.iter().map(|g| g.key.ride).collect();
        v.dedup();
        v
    };
    let bands = ["all", "2-5km", "5-15km", "15-40km"];
    for r in rides {
        let _ = writeln!(o, "\n## Type {r}\n");
        let _ = writeln!(o, "### Variantes principales\n");
        let _ = writeln!(o, "| bande | moitié | variante | n (traces) | échecs | J moyen [IC] | J médian [IC] | ΔJ / neutre [IC] | précision | rappel | détour P50 / P90 | ms P50 / P90 |");
        let _ = writeln!(o, "|---|---|---|---|---|---|---|---|---|---|---|---|");
        for b in bands {
            for h in ['A', 'B'] {
                for v in super::MAIN {
                    let Some(s) = get(r, b, h, v) else { continue };
                    let _ = writeln!(
                        o,
                        "| {b} | {h} | {v} | {} ({}) | {} | {} | {} | {} | {} | {} | {} / {} | {} / {} |",
                        s.n,
                        s.clusters,
                        pct(s.fail_rate),
                        pts_ci(s.jac_mean, s.jac_mean_ci),
                        s.jac_median_ci.map_or(fr(100.0 * s.jac_median, 1), |ci| pts_ci(s.jac_median, ci)),
                        if v == "neutral" { "—".into() } else { signed(s.d_neutral, s.d_neutral_ci) },
                        pct(s.precision_mean),
                        pct(s.recall_mean),
                        fr(s.detour_p50, 2),
                        fr(s.detour_p90, 2),
                        fr(s.ms_p50, 0),
                        fr(s.ms_p90, 0)
                    );
                }
            }
        }
        let _ = writeln!(o, "\n### Part des kilomètres par case, réel contre itinéraire (toutes bandes)\n");
        for h in ['A', 'B'] {
            let cols: Vec<(&str, Option<&Stats>)> =
                ["neutral", "avoid", "best1", "best2"].iter().map(|v| (*v, get(r, "all", h, v))).collect();
            let Some(base) = cols[0].1 else { continue };
            let _ = writeln!(
                o,
                "Moitié {h} ({} km réels sur les tronçons trouvés par le neutre) :\n",
                fr(base.ref_km, 0)
            );
            let _ =
                writeln!(o, "| case | réel | neutral | avoid | best1 | best2 |\n|---|---|---|---|---|---|");
            for (k, name) in CASES.iter().enumerate() {
                let _ = write!(o, "| {k} {name} | {} |", pct(base.ref_share[k]));
                for (_, s) in &cols {
                    let _ = write!(o, " {} |", s.map_or("—".into(), |s| pct(s.route_share[k])));
                }
                let _ = writeln!(o);
            }
            let _ = write!(o, "| non revêtu | {} |", pct(base.ref_offroad_share));
            for (_, s) in &cols {
                let _ = write!(o, " {} |", s.map_or("—".into(), |s| pct(s.route_offroad_share)));
            }
            let _ = writeln!(o, "\n");
        }
        let _ = writeln!(o, "### Réel contre neutre, par bande\n");
        let _ = writeln!(
            o,
            "| moitié | case | {} |",
            bands.iter().map(|b| format!("{b} réel / neutre")).collect::<Vec<_>>().join(" | ")
        );
        let _ = writeln!(o, "|---|---|{}", "---|".repeat(bands.len()));
        for h in ['A', 'B'] {
            for (k, name) in CASES.iter().enumerate() {
                let _ = write!(o, "| {h} | {k} {name} |");
                for b in bands {
                    let c = get(r, b, h, "neutral").map_or("—".into(), |s| {
                        format!("{} / {}", pct(s.ref_share[k]), pct(s.route_share[k]))
                    });
                    let _ = write!(o, " {c} |");
                }
                let _ = writeln!(o);
            }
        }
        let _ = writeln!(o, "\n### Balayage une case à la fois : ΔJ / neutre en points (toutes bandes)\n");
        for h in ['A', 'B'] {
            let _ = writeln!(
                o,
                "Moitié {h} :\n\n| case | {} |\n|---|{}",
                SWEEP.iter().map(|&q| format!("× {}", mult(q))).collect::<Vec<_>>().join(" | "),
                "---|".repeat(SWEEP.len())
            );
            for (k, name) in CASES.iter().enumerate() {
                let _ = write!(o, "| {k} {name} |");
                for &q in &SWEEP {
                    let c = get(r, "all", h, &format!("sweep:c{k}:{q}"))
                        .map_or("—".into(), |s| signed(s.d_neutral, s.d_neutral_ci));
                    let _ = write!(o, " {c} |");
                }
                let _ = writeln!(o);
            }
            let _ = writeln!(o);
        }
        let _ = writeln!(o, "### Crans du curseur et `avoid` (toutes bandes)\n");
        let _ = writeln!(o, "| moitié | variante | J moyen | ΔJ / neutre [IC] | grande route | secondary | tertiary | petite route + rue | cyclable ou verte | chemin | non revêtu | détour P50 |");
        let _ = writeln!(o, "|---|---|---|---|---|---|---|---|---|---|---|---|");
        for h in ['A', 'B'] {
            let mut vs: Vec<String> = vec!["real".into(), "neutral".into(), "avoid".into()];
            for d in ["0.25", "1.00"] {
                for u in CURSOR_U {
                    vs.push(format!("cursor:u{u:.2}:d{d}"));
                }
            }
            for v in vs {
                let (name, s, real) = if v == "real" {
                    ("réel".to_string(), get(r, "all", h, "neutral"), true)
                } else {
                    (v.clone(), get(r, "all", h, &v), false)
                };
                let Some(s) = s else { continue };
                let sh = if real { s.ref_share } else { s.route_share };
                let off = if real { s.ref_offroad_share } else { s.route_offroad_share };
                let _ = writeln!(
                    o,
                    "| {h} | {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                    if real { "—".into() } else { fr(100.0 * s.jac_mean, 1) },
                    if real || v == "neutral" { "—".into() } else { signed(s.d_neutral, s.d_neutral_ci) },
                    pct(sh[0]),
                    pct(sh[6]),
                    pct(sh[1]),
                    pct(sh[2] + sh[3]),
                    pct(sh[4]),
                    pct(sh[5]),
                    pct(off),
                    if real { "—".into() } else { fr(s.detour_p50, 2) }
                );
            }
        }
        if let Some(t) = tuned.iter().find(|t| t.ride == r) {
            let _ = writeln!(o, "\n### `best1` et `best2` (choisis sur A)\n");
            let _ = writeln!(
                o,
                "| case | best1 | best2 | J sur A du balayage ({}) | J sur A des voisins de best1 |",
                LADDER.iter().map(|&q| format!("×{}", mult(q))).collect::<Vec<_>>().join(" ")
            );
            let _ = writeln!(o, "|---|---|---|---|---|");
            for (k, name) in CASES.iter().enumerate() {
                let row = |v: &[Vec<Option<f64>>]| {
                    v.get(k).map_or(String::new(), |r| {
                        r.iter()
                            .map(|x| x.map_or("·".into(), |x| fr(100.0 * x, 2)))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                };
                let _ = writeln!(
                    o,
                    "| {k} {name} | × {} | × {} | {} | {} |",
                    mult(t.best1[k]),
                    mult(t.best2[k]),
                    row(&t.sweep_a),
                    row(&t.desc_a)
                );
            }
            let _ = writeln!(o, "\nJ de best1 sur A : {}", fr(100.0 * t.best1_score_a, 2));
        }
    }
    o
}
