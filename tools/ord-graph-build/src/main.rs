//! `ord-graph-build` : `build` (PBF → fichier de graphe) et `measure` (tailles, durées, requêtes).
//! Lot 0 du générateur de parcours : voir `docs/design/technique/ord-graph-lot0.md`.

use clap::{Parser, Subcommand};

mod build_cmd;
#[cfg(test)]
mod e2e_tests;
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
        Command::Measure(a) => {
            measure::run(&a)?;
        }
    }
    Ok(())
}
