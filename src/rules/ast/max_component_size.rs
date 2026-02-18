use crate::config::{RuleConfig, Severity};
use crate::rules::ast::{is_component_node, parse_file};
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};

/// Flags React components that exceed a configurable line count.
///
/// Walks the AST for function declarations, arrow functions, and class
/// declarations with PascalCase names and reports any whose span
/// (end_row - start_row + 1) exceeds `max_count` (default 150).
pub struct MaxComponentSizeRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
    max_count: usize,
}

impl MaxComponentSizeRule {
    pub fn new(config: &RuleConfig) -> Result<Self, RuleBuildError> {
        Ok(Self {
            id: config.id.clone(),
            severity: config.severity,
            message: config.message.clone(),
            suggest: config.suggest.clone(),
            glob: config.glob.clone(),
            max_count: config.max_count.unwrap_or(150),
        })
    }
}

impl Rule for MaxComponentSizeRule {
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
        self.visit(tree.root_node(), source, ctx, &mut violations);
        violations
    }
}

impl MaxComponentSizeRule {
    fn visit(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        ctx: &ScanContext,
        violations: &mut Vec<Violation>,
    ) {
        if is_component_node(&node, source) {
            let start = node.start_position().row;
            let end = node.end_position().row;
            let line_count = end - start + 1;

            if line_count > self.max_count {
                violations.push(Violation {
                    rule_id: self.id.clone(),
                    severity: self.severity,
                    file: ctx.file_path.to_path_buf(),
                    line: Some(start + 1),
                    column: Some(1),
                    message: self.message.clone(),
                    suggest: self.suggest.clone(),
                    source_line: ctx.content.lines().nth(start).map(String::from),
                    fix: None,
                });
            }
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.visit(child, source, ctx, violations);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_config(max_count: usize) -> RuleConfig {
        RuleConfig {
            id: "max-component-size".into(),
            severity: Severity::Warning,
            message: format!("Component exceeds {} lines", max_count),
            suggest: Some("Split into smaller components".into()),
            glob: Some("**/*.tsx".into()),
            max_count: Some(max_count),
            ..Default::default()
        }
    }

    fn check(rule: &MaxComponentSizeRule, content: &str) -> Vec<Violation> {
        let ctx = ScanContext {
            file_path: Path::new("test.tsx"),
            content,
        };
        rule.check_file(&ctx)
    }

    #[test]
    fn small_component_no_violation() {
        let rule = MaxComponentSizeRule::new(&make_config(10)).unwrap();
        let content = "\
function Small() {
  return <div>hello</div>;
}";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn component_at_exact_limit() {
        let rule = MaxComponentSizeRule::new(&make_config(3)).unwrap();
        // 3 lines exactly — should NOT trigger (> not >=)
        let content = "\
function AtLimit() {
  return <div>hello</div>;
}";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn component_over_limit() {
        let rule = MaxComponentSizeRule::new(&make_config(3)).unwrap();
        // 4 lines — exceeds max_count of 3
        let content = "\
function TooLong() {
  const x = 1;
  const y = 2;
  return <div>{x}{y}</div>;
}";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, Some(1));
    }

    #[test]
    fn arrow_function_component() {
        let rule = MaxComponentSizeRule::new(&make_config(3)).unwrap();
        let content = "\
const Big = () => {
  const a = 1;
  const b = 2;
  return <div />;
};";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn class_component() {
        let rule = MaxComponentSizeRule::new(&make_config(3)).unwrap();
        let content = "\
class BigClass extends React.Component {
  render() {
    const x = 1;
    return <div />;
  }
}";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn lowercase_function_ignored() {
        let rule = MaxComponentSizeRule::new(&make_config(3)).unwrap();
        let content = "\
function helper() {
  const a = 1;
  const b = 2;
  const c = 3;
  return a + b + c;
}";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn non_tsx_file_skipped() {
        let rule = MaxComponentSizeRule::new(&make_config(1)).unwrap();
        let ctx = ScanContext {
            file_path: Path::new("test.rs"),
            content: "fn main() { println!(\"hello\"); }",
        };
        assert!(rule.check_file(&ctx).is_empty());
    }

    #[test]
    fn default_max_count() {
        let config = RuleConfig {
            id: "test".into(),
            severity: Severity::Warning,
            message: "too big".into(),
            ..Default::default()
        };
        let rule = MaxComponentSizeRule::new(&config).unwrap();
        assert_eq!(rule.max_count, 150);
    }
}
