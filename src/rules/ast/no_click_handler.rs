use crate::config::{RuleConfig, Severity};
use crate::rules::ast::parse_file;
use crate::rules::{Rule, RuleBuildError, ScanContext, Violation};

/// Shared logic for flagging non-interactive elements with onClick but no role.
fn check_click_handler(
    ctx: &ScanContext,
    tag_name: &str,
    id: &str,
    severity: Severity,
    message: &str,
    suggest: &Option<String>,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let tree = match parse_file(ctx.file_path, ctx.content) {
        Some(t) => t,
        None => return violations,
    };
    let source = ctx.content.as_bytes();
    visit(
        tree.root_node(),
        source,
        ctx,
        tag_name,
        id,
        severity,
        message,
        suggest,
        &mut violations,
    );
    violations
}

fn visit(
    node: tree_sitter::Node,
    source: &[u8],
    ctx: &ScanContext,
    tag_name: &str,
    id: &str,
    severity: Severity,
    message: &str,
    suggest: &Option<String>,
    violations: &mut Vec<Violation>,
) {
    let kind = node.kind();
    if kind == "jsx_self_closing_element" || kind == "jsx_opening_element" {
        if is_tag(&node, source, tag_name)
            && has_attribute(&node, source, "onClick")
            && !has_role_attribute(&node, source)
        {
            let row = node.start_position().row;
            violations.push(Violation {
                rule_id: id.to_string(),
                severity,
                file: ctx.file_path.to_path_buf(),
                line: Some(row + 1),
                column: Some(node.start_position().column + 1),
                message: message.to_string(),
                suggest: suggest.clone(),
                source_line: ctx.content.lines().nth(row).map(String::from),
                fix: None,
            });
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            visit(child, source, ctx, tag_name, id, severity, message, suggest, violations);
        }
    }
}

fn is_tag(node: &tree_sitter::Node, source: &[u8], expected: &str) -> bool {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "identifier" || child.kind() == "member_expression" {
                if let Ok(name) = child.utf8_text(source) {
                    return name == expected;
                }
            }
        }
    }
    false
}

fn has_attribute(node: &tree_sitter::Node, source: &[u8], attr_name: &str) -> bool {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "jsx_attribute" {
                if let Some(name_node) = child.child(0) {
                    if let Ok(name) = name_node.utf8_text(source) {
                        if name == attr_name {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn has_role_attribute(node: &tree_sitter::Node, source: &[u8]) -> bool {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "jsx_attribute" {
                if let Some(name_node) = child.child(0) {
                    if let Ok(name) = name_node.utf8_text(source) {
                        if name == "role" {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Flags `<div>` elements with `onClick` but no `role` attribute.
pub struct NoDivClickHandlerRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
}

impl NoDivClickHandlerRule {
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

impl Rule for NoDivClickHandlerRule {
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
        check_click_handler(ctx, "div", &self.id, self.severity, &self.message, &self.suggest)
    }
}

/// Flags `<span>` elements with `onClick` but no `role` attribute.
pub struct NoSpanClickHandlerRule {
    id: String,
    severity: Severity,
    message: String,
    suggest: Option<String>,
    glob: Option<String>,
}

impl NoSpanClickHandlerRule {
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

impl Rule for NoSpanClickHandlerRule {
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
        check_click_handler(ctx, "span", &self.id, self.severity, &self.message, &self.suggest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_div_rule() -> NoDivClickHandlerRule {
        NoDivClickHandlerRule::new(&RuleConfig {
            id: "no-div-click-handler".into(),
            severity: Severity::Error,
            message: "Non-interactive <div> with onClick".into(),
            suggest: Some("Use <button> instead".into()),
            glob: Some("**/*.{tsx,jsx}".into()),
            ..Default::default()
        })
        .unwrap()
    }

    fn make_span_rule() -> NoSpanClickHandlerRule {
        NoSpanClickHandlerRule::new(&RuleConfig {
            id: "no-span-click-handler".into(),
            severity: Severity::Error,
            message: "Non-interactive <span> with onClick".into(),
            suggest: Some("Use <button> instead".into()),
            glob: Some("**/*.{tsx,jsx}".into()),
            ..Default::default()
        })
        .unwrap()
    }

    fn check_div(content: &str) -> Vec<Violation> {
        let rule = make_div_rule();
        let ctx = ScanContext {
            file_path: Path::new("test.tsx"),
            content,
        };
        rule.check_file(&ctx)
    }

    fn check_span(content: &str) -> Vec<Violation> {
        let rule = make_span_rule();
        let ctx = ScanContext {
            file_path: Path::new("test.tsx"),
            content,
        };
        rule.check_file(&ctx)
    }

    #[test]
    fn div_with_onclick_no_role_flags() {
        let v = check_div(r#"function App() { return <div onClick={handleClick}>text</div>; }"#);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn div_with_onclick_and_role_no_violation() {
        let v = check_div(
            r#"function App() { return <div role="button" onClick={handleClick}>text</div>; }"#,
        );
        assert!(v.is_empty());
    }

    #[test]
    fn div_without_onclick_no_violation() {
        let v = check_div(r#"function App() { return <div className="card">text</div>; }"#);
        assert!(v.is_empty());
    }

    #[test]
    fn self_closing_div_with_onclick_flags() {
        let v = check_div(r#"function App() { return <div onClick={fn} />; }"#);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn span_with_onclick_no_role_flags() {
        let v = check_span(r#"function App() { return <span onClick={fn}>text</span>; }"#);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn span_with_onclick_and_role_no_violation() {
        let v = check_span(
            r#"function App() { return <span role="link" onClick={fn}>text</span>; }"#,
        );
        assert!(v.is_empty());
    }

    #[test]
    fn button_not_flagged_by_div_rule() {
        let v = check_div(r#"function App() { return <button onClick={fn}>text</button>; }"#);
        assert!(v.is_empty());
    }

    #[test]
    fn non_tsx_skipped() {
        let rule = make_div_rule();
        let ctx = ScanContext {
            file_path: Path::new("test.rs"),
            content: "fn main() {}",
        };
        assert!(rule.check_file(&ctx).is_empty());
    }

    #[test]
    fn multiline_jsx_div() {
        let content = r#"function App() {
  return (
    <div
      className="card"
      onClick={handleClick}
    >
      content
    </div>
  );
}"#;
        let v = check_div(content);
        assert_eq!(v.len(), 1);
    }
}
