//! `ord-graph-overlay` : rattache ce que les équipes ont roulé au graphe vélo OSM (surcouche par équipe) et
//! mesure la part du réseau d'équipe retrouvée dans le graphe. Lot 1 du générateur de parcours :
//! voir `docs/design/technique/ord-graph-lot1.md`.

use clap::{Parser, Subcommand};
// L'appariement vit dans la bibliothèque partagée avec le service ; les chemins `crate::geo`, `crate::index` et
// `crate::matcher` de l'outil restent valables.
use ord_graph_match::{geo, index, matcher};

mod aggregate;
mod build_cmd;
mod checks;
mod export;
mod pipeline;
mod report;

#[cfg(test)]
mod e2e_tests;

#[derive(Debug, Parser)]
#[command(about = "Surcouche de fréquentation d'équipe rattachée au graphe vélo OSM (lot 1)")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Lit l'export des équipes, l'apparie au graphe, écrit une surcouche par équipe et le rapport JSON.
    Build(build_cmd::BuildArgs),
    /// Lit une surcouche, vérifie qu'elle va avec le graphe et en résume le contenu.
    Inspect(build_cmd::InspectArgs),
}

fn main() -> anyhow::Result<()> {
    match Args::parse().command {
        Command::Build(a) => {
            let r = build_cmd::run(&a)?;
            println!("{}", serde_json::to_string_pretty(&r.summary)?);
        }
        Command::Inspect(a) => {
            println!("{}", serde_json::to_string_pretty(&build_cmd::inspect(&a)?)?);
        }
    }
    Ok(())
}
