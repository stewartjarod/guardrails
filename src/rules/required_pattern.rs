use crate::config::{RuleConfig, Severity};
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};
use regex::Regex;

/// Ensures that files matching a glob contain a required pattern.
///
/// If a file matches the glob but does NOT contain the pattern, a violation
/// is emitted. Useful for enforcing conventions like "all page components
/// must have an ErrorBoundary" or "all API routes must validate input".
///
/// Optionally supports a `condition_pattern`: the required pattern is only
/// enforced if the condition pattern is present in the file.
#[derive(Debug)]
pub struct RequiredPatternRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
    pattern: String,
    compiled_regex: Option<Regex>,
    condition_pattern: Option<String>,
    condition_regex: Option<Regex>,
}

impl RequiredPatternRule {
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

        let condition_regex = if config.regex {
            config
                .condition_pattern
                .as_ref()
                .map(|p| {
                    Regex::new(p)
                        .map_err(|e| RuleBuildError::InvalidRegex(config.id.clone(), e))
                })
                .transpose()?
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
            condition_pattern: config.condition_pattern.clone(),
            condition_regex,
        })
    }

    fn content_contains_pattern(&self, content: &str) -> bool {
        if let Some(ref re) = self.compiled_regex {
            re.is_match(content)
        } else {
            content.contains(&self.pattern)
        }
    }

    fn content_matches_condition(&self, content: &str) -> bool {
        match (&self.condition_pattern, &self.condition_regex) {
            (Some(_), Some(re)) => re.is_match(content),
            (Some(pat), None) => content.contains(pat.as_str()),
            (None, _) => true, // no condition = always enforce
        }
    }
}

impl Rule for RequiredPatternRule {
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
        // If there's a condition pattern and the file doesn't match it, skip
        if !self.content_matches_condition(ctx.content) {
            return Vec::new();
        }

        // If the file contains the required pattern, it's fine
        if self.content_contains_pattern(ctx.content) {
            return Vec::new();
        }

        // File matches glob but is missing the required pattern
        vec![Violation {
            rule_id: self.id.clone(),
            severity: self.severity,
            file: ctx.file_path.to_path_buf(),
            line: Some(1),
            column: Some(1),
            message: self.message.clone(),
            suggest: self.suggest.clone(),
            source_line: ctx.content.lines().next().map(|l| l.to_string()),
            fix: None,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_config(pattern: &str, glob: Option<&str>) -> RuleConfig {
        RuleConfig {
            id: "test-required-pattern".into(),
            severity: Severity::Error,
            message: "required pattern missing".into(),
            suggest: Some("add the required pattern".into()),
            pattern: Some(pattern.to_string()),
            glob: glob.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    fn check(rule: &RequiredPatternRule, content: &str) -> Vec<Violation> {
        let ctx = ScanContext {
            file_path: Path::new("src/pages/Home.tsx"),
            content,
        };
        rule.check_file(&ctx)
    }

    #[test]
    fn pattern_present_no_violation() {
        let config = make_config("ErrorBoundary", Some("**/*.tsx"));
        let rule = RequiredPatternRule::new(&config).unwrap();
        let violations = check(&rule, "import { ErrorBoundary } from 'react-error-boundary';");
        assert!(violations.is_empty());
    }

    #[test]
    fn pattern_missing_one_violation() {
        let config = make_config("ErrorBoundary", Some("**/*.tsx"));
        let rule = RequiredPatternRule::new(&config).unwrap();
        let violations = check(&rule, "export default function Home() { return <div/>; }");
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "test-required-pattern");
    }

    #[test]
    fn regex_pattern_present() {
        let mut config = make_config(r"export\s+default", Some("**/*.tsx"));
        config.regex = true;
        let rule = RequiredPatternRule::new(&config).unwrap();
        let violations = check(&rule, "export default function App() {}");
        assert!(violations.is_empty());
    }

    #[test]
    fn regex_pattern_missing() {
        let mut config = make_config(r"export\s+default", Some("**/*.tsx"));
        config.regex = true;
        let rule = RequiredPatternRule::new(&config).unwrap();
        let violations = check(&rule, "const App = () => {};");
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn condition_pattern_met_required_missing() {
        let mut config = make_config("validateInput", Some("**/*.ts"));
        config.condition_pattern = Some("app.post(".to_string());
        let rule = RequiredPatternRule::new(&config).unwrap();
        let violations = check(&rule, "app.post('/api/users', handler);");
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn condition_pattern_met_required_present() {
        let mut config = make_config("validateInput", Some("**/*.ts"));
        config.condition_pattern = Some("app.post(".to_string());
        let rule = RequiredPatternRule::new(&config).unwrap();
        let violations = check(&rule, "app.post('/api', validateInput(schema), handler);");
        assert!(violations.is_empty());
    }

    #[test]
    fn condition_pattern_not_met_skips() {
        let mut config = make_config("validateInput", Some("**/*.ts"));
        config.condition_pattern = Some("app.post(".to_string());
        let rule = RequiredPatternRule::new(&config).unwrap();
        let violations = check(&rule, "app.get('/api/health', handler);");
        assert!(violations.is_empty());
    }

    #[test]
    fn missing_pattern_error() {
        let config = RuleConfig {
            id: "test".into(),
            severity: Severity::Error,
            message: "test".into(),
            ..Default::default()
        };
        let err = RequiredPatternRule::new(&config).unwrap_err();
        assert!(matches!(err, RuleBuildError::MissingField(_, "pattern")));
    }
}
