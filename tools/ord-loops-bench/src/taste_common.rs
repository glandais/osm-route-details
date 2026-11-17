//! Export de la table de goût commune du moteur (lot 4, 3b, §14.3.1) : lit les rapports de la chaîne commune rejouée
//! (`rc05-p3` et `base`) et écrit `crates/ord-loops/src/taste_common.rs`, **généré, à ne pas modifier à la main**.
//!
//! Aucune donnée privée n'en sort : trois tables de 72 entiers et les décimètres générés par boucle sous cette table
//! (des agrégats toutes équipes réunies, jamais par équipe).

use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{anyhow, bail, ensure, Context as _, Result};
use clap::Args;
use ord_graph_match::profile::learn::normalize_q8;
use ord_loops::TASTE_CLASSES;
use serde::Deserialize;
use xxhash_rust::xxh3::xxh3_64;

/// Les types, dans l'ordre de `RideType::index()`.
const RIDES: [&str; 3] = ["ROAD", "GRAVEL", "MTB"];

#[derive(Debug, Args)]
pub struct TasteCommonArgs {
    /// Rapport de la variante recalée à exporter (`<label>-rc05-p3-report.json`).
    #[arg(long)]
    pub report: PathBuf,
    /// Rapport de `base`, jouée à table neutre (les mètres générés d'un type neutralisé).
    #[arg(long)]
    pub base: PathBuf,
    /// Rapport de `rc05-p1` (les mètres que joue le pas 2 de la chaîne commune), avec `--step3` : les mètres des pas du
    /// mélange de la sonde (`generator.profile.mix = sonde`). Sans les deux, ces constantes sont écrites à zéro.
    #[arg(long, requires = "step3")]
    pub step2: Option<PathBuf>,
    /// Rapport de `rc05-p2` (les mètres que joue le pas 3).
    #[arg(long, requires = "step2")]
    pub step3: Option<PathBuf>,
    /// `taste_xxh3` attendu du rapport recalé : refus s'il diffère (le rejeu n'est plus celui de la campagne 2).
    #[arg(long)]
    pub expect_xxh3: String,
    /// Un type dont moins de boucles ont été comptées devient neutre (256 partout).
    #[arg(long, default_value_t = 50)]
    pub min_loops: u32,
    /// Part mise à l'écart de la campagne (pour la provenance seulement).
    #[arg(long, default_value_t = 0.25)]
    pub holdout_share: f64,
    /// Fichier Rust écrit.
    #[arg(long)]
    pub out: PathBuf,
    /// Les multiplicateurs normalisés sont bornés à `[1 / b, b]`.
    #[arg(long, default_value_t = 4.0)]
    pub bound: f64,
}

#[derive(Deserialize)]
struct Table {
    source: String,
    overlay: String,
    ride: String,
    q8: Vec<u16>,
}

#[derive(Deserialize)]
struct TypeMeters {
    ride: String,
    real_m: Vec<f64>,
    gen_m: Vec<f64>,
    loops: u32,
}

#[derive(Deserialize)]
struct Pooled {
    pooled: Vec<TypeMeters>,
}

#[derive(Deserialize)]
struct Variant {
    name: String,
    taste_xxh3: String,
    tables: Vec<Table>,
    class_meters: Pooled,
}

#[derive(Deserialize)]
struct Report {
    meta: Vec<(String, String)>,
    fingerprint: String,
    variant: Variant,
}

fn read_report(p: &std::path::Path) -> Result<Report> {
    let f = std::fs::File::open(p).with_context(|| p.display().to_string())?;
    serde_json::from_reader(std::io::BufReader::with_capacity(1 << 20, f))
        .with_context(|| format!("rapport illisible : {}", p.display()))
}

/// La table recalée `full` d'un type.
fn table_of<'a>(r: &'a Report, ride: &str) -> Result<&'a Table> {
    r.variant
        .tables
        .iter()
        .find(|t| t.source == "recalé" && t.overlay == "full" && t.ride == ride)
        .ok_or_else(|| anyhow!("rapport `{}` : pas de table recalée `full` pour {ride}", r.variant.name))
}

fn meters_of<'a>(r: &'a Report, ride: &str) -> Result<&'a TypeMeters> {
    r.variant.class_meters.pooled.iter().find(|m| m.ride == ride).ok_or_else(|| {
        anyhow!("rapport `{}` sans mètres par classe pour {ride} (rapport d'avant le 3b)", r.variant.name)
    })
}

