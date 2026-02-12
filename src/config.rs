/// Severity level for a rule violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// Parsed rule configuration from `guardrails.toml`.
#[derive(Debug, Clone)]
pub struct RuleConfig {
    pub id: String,
    pub severity: Severity,
    pub message: String,
    pub suggest: Option<String>,
    pub glob: Option<String>,
    /// Classes exempt from enforcement.
    pub allowed_classes: Vec<String>,
    /// User-provided token mappings (`"raw-class=semantic-class"`).
    pub token_map: Vec<String>,
}

impl Default for RuleConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            severity: Severity::Warning,
            message: String::new(),
            suggest: None,
            glob: None,
            allowed_classes: Vec::new(),
            token_map: Vec::new(),
        }
    }
}
