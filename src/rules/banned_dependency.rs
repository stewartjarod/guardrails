use crate::config::{RuleConfig, Severity};
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};
use std::collections::HashSet;

/// Checks `package.json` (or other manifest) files for banned packages
/// in dependency sections.
///
/// Scans `dependencies`, `devDependencies`, `peerDependencies`, and
/// `optionalDependencies` for packages that should not be used.
#[derive(Debug)]
pub struct BannedDependencyRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
    packages: HashSet<String>,
    manifest: String,
}

/// JSON dependency sections to check.
const DEP_SECTIONS: &[&str] = &[
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
];

impl BannedDependencyRule {
    pub fn new(config: &RuleConfig) -> Result<Self, RuleBuildError> {
        if config.packages.is_empty() {
            return Err(RuleBuildError::MissingField(
                config.id.clone(),
                "packages",
            ));
        }

        let packages: HashSet<String> = config.packages.iter().cloned().collect();
        let manifest = config
            .manifest
            .as_deref()
            .unwrap_or("package.json")
            .to_string();

        // Build a glob that only matches the manifest filename
        let glob = config
            .glob
            .clone()
            .or_else(|| Some(format!("**/{}", manifest)));

        Ok(Self {
            id: config.id.clone(),
            severity: config.severity,
            message: config.message.clone(),
            suggest: config.suggest.clone(),
            glob,
            packages,
            manifest,
        })
    }
}

impl Rule for BannedDependencyRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn file_glob(&self) -> Option<&str> {
        self.glob.as_deref()
    }

    fn check_file(&self, ctx: &ScanContext) -> Vec<Violation> {
        // Only process files that match the manifest name
        let file_name = ctx
            .file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if file_name != self.manifest {
            return Vec::new();
        }

        let json: serde_json::Value = match serde_json::from_str(ctx.content) {
            Ok(v) => v,
            Err(_) => return Vec::new(), // skip malformed JSON
        };

        let mut violations = Vec::new();

        for section in DEP_SECTIONS {
            if let Some(deps) = json.get(section).and_then(|v| v.as_object()) {
                for pkg_name in deps.keys() {
                    if self.packages.contains(pkg_name) {
                        // Find the line number by searching for the package name in the raw text
                        let line_num = find_line_number(ctx.content, pkg_name, section);

                        violations.push(Violation {
                            rule_id: self.id.clone(),
                            severity: self.severity,
                            file: ctx.file_path.to_path_buf(),
                            line: line_num,
                            column: None,
                            message: format!(
                                "{}: '{}' in {}",
                                self.message, pkg_name, section
                            ),
                            suggest: self.suggest.clone(),
                            source_line: line_num.and_then(|n| {
                                ctx.content.lines().nth(n - 1).map(|l| l.to_string())
                            }),
                            fix: None,
                        });
                    }
                }
            }
        }

        violations
    }
}

/// Find the line number of a package name within a specific dependency section.
fn find_line_number(content: &str, pkg_name: &str, section: &str) -> Option<usize> {
    let needle = format!(r#""{}""#, pkg_name);
    let section_needle = format!(r#""{}""#, section);

    let mut in_section = false;
    let mut brace_depth = 0;

    for (idx, line) in content.lines().enumerate() {
        if line.contains(&section_needle) {
            in_section = true;
            brace_depth = 0;
            continue;
        }

        if in_section {
            brace_depth += line.matches('{').count() as i32;
            brace_depth -= line.matches('}').count() as i32;

            if brace_depth < 0 {
                in_section = false;
                continue;
            }

            if line.contains(&needle) {
                return Some(idx + 1);
            }
        }
    }

    // Fallback: search anywhere in the file
    for (idx, line) in content.lines().enumerate() {
        if line.contains(&needle) {
            return Some(idx + 1);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_rule(packages: Vec<&str>) -> BannedDependencyRule {
        let config = RuleConfig {
            id: "test-banned-dep".into(),
            severity: Severity::Error,
            message: "banned dependency".into(),
            suggest: Some("remove this package".into()),
            packages: packages.into_iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        BannedDependencyRule::new(&config).unwrap()
    }

    fn check(rule: &BannedDependencyRule, content: &str) -> Vec<Violation> {
        let ctx = ScanContext {
            file_path: Path::new("package.json"),
            content,
        };
        rule.check_file(&ctx)
    }

    #[test]
    fn detects_dependency() {
        let rule = make_rule(vec!["bootstrap"]);
        let content = r#"{
  "dependencies": {
    "bootstrap": "^5.0.0",
    "react": "^18.0.0"
  }
}"#;
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("bootstrap"));
        assert!(violations[0].message.contains("dependencies"));
    }

    #[test]
    fn detects_dev_dependency() {
        let rule = make_rule(vec!["bootstrap"]);
        let content = r#"{
  "devDependencies": {
    "bootstrap": "^5.0.0"
  }
}"#;
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("devDependencies"));
    }

    #[test]
    fn detects_multiple_banned_packages() {
        let rule = make_rule(vec!["bootstrap", "@mui/material"]);
        let content = r#"{
  "dependencies": {
    "bootstrap": "^5.0.0",
    "@mui/material": "^5.0.0",
    "react": "^18.0.0"
  }
}"#;
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn no_match_on_safe_deps() {
        let rule = make_rule(vec!["bootstrap"]);
        let content = r#"{
  "dependencies": {
    "react": "^18.0.0",
    "next": "^14.0.0"
  }
}"#;
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn skips_non_manifest_files() {
        let rule = make_rule(vec!["bootstrap"]);
        let ctx = ScanContext {
            file_path: Path::new("src/component.tsx"),
            content: r#"{"dependencies": {"bootstrap": "^5.0.0"}}"#,
        };
        let violations = rule.check_file(&ctx);
        assert!(violations.is_empty());
    }

    #[test]
    fn skips_malformed_json() {
        let rule = make_rule(vec!["bootstrap"]);
        let violations = check(&rule, "not valid json {{{");
        assert!(violations.is_empty());
    }

    #[test]
    fn finds_line_numbers() {
        let rule = make_rule(vec!["bootstrap"]);
        let content = r#"{
  "name": "my-app",
  "dependencies": {
    "bootstrap": "^5.0.0"
  }
}"#;
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, Some(4));
    }

    #[test]
    fn missing_packages_error() {
        let config = RuleConfig {
            id: "test".into(),
            severity: Severity::Error,
            message: "test".into(),
            ..Default::default()
        };
        let err = BannedDependencyRule::new(&config).unwrap_err();
        assert!(matches!(err, RuleBuildError::MissingField(_, "packages")));
    }

    #[test]
    fn custom_manifest_name() {
        let config = RuleConfig {
            id: "test-banned-dep".into(),
            severity: Severity::Error,
            message: "banned dependency".into(),
            packages: vec!["bootstrap".to_string()],
            manifest: Some("bower.json".to_string()),
            ..Default::default()
        };
        let rule = BannedDependencyRule::new(&config).unwrap();

        // Should not match package.json
        let ctx = ScanContext {
            file_path: Path::new("package.json"),
            content: r#"{"dependencies": {"bootstrap": "^5.0.0"}}"#,
        };
        assert!(rule.check_file(&ctx).is_empty());

        // Should match bower.json
        let ctx = ScanContext {
            file_path: Path::new("bower.json"),
            content: r#"{"dependencies": {"bootstrap": "^5.0.0"}}"#,
        };
        assert_eq!(rule.check_file(&ctx).len(), 1);
    }
}
