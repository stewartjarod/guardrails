use crate::config::{RuleConfig, Severity};
use crate::rules::ast::{is_component_node, parse_file};
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};

/// Flags `new RegExp()` calls inside React component function bodies.
///
/// Creating a RegExp inside a component body means it's re-compiled on every
/// render.  The rule checks that the `new RegExp()` call is inside a component
/// function and NOT at module scope, and not inside `useMemo`/`useCallback`.
pub struct NoRegexpInRenderRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
}

impl NoRegexpInRenderRule {
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

impl Rule for NoRegexpInRenderRule {
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
        // Find components, then search within them for new RegExp()
        self.find_components(tree.root_node(), source, ctx, &mut violations);
        violations
    }
}

impl NoRegexpInRenderRule {
    fn find_components(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        ctx: &ScanContext,
        violations: &mut Vec<Violation>,
    ) {
        if is_component_node(&node, source) {
            // Search this component's body for new RegExp() calls
            self.find_new_regexp(node, source, ctx, violations, false);
            return; // Don't recurse into nested components from here
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.find_components(child, source, ctx, violations);
            }
        }
    }

    fn find_new_regexp(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        ctx: &ScanContext,
        violations: &mut Vec<Violation>,
        in_memo: bool,
    ) {
        // Check if we're entering a useMemo/useCallback
        let entering_memo = !in_memo && is_memo_or_callback_call(&node, source);
        let current_in_memo = in_memo || entering_memo;

        if node.kind() == "new_expression" {
            if let Some(constructor) = node.child_by_field_name("constructor") {
                if let Ok(name) = constructor.utf8_text(source) {
                    if name == "RegExp" && !current_in_memo {
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
                }
            }
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                // Skip nested component definitions
                if is_component_node(&child, source) {
                    continue;
                }
                self.find_new_regexp(child, source, ctx, violations, current_in_memo);
            }
        }
    }
}

fn is_memo_or_callback_call(node: &tree_sitter::Node, source: &[u8]) -> bool {
    if node.kind() == "call_expression" {
        if let Some(func) = node.child_by_field_name("function") {
            if func.kind() == "identifier" {
                if let Ok(name) = func.utf8_text(source) {
                    return name == "useMemo" || name == "useCallback";
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_rule() -> NoRegexpInRenderRule {
        NoRegexpInRenderRule::new(&RuleConfig {
            id: "no-regexp-in-render".into(),
            severity: Severity::Warning,
            message: "new RegExp() in component body re-compiles every render".into(),
            suggest: Some("Move to module scope or useMemo".into()),
            glob: Some("**/*.{tsx,jsx}".into()),
            ..Default::default()
        })
        .unwrap()
    }

    fn check(content: &str) -> Vec<Violation> {
        let rule = make_rule();
        let ctx = ScanContext {
            file_path: Path::new("test.tsx"),
            content,
        };
        rule.check_file(&ctx)
    }

    #[test]
    fn new_regexp_in_component_flags() {
        let content = "\
function MyComponent({ pattern }) {
  const re = new RegExp(pattern);
  return <div>{re.test('abc') ? 'yes' : 'no'}</div>;
}";
        assert_eq!(check(content).len(), 1);
    }

    #[test]
    fn new_regexp_at_module_scope_no_violation() {
        let content = "\
const EMAIL_RE = new RegExp('[^@]+@[^@]+');
function MyComponent() {
  return <div>{EMAIL_RE.test('a@b') ? 'yes' : 'no'}</div>;
}";
        assert!(check(content).is_empty());
    }

    #[test]
    fn new_regexp_in_use_memo_no_violation() {
        let content = "\
function MyComponent({ pattern }) {
  const re = useMemo(() => new RegExp(pattern), [pattern]);
  return <div>{re.test('abc') ? 'yes' : 'no'}</div>;
}";
        assert!(check(content).is_empty());
    }

    #[test]
    fn new_regexp_in_use_callback_no_violation() {
        let content = "\
function MyComponent({ pattern }) {
  const test = useCallback(() => {
    const re = new RegExp(pattern);
    return re.test('abc');
  }, [pattern]);
  return <div />;
}";
        assert!(check(content).is_empty());
    }

    #[test]
    fn non_component_function_no_violation() {
        let content = "\
function helper(pattern) {
  return new RegExp(pattern);
}";
        assert!(check(content).is_empty());
    }

    #[test]
    fn arrow_component_flags() {
        let content = "\
const MyComponent = () => {
  const re = new RegExp('\\\\d+');
  return <div />;
};";
        assert_eq!(check(content).len(), 1);
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
}
