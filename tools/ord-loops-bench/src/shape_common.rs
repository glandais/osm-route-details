//! Export de la répartition générale des figures du moteur (lot 4, 4c, §15.11.1 de `ord-loops-lot4.md`) : lit les
//! boucles réelles retenues d'un rapport `base` (le réel ne dépend pas du moteur) et écrit
//! `crates/ord-loops/src/shape_common.rs`, **généré, à ne pas modifier à la main**.
//!
//! Aucune donnée privée n'en sort : par type, trois comptes (boucles à k ≤ 1, k = 2, k ≥ 3 sommets à 10 % de D) et cinq
//! quantiles du r / D des triangles, toutes équipes réunies.

use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{bail, ensure, Context as _, Result};
use clap::Args;
use serde::Deserialize;

/// Les types, dans l'ordre de `RideType::index()`.
const RIDES: [&str; 3] = ["ROAD", "GRAVEL", "MTB"];

#[derive(Debug, Args)]
pub struct ShapeCommonArgs {
    /// Rapport `base` de la campagne (`<label>-report.json`) dont on lit les boucles réelles retenues.
    #[arg(long)]
    pub report: PathBuf,
    /// Fichier Rust écrit.
    #[arg(long)]
    pub out: PathBuf,
}

#[derive(Deserialize)]
struct Real {
    r_over_d: f64,
    vertices_10: u32,
}

#[derive(Deserialize)]
struct Loop {
    ty: String,
    real: Real,
}

#[derive(Deserialize)]
struct Report {
    meta: Vec<(String, String)>,
    loops: Vec<Loop>,
}

/// Ce que le jeu doit contenir pour que l'export soit celui du 4c : les boucles retenues de l'export de production du
/// 17 novembre 2026 et les parts de sommets du relevé du 4a (§3.3), au dixième de point.
#[derive(Clone, Debug, PartialEq)]
pub struct Expected {
    /// Boucles retenues par type.
    pub loops: [u32; 3],
    /// Parts en pour mille de `[k ≤ 1, k = 2, k ≥ 3]` ; le MTB n'est pas contrôlé.
    pub shares_milli: [Option<[u32; 3]>; 3],
}

impl Expected {
    pub fn production() -> Self {
        Self { loops: [2071, 780, 4], shares_milli: [Some([405, 530, 65]), Some([554, 429, 17]), None] }
    }
}

/// Ce que l'export calcule.
#[derive(Debug, PartialEq)]
pub struct Shapes {
    /// `[k ≤ 1, k = 2, k ≥ 3]` par type.
    pub counts: [[u32; 3]; 3],
    pub tri_q8: [u16; 3],
    pub rd_tri_milli: [[u16; 5]; 3],
}

/// Quantile de type 7 (interpolation linéaire) d'un échantillon trié.
fn quantile7(sorted: &[f64], p: f64) -> f64 {
    let h = (sorted.len() - 1) as f64 * p;
    let (lo, hi) = (h.floor() as usize, h.ceil() as usize);
    sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo])
}

/// Calcule les constantes ; refuse si les comptes ou les parts ne sont pas `expected`.
pub fn compute(loops: &[(usize, u32, f64)], expected: &Expected) -> Result<Shapes> {
    let mut counts = [[0u32; 3]; 3];
    let mut rd: [Vec<f64>; 3] = Default::default();
    for &(ty, k, r_over_d) in loops {
        counts[ty][(k.max(1) as usize - 1).min(2)] += 1;
        if k == 2 {
            rd[ty].push(r_over_d);
        }
    }
    for (ty, ride) in RIDES.iter().enumerate() {
        let n: u32 = counts[ty].iter().sum();
        ensure!(
            n == expected.loops[ty],
            "{ride} : {n} boucles retenues, {} attendues (ce n'est pas le jeu du 4c)",
            expected.loops[ty]
        );
        if let Some(sh) = expected.shares_milli[ty] {
            for (j, &want) in sh.iter().enumerate() {
                let got = (f64::from(counts[ty][j]) / f64::from(n) * 1000.0).round() as u32;
                ensure!(
                    got == want,
                    "{ride} : part de la classe {j} de sommets à {got} pour mille, {want} attendus"
                );
            }
        }
    }
    let mut tri_q8 = [0u16; 3];
    let mut rd_tri_milli = [[0u16; 5]; 3];
    for ty in 0..3 {
        // Le MTB (4 boucles) n'a que des lentilles : aucune part de triangles, aucune loi.
        if ty == 2 {
            continue;
        }
        let (n1, n2) = (f64::from(counts[ty][0]), f64::from(counts[ty][1]));
        tri_q8[ty] = (256.0 * n2 / (n1 + n2)).round() as u16;
        let mut v = std::mem::take(&mut rd[ty]);
        ensure!(v.len() >= 20, "{} : {} triangles seulement pour la loi du r / D", RIDES[ty], v.len());
        v.sort_by(f64::total_cmp);
        for (j, p) in [0.10, 0.25, 0.50, 0.75, 0.90].into_iter().enumerate() {
            rd_tri_milli[ty][j] = (quantile7(&v, p) * 1000.0).round() as u16;
        }
    }
    Ok(Shapes { counts, tri_q8, rd_tri_milli })
}

