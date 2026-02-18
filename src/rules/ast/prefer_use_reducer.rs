use crate::config::{RuleConfig, Severity};
use crate::rules::ast::{count_calls_in_scope, is_component_node, parse_file};
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};

/// Flags React components that have too many `useState` calls.
///
/// When a component accumulates many independent `useState` calls, it often
/// means the state values are related and would be better modelled as a single
/// `useReducer`.  The threshold is configurable via `max_count` (default 4).
pub struct PreferUseReducerRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
    max_count: usize,
}

impl PreferUseReducerRule {
    pub fn new(config: &RuleConfig) -> Result<Self, RuleBuildError> {
        Ok(Self {
            id: config.id.clone(),
            severity: config.severity,
            message: config.message.clone(),
            suggest: config.suggest.clone(),
            glob: config.glob.clone(),
            max_count: config.max_count.unwrap_or(4),
        })
    }
}

impl Rule for PreferUseReducerRule {
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

impl PreferUseReducerRule {
    fn visit(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        ctx: &ScanContext,
        violations: &mut Vec<Violation>,
    ) {
        if is_component_node(&node, source) {
            let count = count_calls_in_scope(node, source, "useState");
            if count >= self.max_count {
                let line = node.start_position().row;
                violations.push(Violation {
                    rule_id: self.id.clone(),
                    severity: self.severity,
                    file: ctx.file_path.to_path_buf(),
                    line: Some(line + 1),
                    column: Some(1),
                    message: self.message.clone(),
                    suggest: self.suggest.clone(),
                    source_line: ctx.content.lines().nth(line).map(String::from),
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
            id: "prefer-use-reducer".into(),
            severity: Severity::Warning,
            message: format!("Component has {}+ useState calls", max_count),
            suggest: Some("Consider useReducer for related state".into()),
            glob: Some("**/*.tsx".into()),
            max_count: Some(max_count),
            ..Default::default()
        }
    }

    fn check(rule: &PreferUseReducerRule, content: &str) -> Vec<Violation> {
        let ctx = ScanContext {
            file_path: Path::new("test.tsx"),
            content,
        };
        rule.check_file(&ctx)
    }

    #[test]
    fn under_threshold_no_violation() {
        let rule = PreferUseReducerRule::new(&make_config(4)).unwrap();
        let content = "\
function MyComponent() {
  const [a, setA] = useState(0);
  const [b, setB] = useState('');
  const [c, setC] = useState(false);
  return <div>{a}{b}{c}</div>;
}";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn at_threshold_triggers() {
        let rule = PreferUseReducerRule::new(&make_config(4)).unwrap();
        let content = "\
function MyComponent() {
  const [a, setA] = useState(0);
  const [b, setB] = useState('');
  const [c, setC] = useState(false);
  const [d, setD] = useState(null);
  return <div />;
}";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, Some(1));
    }

    #[test]
    fn over_threshold_triggers() {
        let rule = PreferUseReducerRule::new(&make_config(3)).unwrap();
        let content = "\
function Form() {
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [phone, setPhone] = useState('');
  const [address, setAddress] = useState('');
  return <form />;
}";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn arrow_function_component() {
        let rule = PreferUseReducerRule::new(&make_config(2)).unwrap();
        let content = "\
const MyComponent = () => {
  const [a, setA] = useState(0);
  const [b, setB] = useState(0);
  return <div />;
};";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn nested_component_counted_separately() {
        let rule = PreferUseReducerRule::new(&make_config(3)).unwrap();
        // Outer has 2 useState, Inner has 2 useState — neither reaches 3
        let content = "\
function Outer() {
  const [a, setA] = useState(0);
  const [b, setB] = useState(0);
  function Inner() {
    const [c, setC] = useState(0);
    const [d, setD] = useState(0);
    return <span />;
  }
  return <Inner />;
}";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn lowercase_function_ignored() {
        let rule = PreferUseReducerRule::new(&make_config(2)).unwrap();
        let content = "\
function useMyHook() {
  const [a, setA] = useState(0);
  const [b, setB] = useState(0);
  const [c, setC] = useState(0);
  return [a, b, c];
}";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn default_max_count() {
        let config = RuleConfig {
            id: "test".into(),
            severity: Severity::Warning,
            message: "test".into(),
            ..Default::default()
        };
        let rule = PreferUseReducerRule::new(&config).unwrap();
        assert_eq!(rule.max_count, 4);
    }

    #[test]
    fn non_tsx_file_skipped() {
        let rule = PreferUseReducerRule::new(&make_config(2)).unwrap();
        let ctx = ScanContext {
            file_path: Path::new("test.rs"),
            content: "fn main() {}",
        };
        assert!(rule.check_file(&ctx).is_empty());
    }
}
