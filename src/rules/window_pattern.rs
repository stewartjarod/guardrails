use crate::config::{RuleConfig, Severity};
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};
use regex::Regex;

/// Enforces that when a trigger pattern appears, a required pattern
/// must also appear within a configurable window of lines.
///
/// Example: "every UPDATE/DELETE query must have an organizationId
/// within 80 lines" or "every async route handler must have try/catch
/// within 10 lines".
///
/// Config fields:
/// - `pattern` — trigger pattern (literal or regex)
/// - `condition_pattern` — required pattern that must appear nearby
/// - `max_count` — window size (number of lines to search after trigger)
/// - `regex` — whether patterns are regex
#[derive(Debug)]
pub struct WindowPatternRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
    trigger: String,
    trigger_re: Option<Regex>,
    required: String,
    required_re: Option<Regex>,
    window_size: usize,
}

impl WindowPatternRule {
    pub fn new(config: &RuleConfig) -> Result<Self, RuleBuildError> {
        let trigger = config
            .pattern
            .as_ref()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| RuleBuildError::MissingField(config.id.clone(), "pattern"))?
            .clone();

        let required = config
            .condition_pattern
            .as_ref()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                RuleBuildError::MissingField(config.id.clone(), "condition_pattern")
            })?
            .clone();

        let window_size = config.max_count.unwrap_or(10);

        let trigger_re = if config.regex {
            Some(
                Regex::new(&trigger)
                    .map_err(|e| RuleBuildError::InvalidRegex(config.id.clone(), e))?,
            )
        } else {
            None
        };

        let required_re = if config.regex {
            Some(
                Regex::new(&required)
                    .map_err(|e| RuleBuildError::InvalidRegex(config.id.clone(), e))?,
            )
        } else {
            None
        };

        Ok(Self {
            id: config.id.clone(),
            severity: config.severity,
            message: config.message.clone(),
            suggest: config.suggest.clone(),
            glob: config.glob.clone(),
            trigger,
            trigger_re,
            required,
            required_re,
            window_size,
        })
    }

    fn line_matches_trigger(&self, line: &str) -> bool {
        match &self.trigger_re {
            Some(re) => re.is_match(line),
            None => line.contains(&self.trigger),
        }
    }

    fn line_matches_required(&self, line: &str) -> bool {
        match &self.required_re {
            Some(re) => re.is_match(line),
            None => line.contains(&self.required),
        }
    }
}

impl Rule for WindowPatternRule {
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
        let lines: Vec<&str> = ctx.content.lines().collect();
        let total = lines.len();

        for (idx, line) in lines.iter().enumerate() {
            if !self.line_matches_trigger(line) {
                continue;
            }

            // Search within window for the required pattern
            let window_end = (idx + self.window_size + 1).min(total);
            let window_start = idx.saturating_sub(self.window_size);

            let found = (window_start..window_end)
                .any(|i| i != idx && self.line_matches_required(lines[i]));

            if !found {
                violations.push(Violation {
                    rule_id: self.id.clone(),
                    severity: self.severity,
                    file: ctx.file_path.to_path_buf(),
                    line: Some(idx + 1),
                    column: Some(1),
                    message: self.message.clone(),
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

    fn make_config(
        trigger: &str,
        required: &str,
        window: usize,
        regex: bool,
    ) -> RuleConfig {
        RuleConfig {
            id: "test-window".into(),
            severity: Severity::Error,
            message: "required pattern not found within window".into(),
            suggest: Some("add the required pattern nearby".into()),
            pattern: Some(trigger.to_string()),
            condition_pattern: Some(required.to_string()),
            max_count: Some(window),
            regex,
            ..Default::default()
        }
    }

    fn check(rule: &WindowPatternRule, content: &str) -> Vec<Violation> {
        let ctx = ScanContext {
            file_path: Path::new("test.ts"),
            content,
        };
        rule.check_file(&ctx)
    }

    #[test]
    fn required_present_within_window() {
        let config = make_config("DELETE FROM", "organizationId", 5, false);
        let rule = WindowPatternRule::new(&config).unwrap();
        let content = "DELETE FROM users\nWHERE organizationId = $1;";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn required_missing_within_window() {
        let config = make_config("DELETE FROM", "organizationId", 2, false);
        let rule = WindowPatternRule::new(&config).unwrap();
        let content = "DELETE FROM users\nWHERE id = $1\nAND active = true;";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, Some(1));
    }

    #[test]
    fn required_outside_window() {
        let config = make_config("DELETE FROM", "organizationId", 2, false);
        let rule = WindowPatternRule::new(&config).unwrap();
        let content = "DELETE FROM users\nWHERE id = $1\nAND active = true\nAND foo = bar\n-- organizationId check";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1, "organizationId is outside 2-line window");
    }

    #[test]
    fn regex_patterns() {
        let config = make_config(r"(UPDATE|DELETE)\s+FROM", r"organization[Ii]d", 5, true);
        let rule = WindowPatternRule::new(&config).unwrap();
        let content = "UPDATE FROM users\nSET name = 'foo'\nWHERE organizationId = $1;";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn multiple_triggers() {
        let config = make_config("DELETE FROM", "organizationId", 3, false);
        let rule = WindowPatternRule::new(&config).unwrap();
        let content = "DELETE FROM users WHERE organizationId = $1;\n\nDELETE FROM posts WHERE id = $1;";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1, "second DELETE is missing organizationId");
    }

    #[test]
    fn window_looks_before_trigger() {
        let config = make_config("DELETE FROM", "organizationId", 3, false);
        let rule = WindowPatternRule::new(&config).unwrap();
        let content = "const orgId = organizationId;\n\nDELETE FROM users WHERE id = orgId;";
        let violations = check(&rule, content);
        assert!(violations.is_empty(), "organizationId appears before the trigger within window");
    }

    #[test]
    fn missing_trigger_pattern_error() {
        let config = RuleConfig {
            id: "test".into(),
            severity: Severity::Error,
            message: "test".into(),
            condition_pattern: Some("required".into()),
            ..Default::default()
        };
        let err = WindowPatternRule::new(&config).unwrap_err();
        assert!(matches!(err, RuleBuildError::MissingField(_, "pattern")));
    }

    #[test]
    fn missing_condition_pattern_error() {
        let config = RuleConfig {
            id: "test".into(),
            severity: Severity::Error,
            message: "test".into(),
            pattern: Some("trigger".into()),
            ..Default::default()
        };
        let err = WindowPatternRule::new(&config).unwrap_err();
        assert!(matches!(err, RuleBuildError::MissingField(_, "condition_pattern")));
    }
}