/// `xxh3` des `q8` des trois tables dans l'ordre (type, classe), petit-boutiste : le calcul de `taste_xxh3` du banc.
pub fn q8_xxh3(q8: &[[u16; TASTE_CLASSES]; 3]) -> String {
    let mut bytes: Vec<u8> = Vec::with_capacity(3 * TASTE_CLASSES * 2);
    for t in q8 {
        for q in t {
            bytes.extend_from_slice(&q.to_le_bytes());
        }
    }
    format!("{:016x}", xxh3_64(&bytes))
}

/// Ce que l'export calcule, avant l'écriture.
#[derive(Debug, PartialEq)]
pub struct Exported {
    pub q8: [[u16; TASTE_CLASSES]; 3],
    pub q8n: [[u16; TASTE_CLASSES]; 3],
    pub gen_dm: [[u32; TASTE_CLASSES]; 3],
    pub loops: [u32; 3],
    /// Mètres toutes équipes réunies des trois pas de la chaîne commune, en décimètres : les entrées du pas `k` sont les
    /// mètres que joue sa table de départ (`base`, `rc05-p1`, `rc05-p2`). `None` : constantes à zéro.
    pub steps: Option<StepsExport>,
}

/// Les mètres des trois pas : `[pas][type][classe]`.
#[derive(Debug, PartialEq)]
pub struct StepsExport {
    pub real_dm: [[[u32; TASTE_CLASSES]; 3]; 3],
    pub gen_dm: [[[u32; TASTE_CLASSES]; 3]; 3],
    pub loops: [[u32; 3]; 3],
}

fn dm(m: f64) -> Result<u32> {
    let v = (m * 10.0).round();
    ensure!(v >= 0.0 && v <= f64::from(u32::MAX), "mètres hors de la plage d'un `u32` en décimètres : {m}");
    Ok(v as u32)
}

/// Les mètres des trois pas : `base` (pas 1), `p1` (pas 2), `p2` (pas 3). Refuse un rapport qui n'est pas la bonne variante.
fn export_steps(base: &Report, p1: &Report, p2: &Report) -> Result<StepsExport> {
    ensure!(
        p1.variant.name.ends_with("-p1") && p2.variant.name.ends_with("-p2"),
        "`--step2` doit être le rapport du pas 1 de la chaîne (`…-p1`) et `--step3` celui du pas 2 (`…-p2`) : reçu `{}` et `{}`",
        p1.variant.name,
        p2.variant.name
    );
    let mut out = StepsExport {
        real_dm: [[[0; TASTE_CLASSES]; 3]; 3],
        gen_dm: [[[0; TASTE_CLASSES]; 3]; 3],
        loops: [[0; 3]; 3],
    };
    for (k, r) in [base, p1, p2].into_iter().enumerate() {
        for (ty, ride) in RIDES.iter().enumerate() {
            let m = meters_of(r, ride)?;
            ensure!(
                m.real_m.len() == TASTE_CLASSES && m.gen_m.len() == TASTE_CLASSES,
                "mètres de {ride} mal formés (rapport `{}`)",
                r.variant.name
            );
            out.loops[k][ty] = m.loops;
            for c in 0..TASTE_CLASSES {
                out.real_dm[k][ty][c] = dm(m.real_m[c])?;
                out.gen_dm[k][ty][c] = dm(m.gen_m[c])?;
            }
        }
    }
    Ok(out)
}

fn export(r: &Report, base: &Report, min_loops: u32, bound: f64) -> Result<Exported> {
    let mut e = Exported {
        q8: [[256; TASTE_CLASSES]; 3],
        q8n: [[256; TASTE_CLASSES]; 3],
        gen_dm: [[0; TASTE_CLASSES]; 3],
        loops: [0; 3],
        steps: None,
    };
    for (ty, ride) in RIDES.iter().enumerate() {
        let (m, t) = (meters_of(r, ride)?, table_of(r, ride)?);
        ensure!(
            t.q8.len() == TASTE_CLASSES && m.gen_m.len() == TASTE_CLASSES,
            "tables de {ride} mal formées"
        );
        let neutral = m.loops < min_loops;
        let (src, loops) =
            if neutral { (meters_of(base, ride)?, meters_of(base, ride)?.loops) } else { (m, m.loops) };
        e.loops[ty] = loops;
        if !neutral {
            e.q8[ty].copy_from_slice(&t.q8);
        }
        if loops > 0 {
            for c in 0..TASTE_CLASSES {
                e.gen_dm[ty][c] = (src.gen_m[c] / f64::from(loops) * 10.0).round() as u32;
            }
        }
        let rf: [f64; TASTE_CLASSES] = std::array::from_fn(|c| f64::from(e.gen_dm[ty][c]));
        e.q8n[ty] = normalize_q8(&e.q8[ty], &rf, bound);
    }
    Ok(e)
}

