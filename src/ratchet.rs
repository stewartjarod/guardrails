use crate::cli::toml_config::{TomlConfig, TomlRule};
use crate::rules::factory;
use crate::rules::ScanContext;
use crate::scan::{self, BaselineResult};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum RatchetError {
    ConfigRead(std::io::Error),
    ConfigParse(toml::de::Error),
    Scan(scan::ScanError),
    RuleNotFound(String),
    RuleAlreadyExists(String),
    BaselineRead(std::io::Error),
    BaselineParse(String),
    NoDecrease {
        rule_id: String,
        current: usize,
        max_count: usize,
    },
}

impl fmt::Display for RatchetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RatchetError::ConfigRead(e) => write!(f, "failed to read config: {}", e),
            RatchetError::ConfigParse(e) => write!(f, "failed to parse config: {}", e),
            RatchetError::Scan(e) => write!(f, "scan failed: {}", e),
            RatchetError::RuleNotFound(id) => {
                write!(f, "no ratchet rule found with id '{}'", id)
            }
            RatchetError::RuleAlreadyExists(id) => {
                write!(f, "a rule with id '{}' already exists", id)
            }
            RatchetError::BaselineRead(e) => write!(f, "failed to read baseline: {}", e),
            RatchetError::BaselineParse(e) => write!(f, "failed to parse baseline JSON: {}", e),
            RatchetError::NoDecrease {
                rule_id,
                current,
                max_count,
            } => {
                write!(
                    f,
                    "rule '{}': current count ({}) has not decreased below max_count ({})",
                    rule_id, current, max_count
                )
            }
        }
    }
}

impl std::error::Error for RatchetError {}

/// Convert a pattern string into a valid rule ID slug.
pub fn slugify(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    for ch in pattern.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
        } else if !result.is_empty() && !result.ends_with('-') {
            result.push('-');
        }
    }
    // Trim trailing dash
    while result.ends_with('-') {
        result.pop();
    }
    if result.is_empty() {
        "ratchet-rule".to_string()
    } else {
        result
    }
}

/// Count occurrences of a pattern across files using the scan infrastructure.
fn count_pattern(
    config_path: &Path,
    pattern: &str,
    glob: &str,
    regex: bool,
    paths: &[PathBuf],
) -> Result<usize, RatchetError> {
    // Read config to get exclude patterns
    let config_text = fs::read_to_string(config_path).map_err(RatchetError::ConfigRead)?;
    let toml_config: TomlConfig =
        toml::from_str(&config_text).map_err(RatchetError::ConfigParse)?;

    let exclude_set = scan::build_glob_set(&toml_config.baseline.exclude)
        .map_err(RatchetError::Scan)?;

    // Build a temporary ratchet rule (max_count is required by RatchetRule)
    let toml_rule = TomlRule {
        id: "__ratchet_count__".into(),
        rule_type: "ratchet".into(),
        pattern: Some(pattern.to_string()),
        glob: Some(glob.to_string()),
        regex,
        max_count: Some(usize::MAX),
        message: "counting".into(),
        ..Default::default()
    };

    let rule_config = toml_rule.to_rule_config();
    let rule = factory::build_rule("ratchet", &rule_config)
        .map_err(|e| RatchetError::Scan(scan::ScanError::RuleFactory(e)))?;

    let rule_glob = if let Some(ref pat) = rule.file_glob() {
        Some(scan::build_glob_set_from_pattern(pat).map_err(RatchetError::Scan)?)
    } else {
        None
    };

    let files = scan::collect_files(paths, &exclude_set);

    let mut count = 0usize;
    for file_path in &files {
        if let Some(ref gs) = rule_glob {
            let file_str = file_path.to_string_lossy();
            let file_name = file_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            if !gs.is_match(&*file_str) && !gs.is_match(&*file_name) {
                continue;
            }
        }
        if let Ok(content) = fs::read_to_string(file_path) {
            let ctx = ScanContext {
                file_path,
                content: &content,
            };
            count += rule.check_file(&ctx).len();
        }
    }

    Ok(count)
}

/// Specification for a ratchet rule to be appended to config.
struct RatchetRuleSpec {
    id: String,
    pattern: String,
    glob: String,
    regex: bool,
    max_count: usize,
    message: String,
}

