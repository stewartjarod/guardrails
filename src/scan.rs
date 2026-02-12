use crate::cli::toml_config::TomlConfig;
use crate::presets::{self, PresetError};
use crate::rules::factory::{self, FactoryError};
use crate::rules::{Rule, ScanContext, Violation};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug)]
pub enum ScanError {
    ConfigRead(std::io::Error),
    ConfigParse(toml::de::Error),
    GlobParse(globset::Error),
    RuleFactory(FactoryError),
    Preset(PresetError),
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScanError::ConfigRead(e) => write!(f, "failed to read config: {}", e),
            ScanError::ConfigParse(e) => write!(f, "failed to parse config: {}", e),
            ScanError::GlobParse(e) => write!(f, "invalid glob pattern: {}", e),
            ScanError::RuleFactory(e) => write!(f, "failed to build rule: {}", e),
            ScanError::Preset(e) => write!(f, "preset error: {}", e),
        }
    }
}

impl std::error::Error for ScanError {}

pub struct ScanResult {
    pub violations: Vec<Violation>,
    pub files_scanned: usize,
    pub rules_loaded: usize,
    /// For each ratchet rule: (found_count, max_count).
    pub ratchet_counts: HashMap<String, (usize, usize)>,
}

#[derive(Debug, Serialize)]
pub struct BaselineEntry {
    pub rule_id: String,
    pub pattern: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct BaselineResult {
    pub entries: Vec<BaselineEntry>,
    pub files_scanned: usize,
}

/// Run a full scan: parse config, build rules, walk files, collect violations.
pub fn run_scan(config_path: &Path, target_paths: &[PathBuf]) -> Result<ScanResult, ScanError> {
    // 1. Read and parse TOML config
    let config_text = fs::read_to_string(config_path).map_err(ScanError::ConfigRead)?;
    let toml_config: TomlConfig = toml::from_str(&config_text).map_err(ScanError::ConfigParse)?;

    // 2. Resolve presets and merge with user-defined rules
    let resolved_rules = presets::resolve_rules(
        &toml_config.guardrails.extends,
        &toml_config.rule,
    )
    .map_err(ScanError::Preset)?;

    // 3. Build exclude glob set
    // Include patterns are advisory for project-wide scanning; CLI-provided targets
    // override them (the user explicitly chose what to scan). Exclude patterns still
    // apply to skip directories like node_modules.
    let exclude_set = build_glob_set(&toml_config.guardrails.exclude)?;

    // 4. Build rules via factory, tracking ratchet metadata
    let mut rules: Vec<(Box<dyn Rule>, Option<GlobSet>)> = Vec::new();
    let mut ratchet_thresholds: HashMap<String, usize> = HashMap::new();

    for toml_rule in &resolved_rules {
        let rule_config = toml_rule.to_rule_config();
        let rule = factory::build_rule(&toml_rule.rule_type, &rule_config)
            .map_err(ScanError::RuleFactory)?;

        if toml_rule.rule_type == "ratchet" {
            if let Some(max) = toml_rule.max_count {
                ratchet_thresholds.insert(rule.id().to_string(), max);
            }
        }

        // Build per-rule glob if specified
        let rule_glob = if let Some(ref pattern) = rule.file_glob() {
            let gs = GlobSetBuilder::new()
                .add(Glob::new(pattern).map_err(ScanError::GlobParse)?)
                .build()
                .map_err(ScanError::GlobParse)?;
            Some(gs)
        } else {
            None
        };

        rules.push((rule, rule_glob));
    }

    let rules_loaded = rules.len();

    // 4. Walk target paths and collect files
    let files = collect_files(target_paths, &exclude_set);

    // 5. Run rules on each file
    let mut violations: Vec<Violation> = Vec::new();
    let mut files_scanned = 0;

    for file_path in &files {
        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue, // skip binary/unreadable files
        };

        files_scanned += 1;
        let ctx = ScanContext {
            file_path,
            content: &content,
        };

        for (rule, rule_glob) in &rules {
            // Apply per-rule glob filter
            if let Some(ref gs) = rule_glob {
                let file_str = file_path.to_string_lossy();
                let file_name = file_path.file_name().unwrap_or_default().to_string_lossy();
                if !gs.is_match(&*file_str) && !gs.is_match(&*file_name) {
                    continue;
                }
            }

            let mut file_violations = rule.check_file(&ctx);
            violations.append(&mut file_violations);
        }
    }

