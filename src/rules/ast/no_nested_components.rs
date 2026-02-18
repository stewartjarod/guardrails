use crate::config::{RuleConfig, Severity};
use crate::rules::ast::{is_component_node, parse_file};
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};

/// Flags React component definitions that appear inside another component.
///
/// Nested component definitions cause the inner component to be re-created on
/// every render of the outer component, destroying and remounting its DOM and
/// losing all state.
pub struct NoNestedComponentsRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
}

impl NoNestedComponentsRule {
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

impl Rule for NoNestedComponentsRule {
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

impl NoNestedComponentsRule {
    fn visit(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        ctx: &ScanContext,
        violations: &mut Vec<Violation>,
    ) {
        if is_component_node(&node, source) && has_component_ancestor(&node, source) {
            let line = node.start_position().row;
            violations.push(Violation {
                rule_id: self.id.clone(),
                severity: self.severity,
                file: ctx.file_path.to_path_buf(),
                line: Some(line + 1),
                column: Some(node.start_position().column + 1),
                message: self.message.clone(),
                suggest: self.suggest.clone(),
                source_line: ctx.content.lines().nth(line).map(String::from),
                fix: None,
            });
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.visit(child, source, ctx, violations);
            }
        }
    }
}

/// Walk up the parent chain to see if any ancestor is a component node.
fn has_component_ancestor(node: &tree_sitter::Node, source: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if is_component_node(&parent, source) {
            return true;
        }
        current = parent.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_config() -> RuleConfig {
        RuleConfig {
            id: "no-nested-components".into(),
            severity: Severity::Error,
            message: "Nested component definition".into(),
            suggest: Some("Move the component to the module level".into()),
            glob: Some("**/*.tsx".into()),
            ..Default::default()
        }
    }

    fn check(rule: &NoNestedComponentsRule, content: &str) -> Vec<Violation> {
        let ctx = ScanContext {
            file_path: Path::new("test.tsx"),
            content,
        };
        rule.check_file(&ctx)
    }

    #[test]
    fn no_nesting_no_violation() {
        let rule = NoNestedComponentsRule::new(&make_config()).unwrap();
        let content = "\
function Outer() {
  return <div>hello</div>;
}

function Other() {
  return <span>world</span>;
}";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn nested_function_declaration() {
        let rule = NoNestedComponentsRule::new(&make_config()).unwrap();
        let content = "\
function Outer() {
  function Inner() {
    return <span>nested</span>;
  }
  return <div><Inner /></div>;
}";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, Some(2));
    }

    #[test]
    fn nested_arrow_component() {
        let rule = NoNestedComponentsRule::new(&make_config()).unwrap();
        let content = "\
const Outer = () => {
  const Inner = () => {
    return <div>nested</div>;
  };
  return <Inner />;
};";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn nested_inside_class_component() {
        let rule = NoNestedComponentsRule::new(&make_config()).unwrap();
        let content = "\
class Outer extends React.Component {
  render() {
    function Inner() {
      return <span />;
    }
    return <Inner />;
  }
}";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn lowercase_nested_function_ignored() {
        let rule = NoNestedComponentsRule::new(&make_config()).unwrap();
        let content = "\
function Outer() {
  function helper() {
    return 42;
  }
  return <div>{helper()}</div>;
}";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn deeply_nested_components() {
        let rule = NoNestedComponentsRule::new(&make_config()).unwrap();
        let content = "\
function A() {
  function B() {
    function C() {
      return <div />;
    }
    return <C />;
  }
  return <B />;
}";
        let violations = check(&rule, content);
        // B is nested in A, C is nested in B (and A)
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn non_tsx_file_skipped() {
        let rule = NoNestedComponentsRule::new(&make_config()).unwrap();
        let ctx = ScanContext {
            file_path: Path::new("test.rs"),
            content: "fn main() {}",
        };
        assert!(rule.check_file(&ctx).is_empty());
    }
}