/// Append a `[[rule]]` block for a ratchet rule to the end of config text.
fn append_ratchet_rule(config_text: &str, spec: &RatchetRuleSpec) -> String {
    let mut result = config_text.to_string();
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result.push('\n');
    result.push_str("[[rule]]\n");
    result.push_str(&format!("id = \"{}\"\n", spec.id));
    result.push_str("type = \"ratchet\"\n");
    result.push_str("severity = \"warning\"\n");
    result.push_str(&format!("pattern = \"{}\"\n", escape_toml_string(&spec.pattern)));
    if spec.regex {
        result.push_str("regex = true\n");
    }
    if spec.glob != "**/*" {
        result.push_str(&format!("glob = \"{}\"\n", escape_toml_string(&spec.glob)));
    }
    result.push_str(&format!("max_count = {}\n", spec.max_count));
    result.push_str(&format!(
        "message = \"{}\"\n",
        escape_toml_string(&spec.message)
    ));
    result
}

/// Escape a string for use in a TOML double-quoted value.
fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Update the `max_count` value for a specific rule ID in config text.
/// Also updates the message if it contains an "N remaining" pattern.
fn update_max_count(config_text: &str, rule_id: &str, new_max: usize) -> Result<String, RatchetError> {
    let lines: Vec<&str> = config_text.lines().collect();
    let mut result_lines: Vec<String> = Vec::with_capacity(lines.len());

    let mut in_target_rule = false;
    let mut found = false;
    let mut updated_max = false;

    for line in &lines {
        // Detect `[[rule]]` boundaries
        let trimmed = line.trim();
        if trimmed == "[[rule]]" {
            // If we were in the target rule but never updated max_count, error
            if in_target_rule && !updated_max {
                return Err(RatchetError::RuleNotFound(rule_id.to_string()));
            }
            in_target_rule = false;
            result_lines.push(line.to_string());
            continue;
        }

        // Check if this is the target rule by its id
        if !in_target_rule && !found {
            if let Some(id_val) = extract_toml_string_value(trimmed, "id") {
                if id_val == rule_id {
                    in_target_rule = true;
                    found = true;
                }
            }
        }

        // Update max_count line if in target rule
        if in_target_rule && trimmed.starts_with("max_count") {
            result_lines.push(format!("max_count = {}", new_max));
            updated_max = true;
            continue;
        }

        // Update message if it contains "N remaining"
        if in_target_rule && trimmed.starts_with("message") {
            let updated = update_remaining_in_message(line, new_max);
            result_lines.push(updated);
            continue;
        }

        result_lines.push(line.to_string());
    }

    if !found {
        return Err(RatchetError::RuleNotFound(rule_id.to_string()));
    }

    let mut output = result_lines.join("\n");
    // Preserve trailing newline if original had one
    if config_text.ends_with('\n') && !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}

/// Extract a TOML string value from a line like `key = "value"`.
fn extract_toml_string_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let line = line.trim();
    if !line.starts_with(key) {
        return None;
    }
    let rest = line[key.len()..].trim();
    if !rest.starts_with('=') {
        return None;
    }
    let rest = rest[1..].trim();
    if rest.starts_with('"') && rest.len() >= 2 {
        let end = rest[1..].find('"')?;
        Some(&rest[1..1 + end])
    } else {
        None
    }
}

/// If a message line contains "N remaining", update N to new_max.
fn update_remaining_in_message(line: &str, new_max: usize) -> String {
    // Match pattern like: message = "... 42 remaining ..."
    let re = regex::Regex::new(r"\d+ remaining").unwrap();
    if re.is_match(line) {
        re.replace(line, &format!("{} remaining", new_max))
            .to_string()
    } else {
        line.to_string()
    }
}

/// Entry point dispatching to subcommands.
pub fn run(command: crate::cli::RatchetCommands) -> Result<(), RatchetError> {
    match command {
        crate::cli::RatchetCommands::Add {
            pattern,
            id,
            glob,
            regex,
            message,
            config,
            paths,
        } => run_add(&config, &pattern, id.as_deref(), &glob, regex, message.as_deref(), &paths),

        crate::cli::RatchetCommands::Down {
            rule_id,
            config,
            paths,
        } => run_down(&config, &rule_id, &paths),

        crate::cli::RatchetCommands::From { baseline, config } => {
            run_from(&config, &baseline)
        }
    }
}

