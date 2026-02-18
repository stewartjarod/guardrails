use crate::config::{RuleConfig, Severity};
use crate::rules::ast::parse_file;
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};

/// Flags object or array literals inside `useEffect`/`useMemo`/`useCallback`
/// dependency arrays.
///
/// Object and array literals create new references on every render, defeating
/// the purpose of the dependency array.
pub struct NoObjectDepArrayRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
}

impl NoObjectDepArrayRule {
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

const HOOKS_WITH_DEPS: &[&str] = &["useEffect", "useMemo", "useCallback"];

impl Rule for NoObjectDepArrayRule {
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

impl NoObjectDepArrayRule {
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
                        if HOOKS_WITH_DEPS.contains(&name) {
                            if let Some(args) = node.child_by_field_name("arguments") {
                                // 2nd argument is the dep array (index 1)
                                if let Some(dep_array) = args.named_child(1) {
                                    if dep_array.kind() == "array" {
                                        self.check_dep_array(
                                            &dep_array, source, ctx, violations,
                                        );
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

    fn check_dep_array(
        &self,
        array_node: &tree_sitter::Node,
        _source: &[u8],
        ctx: &ScanContext,
        violations: &mut Vec<Violation>,
    ) {
        for i in 0..array_node.named_child_count() {
            if let Some(elem) = array_node.named_child(i) {
                if elem.kind() == "object" || elem.kind() == "array" {
                    let line = elem.start_position().row;
                    violations.push(Violation {
                        rule_id: self.id.clone(),
                        severity: self.severity,
                        file: ctx.file_path.to_path_buf(),
                        line: Some(line + 1),
                        column: Some(elem.start_position().column + 1),
                        message: self.message.clone(),
                        suggest: self.suggest.clone(),
                        source_line: ctx.content.lines().nth(line).map(String::from),
                        fix: None,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_rule() -> NoObjectDepArrayRule {
        NoObjectDepArrayRule::new(&RuleConfig {
            id: "no-object-dep-array".into(),
            severity: Severity::Warning,
            message: "Object/array literal in dependency array".into(),
            suggest: Some("Extract to useMemo or a ref".into()),
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
    fn object_literal_in_use_effect_dep_flags() {
        let content = "\
function MyComponent({ a }) {
  useEffect(() => {
    doSomething();
  }, [{ key: a }]);
  return <div />;
}";
        assert_eq!(check(content).len(), 1);
    }

    #[test]
    fn array_literal_in_use_memo_dep_flags() {
        let content = "\
function MyComponent({ items }) {
  const result = useMemo(() => compute(items), [[1, 2, 3]]);
  return <div />;
}";
        assert_eq!(check(content).len(), 1);
    }

    #[test]
    fn identifier_deps_no_violation() {
        let content = "\
function MyComponent({ data }) {
  useEffect(() => {
    process(data);
  }, [data]);
  return <div />;
}";
        assert!(check(content).is_empty());
    }

    #[test]
    fn empty_dep_array_no_violation() {
        let content = "\
function MyComponent() {
  useEffect(() => {
    init();
  }, []);
  return <div />;
}";
        assert!(check(content).is_empty());
    }

    #[test]
    fn use_callback_with_object_dep_flags() {
        let content = "\
function MyComponent({ config }) {
  const handler = useCallback(() => {
    process(config);
  }, [{ ...config }]);
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
