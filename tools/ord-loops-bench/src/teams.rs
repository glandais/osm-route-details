//! `--dump-teams` (finition 2 du lot 6, §20.4) : construit les données d'équipe et les tables du contrôleur de chaque
//! surcouche du dossier et écrit, par équipe, l'empreinte xxh3 de leur forme canonique en octets (lettre, objet,
//! empreinte, taille). Deux binaires qui construisent différemment doivent écrire les mêmes lignes ; avec
//! `--teams-reference`, ce binaire construit aussi chaque équipe à l'ancienne et compare les octets.
//!
//! **Privé** : les surcouches sont des données d'équipes réelles ; seules les lettres sortent.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, Result};
use ord_graph::overlay::Overlay;
use ord_loops::{Engine, TeamData};
use ord_loops_check::{Config, GraphIndex, TeamCheck};
use xxhash_rust::xxh3::xxh3_64;

use crate::windows::AnonSampler;

#[allow(clippy::too_many_arguments)]
pub fn dump<B: AsRef<[u8]> + Sync>(
    engine: &Engine<B>,
    graph_index: &GraphIndex,
    overlays: &Path,
    letters: &BTreeMap<String, String>,
    only: &[String],
    cfg: &Config,
    with_reference: bool,
    out: &Path,
) -> Result<()> {
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(overlays)? {
        let p = entry?.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        if let Some(id) = name.strip_suffix(".ordo").and_then(|n| n.split('-').next()) {
            let letter = letters.get(id).cloned().unwrap_or_else(|| "?".into());
            files.push((letter, p.clone()));
        }
    }
    files.sort();
    let sampler = AnonSampler::start();
    let mut lines: Vec<String> = Vec::new();
    for (letter, path) in &files {
        if !only.is_empty() && !only.contains(letter) {
            continue;
        }
        let overlay = Overlay::read(path).map_err(|e| anyhow!("surcouche de l'équipe {letter} : {e}"))?;
        let _ = sampler.take_max_mb();
        let t = std::time::Instant::now();
        let plan = TeamData::plan_build(engine, &overlay).map_err(|e| anyhow!("équipe {letter} : {e}"))?;
        let data = TeamData::build_planned(engine, &overlay, &plan).map_err(|e| anyhow!("{letter} : {e}"))?;
        let data_s = t.elapsed().as_secs_f64();
        let data_peak = sampler.take_max_mb();
        let t = std::time::Instant::now();
        let check_plan = TeamCheck::plan(engine.base(), &overlay, cfg);
        let check = TeamCheck::build_planned(engine.base(), graph_index, &overlay, cfg, &check_plan)
            .map_err(|e| anyhow!("équipe {letter} : {e}"))?;
        let check_s = t.elapsed().as_secs_f64();
        let check_peak = sampler.take_max_mb();
        eprintln!(
            "équipe {letter} : données {data_s:.2} s (pic anonyme {data_peak:.0} Mo, estimé {:.0} Mo), contrôleur {check_s:.2} s (pic {check_peak:.0} Mo, estimé {:.0} Mo)",
            plan.estimated_bytes as f64 / 1e6,
            check_plan.estimated_bytes as f64 / 1e6
        );
        let (db, cb) = (data.canonical_bytes(), check.canonical_bytes());
        lines.push(format!("{letter} data {:016x} {}", xxh3_64(&db), db.len()));
        lines.push(format!("{letter} check {:016x} {}", xxh3_64(&cb), cb.len()));
        if with_reference {
            reference_compare(engine, graph_index, &overlay, cfg, letter, &db, &cb)?;
        }
    }
    std::fs::write(out, lines.join("\n") + "\n")?;
    eprintln!("{} lignes écrites", lines.len());
    Ok(())
}

fn reference_compare<B: AsRef<[u8]> + Sync>(
    engine: &Engine<B>,
    graph_index: &GraphIndex,
    overlay: &Overlay,
    cfg: &Config,
    letter: &str,
    data: &[u8],
    check: &[u8],
) -> Result<()> {
    let t = std::time::Instant::now();
    let reference = TeamData::build_reference(engine, overlay).map_err(|e| anyhow!("{letter} : {e}"))?;
    anyhow::ensure!(
        reference.canonical_bytes() == data,
        "équipe {letter} : les données d'équipe diffèrent de la référence"
    );
    eprintln!("équipe {letter} : données identiques à la référence ({:.2} s)", t.elapsed().as_secs_f64());
    let t = std::time::Instant::now();
    let reference = TeamCheck::build_reference(engine.base(), graph_index, overlay, cfg);
    anyhow::ensure!(
        reference.canonical_bytes() == check,
        "équipe {letter} : les tables du contrôleur diffèrent de la référence"
    );
    eprintln!(
        "équipe {letter} : tables du contrôleur identiques à la référence ({:.2} s)",
        t.elapsed().as_secs_f64()
    );
    Ok(())
}