fn run_add(
    config_path: &Path,
    pattern: &str,
    id: Option<&str>,
    glob: &str,
    regex: bool,
    message: Option<&str>,
    paths: &[PathBuf],
) -> Result<(), RatchetError> {
    let config_text = fs::read_to_string(config_path).map_err(RatchetError::ConfigRead)?;
    let toml_config: TomlConfig =
        toml::from_str(&config_text).map_err(RatchetError::ConfigParse)?;

    let rule_id = id
        .map(|s| s.to_string())
        .unwrap_or_else(|| slugify(pattern));

    // Check for duplicate ID
    if toml_config.rule.iter().any(|r| r.id == rule_id) {
        return Err(RatchetError::RuleAlreadyExists(rule_id));
    }

    let count = count_pattern(config_path, pattern, glob, regex, paths)?;

    let msg = message
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{} remaining", count));

    let spec = RatchetRuleSpec {
        id: rule_id.clone(),
        pattern: pattern.to_string(),
        glob: glob.to_string(),
        regex,
        max_count: count,
        message: msg,
    };

    let updated = append_ratchet_rule(&config_text, &spec);
    fs::write(config_path, &updated).map_err(RatchetError::ConfigRead)?;

    eprintln!(
        "\x1b[32m✓\x1b[0m Added ratchet rule '{}' (max_count = {}, {} current occurrence{})",
        rule_id,
        count,
        count,
        if count == 1 { "" } else { "s" }
    );

    Ok(())
}

fn run_down(
    config_path: &Path,
    rule_id: &str,
    paths: &[PathBuf],
) -> Result<(), RatchetError> {
    let config_text = fs::read_to_string(config_path).map_err(RatchetError::ConfigRead)?;
    let toml_config: TomlConfig =
        toml::from_str(&config_text).map_err(RatchetError::ConfigParse)?;

    // Find the existing ratchet rule
    let toml_rule = toml_config
        .rule
        .iter()
        .find(|r| r.id == rule_id && r.rule_type == "ratchet")
        .ok_or_else(|| RatchetError::RuleNotFound(rule_id.to_string()))?;

    let old_max = toml_rule.max_count.unwrap_or(0);
    let pattern = toml_rule
        .pattern
        .as_deref()
        .unwrap_or("");
    let glob = toml_rule.glob.as_deref().unwrap_or("**/*");
    let regex = toml_rule.regex;

    let current = count_pattern(config_path, pattern, glob, regex, paths)?;

    if current >= old_max {
        return Err(RatchetError::NoDecrease {
            rule_id: rule_id.to_string(),
            current,
            max_count: old_max,
        });
    }

    let updated = update_max_count(&config_text, rule_id, current)?;
    fs::write(config_path, &updated).map_err(RatchetError::ConfigRead)?;

    eprintln!(
        "\x1b[32m✓\x1b[0m Ratcheted down '{}': {} → {}",
        rule_id, old_max, current
    );

    Ok(())
}