    // 6. Apply ratchet thresholds
    let ratchet_counts = apply_ratchet_thresholds(&mut violations, &ratchet_thresholds);

    Ok(ScanResult {
        violations,
        files_scanned,
        rules_loaded,
        ratchet_counts,
    })
}

/// Suppress ratchet violations that are within budget. Returns counts for display.
fn apply_ratchet_thresholds(
    violations: &mut Vec<Violation>,
    thresholds: &HashMap<String, usize>,
) -> HashMap<String, (usize, usize)> {
    if thresholds.is_empty() {
        return HashMap::new();
    }

    // Count violations per ratchet rule
    let mut counts: HashMap<String, usize> = HashMap::new();
    for v in violations.iter() {
        if thresholds.contains_key(&v.rule_id) {
            *counts.entry(v.rule_id.clone()).or_insert(0) += 1;
        }
    }

    // Build result map and determine which rules to suppress
    let mut result: HashMap<String, (usize, usize)> = HashMap::new();
    let mut suppress: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (rule_id, &max) in thresholds {
        let found = counts.get(rule_id).copied().unwrap_or(0);
        result.insert(rule_id.clone(), (found, max));
        if found <= max {
            suppress.insert(rule_id.clone());
        }
    }

    // Remove suppressed violations
    if !suppress.is_empty() {
        violations.retain(|v| !suppress.contains(&v.rule_id));
    }

    result
}

/// Run baseline counting: parse config, build only ratchet rules, count matches.
pub fn run_baseline(
    config_path: &Path,
    target_paths: &[PathBuf],
) -> Result<BaselineResult, ScanError> {
    let config_text = fs::read_to_string(config_path).map_err(ScanError::ConfigRead)?;
    let toml_config: TomlConfig = toml::from_str(&config_text).map_err(ScanError::ConfigParse)?;

    // Resolve presets and merge with user-defined rules
    let resolved_rules = presets::resolve_rules(
        &toml_config.guardrails.extends,
        &toml_config.rule,
    )
    .map_err(ScanError::Preset)?;

    let exclude_set = build_glob_set(&toml_config.guardrails.exclude)?;

    // Build only ratchet rules
    let mut rules: Vec<(Box<dyn Rule>, Option<GlobSet>, String)> = Vec::new();
    for toml_rule in &resolved_rules {
        if toml_rule.rule_type != "ratchet" {
            continue;
        }
        let rule_config = toml_rule.to_rule_config();
        let rule = factory::build_rule(&toml_rule.rule_type, &rule_config)
            .map_err(ScanError::RuleFactory)?;

        let pattern = toml_rule.pattern.clone().unwrap_or_default();

        let rule_glob = if let Some(ref pat) = rule.file_glob() {
            let gs = GlobSetBuilder::new()
                .add(Glob::new(pat).map_err(ScanError::GlobParse)?)
                .build()
                .map_err(ScanError::GlobParse)?;
            Some(gs)
        } else {
            None
        };

        rules.push((rule, rule_glob, pattern));
    }

    let files = collect_files(target_paths, &exclude_set);

    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut files_scanned = 0;

    for file_path in &files {
        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        files_scanned += 1;
        let ctx = ScanContext {
            file_path,
            content: &content,
        };

        for (rule, rule_glob, _) in &rules {
            if let Some(ref gs) = rule_glob {
                let file_str = file_path.to_string_lossy();
                let file_name = file_path.file_name().unwrap_or_default().to_string_lossy();
                if !gs.is_match(&*file_str) && !gs.is_match(&*file_name) {
                    continue;
                }
            }

            let violations = rule.check_file(&ctx);
            *counts.entry(rule.id().to_string()).or_insert(0) += violations.len();
        }
    }

    let entries: Vec<BaselineEntry> = rules
        .iter()
        .map(|(rule, _, pattern)| BaselineEntry {
            rule_id: rule.id().to_string(),
            pattern: pattern.clone(),
            count: counts.get(rule.id()).copied().unwrap_or(0),
        })
        .collect();

    Ok(BaselineResult {
        entries,
        files_scanned,
    })
}