fn render(s: &Shapes, provenance: &str) -> String {
    let mut o = String::new();
    o.push_str("//! Répartition générale des figures par type (lot 4, 4c, §15.3 de `docs/design/technique/ord-loops-lot4.md`) :\n");
    o.push_str("//! **généré** par `ord-loops-bench shape-common`, ne pas modifier à la main. Comptes des boucles réelles retenues\n");
    o.push_str("//! par nombre de sommets à 10 % de D, part de triangles et quantiles du r / D des triangles, toutes équipes\n");
    o.push_str("//! réunies : des agrégats, aucune donnée d'équipe.\n//!\n");
    for l in provenance.lines() {
        let _ = writeln!(o, "//! {l}");
    }
    o.push_str("\nuse crate::{FigureMix, RideType};\n\n");
    o.push_str("/// Boucles réelles retenues par type (`RideType::index()`), `[k ≤ 1, k = 2, k ≥ 3]` sommets à 10 % de D.\n");
    o.push_str("#[allow(dead_code)] // la provenance des deux constantes suivantes ; un test la relit\npub(crate) const SHAPE_COUNTS: [[u32; 3]; 3] = [\n");
    for c in &s.counts {
        let _ = writeln!(o, "    [{}, {}, {}],", c[0], c[1], c[2]);
    }
    o.push_str("];\n\n");
    o.push_str("/// Part des triangles `round(256 · n₂ / (n₁ + n₂))` (la part du quadrilatère est ôtée, parts renormalisées) ; le MTB, 4\n/// boucles, est forcé à 0.\n");
    let q: Vec<String> = s.tri_q8.iter().map(u16::to_string).collect();
    let _ = writeln!(o, "pub(crate) const SHAPE_TRI_Q8: [u16; 3] = [{}];\n", q.join(", "));
    o.push_str(
        "/// P10, P25, P50, P75, P90 (type 7) du r / D des boucles réelles à deux sommets, en millièmes.\n",
    );
    o.push_str("pub(crate) const SHAPE_RD_TRI_MILLI: [[u16; 5]; 3] = [\n");
    for r in &s.rd_tri_milli {
        let v: Vec<String> = r.iter().map(u16::to_string).collect();
        let _ = writeln!(o, "    [{}],", v.join(", "));
    }
    o.push_str("];\n\n");
    o.push_str(
        "/// La répartition générale d'un type.\npub(crate) fn general(ride: RideType) -> FigureMix {\n",
    );
    o.push_str("    let i = ride.index();\n    FigureMix { tri_q8: SHAPE_TRI_Q8[i], rd_tri_milli: SHAPE_RD_TRI_MILLI[i] }\n}\n");
    o
}

