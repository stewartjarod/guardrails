use crate::cli::toml_config::TomlRule;
use std::collections::HashMap;
use std::fmt;

#[derive(Debug)]
pub enum PresetError {
    UnknownPreset {
        name: String,
        available: Vec<&'static str>,
    },
}

impl fmt::Display for PresetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PresetError::UnknownPreset { name, available } => {
                write!(
                    f,
                    "unknown preset '{}'. available presets: {}",
                    name,
                    available.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for PresetError {}

#[derive(Debug, Clone, Copy)]
enum Preset {
    ShadcnStrict,
    ShadcnMigrate,
    AiSafety,
}

/// Returns the list of all available preset names.
pub fn available_presets() -> &'static [&'static str] {
    &["shadcn-strict", "shadcn-migrate", "ai-safety"]
}

fn resolve_preset(name: &str) -> Option<Preset> {
    match name {
        "shadcn-strict" => Some(Preset::ShadcnStrict),
        "shadcn-migrate" => Some(Preset::ShadcnMigrate),
        "ai-safety" => Some(Preset::AiSafety),
        _ => None,
    }
}

fn preset_rules(preset: Preset) -> Vec<TomlRule> {
    match preset {
        Preset::ShadcnStrict => vec![
            TomlRule {
                id: "enforce-dark-mode".into(),
                rule_type: "tailwind-dark-mode".into(),
                severity: "error".into(),
                glob: Some("**/*.{tsx,jsx}".into()),
                message: "Missing dark: variant for color class".into(),
                suggest: Some(
                    "Use a shadcn semantic token class or add an explicit dark: counterpart"
                        .into(),
                ),
                ..Default::default()
            },
            TomlRule {
                id: "use-theme-tokens".into(),
                rule_type: "tailwind-theme-tokens".into(),
                severity: "error".into(),
                glob: Some("**/*.{tsx,jsx}".into()),
                message: "Use shadcn semantic token instead of raw color".into(),
                ..Default::default()
            },
            TomlRule {
                id: "no-inline-styles".into(),
                rule_type: "banned-pattern".into(),
                severity: "warning".into(),
                glob: Some("**/*.{tsx,jsx}".into()),
                pattern: Some("style={{".into()),
                message: "Avoid inline styles — use Tailwind utility classes instead".into(),
                suggest: Some("Replace style={{ ... }} with Tailwind classes".into()),
                ..Default::default()
            },
            TomlRule {
                id: "no-css-in-js".into(),
                rule_type: "banned-import".into(),
                severity: "error".into(),
                packages: vec![
                    "styled-components".into(),
                    "@emotion/styled".into(),
                    "@emotion/css".into(),
                    "@emotion/react".into(),
                ],
                message: "CSS-in-JS libraries conflict with Tailwind — use utility classes instead"
                    .into(),
                ..Default::default()
            },
            TomlRule {
                id: "no-competing-frameworks".into(),
                rule_type: "banned-dependency".into(),
                severity: "error".into(),
                packages: vec![
                    "bootstrap".into(),
                    "bulma".into(),
                    "@mui/material".into(),
                    "antd".into(),
                ],
                message:
                    "Competing CSS framework detected — this project uses Tailwind + shadcn/ui"
                        .into(),
                ..Default::default()
            },
        ],
        Preset::ShadcnMigrate => vec![
            TomlRule {
                id: "enforce-dark-mode".into(),
                rule_type: "tailwind-dark-mode".into(),
                severity: "error".into(),
                glob: Some("**/*.{tsx,jsx}".into()),
                message: "Missing dark: variant for color class".into(),
                suggest: Some(
                    "Use a shadcn semantic token class or add an explicit dark: counterpart"
                        .into(),
                ),
                ..Default::default()
            },
            TomlRule {
                id: "use-theme-tokens".into(),
                rule_type: "tailwind-theme-tokens".into(),
                severity: "warning".into(),
                glob: Some("**/*.{tsx,jsx}".into()),
                message: "Use shadcn semantic token instead of raw color".into(),
                ..Default::default()
            },
        ],
        Preset::AiSafety => vec![
            TomlRule {
                id: "no-moment".into(),
                rule_type: "banned-dependency".into(),
                severity: "error".into(),
                packages: vec!["moment".into(), "moment-timezone".into()],
                message: "moment.js is deprecated — use date-fns or Temporal API".into(),
                ..Default::default()
            },
            TomlRule {
                id: "no-lodash".into(),
                rule_type: "banned-dependency".into(),
                severity: "error".into(),
                packages: vec!["lodash".into()],
                message: "lodash is unnecessary — use native JS methods".into(),
                ..Default::default()
            },
            TomlRule {
                id: "no-deprecated-request".into(),
                rule_type: "banned-dependency".into(),
                severity: "error".into(),
                packages: vec!["request".into(), "request-promise".into()],
                message: "The 'request' package is deprecated — use 'node-fetch' or 'undici'".into(),
                ..Default::default()
            },
        ],
    }
}

/// Merge preset rules with user-defined rules. User rules with the same `id`
/// as a preset rule replace the preset version entirely. New user rules are
/// appended after all preset rules.
fn merge_rules(preset_rules: Vec<TomlRule>, user_rules: &[TomlRule]) -> Vec<TomlRule> {
    let mut merged = preset_rules;

    // Index preset rules by id for O(1) lookup
    let mut id_to_index: HashMap<String, usize> = HashMap::new();
    for (i, rule) in merged.iter().enumerate() {
        id_to_index.insert(rule.id.clone(), i);
    }

    for user_rule in user_rules {
        if let Some(&idx) = id_to_index.get(&user_rule.id) {
            // User rule overrides preset rule with same id
            merged[idx] = user_rule.clone();
        } else {
            // New user rule appended
            merged.push(user_rule.clone());
        }
    }

    merged
}

/// Resolve all `extends` presets and merge with user-defined rules.
/// Returns the final list of `TomlRule` entries ready for the build pipeline.
pub fn resolve_rules(
    extends: &[String],
    user_rules: &[TomlRule],
) -> Result<Vec<TomlRule>, PresetError> {
    if extends.is_empty() {
        return Ok(user_rules.to_vec());
    }

    // Collect all preset rules in order, later presets override earlier ones
    let mut all_preset_rules: Vec<TomlRule> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();

    for preset_name in extends {
        let preset = resolve_preset(preset_name).ok_or_else(|| PresetError::UnknownPreset {
            name: preset_name.clone(),
            available: available_presets().to_vec(),
        })?;

        for rule in preset_rules(preset) {
            if let Some(&idx) = seen.get(&rule.id) {
                // Later preset overrides earlier for same id
                all_preset_rules[idx] = rule;
            } else {
                seen.insert(rule.id.clone(), all_preset_rules.len());
                all_preset_rules.push(rule);
            }
        }
    }

    Ok(merge_rules(all_preset_rules, user_rules))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadcn_strict_has_five_rules() {
        let rules = preset_rules(Preset::ShadcnStrict);
        assert_eq!(rules.len(), 5);
        let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"enforce-dark-mode"));
        assert!(ids.contains(&"use-theme-tokens"));
        assert!(ids.contains(&"no-inline-styles"));
        assert!(ids.contains(&"no-css-in-js"));
        assert!(ids.contains(&"no-competing-frameworks"));
    }

    #[test]
    fn shadcn_migrate_has_two_rules() {
        let rules = preset_rules(Preset::ShadcnMigrate);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].id, "enforce-dark-mode");
        assert_eq!(rules[1].id, "use-theme-tokens");
        // migrate uses warning for theme tokens
        assert_eq!(rules[1].severity, "warning");
    }

    #[test]
    fn ai_safety_has_three_rules() {
        let rules = preset_rules(Preset::AiSafety);
        assert_eq!(rules.len(), 3);
        let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"no-moment"));
        assert!(ids.contains(&"no-lodash"));
        assert!(ids.contains(&"no-deprecated-request"));
    }

    #[test]
    fn resolve_unknown_preset_errors() {
        let result = resolve_rules(&["unknown-preset".to_string()], &[]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("unknown preset 'unknown-preset'"));
        assert!(msg.contains("shadcn-strict"));
    }

    #[test]
    fn resolve_empty_extends_returns_user_rules() {
        let user_rules = vec![TomlRule {
            id: "custom-rule".into(),
            rule_type: "banned-pattern".into(),
            pattern: Some("TODO".into()),
            message: "No TODOs".into(),
            ..Default::default()
        }];
        let result = resolve_rules(&[], &user_rules).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "custom-rule");
    }

    #[test]
    fn user_rule_overrides_preset() {
        let user_rules = vec![TomlRule {
            id: "use-theme-tokens".into(),
            rule_type: "tailwind-theme-tokens".into(),
            severity: "warning".into(),
            glob: Some("**/*.{tsx,jsx}".into()),
            message: "Custom message".into(),
            ..Default::default()
        }];
        let result = resolve_rules(&["shadcn-strict".to_string()], &user_rules).unwrap();
        assert_eq!(result.len(), 5);
        let token_rule = result.iter().find(|r| r.id == "use-theme-tokens").unwrap();
        assert_eq!(token_rule.severity, "warning");
        assert_eq!(token_rule.message, "Custom message");
    }

    #[test]
    fn user_rule_appended_after_preset() {
        let user_rules = vec![TomlRule {
            id: "my-custom".into(),
            rule_type: "banned-pattern".into(),
            pattern: Some("foo".into()),
            message: "no foo".into(),
            ..Default::default()
        }];
        let result = resolve_rules(&["shadcn-strict".to_string()], &user_rules).unwrap();
        assert_eq!(result.len(), 6);
        assert_eq!(result[5].id, "my-custom");
    }

    #[test]
    fn later_preset_overrides_earlier() {
        // shadcn-strict sets use-theme-tokens severity to "error"
        // shadcn-migrate sets it to "warning"
        let result = resolve_rules(
            &["shadcn-strict".to_string(), "shadcn-migrate".to_string()],
            &[],
        )
        .unwrap();
        let token_rule = result.iter().find(|r| r.id == "use-theme-tokens").unwrap();
        assert_eq!(token_rule.severity, "warning");
        // Should have 5 unique rules (strict has 5, migrate shares 2 ids)
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn multiple_presets_combine() {
        let result = resolve_rules(
            &["shadcn-migrate".to_string(), "ai-safety".to_string()],
            &[],
        )
        .unwrap();
        // 2 from migrate + 3 from ai-safety = 5
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn all_preset_names_resolve() {
        for name in available_presets() {
            assert!(
                resolve_preset(name).is_some(),
                "preset '{}' should resolve",
                name
            );
        }
    }
}