fn table_text(name: &str, doc: &str, t: &[[u16; TASTE_CLASSES]; 3]) -> String {
    let mut s = format!("{doc}\npub const {name}: [[u16; TASTE_CLASSES]; 3] = [\n");
    for row in t {
        s.push_str("    [\n");
        for chunk in row.chunks(12) {
            let v: Vec<String> = chunk.iter().map(u16::to_string).collect();
            let _ = writeln!(s, "        {},", v.join(", "));
        }
        s.push_str("    ],\n");
    }
    s.push_str("];\n");
    s
}

fn steps_text(name: &str, v: &[[[u32; TASTE_CLASSES]; 3]; 3]) -> String {
    let mut s = format!("pub const {name}: [[[u32; TASTE_CLASSES]; 3]; 3] = [\n");
    for step in v {
        s.push_str("    [\n");
        for row in step {
            s.push_str("        [\n");
            for chunk in row.chunks(8) {
                let c: Vec<String> = chunk.iter().map(u32::to_string).collect();
                let _ = writeln!(s, "            {},", c.join(", "));
            }
            s.push_str("        ],\n");
        }
        s.push_str("    ],\n");
    }
    s.push_str("];\n");
    s
}

/// Le fichier Rust : provenance en tête (aucune donnée privée), puis les constantes.
fn render(e: &Exported, provenance: &str) -> String {
    let mut s = String::new();
    s.push_str("//! Table de goût commune du moteur (lot 4, 3b, §14.3) : **généré** par `ord-loops-bench taste-common`, ne pas\n");
    s.push_str("//! modifier à la main. Trois tables de 72 entiers (ROAD, GRAVEL, MTB) apprises sur les boucles de plusieurs\n");
    s.push_str("//! équipes réunies : un agrégat, aucune donnée d'équipe.\n//!\n");
    for l in provenance.lines() {
        let _ = writeln!(s, "//! {l}");
    }
    s.push_str("\nuse crate::cost::TASTE_CLASSES;\n\n");
    s.push_str(&table_text(
        "COMMON_Q8",
        "/// Multiplicateurs en 256es par type (`RideType::index()`) et classe de goût : la table recalée commune à trois pas.",
        &e.q8,
    ));
    s.push('\n');
    s.push_str(&table_text(
        "COMMON_Q8_NORMALIZED",
        "/// La même table, ramenée à une moyenne pondérée de 1 par les décimètres générés `COMMON_GEN_DM_PER_LOOP`.",
        &e.q8n,
    ));
    s.push_str("\n/// Décimètres générés par boucle sous `COMMON_Q8` (les deux curseurs), par type et par classe : le `R` du mélange.\n");
    s.push_str("pub const COMMON_GEN_DM_PER_LOOP: [[u32; TASTE_CLASSES]; 3] = [\n");
    for row in &e.gen_dm {
        s.push_str("    [\n");
        for chunk in row.chunks(8) {
            let v: Vec<String> = chunk.iter().map(u32::to_string).collect();
            let _ = writeln!(s, "        {},", v.join(", "));
        }
        s.push_str("    ],\n");
    }
    s.push_str("];\n\n/// Boucles comptées par type pour `COMMON_GEN_DM_PER_LOOP`.\n");
    let _ = writeln!(s, "pub const COMMON_LOOPS: [u32; 3] = {:?};\n", e.loops);
    let zero = StepsExport {
        real_dm: [[[0; TASTE_CLASSES]; 3]; 3],
        gen_dm: [[[0; TASTE_CLASSES]; 3]; 3],
        loops: [[0; 3]; 3],
    };
    let st = e.steps.as_ref().unwrap_or(&zero);
    s.push_str("\n/// Mètres du réel par pas de la chaîne commune (le pas `k` lit ceux que joue sa table de départ), par type et par\n/// classe, en décimètres, toutes équipes réunies : la cible du mélange de la sonde (`generator.profile.mix`).\n");
    s.push_str(&steps_text("COMMON_STEP_REAL_DM", &st.real_dm));
    s.push_str("\n/// Mètres générés, comme `COMMON_STEP_REAL_DM`.\n");
    s.push_str(&steps_text("COMMON_STEP_GEN_DM", &st.gen_dm));
    s.push_str("\n/// Boucles comptées par pas et par type.\n");
    let _ = writeln!(s, "pub const COMMON_STEP_LOOPS: [[u32; 3]; 3] = {:?};\n", st.loops);
    s.push_str("/// D'où vient la table (étiquette de la campagne, variante, empreintes).\n");
    let _ = writeln!(s, "pub const COMMON_SOURCE: &str = {provenance:?};");
    s
}

