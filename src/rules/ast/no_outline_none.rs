use crate::config::{RuleConfig, Severity};
use crate::rules::ast::{collect_class_attributes, parse_file};
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};

/// Flags `outline-none` or `outline-0` in className attributes when there is
/// no companion `focus-visible:` ring class in the same attribute.
pub struct NoOutlineNoneRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
}

impl NoOutlineNoneRule {
    pub fn new(config: &RuleConfig) -> Result<Self, RuleBuildError> {
        Ok(Self {
            id: config.id.clone(),
            severity: config.severity,
            message: config.message.clone(),
            suggest: config.suggest.clone(),
            glob: config.glob.clone(),
        })
    }
}

impl Rule for NoOutlineNoneRule {
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
        let tree = match parse_file(ctx.file_path, ctx.content) {
            Some(t) => t,
            None => return violations,
        };
        let source = ctx.content.as_bytes();
        let attrs = collect_class_attributes(&tree, source);

        for fragments in &attrs {
            // Collect all tokens across all fragments in this attribute
            let mut all_tokens: Vec<&str> = Vec::new();
            for frag in fragments {
                for token in frag.value.split_whitespace() {
                    all_tokens.push(token);
                }
            }

            let has_outline_remove = all_tokens
                .iter()
                .any(|t| *t == "outline-none" || *t == "outline-0");
            if !has_outline_remove {
                continue;
            }

            let has_focus_visible_ring = all_tokens.iter().any(|t| {
                t.starts_with("focus-visible:ring") || t.starts_with("focus-visible:outline")
            });
            if has_focus_visible_ring {
                continue;
            }

            // Find the fragment containing the offending token for line/col
            for frag in fragments {
                for token in frag.value.split_whitespace() {
                    if token == "outline-none" || token == "outline-0" {
                        let col_offset = frag.value.find(token).unwrap_or(0);
                        let line = frag.line;
                        violations.push(Violation {
                            rule_id: self.id.clone(),
                            severity: self.severity,
                            file: ctx.file_path.to_path_buf(),
                            line: Some(line + 1),
                            column: Some(frag.col + col_offset + 1),
                            message: self.message.clone(),
                            suggest: self.suggest.clone(),
                            source_line: ctx.content.lines().nth(line).map(String::from),
                            fix: None,
                        });
                        break;
                    }
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

    fn make_rule() -> NoOutlineNoneRule {
        NoOutlineNoneRule::new(&RuleConfig {
            id: "no-outline-none".into(),
            severity: Severity::Warning,
            message: "outline-none removes the focus indicator".into(),
            suggest: Some("Use focus-visible:outline-none with a custom focus ring instead".into()),
            glob: Some("**/*.{tsx,jsx}".into()),
            ..Default::default()
        })
        .unwrap()
    }

    fn check(rule: &NoOutlineNoneRule, content: &str) -> Vec<Violation> {
        let ctx = ScanContext {
            file_path: Path::new("test.tsx"),
            content,
        };
        rule.check_file(&ctx)
    }

    #[test]
    fn outline_none_without_ring_flags() {
        let rule = make_rule();
        let violations = check(&rule, r#"function App() { return <div className="outline-none p-4" />; }"#);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "no-outline-none");
    }

    #[test]
    fn outline_none_with_focus_visible_ring_no_violation() {
        let rule = make_rule();
        let violations = check(
            &rule,
            r#"function App() { return <div className="outline-none focus-visible:ring-2" />; }"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn outline_none_with_focus_visible_outline_no_violation() {
        let rule = make_rule();
        let violations = check(
            &rule,
            r#"function App() { return <div className="outline-none focus-visible:outline-2" />; }"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn outline_0_without_ring_flags() {
        let rule = make_rule();
        let violations = check(&rule, r#"function App() { return <input className="outline-0 bg-white" />; }"#);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn inside_cn_call() {
        let rule = make_rule();
        let violations = check(
            &rule,
            r#"function App() { return <div className={cn("outline-none", "p-4")} />; }"#,
        );
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn inside_cn_call_with_ring() {
        let rule = make_rule();
        let violations = check(
            &rule,
            r#"function App() { return <div className={cn("outline-none", "focus-visible:ring-2")} />; }"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn non_tsx_skipped() {
        let rule = make_rule();
        let ctx = ScanContext {
            file_path: Path::new("test.rs"),
            content: "fn main() {}",
        };
        assert!(rule.check_file(&ctx).is_empty());
    }

    #[test]
    fn no_outline_classes_no_violation() {
        let rule = make_rule();
        let violations = check(&rule, r#"function App() { return <div className="bg-white p-4" />; }"#);
        assert!(violations.is_empty());
    }
}
