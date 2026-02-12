use crate::config::Severity;
use crate::rules::Violation;
use crate::scan::ScanResult;
use serde_json::json;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::io::Write;

/// Print violations grouped by file with ANSI colors.
pub fn print_pretty(result: &ScanResult) {
    if result.violations.is_empty() {
        println!(
            "\x1b[32m✓\x1b[0m No violations found ({} files scanned, {} rules loaded)",
            result.files_scanned, result.rules_loaded
        );
        print_ratchet_summary(&result.ratchet_counts);
        return;
    }

    // Group violations by file
    let mut by_file: BTreeMap<String, Vec<&Violation>> = BTreeMap::new();
    for v in &result.violations {
        by_file
            .entry(v.file.display().to_string())
            .or_default()
            .push(v);
    }

    for (file, violations) in &by_file {
        println!("\n\x1b[4m{}\x1b[0m", file);
        for v in violations {
            let severity_str = match v.severity {
                Severity::Error => "\x1b[31merror\x1b[0m",
                Severity::Warning => "\x1b[33mwarn \x1b[0m",
            };

            let location = match (v.line, v.column) {
                (Some(l), Some(c)) => format!("{}:{}", l, c),
                (Some(l), None) => format!("{}:1", l),
                _ => "1:1".to_string(),
            };

            println!(
                "  \x1b[90m{:<8}\x1b[0m {} \x1b[90m{:<25}\x1b[0m {}",
                location, severity_str, v.rule_id, v.message
            );

            if let Some(ref source) = v.source_line {
                println!("           \x1b[90m│\x1b[0m {}", source.trim());
            }

            if let Some(ref suggest) = v.suggest {
                println!("           \x1b[90m└─\x1b[0m \x1b[36m{}\x1b[0m", suggest);
            }
        }
    }

    let errors = result
        .violations
        .iter()
        .filter(|v| v.severity == Severity::Error)
        .count();
    let warnings = result
        .violations
        .iter()
        .filter(|v| v.severity == Severity::Warning)
        .count();

    println!();
    print!("\x1b[1m");
    if errors > 0 {
        print!("\x1b[31m{} error{}\x1b[0m\x1b[1m", errors, if errors == 1 { "" } else { "s" });
    }
    if errors > 0 && warnings > 0 {
        print!(", ");
    }
    if warnings > 0 {
        print!("\x1b[33m{} warning{}\x1b[0m\x1b[1m", warnings, if warnings == 1 { "" } else { "s" });
    }
    println!(
        " ({} files scanned, {} rules loaded)\x1b[0m",
        result.files_scanned, result.rules_loaded
    );

    print_ratchet_summary(&result.ratchet_counts);
}

fn print_ratchet_summary(ratchet_counts: &HashMap<String, (usize, usize)>) {
    if ratchet_counts.is_empty() {
        return;
    }

    println!("\n\x1b[1mRatchet rules:\x1b[0m");
    let mut sorted: Vec<_> = ratchet_counts.iter().collect();
    sorted.sort_by_key(|(id, _)| (*id).clone());

    for (rule_id, &(found, max)) in &sorted {
        let status = if found <= max {
            format!("\x1b[32m✓ pass\x1b[0m ({}/{})", found, max)
        } else {
            format!("\x1b[31m✗ OVER\x1b[0m ({}/{})", found, max)
        };
        println!("  {:<30} {}", rule_id, status);
    }
}

