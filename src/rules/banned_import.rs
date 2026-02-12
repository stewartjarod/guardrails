use crate::config::{RuleConfig, Severity};
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};
use regex::Regex;

/// Scans source files for import/require statements referencing banned packages.
///
/// Detects patterns like:
/// - `import ... from 'pkg'`
/// - `import 'pkg'`
/// - `require('pkg')`
/// - Subpath imports like `import ... from 'lodash/debounce'`
///
/// Uses word-boundary matching to avoid false positives (e.g., `moment` won't
/// match `momentum`).
#[derive(Debug)]
pub struct BannedImportRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
    #[allow(dead_code)]
    packages: Vec<String>,
    import_re: Regex,
}

impl BannedImportRule {
    pub fn new(config: &RuleConfig) -> Result<Self, RuleBuildError> {
        if config.packages.is_empty() {
            return Err(RuleBuildError::MissingField(
                config.id.clone(),
                "packages",
            ));
        }

        // Build a regex that matches import/require of any banned package.
        // Escaped package names joined with | to form alternatives.
        let escaped: Vec<String> = config
            .packages
            .iter()
            .map(|p| regex::escape(p))
            .collect();
        let pkg_group = escaped.join("|");

        // Match:
        //   import ... from ['"]pkg['"]      (named/default import)
        //   import ['"]pkg['"]               (side-effect import)
        //   require\(['"]pkg['"]\)           (CommonJS require)
        //   export ... from ['"]pkg['"]      (re-exports)
        // Also match subpath imports: pkg/subpath
        let pattern = format!(
            r#"(?:import\s+.*?\s+from\s+|import\s+|export\s+.*?\s+from\s+|require\s*\(\s*)['"]({})(?:/[^'"]*)?['"]"#,
            pkg_group
        );

        let import_re = Regex::new(&pattern)
            .map_err(|e| RuleBuildError::InvalidRegex(config.id.clone(), e))?;

        let default_glob = "**/*.{ts,tsx,js,jsx,mjs,cjs}".to_string();

        Ok(Self {
            id: config.id.clone(),
            severity: config.severity,
            message: config.message.clone(),
            suggest: config.suggest.clone(),
            glob: config.glob.clone().or(Some(default_glob)),
            packages: config.packages.clone(),
            import_re,
        })
    }
}

impl Rule for BannedImportRule {
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
        let mut violations = Vec::new();

        for (line_idx, line) in ctx.content.lines().enumerate() {
            for cap in self.import_re.captures_iter(line) {
                let matched_pkg = cap.get(1).unwrap().as_str();
                let full_match = cap.get(0).unwrap();

                violations.push(Violation {
                    rule_id: self.id.clone(),
                    severity: self.severity,
                    file: ctx.file_path.to_path_buf(),
                    line: Some(line_idx + 1),
                    column: Some(full_match.start() + 1),
                    message: format!("{}: '{}'", self.message, matched_pkg),
                    suggest: self.suggest.clone(),
                    source_line: Some(line.to_string()),
                    fix: None,
                });
            }
        }

        violations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_rule(packages: Vec<&str>) -> BannedImportRule {
        let config = RuleConfig {
            id: "test-banned-import".into(),
            severity: Severity::Error,
            message: "banned import".into(),
            suggest: Some("use an alternative".into()),
            packages: packages.into_iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        BannedImportRule::new(&config).unwrap()
    }

    fn check(rule: &BannedImportRule, content: &str) -> Vec<Violation> {
        let ctx = ScanContext {
            file_path: Path::new("test.ts"),
            content,
        };
        rule.check_file(&ctx)
    }

    #[test]
    fn detects_named_import() {
        let rule = make_rule(vec!["moment"]);
        let violations = check(&rule, r#"import moment from 'moment';"#);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("moment"));
    }

    #[test]
    fn detects_destructured_import() {
        let rule = make_rule(vec!["moment"]);
        let violations = check(&rule, r#"import { format } from "moment";"#);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn detects_side_effect_import() {
        let rule = make_rule(vec!["styled-components"]);
        let violations = check(&rule, r#"import 'styled-components';"#);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn detects_require() {
        let rule = make_rule(vec!["moment"]);
        let violations = check(&rule, r#"const moment = require('moment');"#);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn detects_subpath_import() {
        let rule = make_rule(vec!["lodash"]);
        let violations = check(&rule, r#"import debounce from 'lodash/debounce';"#);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("lodash"));
    }

    #[test]
    fn no_false_positive_on_similar_names() {
        let rule = make_rule(vec!["moment"]);
        let violations = check(&rule, r#"import momentum from 'momentum';"#);
        assert!(violations.is_empty(), "should not match 'momentum' when banning 'moment'");
    }

    #[test]
    fn detects_scoped_package() {
        let rule = make_rule(vec!["@emotion/styled"]);
        let violations = check(&rule, r#"import styled from '@emotion/styled';"#);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn detects_export_from() {
        let rule = make_rule(vec!["moment"]);
        let violations = check(&rule, r#"export { default } from 'moment';"#);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn no_match_on_safe_imports() {
        let rule = make_rule(vec!["moment"]);
        let violations = check(&rule, r#"import { format } from 'date-fns';"#);
        assert!(violations.is_empty());
    }

    #[test]
    fn multiple_banned_packages() {
        let rule = make_rule(vec!["styled-components", "@emotion/styled", "@emotion/css"]);
        let content = r#"import styled from 'styled-components';
import { css } from '@emotion/css';
import { jsx } from '@emotion/react';"#;
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn missing_packages_error() {
        let config = RuleConfig {
            id: "test".into(),
            severity: Severity::Error,
            message: "test".into(),
            ..Default::default()
        };
        let err = BannedImportRule::new(&config).unwrap_err();
        assert!(matches!(err, RuleBuildError::MissingField(_, "packages")));
    }

    #[test]
    fn default_glob_set() {
        let rule = make_rule(vec!["moment"]);
        assert_eq!(rule.file_glob(), Some("**/*.{ts,tsx,js,jsx,mjs,cjs}"));
    }
}
