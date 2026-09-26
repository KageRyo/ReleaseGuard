mod config;
mod dataset;
mod manifest;
mod validate;

use clap::{Parser, Subcommand, ValueEnum};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use validate::Status;

#[derive(Parser)]
#[command(version, about = "A standalone dataset release gate for CI pipelines")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init {
        dataset: PathBuf,
    },
    Validate {
        dataset: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    Manifest {
        dataset: PathBuf,
    },
    Verify {
        dataset: PathBuf,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

const STARTER: &str = "release:\n  name: example-dataset\n  version: 0.1.0\n\nfiles:\n  data:\n    path: data.csv\n    format: csv\n    schema:\n      id:\n        type: string\n        nullable: false\n\nchecks:\n  - id: id-unique\n    type: unique\n    field: data.id\n";

fn run() -> Result<i32, String> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { dataset } => {
            fs::create_dir_all(&dataset).map_err(|e| format!("{}: {e}", dataset.display()))?;
            let path = dataset.join("release.yaml");
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            file.write_all(STARTER.as_bytes())
                .map_err(|e| format!("{}: {e}", path.display()))?;
            println!("Created {}", path.display());
            Ok(0)
        }
        Command::Validate { dataset, format } => {
            let config = config::load(&dataset)?;
            let report = validate::run(&dataset, &config)?;
            match format {
                OutputFormat::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
                ),
                OutputFormat::Text => {
                    println!(
                        "ReleaseGuard\n\nDataset: {}\nVersion: {}\n",
                        report.release.name, report.release.version
                    );
                    for check in &report.checks {
                        let mark = if check.status == Status::Passed {
                            "PASS"
                        } else {
                            "FAIL"
                        };
                        println!("{mark} {}: {}", check.id, check.message);
                        for example in &check.examples {
                            println!("  - {example}");
                        }
                    }
                    let passed = report
                        .checks
                        .iter()
                        .filter(|c| c.status == Status::Passed)
                        .count();
                    println!(
                        "\n{} passed, {} failed.\nValidation {}.",
                        passed,
                        report.checks.len() - passed,
                        if report.status == Status::Passed {
                            "PASSED"
                        } else {
                            "FAILED"
                        }
                    );
                }
            }
            Ok(if report.status == Status::Passed {
                0
            } else {
                1
            })
        }
        Command::Manifest { dataset } => {
            let config = config::load(&dataset)?;
            manifest::generate(&dataset, &config)?;
            println!("Created {}", dataset.join("manifest.json").display());
            Ok(0)
        }
        Command::Verify { dataset } => {
            let config = config::load(&dataset)?;
            let verification = manifest::verify(&dataset, &config)?;
            for (name, passed, message) in &verification.checks {
                println!(
                    "{} {name}: {message}",
                    if *passed { "PASS" } else { "FAIL" }
                );
            }
            println!(
                "Verification {}.",
                if verification.passed() {
                    "PASSED"
                } else {
                    "FAILED"
                }
            );
            Ok(if verification.passed() { 0 } else { 1 })
        }
    }
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("ReleaseGuard: {error}");
            std::process::exit(2);
        }
    }
}