/// Print violations as structured JSON.
pub fn print_json(result: &ScanResult) {
    let violations: Vec<_> = result
        .violations
        .iter()
        .map(|v| {
            json!({
                "rule_id": v.rule_id,
                "severity": match v.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                },
                "file": v.file.display().to_string(),
                "line": v.line,
                "column": v.column,
                "message": v.message,
                "suggest": v.suggest,
                "source_line": v.source_line,
            })
        })
        .collect();

    let ratchet: serde_json::Map<String, serde_json::Value> = result
        .ratchet_counts
        .iter()
        .map(|(id, &(found, max))| {
            (
                id.clone(),
                json!({ "found": found, "max": max, "pass": found <= max }),
            )
        })
        .collect();

    let output = json!({
        "violations": violations,
        "summary": {
            "total": result.violations.len(),
            "errors": result.violations.iter().filter(|v| v.severity == Severity::Error).count(),
            "warnings": result.violations.iter().filter(|v| v.severity == Severity::Warning).count(),
            "files_scanned": result.files_scanned,
            "rules_loaded": result.rules_loaded,
        },
        "ratchet": ratchet,
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

/// Print violations in compact one-line-per-violation format.
/// Violations go to stdout; summary goes to stderr.
pub fn print_compact(result: &ScanResult) {
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    write_compact(result, &mut stdout, &mut stderr);
}

fn write_compact(result: &ScanResult, out: &mut dyn Write, err: &mut dyn Write) {
    for v in &result.violations {
        let severity = match v.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let line = v.line.unwrap_or(1);
        let col = v.column.unwrap_or(1);

        let _ = writeln!(
            out,
            "{}:{}:{}: {}[{}] {}",
            v.file.display(),
            line,
            col,
            severity,
            v.rule_id,
            v.message
        );
    }

    write_summary_stderr(result, err);
    write_ratchet_stderr(&result.ratchet_counts, err);
}

/// Print violations as GitHub Actions workflow commands.
/// Violations go to stdout; summary goes to stderr.
pub fn print_github(result: &ScanResult) {
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    write_github(result, &mut stdout, &mut stderr);
}

fn write_github(result: &ScanResult, out: &mut dyn Write, err: &mut dyn Write) {
    for v in &result.violations {
        let level = match v.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };

        let line = v.line.unwrap_or(1);
        let mut props = format!("file={},line={}", v.file.display(), line);
        if let Some(col) = v.column {
            props.push_str(&format!(",col={}", col));
        }
        props.push_str(&format!(",title={}", v.rule_id));

        let _ = writeln!(out, "::{} {}::{}", level, props, v.message);
    }

    // Ratchet failures as annotations
    let mut sorted: Vec<_> = result.ratchet_counts.iter().collect();
    sorted.sort_by_key(|(id, _)| (*id).clone());
    for (rule_id, &(found, max)) in &sorted {
        if found > max {
            let _ = writeln!(
                out,
                "::error title=ratchet-{}::Ratchet rule '{}' exceeded budget: {} found, max {}",
                rule_id, rule_id, found, max
            );
        }
    }

    write_summary_stderr(result, err);
}

fn write_summary_stderr(result: &ScanResult, err: &mut dyn Write) {
    let errors = result
        .violations
        .iter()
        .filter(|v| v.severity == Severity::Error)
        .count();
    let warnings = result
        .violations
        .iter()
        .filter(|v| v.severity == Severity::Warning)
        .count();

    if errors > 0 || warnings > 0 {
        let mut parts = Vec::new();
        if errors > 0 {
            parts.push(format!(
                "{} error{}",
                errors,
                if errors == 1 { "" } else { "s" }
            ));
        }
        if warnings > 0 {
            parts.push(format!(
                "{} warning{}",
                warnings,
                if warnings == 1 { "" } else { "s" }
            ));
        }
        let _ = writeln!(
            err,
            "{} ({} files scanned, {} rules loaded)",
            parts.join(", "),
            result.files_scanned,
            result.rules_loaded
        );
    } else {
        let _ = writeln!(
            err,
            "No violations found ({} files scanned, {} rules loaded)",
            result.files_scanned,
            result.rules_loaded
        );
    }
}

fn write_ratchet_stderr(
    ratchet_counts: &HashMap<String, (usize, usize)>,
    err: &mut dyn Write,
) {
    if ratchet_counts.is_empty() {
        return;
    }

    let mut sorted: Vec<_> = ratchet_counts.iter().collect();
    sorted.sort_by_key(|(id, _)| (*id).clone());

    for (rule_id, &(found, max)) in &sorted {
        let status = if found <= max { "pass" } else { "OVER" };
        let _ = writeln!(err, "ratchet: {} {} ({}/{})", rule_id, status, found, max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Severity;
    use std::path::PathBuf;

    fn make_result(violations: Vec<Violation>) -> ScanResult {
        ScanResult {
            violations,
            files_scanned: 5,
            rules_loaded: 2,
            ratchet_counts: HashMap::new(),
        }
    }

    fn make_violation(
        file: &str,
        line: usize,
        col: usize,
        severity: Severity,
        rule_id: &str,
        message: &str,
    ) -> Violation {
        Violation {
            rule_id: rule_id.to_string(),
            severity,
            file: PathBuf::from(file),
            line: Some(line),
            column: Some(col),
            message: message.to_string(),
            suggest: None,
            source_line: None,
        }
    }

    #[test]
    fn compact_single_error() {
        let result = make_result(vec![make_violation(
            "src/Foo.tsx",
            12,
            24,
            Severity::Error,
            "dark-mode",
            "bg-white missing dark variant",
        )]);
        let mut out = Vec::new();
        let mut err = Vec::new();
        write_compact(&result, &mut out, &mut err);

        let stdout = String::from_utf8(out).unwrap();
        assert_eq!(
            stdout,
            "src/Foo.tsx:12:24: error[dark-mode] bg-white missing dark variant\n"
        );
    }

    #[test]
    fn compact_mixed_severities() {
        let result = make_result(vec![
            make_violation("a.ts", 1, 1, Severity::Error, "r1", "err msg"),
            make_violation("b.ts", 5, 10, Severity::Warning, "r2", "warn msg"),
        ]);
        let mut out = Vec::new();
        let mut err = Vec::new();
        write_compact(&result, &mut out, &mut err);

        let stdout = String::from_utf8(out).unwrap();
        assert!(stdout.contains("a.ts:1:1: error[r1] err msg\n"));
        assert!(stdout.contains("b.ts:5:10: warning[r2] warn msg\n"));

        let stderr = String::from_utf8(err).unwrap();
        assert!(stderr.contains("1 error, 1 warning"));
    }

    #[test]
    fn compact_no_violations() {
        let result = make_result(vec![]);
        let mut out = Vec::new();
        let mut err = Vec::new();
        write_compact(&result, &mut out, &mut err);

        let stdout = String::from_utf8(out).unwrap();
        assert!(stdout.is_empty());

        let stderr = String::from_utf8(err).unwrap();
        assert!(stderr.contains("No violations found"));
    }

    #[test]
    fn compact_ratchet_on_stderr() {
        let mut result = make_result(vec![]);
        result
            .ratchet_counts
            .insert("legacy-api".to_string(), (3, 5));
        let mut out = Vec::new();
        let mut err = Vec::new();
        write_compact(&result, &mut out, &mut err);

        let stderr = String::from_utf8(err).unwrap();
        assert!(stderr.contains("ratchet: legacy-api pass (3/5)"));
    }

    #[test]
    fn github_single_warning() {
        let result = make_result(vec![make_violation(
            "src/Foo.tsx",
            15,
            8,
            Severity::Warning,
            "theme-tokens",
            "raw color class",
        )]);
        let mut out = Vec::new();
        let mut err = Vec::new();
        write_github(&result, &mut out, &mut err);

        let stdout = String::from_utf8(out).unwrap();
        assert_eq!(
            stdout,
            "::warning file=src/Foo.tsx,line=15,col=8,title=theme-tokens::raw color class\n"
        );
    }

    #[test]
    fn github_missing_column_omits_col() {
        let v = Violation {
            rule_id: "test".to_string(),
            severity: Severity::Error,
            file: PathBuf::from("a.ts"),
            line: Some(3),
            column: None,
            message: "msg".to_string(),
            suggest: None,
            source_line: None,
        };
        let result = make_result(vec![v]);
        let mut out = Vec::new();
        let mut err = Vec::new();
        write_github(&result, &mut out, &mut err);

        let stdout = String::from_utf8(out).unwrap();
        assert_eq!(stdout, "::error file=a.ts,line=3,title=test::msg\n");
        assert!(!stdout.contains("col="));
    }

    #[test]
    fn github_ratchet_over_budget() {
        let mut result = make_result(vec![]);
        result
            .ratchet_counts
            .insert("legacy-api".to_string(), (10, 5));
        let mut out = Vec::new();
        let mut err = Vec::new();
        write_github(&result, &mut out, &mut err);

        let stdout = String::from_utf8(out).unwrap();
        assert!(stdout.contains("::error title=ratchet-legacy-api"));
        assert!(stdout.contains("10 found, max 5"));
    }

    #[test]
    fn github_ratchet_pass_is_silent() {
        let mut result = make_result(vec![]);
        result
            .ratchet_counts
            .insert("legacy-api".to_string(), (3, 5));
        let mut out = Vec::new();
        let mut err = Vec::new();
        write_github(&result, &mut out, &mut err);

        let stdout = String::from_utf8(out).unwrap();
        assert!(stdout.is_empty());
    }
}