pub fn run(a: &ShapeCommonArgs) -> Result<()> {
    let f = std::fs::File::open(&a.report).with_context(|| a.report.display().to_string())?;
    let r: Report = serde_json::from_reader(std::io::BufReader::with_capacity(1 << 20, f))
        .with_context(|| format!("rapport illisible : {}", a.report.display()))?;
    let mut loops = Vec::with_capacity(r.loops.len());
    for l in &r.loops {
        let Some(ty) = RIDES.iter().position(|x| *x == l.ty) else { bail!("type inconnu : {}", l.ty) };
        loops.push((ty, l.real.vertices_10, l.real.r_over_d));
    }
    let shapes = compute(&loops, &Expected::production())?;
    let meta = |k: &str| r.meta.iter().find(|(n, _)| n == k).map_or("?", |(_, v)| v.as_str());
    let provenance = format!(
        "Provenance : rapport `base` du banc de vraisemblance du {} (commit {}), boucles retenues ROAD {}, GRAVEL {}, MTB {}.",
        meta("date"),
        meta("commit"),
        shapes.counts[0].iter().sum::<u32>(),
        shapes.counts[1].iter().sum::<u32>(),
        shapes.counts[2].iter().sum::<u32>(),
    );
    std::fs::write(&a.out, render(&shapes, &provenance)).with_context(|| a.out.display().to_string())?;
    println!("écrit : {}", a.out.display());
    for (ty, ride) in RIDES.iter().enumerate() {
        println!(
            "{ride} : comptes {:?}, tri_q8 {}, r / D des triangles {:?}",
            shapes.counts[ty], shapes.tri_q8[ty], shapes.rd_tri_milli[ty]
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un jeu synthétique : par type, `(n₁, n₂, n₃)` boucles, les triangles à r / D de 0,20 à 0,40 régulièrement.
    fn synthetic(counts: [[u32; 3]; 3]) -> Vec<(usize, u32, f64)> {
        let mut out = Vec::new();
        for (ty, c) in counts.iter().enumerate() {
            for (j, &n) in c.iter().enumerate() {
                for i in 0..n {
                    let k = [1, 2, 3][j];
                    out.push((ty, k, 0.20 + 0.20 * f64::from(i) / f64::from(n.max(1))));
                }
            }
        }
        out
    }

    fn expected(counts: [[u32; 3]; 3]) -> Expected {
        Expected { loops: counts.map(|c| c.iter().sum()), shares_milli: [None; 3] }
    }

    #[test]
    fn counts_shares_and_quantiles_come_out_of_a_synthetic_report_and_mtb_is_forced_to_zero() {
        let counts = [[40, 50, 10], [55, 40, 5], [3, 1, 0]];
        let s = compute(&synthetic(counts), &expected(counts)).unwrap();
        assert_eq!(s.counts, counts);
        assert_eq!(s.tri_q8[0], (256.0f64 * 50.0 / 90.0).round() as u16);
        assert_eq!(s.tri_q8[1], (256.0f64 * 40.0 / 95.0).round() as u16);
        assert_eq!((s.tri_q8[2], s.rd_tri_milli[2]), (0, [0; 5]), "MTB à 0");
        let r = s.rd_tri_milli[0];
        assert!(r.windows(2).all(|w| w[0] < w[1]) && r[0] >= 200 && r[4] <= 400, "{r:?}");
        assert_eq!(r[2], 298, "médiane de 50 triangles de 0,200 à 0,396");
    }

    #[test]
    fn a_report_that_is_not_the_production_set_is_refused() {
        let counts = [[40, 50, 10], [55, 40, 5], [3, 1, 0]];
        let err = compute(&synthetic(counts), &Expected::production()).unwrap_err().to_string();
        assert!(err.contains("ROAD : 100 boucles retenues, 2071 attendues"), "{err}");
        // Bons comptes, mauvaises parts.
        let counts = [[1000, 1000, 71], [432, 335, 13], [3, 1, 0]];
        let err = compute(&synthetic(counts), &Expected::production()).unwrap_err().to_string();
        assert!(err.contains("2071") || err.contains("classe"), "{err}");
    }

    #[test]
    fn the_rendered_file_names_the_constants_and_the_general_accessor() {
        let counts = [[40, 50, 10], [55, 40, 5], [3, 1, 0]];
        let s = compute(&synthetic(counts), &expected(counts)).unwrap();
        let text = render(&s, "Provenance : synthétique.");
        for needle in [
            "SHAPE_COUNTS",
            "SHAPE_TRI_Q8",
            "SHAPE_RD_TRI_MILLI",
            "pub(crate) fn general",
            "[0, 0, 0, 0, 0],",
        ] {
            assert!(text.contains(needle), "{needle}");
        }
        assert!(text.contains("//! Provenance : synthétique."));
    }
}
