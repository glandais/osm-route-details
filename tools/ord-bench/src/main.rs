//! `ord-bench` : `check`, `fill-cache`, `run --offline`, `diagnose`, `diagnose-structures`,
//! `latency` (L1 à froid) (T05 §5.1). Les
//! autres sous-commandes (`judge`, `compare`, `dsm-placement`) arrivent avec les incréments
//! suivants.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};
use ord_bench::campaign::{default_report_dir, fill_cache, latency_cold, run};
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
    /// Remplit le cache de banc en ligne (IGN à 20 req/s, Mapterhorn, couverture) et écrit
    /// `cache.lock` dans le cache.
    FillCache {
        /// Identifiants de cas (défaut : tous ceux des groupes choisis).
        #[arg(long, value_delimiter = ',')]
        cas: Vec<String>,
        /// Groupes de traces personnelles.
        #[arg(long, value_delimiter = ',', default_value = "dplus,plaine,ouvrage,hors_france,osm_surface")]
        groupes: Vec<String>,
        /// Parcours vcyclist des tests de cohérence avec la vérité `T`.
        #[arg(long, value_delimiter = ',', default_value = "test1,test2,test3,test4")]
        coherence: Vec<String>,
        /// Scénarios golden (ouvrages de calibration).
        #[arg(long, value_delimiter = ',', default_value = "krk,liege-belle-liegeoise")]
        golden: Vec<String>,
        /// Fixtures hors git du manifest (test de méthode `vc-strava`), lues dans bench/private/.
        #[arg(long, value_delimiter = ',', default_value = "vc-strava")]
        hors_git: Vec<String>,
        /// GPX supplémentaires.
        #[arg(long)]
        gpx: Vec<PathBuf>,
    },
    /// Campagne de calibration sur le cache de banc, sans réseau.
    Run {
        /// Obligatoire : seul le mode hors ligne existe.
        #[arg(long)]
        offline: bool,
        #[arg(long, value_delimiter = ',')]
        cas: Vec<String>,
        #[arg(long, value_delimiter = ',', default_value = "dplus,plaine,ouvrage,hors_france,osm_surface")]
        groupes: Vec<String>,
        /// Fichier de sortie (défaut : bench/reports/<date>-<rules_version>/results.json, T05 §5.2).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Causes des plages non appariées et des désaccords de revêtement d'un cas (tuiles du cache).
    Diagnose {
        #[arg(long, value_delimiter = ',', required = true)]
        cas: Vec<String>,
    },
    /// Groupes d'ouvrages d'un cas (MNT, MNS, sortie par station d'axe), hors ligne.
    DiagnoseStructures {
        #[arg(long)]
        cas: String,
        #[arg(long, default_value_t = 0.0)]
        de: f64,
        #[arg(long, default_value_t = f64::INFINITY)]
        a: f64,
    },
    /// Vidage local et privé des profils de traces perso de calibration (grille de 5 m, mode
    /// `distance`) dans `bench/private/profils/` ; une trace tenue à l'écart est refusée.
    DumpProfils {
        #[arg(long, value_delimiter = ',', required = true)]
        cas: Vec<String>,
    },
    /// L1 : latence à froid (live, caches disque et mémoire neufs à chaque essai).
    Latency {
        /// Cas personnels (traces de 90 à 130 km).
        #[arg(long, value_delimiter = ',', required = true)]
        cas: Vec<String>,
        #[arg(long, default_value_t = 1)]
        essais: usize,
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
        Command::FillCache { cas, groupes, coherence, golden, hors_git, gpx } => {
            let lock = fill_cache(&paths, &groupes, &cas, &coherence, &hors_git, &golden, &gpx).await?;
            println!("{}", serde_json::to_string_pretty(&lock)?);
            Ok(true)
        }
        Command::Run { offline, cas, groupes, out } => {
            if !offline {
                anyhow::bail!("seul `run --offline` existe (cache de banc, sans réseau)");
            }
            let out = out.unwrap_or_else(|| default_report_dir(&paths).join("results.json"));
            // Un rapport consigné n'est jamais réécrit (T05 §5.2) : changer `rules_version` ou `--out`.
            if out.exists() {
                anyhow::bail!("{} existe déjà : rapport non réécrit", out.display());
            }
            let results = run(&paths, &groupes, &cas).await?;
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
        Command::Diagnose { cas } => {
            for id in &cas {
                let d = ord_bench::diagnose::diagnose_case(&paths, id).await?;
                println!("{}", serde_json::to_string_pretty(&d)?);
            }
            Ok(true)
        }
        Command::DiagnoseStructures { cas, de, a } => {
            let d = ord_bench::structures_diag::diagnose_structures(&paths, &cas, de, a).await?;
            println!("{}", serde_json::to_string_pretty(&d)?);
            Ok(true)
        }
        Command::DumpProfils { cas } => {
            // Contenu jamais affiché : chemin et nombre de lignes seulement.
            for (path, rows) in ord_bench::dump::dump_profiles(&paths, &cas).await? {
                println!("{} : {rows} lignes", path.display());
            }
            Ok(true)
        }
        Command::Latency { cas, essais } => {
            let result = latency_cold(&paths, &cas, essais).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
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