pub fn run(a: &TasteCommonArgs) -> Result<()> {
    let (r, base) = (read_report(&a.report)?, read_report(&a.base)?);
    ensure!(
        r.variant.taste_xxh3 == a.expect_xxh3,
        "`taste_xxh3` du rapport ({}) différent de celui attendu ({}) : le rejeu n'est pas celui de la campagne 2",
        r.variant.taste_xxh3,
        a.expect_xxh3
    );
    // Le même calcul sur les tables lues : le rapport ne ment pas sur ses propres tables.
    let mut all = [[0u16; TASTE_CLASSES]; 3];
    for (ty, ride) in RIDES.iter().enumerate() {
        all[ty].copy_from_slice(&table_of(&r, ride)?.q8);
    }
    ensure!(q8_xxh3(&all) == a.expect_xxh3, "les tables du rapport ne redonnent pas leur `taste_xxh3`");
    if base.variant.name != "base" {
        bail!("`--base` doit être le rapport de `base` (reçu : {})", base.variant.name);
    }
    let mut e = export(&r, &base, a.min_loops, a.bound)?;
    if let (Some(p1), Some(p2)) = (&a.step2, &a.step3) {
        e.steps = Some(export_steps(&base, &read_report(p1)?, &read_report(p2)?)?);
    }
    let meta = |k: &str| r.meta.iter().find(|(n, _)| n == k).map_or("?", |(_, v)| v.as_str()).to_string();
    let provenance = format!(
        "Variante : {} (empreinte de résultat {}, taste_xxh3 {}), rejouée sans les {:.0} % de boucles mises à l'écart ;\nappris sur les boucles restantes, toutes équipes réunies, deux curseurs.\nTypes neutralisés (moins de {} boucles) : {}.\nBoucles comptées : {:?}.\nBanc : commit {}, le {}.",
        r.variant.name,
        r.fingerprint,
        r.variant.taste_xxh3,
        a.holdout_share * 100.0,
        a.min_loops,
        RIDES
            .iter()
            .enumerate()
            .filter(|(ty, _)| e.q8[*ty].iter().all(|&q| q == 256))
            .map(|(_, n)| *n)
            .collect::<Vec<_>>()
            .join(", "),
        e.loops,
        meta("commit"),
        meta("date"),
    ) + if e.steps.is_some() {
        "\nPas du mélange de la sonde : mètres de `base`, `rc05-p1` et `rc05-p2`, toutes équipes réunies."
    } else {
        ""
    };
    std::fs::write(&a.out, render(&e, &provenance)).with_context(|| a.out.display().to_string())?;
    eprintln!(
        "table commune écrite : {} (taste_xxh3 {}, tables {})",
        a.out.display(),
        a.expect_xxh3,
        q8_xxh3(&e.q8)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(ride: &str, v: u16) -> Table {
        Table {
            source: "recalé".into(),
            overlay: "full".into(),
            ride: ride.into(),
            q8: vec![v; TASTE_CLASSES],
        }
    }

    fn meters(ride: &str, loops: u32, gen: f64) -> TypeMeters {
        TypeMeters {
            ride: ride.into(),
            real_m: vec![gen / 2.0; TASTE_CLASSES],
            gen_m: vec![gen; TASTE_CLASSES],
            loops,
        }
    }

    fn report(name: &str, q: [u16; 3], loops: [u32; 3], gen: f64) -> Report {
        Report {
            meta: vec![("commit".into(), "abc1234".into()), ("date".into(), "2026-11-17".into())],
            fingerprint: "ffff".into(),
            variant: Variant {
                name: name.into(),
                taste_xxh3: "0".into(),
                tables: (0..3).map(|i| table(RIDES[i], q[i])).collect(),
                class_meters: Pooled { pooled: (0..3).map(|i| meters(RIDES[i], loops[i], gen)).collect() },
            },
        }
    }

    #[test]
    fn export_rounds_the_decimetres_neutralises_small_types_and_normalises() {
        let r = report("rc05-p3", [128, 512, 300], [100, 60, 4], 1_000.0);
        let b = report("base", [256; 3], [110, 70, 9], 900.0);
        let e = export(&r, &b, 50, 4.0).unwrap();
        // 1 000 m générés sur 100 boucles : 10 m = 100 dm par boucle et par classe.
        assert_eq!(e.gen_dm[0][5], 100);
        assert_eq!(e.gen_dm[1][5], 167);
        assert_eq!(e.loops, [100, 60, 9]);
        // MTB (4 boucles) : neutre, mètres de `base` (900 / 9 × 10 = 1 000).
        assert!(e.q8[2].iter().all(|&q| q == 256));
        assert_eq!(e.gen_dm[2][0], 1_000);
        assert_eq!(e.q8n[2], e.q8[2]);
        // Table uniforme : la moyenne pondérée est la valeur, la normalisée vaut 256 partout.
        assert!(e.q8[0].iter().all(|&q| q == 128) && e.q8n[0].iter().all(|&q| q == 256));
        assert!(e.q8n[1].iter().all(|&q| q == 256));
    }

    #[test]
    fn the_steps_read_the_meters_each_step_plays_in_decimetres_and_refuse_a_wrong_report() {
        let base = report("base", [256; 3], [10, 20, 3], 1_000.0);
        let p1 = report("rc05-p1", [256; 3], [11, 21, 3], 2_000.0);
        let p2 = report("rc05-p2", [256; 3], [12, 22, 3], 3_000.5);
        let st = export_steps(&base, &p1, &p2).unwrap();
        assert_eq!(st.loops, [[10, 20, 3], [11, 21, 3], [12, 22, 3]]);
        assert_eq!((st.gen_dm[0][0][0], st.gen_dm[1][1][5], st.gen_dm[2][2][71]), (10_000, 20_000, 30_005));
        assert_eq!((st.real_dm[0][0][0], st.real_dm[2][0][3]), (5_000, 15_003));
        // Les rapports dans le désordre : refusés.
        assert!(export_steps(&base, &p2, &p1).is_err());
        let text = render(
            &Exported {
                q8: [[256; TASTE_CLASSES]; 3],
                q8n: [[256; TASTE_CLASSES]; 3],
                gen_dm: [[7; TASTE_CLASSES]; 3],
                loops: [1, 2, 3],
                steps: Some(st),
            },
            "x",
        );
        assert!(text.contains("COMMON_STEP_LOOPS: [[u32; 3]; 3] = [[10, 20, 3], [11, 21, 3], [12, 22, 3]];"));
        assert!(text.contains("30005"));
    }

    #[test]
    fn normalisation_is_bounded_and_idempotent_without_weights() {
        let mut q = [256u16; TASTE_CLASSES];
        q[0] = 1_024;
        let mut r = [0.0; TASTE_CLASSES];
        assert_eq!(normalize_q8(&q, &r, 4.0), q, "Σ R = 0 : inchangée");
        r[0] = 1.0;
        r[1] = 1.0;
        // Moyenne (1 024 + 256) / 2 = 640 : 256 × 1 024 / 640 = 410 ; 256 × 256 / 640 = 102.
        let n = normalize_q8(&q, &r, 4.0);
        assert_eq!((n[0], n[1]), (410, 102));
        // Borne basse : 256 / 4 = 64.
        r[0] = 1_000.0;
        assert_eq!(normalize_q8(&q, &r, 4.0)[1], 64);
    }

    #[test]
    fn the_hash_is_the_benchs_taste_hash() {
        let mut q = [[256u16; TASTE_CLASSES]; 3];
        let a = q8_xxh3(&q);
        q[2][71] = 255;
        assert_ne!(a, q8_xxh3(&q));
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn the_rendered_file_names_every_constant_with_72_entries_per_row() {
        let e = Exported {
            q8: [[256; TASTE_CLASSES]; 3],
            q8n: [[256; TASTE_CLASSES]; 3],
            gen_dm: [[7; TASTE_CLASSES]; 3],
            loops: [1, 2, 3],
            steps: None,
        };
        let text = render(&e, "ligne un\nligne deux");
        for k in [
            "COMMON_Q8:",
            "COMMON_Q8_NORMALIZED:",
            "COMMON_GEN_DM_PER_LOOP:",
            "COMMON_LOOPS:",
            "COMMON_STEP_REAL_DM:",
            "COMMON_STEP_GEN_DM:",
            "COMMON_STEP_LOOPS:",
            "COMMON_SOURCE:",
        ] {
            assert!(text.contains(k), "{k}");
        }
        assert!(text.starts_with("//! Table de goût commune"));
        let nums = text.split("COMMON_Q8:").nth(1).unwrap().split("\n];").next().unwrap();
        assert_eq!(nums.matches("256").count(), 3 * TASTE_CLASSES);
    }
}
