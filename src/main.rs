use clap::Parser;
use guardrails::cli::format;
use guardrails::cli::{Cli, Commands, OutputFormat};
use guardrails::config::Severity;
use guardrails::init;
use guardrails::scan;
use std::fs;
use std::process;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Scan {
            paths,
            config,
            format: output_format,
        } => {
            let result = match scan::run_scan(&config, &paths) {
                Ok(r) => r,
                Err(scan::ScanError::ConfigRead(ref e))
                    if e.kind() == std::io::ErrorKind::NotFound =>
                {
                    eprintln!(
                        "\x1b[31merror\x1b[0m: config file '{}' not found",
                        config.display()
                    );
                    eprintln!(
                        "\x1b[90mhint\x1b[0m: run \x1b[1mguardrails init\x1b[0m to generate a starter config"
                    );
                    process::exit(2);
                }
                Err(e) => {
                    eprintln!("\x1b[31merror\x1b[0m: {}", e);
                    process::exit(2);
                }
            };

            match output_format {
                OutputFormat::Pretty => format::print_pretty(&result),
                OutputFormat::Json => format::print_json(&result),
                OutputFormat::Compact => format::print_compact(&result),
                OutputFormat::Github => format::print_github(&result),
            }

            let has_errors = result
                .violations
                .iter()
                .any(|v| v.severity == Severity::Error);

            process::exit(if has_errors { 1 } else { 0 });
        }

        Commands::Baseline {
            paths,
            config,
            output,
        } => {
            let result = match scan::run_baseline(&config, &paths) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("\x1b[31merror\x1b[0m: {}", e);
                    process::exit(2);
                }
            };

            let json = serde_json::to_string_pretty(&result).unwrap();
            if let Err(e) = fs::write(&output, &json) {
                eprintln!("\x1b[31merror\x1b[0m: failed to write baseline: {}", e);
                process::exit(2);
            }

            eprintln!(
                "\x1b[32m✓\x1b[0m Baseline written to {} ({} ratchet rule{}, {} files scanned)",
                output.display(),
                result.entries.len(),
                if result.entries.len() == 1 { "" } else { "s" },
                result.files_scanned
            );

            for entry in &result.entries {
                eprintln!(
                    "  {:<30} {} occurrence{}",
                    entry.rule_id,
                    entry.count,
                    if entry.count == 1 { "" } else { "s" }
                );
            }
        }

        Commands::Init { output, force } => {
            if output.exists() && !force {
                eprintln!(
                    "\x1b[31merror\x1b[0m: '{}' already exists (use --force to overwrite)",
                    output.display()
                );
                process::exit(2);
            }

            let project_dir = std::env::current_dir().unwrap_or_default();
            let project_type = init::detect_project(&project_dir);
            let config = init::generate_config(&project_type);

            if let Err(e) = fs::write(&output, &config) {
                eprintln!("\x1b[31merror\x1b[0m: failed to write config: {}", e);
                process::exit(2);
            }

            let type_label = match project_type {
                init::ProjectType::ShadcnTailwind => "shadcn + Tailwind",
                init::ProjectType::TailwindOnly => "Tailwind CSS",
                init::ProjectType::Generic => "generic",
                init::ProjectType::Unknown => "generic",
            };

            eprintln!(
                "\x1b[32m✓\x1b[0m Created {} (detected: {})",
                output.display(),
                type_label
            );
            eprintln!(
                "\x1b[90mhint\x1b[0m: run \x1b[1mguardrails scan .\x1b[0m to find violations"
            );
        }
    }
}
