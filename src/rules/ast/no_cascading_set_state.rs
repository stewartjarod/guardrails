use crate::config::{RuleConfig, Severity};
use crate::rules::ast::parse_file;
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};

/// Flags `useEffect` callbacks that call too many setState functions.
///
/// Multiple `set*` calls inside a single `useEffect` often cause cascading
/// re-renders.  This rule counts calls to functions matching `set[A-Z]*`
/// within each `useEffect` callback body and flags when the count reaches
/// `max_count` (default 3).
pub struct NoCascadingSetStateRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
    max_count: usize,
}

impl NoCascadingSetStateRule {
    pub fn new(config: &RuleConfig) -> Result<Self, RuleBuildError> {
        Ok(Self {
            id: config.id.clone(),
            severity: config.severity,
            message: config.message.clone(),
            suggest: config.suggest.clone(),
            glob: config.glob.clone(),
            max_count: config.max_count.unwrap_or(3),
        })
    }
}

impl Rule for NoCascadingSetStateRule {
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

impl NoCascadingSetStateRule {
    fn visit(
        &self,
        node: tree_sitter::Node,
        source: &[u8],
        ctx: &ScanContext,
        violations: &mut Vec<Violation>,
    ) {
        // Look for useEffect(...) call expressions.
        if node.kind() == "call_expression" {
            if let Some(func) = node.child_by_field_name("function") {
                if func.kind() == "identifier" {
                    if let Ok(name) = func.utf8_text(source) {
                        if name == "useEffect" {
                            if let Some(args) = node.child_by_field_name("arguments") {
                                if let Some(callback) = args.named_child(0) {
                                    let count = count_set_state_calls(callback, source);
                                    if count >= self.max_count {
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
}

/// Count calls to `set*` functions (PascalCase after `set`) in a subtree.
fn count_set_state_calls(node: tree_sitter::Node, source: &[u8]) -> usize {
    let mut count = 0;

    if node.kind() == "call_expression" {
        if let Some(func) = node.child_by_field_name("function") {
            if func.kind() == "identifier" {
                if let Ok(name) = func.utf8_text(source) {
                    if is_set_state_name(name) {
                        count += 1;
                    }
                }
            }
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            count += count_set_state_calls(child, source);
        }
    }

    count
}

/// Returns true for names like `setFoo`, `setIsLoading`, etc.
fn is_set_state_name(name: &str) -> bool {
    if let Some(rest) = name.strip_prefix("set") {
        rest.starts_with(|c: char| c.is_ascii_uppercase())
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_config(max_count: usize) -> RuleConfig {
        RuleConfig {
            id: "no-cascading-set-state".into(),
            severity: Severity::Warning,
            message: format!("useEffect has {}+ setState calls", max_count),
            suggest: Some("Consider useReducer".into()),
            glob: Some("**/*.tsx".into()),
            max_count: Some(max_count),
            ..Default::default()
        }
    }

    fn check(rule: &NoCascadingSetStateRule, content: &str) -> Vec<Violation> {
        let ctx = ScanContext {
            file_path: Path::new("test.tsx"),
            content,
        };
        rule.check_file(&ctx)
    }

    #[test]
    fn under_threshold_no_violation() {
        let rule = NoCascadingSetStateRule::new(&make_config(3)).unwrap();
        let content = "\
function MyComponent() {
  const [a, setA] = useState(0);
  const [b, setB] = useState(0);
  useEffect(() => {
    setA(1);
    setB(2);
  }, []);
  return <div />;
}";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn at_threshold_triggers() {
        let rule = NoCascadingSetStateRule::new(&make_config(3)).unwrap();
        let content = "\
function MyComponent() {
  const [a, setA] = useState(0);
  const [b, setB] = useState(0);
  const [c, setC] = useState(0);
  useEffect(() => {
    setA(1);
    setB(2);
    setC(3);
  }, []);
  return <div />;
}";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn over_threshold_triggers() {
        let rule = NoCascadingSetStateRule::new(&make_config(2)).unwrap();
        let content = "\
function MyComponent() {
  useEffect(() => {
    setName('test');
    setEmail('test@test.com');
    setPhone('123');
  }, []);
  return <div />;
}";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn separate_effects_counted_independently() {
        let rule = NoCascadingSetStateRule::new(&make_config(3)).unwrap();
        // Two effects each with 2 setState — neither reaches 3
        let content = "\
function MyComponent() {
  useEffect(() => {
    setA(1);
    setB(2);
  }, []);
  useEffect(() => {
    setC(3);
    setD(4);
  }, []);
  return <div />;
}";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn non_set_state_calls_ignored() {
        let rule = NoCascadingSetStateRule::new(&make_config(2)).unwrap();
        let content = "\
function MyComponent() {
  useEffect(() => {
    console.log('hi');
    fetchData();
    setup();
    setA(1);
  }, []);
  return <div />;
}";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn lowercase_set_not_counted() {
        let rule = NoCascadingSetStateRule::new(&make_config(2)).unwrap();
        // `settings()` and `setup()` start with "set" but next char is lowercase
        let content = "\
function MyComponent() {
  useEffect(() => {
    settings();
    setup();
    setA(1);
  }, []);
  return <div />;
}";
        let violations = check(&rule, content);
        assert!(violations.is_empty());
    }

    #[test]
    fn function_expression_callback() {
        let rule = NoCascadingSetStateRule::new(&make_config(2)).unwrap();
        let content = "\
function MyComponent() {
  useEffect(function() {
    setA(1);
    setB(2);
  }, []);
  return <div />;
}";
        let violations = check(&rule, content);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn default_max_count() {
        let config = RuleConfig {
            id: "test".into(),
            severity: Severity::Warning,
            message: "test".into(),
            ..Default::default()
        };
        let rule = NoCascadingSetStateRule::new(&config).unwrap();
        assert_eq!(rule.max_count, 3);
    }

    #[test]
    fn non_tsx_file_skipped() {
        let rule = NoCascadingSetStateRule::new(&make_config(1)).unwrap();
        let ctx = ScanContext {
            file_path: Path::new("test.rs"),
            content: "fn main() {}",
        };
        assert!(rule.check_file(&ctx).is_empty());
    }

    #[test]
    fn is_set_state_name_cases() {
        assert!(is_set_state_name("setA"));
        assert!(is_set_state_name("setName"));
        assert!(is_set_state_name("setIsLoading"));
        assert!(!is_set_state_name("set"));
        assert!(!is_set_state_name("setup"));
        assert!(!is_set_state_name("settings"));
        assert!(!is_set_state_name("reset"));
        assert!(!is_set_state_name("useState"));
    }
}