fn run_from(config_path: &Path, baseline_path: &Path) -> Result<(), RatchetError> {
    let baseline_text =
        fs::read_to_string(baseline_path).map_err(RatchetError::BaselineRead)?;
    let baseline: BaselineResult = serde_json::from_str(&baseline_text)
        .map_err(|e| RatchetError::BaselineParse(e.to_string()))?;

    let config_text = fs::read_to_string(config_path).map_err(RatchetError::ConfigRead)?;
    let toml_config: TomlConfig =
        toml::from_str(&config_text).map_err(RatchetError::ConfigParse)?;

    // Collect existing rule IDs to skip duplicates
    let existing_ids: std::collections::HashSet<&str> =
        toml_config.rule.iter().map(|r| r.id.as_str()).collect();

    let mut updated = config_text.clone();
    let mut added = 0usize;

    for entry in &baseline.entries {
        if existing_ids.contains(entry.rule_id.as_str()) {
            eprintln!(
                "\x1b[33m⚠\x1b[0m Skipping '{}': rule already exists",
                entry.rule_id
            );
            continue;
        }

        let spec = RatchetRuleSpec {
            id: entry.rule_id.clone(),
            pattern: entry.pattern.clone(),
            glob: "**/*".to_string(),
            regex: false,
            max_count: entry.count,
            message: format!("{} remaining", entry.count),
        };

        updated = append_ratchet_rule(&updated, &spec);
        added += 1;

        eprintln!(
            "  {} (max_count = {})",
            entry.rule_id, entry.count
        );
    }

    fs::write(config_path, &updated).map_err(RatchetError::ConfigRead)?;

    eprintln!(
        "\x1b[32m✓\x1b[0m Added {} ratchet rule{} from baseline",
        added,
        if added == 1 { "" } else { "s" }
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── slugify tests ──

    #[test]
    fn slugify_simple_pattern() {
        assert_eq!(slugify("console.log"), "console-log");
    }

    #[test]
    fn slugify_regex_pattern() {
        assert_eq!(slugify(r"console\.log"), "console-log");
    }

    #[test]
    fn slugify_complex_pattern() {
        assert_eq!(slugify("TODO|FIXME|HACK"), "todo-fixme-hack");
    }

    #[test]
    fn slugify_leading_special_chars() {
        assert_eq!(slugify("...hello"), "hello");
    }

    #[test]
    fn slugify_empty() {
        assert_eq!(slugify(""), "ratchet-rule");
    }

    #[test]
    fn slugify_all_special() {
        assert_eq!(slugify("..."), "ratchet-rule");
    }

    #[test]
    fn slugify_preserves_numbers() {
        assert_eq!(slugify("v2_api"), "v2-api");
    }

    // ── append_ratchet_rule tests ──

    #[test]
    fn append_generates_valid_toml() {
        let config = "[baseline]\n";
        let spec = RatchetRuleSpec {
            id: "no-console".into(),
            pattern: r"console\.log".into(),
            glob: "**/*.ts".into(),
            regex: true,
            max_count: 42,
            message: "42 remaining".into(),
        };

        let result = append_ratchet_rule(config, &spec);

        // Verify the result is valid TOML
        let parsed: TomlConfig = toml::from_str(&result).unwrap();
        assert_eq!(parsed.rule.len(), 1);
        assert_eq!(parsed.rule[0].id, "no-console");
        assert_eq!(parsed.rule[0].rule_type, "ratchet");
        assert_eq!(parsed.rule[0].pattern.as_deref(), Some(r"console\.log"));
        assert_eq!(parsed.rule[0].max_count, Some(42));
        assert_eq!(parsed.rule[0].glob.as_deref(), Some("**/*.ts"));
        assert!(parsed.rule[0].regex);
    }

    #[test]
    fn append_default_glob_omitted() {
        let config = "[baseline]\n";
        let spec = RatchetRuleSpec {
            id: "test".into(),
            pattern: "foo".into(),
            glob: "**/*".into(),
            regex: false,
            max_count: 5,
            message: "5 remaining".into(),
        };

        let result = append_ratchet_rule(config, &spec);
        assert!(!result.contains("glob = "));
    }

    #[test]
    fn append_escapes_quotes() {
        let config = "[baseline]\n";
        let spec = RatchetRuleSpec {
            id: "test".into(),
            pattern: r#"say "hello""#.into(),
            glob: "**/*".into(),
            regex: false,
            max_count: 1,
            message: r#"found "hello""#.into(),
        };

        let result = append_ratchet_rule(config, &spec);
        // Should parse without error
        let parsed: TomlConfig = toml::from_str(&result).unwrap();
        assert_eq!(parsed.rule[0].pattern.as_deref(), Some(r#"say "hello""#));
    }

    // ── update_max_count tests ──

    #[test]
    fn update_max_count_basic() {
        let config = r#"[baseline]

[[rule]]
id = "legacy-api"
type = "ratchet"
pattern = "legacyCall"
max_count = 42
message = "42 remaining"
"#;

        let result = update_max_count(config, "legacy-api", 10).unwrap();
        assert!(result.contains("max_count = 10"));
        assert!(result.contains("10 remaining"));
        assert!(!result.contains("max_count = 42"));
    }

    #[test]
    fn update_max_count_nonexistent_id() {
        let config = r#"[baseline]

[[rule]]
id = "legacy-api"
type = "ratchet"
max_count = 42
message = "test"
"#;

        let result = update_max_count(config, "nonexistent", 10);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RatchetError::RuleNotFound(_)));
    }

    #[test]
    fn update_max_count_multiple_rules() {
        let config = r#"[baseline]

[[rule]]
id = "rule-a"
type = "ratchet"
pattern = "a"
max_count = 100
message = "100 remaining"

[[rule]]
id = "rule-b"
type = "ratchet"
pattern = "b"
max_count = 200
message = "200 remaining"
"#;

        let result = update_max_count(config, "rule-b", 50).unwrap();
        // rule-a should be unchanged
        assert!(result.contains("max_count = 100"));
        assert!(result.contains("100 remaining"));
        // rule-b should be updated
        assert!(result.contains("max_count = 50"));
        assert!(result.contains("50 remaining"));
        assert!(!result.contains("max_count = 200"));
    }

    #[test]
    fn update_max_count_preserves_trailing_newline() {
        let config = "[baseline]\n\n[[rule]]\nid = \"test\"\ntype = \"ratchet\"\nmax_count = 5\nmessage = \"test\"\n";
        let result = update_max_count(config, "test", 3).unwrap();
        assert!(result.ends_with('\n'));
    }

    // ── extract_toml_string_value tests ──

    #[test]
    fn extract_value_basic() {
        assert_eq!(
            extract_toml_string_value(r#"id = "my-rule""#, "id"),
            Some("my-rule")
        );
    }

    #[test]
    fn extract_value_with_spaces() {
        assert_eq!(
            extract_toml_string_value(r#"id   =   "my-rule""#, "id"),
            Some("my-rule")
        );
    }

    #[test]
    fn extract_value_wrong_key() {
        assert_eq!(
            extract_toml_string_value(r#"type = "ratchet""#, "id"),
            None
        );
    }

    // ── escape_toml_string tests ──

    #[test]
    fn escape_backslash() {
        assert_eq!(escape_toml_string(r"console\.log"), r"console\\.log");
    }

    #[test]
    fn escape_quotes() {
        assert_eq!(escape_toml_string(r#"say "hi""#), r#"say \"hi\""#);
    }

    // ── update_remaining_in_message tests ──

    #[test]
    fn update_remaining_replaces_count() {
        let line = r#"message = "42 remaining""#;
        assert_eq!(
            update_remaining_in_message(line, 10),
            r#"message = "10 remaining""#
        );
    }

    #[test]
    fn update_remaining_no_match_passthrough() {
        let line = r#"message = "legacy API usage""#;
        assert_eq!(
            update_remaining_in_message(line, 10),
            r#"message = "legacy API usage""#
        );
    }

    // ── RatchetError Display tests ──

    #[test]
    fn error_display_config_read() {
        let err = RatchetError::ConfigRead(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not found",
        ));
        assert!(err.to_string().contains("failed to read config"));
    }

    #[test]
    fn error_display_rule_not_found() {
        let err = RatchetError::RuleNotFound("my-rule".into());
        assert!(err.to_string().contains("my-rule"));
    }

    #[test]
    fn error_display_rule_already_exists() {
        let err = RatchetError::RuleAlreadyExists("my-rule".into());
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn error_display_no_decrease() {
        let err = RatchetError::NoDecrease {
            rule_id: "test".into(),
            current: 10,
            max_count: 10,
        };
        let msg = err.to_string();
        assert!(msg.contains("10"));
        assert!(msg.contains("not decreased"));
    }

    #[test]
    fn error_display_baseline_parse() {
        let err = RatchetError::BaselineParse("bad json".into());
        assert!(err.to_string().contains("bad json"));
    }

    // ── Integration tests (filesystem) ──

    #[test]
    fn run_add_creates_rule_and_counts() {
        let dir = tempfile::tempdir().unwrap();

        let config = dir.path().join("baseline.toml");
        fs::write(&config, "[baseline]\n").unwrap();

        let src_dir = dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("app.ts"), "TODO: fix\nTODO: cleanup\nok\n").unwrap();

        run_add(&config, "TODO", None, "**/*", false, None, &[src_dir]).unwrap();

        let result = fs::read_to_string(&config).unwrap();
        let parsed: TomlConfig = toml::from_str(&result).unwrap();
        assert_eq!(parsed.rule.len(), 1);
        assert_eq!(parsed.rule[0].id, "todo");
        assert_eq!(parsed.rule[0].max_count, Some(2));
    }

    #[test]
    fn run_add_custom_id_and_message() {
        let dir = tempfile::tempdir().unwrap();

        let config = dir.path().join("baseline.toml");
        fs::write(&config, "[baseline]\n").unwrap();

        let src_dir = dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("app.ts"), "legacy()\n").unwrap();

        run_add(
            &config,
            "legacy",
            Some("my-legacy"),
            "**/*",
            false,
            Some("stop using legacy"),
            &[src_dir],
        )
        .unwrap();

        let result = fs::read_to_string(&config).unwrap();
        let parsed: TomlConfig = toml::from_str(&result).unwrap();
        assert_eq!(parsed.rule[0].id, "my-legacy");
        assert_eq!(parsed.rule[0].message, "stop using legacy");
    }

    #[test]
    fn run_add_duplicate_id_errors() {
        let dir = tempfile::tempdir().unwrap();

        let config = dir.path().join("baseline.toml");
        fs::write(
            &config,
            r#"[baseline]

[[rule]]
id = "existing"
type = "banned-pattern"
pattern = "x"
message = "m"
"#,
        )
        .unwrap();

        let result = run_add(
            &config,
            "x",
            Some("existing"),
            "**/*",
            false,
            None,
            &[dir.path().to_path_buf()],
        );
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RatchetError::RuleAlreadyExists(_)
        ));
    }

    #[test]
    fn run_add_with_regex() {
        let dir = tempfile::tempdir().unwrap();

        let config = dir.path().join("baseline.toml");
        fs::write(&config, "[baseline]\n").unwrap();

        let src_dir = dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("app.ts"), "console.log('a')\nconsole.warn('b')\n").unwrap();

        run_add(
            &config,
            r"console\.(log|warn)",
            None,
            "**/*",
            true,
            None,
            &[src_dir],
        )
        .unwrap();

        let result = fs::read_to_string(&config).unwrap();
        let parsed: TomlConfig = toml::from_str(&result).unwrap();
        assert_eq!(parsed.rule[0].max_count, Some(2));
        assert!(parsed.rule[0].regex);
    }

    #[test]
    fn run_down_lowers_max_count() {
        let dir = tempfile::tempdir().unwrap();

        let config = dir.path().join("baseline.toml");
        fs::write(
            &config,
            r#"[baseline]

[[rule]]
id = "legacy-api"
type = "ratchet"
pattern = "legacyCall"
max_count = 10
message = "10 remaining"
"#,
        )
        .unwrap();

        // Create source file with fewer matches than max_count
        let src_dir = dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("app.ts"), "legacyCall()\nlegacyCall()\nok\n").unwrap();

        run_down(&config, "legacy-api", &[src_dir]).unwrap();

        let result = fs::read_to_string(&config).unwrap();
        assert!(result.contains("max_count = 2"));
        assert!(result.contains("2 remaining"));
    }

    #[test]
    fn run_down_no_decrease_errors() {
        let dir = tempfile::tempdir().unwrap();

        let config = dir.path().join("baseline.toml");
        fs::write(
            &config,
            r#"[baseline]

[[rule]]
id = "legacy-api"
type = "ratchet"
pattern = "legacyCall"
max_count = 2
message = "test"
"#,
        )
        .unwrap();

        let src_dir = dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("app.ts"), "legacyCall()\nlegacyCall()\nlegacyCall()\n").unwrap();

        let result = run_down(&config, "legacy-api", &[src_dir]);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RatchetError::NoDecrease { .. }));
    }

    #[test]
    fn run_down_rule_not_found_errors() {
        let dir = tempfile::tempdir().unwrap();

        let config = dir.path().join("baseline.toml");
        fs::write(&config, "[baseline]\n").unwrap();

        let result = run_down(&config, "nonexistent", &[dir.path().to_path_buf()]);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RatchetError::RuleNotFound(_)));
    }

    #[test]
    fn run_from_creates_rules_from_baseline() {
        let dir = tempfile::tempdir().unwrap();

        let config = dir.path().join("baseline.toml");
        fs::write(&config, "[baseline]\n").unwrap();

        let baseline = dir.path().join("baseline.json");
        fs::write(
            &baseline,
            r#"{"entries":[{"rule_id":"todo","pattern":"TODO","count":5},{"rule_id":"fixme","pattern":"FIXME","count":3}],"files_scanned":10}"#,
        )
        .unwrap();

        run_from(&config, &baseline).unwrap();

        let result = fs::read_to_string(&config).unwrap();
        let parsed: TomlConfig = toml::from_str(&result).unwrap();
        assert_eq!(parsed.rule.len(), 2);
        assert_eq!(parsed.rule[0].id, "todo");
        assert_eq!(parsed.rule[0].max_count, Some(5));
        assert_eq!(parsed.rule[1].id, "fixme");
        assert_eq!(parsed.rule[1].max_count, Some(3));
    }

    #[test]
    fn run_from_skips_existing_rules() {
        let dir = tempfile::tempdir().unwrap();

        let config = dir.path().join("baseline.toml");
        fs::write(
            &config,
            r#"[baseline]

[[rule]]
id = "todo"
type = "ratchet"
pattern = "TODO"
max_count = 99
message = "existing"
"#,
        )
        .unwrap();

        let baseline = dir.path().join("baseline.json");
        fs::write(
            &baseline,
            r#"{"entries":[{"rule_id":"todo","pattern":"TODO","count":5},{"rule_id":"fixme","pattern":"FIXME","count":3}],"files_scanned":10}"#,
        )
        .unwrap();

        run_from(&config, &baseline).unwrap();

        let result = fs::read_to_string(&config).unwrap();
        let parsed: TomlConfig = toml::from_str(&result).unwrap();
        // Should have 2 rules: existing "todo" (unchanged) + new "fixme"
        assert_eq!(parsed.rule.len(), 2);
        assert_eq!(parsed.rule[0].id, "todo");
        assert_eq!(parsed.rule[0].max_count, Some(99)); // unchanged
        assert_eq!(parsed.rule[1].id, "fixme");
        assert_eq!(parsed.rule[1].max_count, Some(3));
    }

    #[test]
    fn run_from_invalid_baseline_errors() {
        let dir = tempfile::tempdir().unwrap();

        let config = dir.path().join("baseline.toml");
        fs::write(&config, "[baseline]\n").unwrap();

        let baseline = dir.path().join("baseline.json");
        fs::write(&baseline, "not valid json").unwrap();

        let result = run_from(&config, &baseline);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RatchetError::BaselineParse(_)));
    }

    #[test]
    fn run_from_missing_baseline_errors() {
        let dir = tempfile::tempdir().unwrap();

        let config = dir.path().join("baseline.toml");
        fs::write(&config, "[baseline]\n").unwrap();

        let result = run_from(&config, &dir.path().join("nonexistent.json"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RatchetError::BaselineRead(_)));
    }

    #[test]
    fn count_pattern_counts_matches() {
        let dir = tempfile::tempdir().unwrap();

        let config = dir.path().join("baseline.toml");
        fs::write(&config, "[baseline]\n").unwrap();

        let src_dir = dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("a.ts"), "TODO\nTODO\nok\n").unwrap();
        fs::write(src_dir.join("b.ts"), "TODO\n").unwrap();

        let count = count_pattern(&config, "TODO", "**/*", false, &[src_dir]).unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn count_pattern_respects_glob() {
        let dir = tempfile::tempdir().unwrap();

        let config = dir.path().join("baseline.toml");
        fs::write(&config, "[baseline]\n").unwrap();

        let src_dir = dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("a.ts"), "TODO\n").unwrap();
        fs::write(src_dir.join("b.rs"), "TODO\n").unwrap();

        let count = count_pattern(&config, "TODO", "**/*.ts", false, &[src_dir]).unwrap();
        assert_eq!(count, 1);
    }
}
