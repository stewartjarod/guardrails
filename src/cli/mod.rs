pub mod format;
pub mod toml_config;

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "guardrails", about = "Enforce architectural decisions AI coding tools keep ignoring")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Scan files for rule violations
    Scan {
        /// Paths to scan (files or directories)
        #[arg(required = true)]
        paths: Vec<PathBuf>,

        /// Path to guardrails.toml config file
        #[arg(short, long, default_value = "guardrails.toml")]
        config: PathBuf,

        /// Output format
        #[arg(short, long, value_enum, default_value_t = OutputFormat::Pretty)]
        format: OutputFormat,
    },

    /// Count current occurrences of ratchet patterns and write a baseline JSON file
    Baseline {
        /// Paths to scan (files or directories)
        #[arg(required = true)]
        paths: Vec<PathBuf>,

        /// Path to guardrails.toml config file
        #[arg(short, long, default_value = "guardrails.toml")]
        config: PathBuf,

        /// Output file path for the baseline JSON
        #[arg(short, long, default_value = ".guardrails-baseline.json")]
        output: PathBuf,
    },

    /// Generate a starter guardrails.toml for your project
    Init {
        /// Output file path for the generated config
        #[arg(short, long, default_value = "guardrails.toml")]
        output: PathBuf,

        /// Overwrite existing config file
        #[arg(long)]
        force: bool,
    },
}

#[derive(Clone, ValueEnum)]
pub enum OutputFormat {
    Pretty,
    Json,
    Compact,
    Github,
}
