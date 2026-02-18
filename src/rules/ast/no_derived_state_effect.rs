use crate::config::{RuleConfig, Severity};
use crate::rules::ast::parse_file;
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};

/// Flags `useEffect` callbacks where the body contains ONLY `set*()` calls.
///
/// When every statement in a useEffect callback is a setState call, the effect
/// is computing derived state and should be replaced with `useMemo` or inline
/// computation during render.
pub struct NoDerivedStateEffectRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
}

impl NoDerivedStateEffectRule {
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

impl Rule for NoDerivedStateEffectRule {
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

impl NoDerivedStateEffectRule {
    fn visit(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        ctx: &ScanContext,
        violations: &mut Vec<Violation>,
    ) {
        if node.kind() == "call_expression" {
            if let Some(func) = node.child_by_field_name("function") {
                if func.kind() == "identifier" {
                    if let Ok(name) = func.utf8_text(source) {
                        if name == "useEffect" {
                            if let Some(args) = node.child_by_field_name("arguments") {
                                if let Some(callback) = args.named_child(0) {
                                    if self.is_only_set_state(&callback, source) {
                                        let line = node.start_position().row;
                                        violations.push(Violation {
                                            rule_id: self.id.clone(),
                                            severity: self.severity,
                                            file: ctx.file_path.to_path_buf(),
                                            line: Some(line + 1),
                                            column: Some(node.start_position().column + 1),
                                            message: self.message.clone(),
                                            suggest: self.suggest.clone(),
                                            source_line: ctx
                                                .content
                                                .lines()
                                                .nth(line)
                                                .map(String::from),
                                            fix: None,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.visit(child, source, ctx, violations);
            }
        }
    }

    /// Check if a callback body contains ONLY set* calls (at least one).
    fn is_only_set_state(&self, callback: &tree_sitter::Node, source: &[u8]) -> bool {
        // Find the statement_block in the callback
        let body = self.find_body(callback);
        let body = match body {
            Some(b) => b,
            None => {
                // Arrow function with expression body (no block): e.g. () => setFoo(x)
                // Check if the expression itself is a set* call
                if callback.kind() == "arrow_function" {
                    if let Some(body_node) = callback.child_by_field_name("body") {
                        if body_node.kind() == "call_expression" {
                            return is_set_state_call(&body_node, source);
                        }
                    }
                }
                return false;
            }
        };

        let mut count = 0;
        for i in 0..body.named_child_count() {
            if let Some(stmt) = body.named_child(i) {
                if stmt.kind() == "expression_statement" {
                    if let Some(expr) = stmt.named_child(0) {
                        if expr.kind() == "call_expression" && is_set_state_call(&expr, source) {
                            count += 1;
                            continue;
                        }
                    }
                }
                // Non-setState statement found
                return false;
            }
        }
        count > 0
    }

    fn find_body<'a>(&self, node: &'a tree_sitter::Node<'a>) -> Option<tree_sitter::Node<'a>> {
        match node.kind() {
            "arrow_function" | "function_expression" | "function" => {
                node.child_by_field_name("body")
                    .filter(|b| b.kind() == "statement_block")
            }
            _ => None,
        }
    }
}

fn is_set_state_call(node: &tree_sitter::Node, source: &[u8]) -> bool {
    if let Some(func) = node.child_by_field_name("function") {
        if func.kind() == "identifier" {
            if let Ok(name) = func.utf8_text(source) {
                if let Some(rest) = name.strip_prefix("set") {
                    return rest.starts_with(|c: char| c.is_ascii_uppercase());
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

    fn make_rule() -> NoDerivedStateEffectRule {
        NoDerivedStateEffectRule::new(&RuleConfig {
            id: "no-derived-state-effect".into(),
            severity: Severity::Warning,
            message: "useEffect that only calls setState is derived state".into(),
            suggest: Some("Compute during render with useMemo instead".into()),
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
    fn only_set_state_flags() {
        let content = "\
function MyComponent({ data }) {
  const [derived, setDerived] = useState('');
  useEffect(() => {
    setDerived(compute(data));
  }, [data]);
  return <div>{derived}</div>;
}";
        assert_eq!(check(content).len(), 1);
    }

    #[test]
    fn multiple_set_state_only_flags() {
        let content = "\
function MyComponent({ a, b }) {
  const [x, setX] = useState(0);
  const [y, setY] = useState(0);
  useEffect(() => {
    setX(a * 2);
    setY(b * 3);
  }, [a, b]);
  return <div />;
}";
        assert_eq!(check(content).len(), 1);
    }

    #[test]
    fn mixed_statements_no_violation() {
        let content = "\
function MyComponent({ id }) {
  const [data, setData] = useState(null);
  useEffect(() => {
    fetch('/api/' + id).then(r => r.json()).then(setData);
  }, [id]);
  return <div />;
}";
        assert!(check(content).is_empty());
    }

    #[test]
    fn set_state_plus_other_no_violation() {
        let content = "\
function MyComponent({ value }) {
  const [x, setX] = useState(0);
  useEffect(() => {
    console.log('updating');
    setX(value * 2);
  }, [value]);
  return <div />;
}";
        assert!(check(content).is_empty());
    }

    #[test]
    fn empty_effect_no_violation() {
        let content = "\
function MyComponent() {
  useEffect(() => {
  }, []);
  return <div />;
}";
        assert!(check(content).is_empty());
    }

    #[test]
    fn arrow_expression_body_set_state_flags() {
        let content = "\
function MyComponent({ data }) {
  const [x, setX] = useState(0);
  useEffect(() => setX(data.length), [data]);
  return <div />;
}";
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