fn collect_files(target_paths: &[PathBuf], exclude_set: &GlobSet) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    for target in target_paths {
        if target.is_file() {
            files.push(target.clone());
        } else {
            for entry in WalkDir::new(target).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    let path = entry.into_path();
                    let rel = path.strip_prefix(target).unwrap_or(&path);
                    if exclude_set.is_match(rel.to_string_lossy().as_ref()) {
                        continue;
                    }
                    files.push(path);
                }
            }
        }
    }
    files
}

fn build_glob_set(patterns: &[String]) -> Result<GlobSet, ScanError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).map_err(ScanError::GlobParse)?);
    }
    builder.build().map_err(ScanError::GlobParse)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Severity;

    fn make_violation(rule_id: &str) -> Violation {
        Violation {
            rule_id: rule_id.to_string(),
            severity: Severity::Error,
            file: PathBuf::from("test.ts"),
            line: Some(1),
            column: Some(1),
            message: "test".to_string(),
            suggest: None,
            source_line: None,
        }
    }

    #[test]
    fn ratchet_under_budget_suppresses() {
        let mut violations = vec![
            make_violation("ratchet-legacy"),
            make_violation("ratchet-legacy"),
            make_violation("other-rule"),
        ];
        let mut thresholds = HashMap::new();
        thresholds.insert("ratchet-legacy".to_string(), 5);

        let counts = apply_ratchet_thresholds(&mut violations, &thresholds);

        assert_eq!(violations.len(), 1); // only "other-rule" remains
        assert_eq!(violations[0].rule_id, "other-rule");
        assert_eq!(counts["ratchet-legacy"], (2, 5));
    }

    #[test]
    fn ratchet_over_budget_keeps_all() {
        let mut violations = vec![
            make_violation("ratchet-legacy"),
            make_violation("ratchet-legacy"),
            make_violation("ratchet-legacy"),
            make_violation("other-rule"),
        ];
        let mut thresholds = HashMap::new();
        thresholds.insert("ratchet-legacy".to_string(), 2);

        let counts = apply_ratchet_thresholds(&mut violations, &thresholds);

        assert_eq!(violations.len(), 4); // all kept
        assert_eq!(counts["ratchet-legacy"], (3, 2));
    }

    #[test]
    fn ratchet_exactly_at_budget_suppresses() {
        let mut violations = vec![
            make_violation("ratchet-legacy"),
            make_violation("ratchet-legacy"),
        ];
        let mut thresholds = HashMap::new();
        thresholds.insert("ratchet-legacy".to_string(), 2);

        let counts = apply_ratchet_thresholds(&mut violations, &thresholds);

        assert_eq!(violations.len(), 0); // suppressed (at budget)
        assert_eq!(counts["ratchet-legacy"], (2, 2));
    }

    #[test]
    fn no_ratchet_rules_is_noop() {
        let mut violations = vec![make_violation("other-rule")];
        let thresholds = HashMap::new();

        let counts = apply_ratchet_thresholds(&mut violations, &thresholds);

        assert_eq!(violations.len(), 1);
        assert!(counts.is_empty());
    }

    #[test]
    fn ratchet_zero_with_matches_keeps_all() {
        let mut violations = vec![make_violation("ratchet-zero")];
        let mut thresholds = HashMap::new();
        thresholds.insert("ratchet-zero".to_string(), 0);

        let counts = apply_ratchet_thresholds(&mut violations, &thresholds);

        assert_eq!(violations.len(), 1);
        assert_eq!(counts["ratchet-zero"], (1, 0));
    }

    #[test]
    fn ratchet_zero_no_matches_suppresses() {
        let mut violations: Vec<Violation> = vec![];
        let mut thresholds = HashMap::new();
        thresholds.insert("ratchet-zero".to_string(), 0);

        let counts = apply_ratchet_thresholds(&mut violations, &thresholds);

        assert!(violations.is_empty());
        assert_eq!(counts["ratchet-zero"], (0, 0));
    }
}
