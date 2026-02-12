use crate::config::{RuleConfig, Severity};
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};
use regex::Regex;

/// Scans files line-by-line for a literal string or regex match.
///
/// Useful for banning code patterns like `style={{`, `console.log(`, `// @ts-ignore`, etc.
/// When `regex` is true in the config, the pattern is treated as a regular expression.
#[derive(Debug)]
pub struct BannedPatternRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
    pattern: String,
    compiled_regex: Option<Regex>,
}

impl BannedPatternRule {
    pub fn new(config: &RuleConfig) -> Result<Self, RuleBuildError> {
        let pattern = config
            .pattern
            .as_ref()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| RuleBuildError::MissingField(config.id.clone(), "pattern"))?
            .clone();

        let compiled_regex = if config.regex {
            let re = Regex::new(&pattern)
                .map_err(|e| RuleBuildError::InvalidRegex(config.id.clone(), e))?;
            Some(re)
        } else {
            None
        };

        Ok(Self {
            id: config.id.clone(),
            severity: config.severity,
            message: config.message.clone(),
            suggest: config.suggest.clone(),
            glob: config.glob.clone(),
            pattern,
            compiled_regex,
        })
    }
}

impl Rule for BannedPatternRule {
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
            if let Some(ref re) = self.compiled_regex {
                // Regex mode: report each match
                for m in re.find_iter(line) {
                    violations.push(Violation {
                        rule_id: self.id.clone(),
                        severity: self.severity,
                        file: ctx.file_path.to_path_buf(),
                        line: Some(line_idx + 1),
                        column: Some(m.start() + 1),
                        message: self.message.clone(),
                        suggest: self.suggest.clone(),
                        source_line: Some(line.to_string()),
                        fix: None,
                    });
                }
            } else {
                // Literal mode: find all occurrences
                let pat = self.pattern.as_str();
                let pat_len = pat.len();
                let mut search_start = 0;
                while let Some(pos) = line[search_start..].find(pat) {
                    let col = search_start + pos;
                    violations.push(Violation {
                        rule_id: self.id.clone(),
                        severity: self.severity,
                        file: ctx.file_path.to_path_buf(),
                        line: Some(line_idx + 1),
                        column: Some(col + 1),
                        message: self.message.clone(),
                        suggest: self.suggest.clone(),
                        source_line: Some(line.to_string()),
                        fix: None,
                    });
                    search_start = col + pat_len;
                }
            }
        }

        violations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_config(pattern: &str, regex: bool) -> RuleConfig {
        RuleConfig {
            id: "test-banned-pattern".into(),
            severity: Severity::Warning,
            message: "banned pattern found".into(),
            suggest: Some("remove this pattern".into()),
            pattern: Some(pattern.to_string()),
            regex,
            ..Default::default()
        }
    }

    fn check(rule: &BannedPatternRule, content: &str) -> Vec<Violation> {
        let ctx = ScanContext {
            file_path: Path::new("test.tsx"),
            content,
        };
        rule.check_file(&ctx)
    }

    #[test]
    fn literal_match() {
        let config = make_config("style={{", false);
        let rule = BannedPatternRule::new(&config).unwrap();
        let violations = check(&rule, r#"<div style={{ color: "red" }}>"#);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, Some(1));
        assert_eq!(violations[0].column, Some(6));
    }

    #[test]
    fn literal_multiple_matches_per_line() {
        let config = make_config("TODO", false);
        let rule = BannedPatternRule::new(&config).unwrap();
        let violations = check(&rule, "// TODO fix this TODO");
        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0].column, Some(4));
        assert_eq!(violations[1].column, Some(18));
    }

    #[test]
    fn literal_no_match() {
        let config = make_config("style={{", false);
        let rule = BannedPatternRule::new(&config).unwrap();
        let violations = check(&rule, r#"<div className="bg-white">"#);
        assert!(violations.is_empty());
    }

    #[test]
    fn literal_multiline() {
        let config = make_config("console.log(", false);
        let rule = BannedPatternRule::new(&config).unwrap();
        let content = "const x = 1;\nconsole.log(x);\nconst y = 2;";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, Some(2));
        assert_eq!(violations[0].column, Some(1));
    }

    #[test]
    fn regex_match() {
        let config = make_config(r"console\.(log|debug)\(", true);
        let rule = BannedPatternRule::new(&config).unwrap();
        let content = "console.log('hi');\nconsole.debug('x');\nconsole.error('e');";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0].line, Some(1));
        assert_eq!(violations[1].line, Some(2));
    }

    #[test]
    fn regex_no_match() {
        let config = make_config(r"console\.log\(", true);
        let rule = BannedPatternRule::new(&config).unwrap();
        let violations = check(&rule, "console.error('e');");
        assert!(violations.is_empty());
    }

    #[test]
    fn invalid_regex_error() {
        let config = make_config(r"(unclosed", true);
        let err = BannedPatternRule::new(&config).unwrap_err();
        assert!(matches!(err, RuleBuildError::InvalidRegex(_, _)));
    }

    #[test]
    fn missing_pattern_error() {
        let config = RuleConfig {
            id: "test".into(),
            severity: Severity::Warning,
            message: "test".into(),
            ..Default::default()
        };
        let err = BannedPatternRule::new(&config).unwrap_err();
        assert!(matches!(err, RuleBuildError::MissingField(_, "pattern")));
    }

    #[test]
    fn empty_pattern_error() {
        let config = make_config("", false);
        let err = BannedPatternRule::new(&config).unwrap_err();
        assert!(matches!(err, RuleBuildError::MissingField(_, "pattern")));
    }

    #[test]
    fn violation_metadata() {
        let config = make_config("style={{", false);
        let rule = BannedPatternRule::new(&config).unwrap();
        let violations = check(&rule, r#"<div style={{ color: "red" }}>"#);
        assert_eq!(violations[0].rule_id, "test-banned-pattern");
        assert_eq!(violations[0].severity, Severity::Warning);
        assert_eq!(violations[0].message, "banned pattern found");
        assert_eq!(violations[0].suggest.as_deref(), Some("remove this pattern"));
        assert!(violations[0].source_line.is_some());
    }
}
