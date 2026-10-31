//! `ord-bench` : `check`, `fill-cache`, `run --offline` (T05 §5.1). Les autres sous-commandes
//! (`judge`, `compare`, `latency`, `dsm-placement`) arrivent avec les incréments suivants.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};
use ord_bench::campaign::{default_report_dir, fill_cache, run};
use ord_bench::check::check;
use ord_bench::BenchPaths;

#[derive(Debug, Parser)]
#[command(about = "Banc qualité d'osm-route-details")]
struct Args {
    /// Dossier `bench/` du dépôt.
    #[arg(long, global = true, default_value = "bench")]
    bench_dir: PathBuf,
    /// Cache de banc local (défaut : `ORD_BENCH_CACHE`, sinon ~/.cache/osm-route-details/bench).
    #[arg(long, global = true)]
    cache_dir: Option<PathBuf>,
    /// Traces personnelles (défaut : `<bench>/private`).
    #[arg(long, global = true)]
    private_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Empreintes du manifest, cohérence criteria.yaml ↔ README, Q1.
    Check,
    /// Remplit le cache de banc en ligne (Mapterhorn, sobrement) et écrit `cache.lock` dans le cache.
    FillCache {
        /// Identifiants de cas (défaut : tous ceux des groupes choisis).
        #[arg(long, value_delimiter = ',')]
        cas: Vec<String>,
        /// Groupes de traces personnelles.
        #[arg(long, value_delimiter = ',', default_value = "hors_france")]
        groupes: Vec<String>,
        /// Parcours vcyclist de la cohérence du demi-pixel.
        #[arg(long, value_delimiter = ',', default_value = "test4")]
        coherence: Vec<String>,
        /// GPX supplémentaires (test de méthode `strava.gpx`).
        #[arg(long)]
        gpx: Vec<PathBuf>,
    },
    /// Campagne de calibration sur le cache de banc, sans réseau.
    Run {
        /// Obligatoire à l'incrément 1 : seul le mode hors ligne existe.
        #[arg(long)]
        offline: bool,
        #[arg(long, value_delimiter = ',')]
        cas: Vec<String>,
        #[arg(long, value_delimiter = ',', default_value = "hors_france")]
        groupes: Vec<String>,
        /// Fichier de sortie (défaut : bench/reports/<date>-<rules_version>/results.json, T05 §5.2).
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

async fn main_inner() -> anyhow::Result<bool> {
    let args = Args::parse();
    let paths = BenchPaths::new(&args.bench_dir, args.cache_dir.as_deref(), args.private_dir.as_deref());
    match args.command {
        Command::Check => {
            let r = check(&paths)?;
            println!("manifest sha256 : {}", r.manifest_sha256);
            println!("criteria sha256 : {}", r.criteria_sha256);
            println!("fichiers vérifiés : {}", r.verified);
            println!(
                "traces privées : {}",
                if r.private_present { "présentes, vérifiées" } else { "absentes" }
            );
            for s in &r.skipped {
                println!("sauté : {s}");
            }
            for f in &r.failures {
                println!("ÉCHEC : {f}");
            }
            let verdicts: Vec<String> = r.q1.verdicts.iter().map(|(k, v)| format!("{v} {k}")).collect();
            println!(
                "Q1 : {} — {}/{} négatifs non fiables ({})",
                if r.q1.pass() { "pass" } else { "fail" },
                r.q1.negatives - r.q1.reliable,
                r.q1.negatives,
                verdicts.join(", ")
            );
            Ok(r.ok())
        }
        Command::FillCache { cas, groupes, coherence, gpx } => {
            let lock = fill_cache(&paths, &groupes, &cas, &coherence, &gpx).await?;
            println!("{}", serde_json::to_string_pretty(&lock)?);
            Ok(true)
        }
        Command::Run { offline, cas, groupes, out } => {
            if !offline {
                anyhow::bail!("seul `run --offline` existe à l'incrément 1 (cache de banc, sans réseau)");
            }
            let results = run(&paths, &groupes, &cas).await?;
            let out = out.unwrap_or_else(|| default_report_dir(&paths).join("results.json"));
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir).with_context(|| format!("création de {}", dir.display()))?;
            }
            std::fs::write(&out, serde_json::to_string_pretty(&results)? + "\n")?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "agregats": results["agregats"],
                    "verdicts": results["verdicts"],
                    "cas_sautes": results["cas_sautes"],
                }))?
            );
            for c in results["cas"].as_array().into_iter().flatten() {
                println!(
                    "{} ({}) : {}",
                    c["id"].as_str().unwrap_or(""),
                    c["groupe"].as_str().unwrap_or(""),
                    c["metriques"]
                );
            }
            println!("écrit : {}", out.display());
            Ok(true)
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match main_inner().await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("erreur : {e:#}");
            ExitCode::FAILURE
        }
    }
}
