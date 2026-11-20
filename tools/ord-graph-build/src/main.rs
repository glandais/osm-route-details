//! `ord-graph-build` : `build` (PBF → fichier de graphe) et `measure` (tailles, durées, requêtes).
//! Lot 0 du générateur de parcours : voir `docs/design/technique/ord-graph-lot0.md`.

use clap::{Parser, Subcommand};

mod attractors_cmd;
mod build_cmd;
#[cfg(test)]
mod e2e_tests;
mod index_cmd;
mod measure;
mod util;
mod views_cmd;

#[derive(Debug, Parser)]
#[command(
    about = "Graphe vélo en cellules : construction depuis un extrait OSM, vues par type de parcours et mesures (lots 0 et 0 bis)"
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Construit le graphe d'un extrait PBF et l'écrit en un fichier `.ordg`.
    Build(build_cmd::BuildArgs),
    /// Construit, hors ligne, les vues par type de parcours (`road`, `gravel`, `all`) d'un graphe de base.
    Views(views_cmd::ViewsArgs),
    /// Écrit les fichiers d'attracteurs (`.orda`) des vues `road` et `gravel` depuis l'extrait du graphe (lot 4d).
    Attractors(attractors_cmd::AttractorsArgs),
    /// Écrit le fichier d'index de service (`.ordi`) d'un graphe et de ses vues `road` et `gravel` : ce que le
    /// générateur calculait à chaque démarrage.
    Index(index_cmd::IndexArgs),
    /// Tailles, comptes et recherches bornées (froid / chaud) sur un graphe de base et ses vues.
    Measure(measure::MeasureArgs),
}

fn main() -> anyhow::Result<()> {
    match Args::parse().command {
        Command::Build(a) => {
            let r = build_cmd::run(&a)?;
            println!("{}", serde_json::to_string_pretty(&r)?);
        }
        Command::Views(a) => {
            let r = views_cmd::run(&a)?;
            println!("{}", serde_json::to_string_pretty(&r)?);
        }
        Command::Index(a) => {
            let r = index_cmd::run(&a)?;
            println!("{}", serde_json::to_string_pretty(&r)?);
        }
        Command::Attractors(a) => {
            let r = attractors_cmd::run(&a)?;
            println!("{}", serde_json::to_string_pretty(&r)?);
        }
        Command::Measure(a) => {
            measure::run(&a)?;
        }
    }
    Ok(())
}
