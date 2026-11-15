//! `ord-graph-build` : `build` (PBF → fichier de graphe) et `measure` (tailles, durées, requêtes).
//! Lot 0 du générateur de parcours : voir `docs/design/technique/ord-graph-lot0.md`.

use clap::{Parser, Subcommand};

mod build_cmd;
#[cfg(test)]
mod e2e_tests;
mod measure;
mod util;

#[derive(Debug, Parser)]
#[command(about = "Graphe vélo en cellules : construction depuis un extrait OSM et mesures (lot 0)")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Construit le graphe d'un extrait PBF et l'écrit en un fichier `.ordg`.
    Build(build_cmd::BuildArgs),
    /// Tailles, comptes et recherches bornées (froid / chaud) sur un fichier de graphe.
    Measure(measure::MeasureArgs),
}

fn main() -> anyhow::Result<()> {
    match Args::parse().command {
        Command::Build(a) => {
            let r = build_cmd::run(&a)?;
            println!("{}", serde_json::to_string_pretty(&r)?);
        }
        Command::Measure(a) => {
            measure::run(&a)?;
        }
    }
    Ok(())
}
