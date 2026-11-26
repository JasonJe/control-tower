//! Shared validation of Mihomo `config.yaml` bodies.
//!
//! This lives in the core crate so `ctsvc` (Web UI, safe save with rollback)
//! and `ctctl config validate` report exactly the same errors for the same
//! file. It is purely structural and never touches disk.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_yaml_ng::Value;

/// Counts extracted from a config, shown after validation/save.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConfigSummary {
    pub proxies: usize,
    pub proxy_groups: usize,
    pub rules: usize,
    pub rule_providers: usize,
    pub proxy_providers: usize,
    pub size: usize,
}

/// Result of validating a candidate config.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<ConfigSummary>,
}

/// Validate a config body without touching disk.
///
/// Structural errors (unparsable YAML, wrong types, unknown group members)
/// are reported as errors; cosmetic/unsupported things are warnings.
pub fn validate(content: &str) -> ValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if content.trim().is_empty() {
        errors.push("Config is empty".to_string());
        return ValidationReport {
            ok: false,
            errors,
            warnings,
            summary: None,
        };
    }

    let value: Value = match serde_yaml_ng::from_str(content) {
        Ok(v) => v,
        Err(e) => {
            errors.push(format!("Invalid YAML: {}", e));
            return ValidationReport {
                ok: false,
                errors,
                warnings,
                summary: None,
            };
        }
    };

    let Some(map) = value.as_mapping() else {
        errors.push("Config root must be a mapping (key: value pairs)".to_string());
        return ValidationReport {
            ok: false,
            errors,
            warnings,
            summary: None,
        };
    };

    let section = |name: &str| map.get(Value::String(name.to_string()));

    let mut summary = ConfigSummary {
        size: content.len(),
        ..Default::default()
    };

    // proxies
    let mut proxy_names: BTreeSet<String> = BTreeSet::new();
    if let Some(proxies) = section("proxies") {
        match proxies.as_sequence() {
            Some(list) => {
                summary.proxies = list.len();
                for (index, proxy) in list.iter().enumerate() {
                    let Some(proxy_map) = proxy.as_mapping() else {
                        errors.push(format!("proxies[{}] must be a mapping", index));
                        continue;
                    };
                    let name = proxy_map
                        .get(Value::String("name".to_string()))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if name.is_empty() {
                        errors.push(format!("proxies[{}] is missing `name`", index));
                    } else if !proxy_names.insert(name.to_string()) {
                        errors.push(format!("Duplicate proxy name `{}`", name));
                    }
                    for required in ["type", "server", "port"] {
                        if !proxy_map.contains_key(Value::String(required.to_string())) {
                            errors.push(format!(
                                "proxies[{}] ({}) is missing `{}`",
                                index,
                                if name.is_empty() { "unnamed" } else { name },
                                required
                            ));
                        }
                    }
                }
            }
            None => errors.push("`proxies` must be a list".to_string()),
        }
    }

    // proxy-providers (names usable as group members)
    let mut provider_names: BTreeSet<String> = BTreeSet::new();
    if let Some(providers) = section("proxy-providers") {
        if let Some(mapping) = providers.as_mapping() {
            summary.proxy_providers = mapping.len();
            for key in mapping.keys().filter_map(Value::as_str) {
                provider_names.insert(key.to_string());
            }
        } else {
            warnings.push("`proxy-providers` should be a mapping".to_string());
        }
    }

    // proxy-groups
    let mut group_names: BTreeSet<String> = BTreeSet::new();
    if let Some(groups) = section("proxy-groups") {
        match groups.as_sequence() {
            Some(list) => {
                summary.proxy_groups = list.len();
                for (index, group) in list.iter().enumerate() {
                    let Some(group_map) = group.as_mapping() else {
                        errors.push(format!("proxy-groups[{}] must be a mapping", index));
                        continue;
                    };
                    let name = group_map
                        .get(Value::String("name".to_string()))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if name.is_empty() {
                        errors.push(format!("proxy-groups[{}] is missing `name`", index));
                    } else if !group_names.insert(name.to_string()) {
                        errors.push(format!("Duplicate proxy group name `{}`", name));
                    }
                    if let Some(members) = group_map.get(Value::String("proxies".to_string())) {
                        match members.as_sequence() {
                            Some(members) => {
                                for member in members.iter().filter_map(Value::as_str) {
                                    if !is_known_target(
                                        member,
                                        &proxy_names,
                                        &group_names,
                                        &provider_names,
                                    ) {
                                        warnings.push(format!(
                                            "Group `{}` references unknown target `{}`",
                                            if name.is_empty() { "unnamed" } else { name },
                                            member
                                        ));
                                    }
                                }
                            }
                            None => errors.push(format!(
                                "proxy-groups[{}] `proxies` must be a list",
                                index
                            )),
                        }
                    }
                }
            }
            None => errors.push("`proxy-groups` must be a list".to_string()),
        }
    }

    // rule-providers
    if let Some(providers) = section("rule-providers") {
        if let Some(mapping) = providers.as_mapping() {
            summary.rule_providers = mapping.len();
        } else {
            warnings.push("`rule-providers` should be a mapping".to_string());
        }
    }

    // rules
    if let Some(rules) = section("rules") {
        match rules.as_sequence() {
            Some(list) => {
                summary.rules = list.len();
                for (index, rule) in list.iter().enumerate() {
                    let Some(text) = rule.as_str() else {
                        errors.push(format!("rules[{}] must be a string", index));
                        continue;
                    };
                    if text.trim().is_empty() {
                        errors.push(format!("rules[{}] is empty", index));
                        continue;
                    }
                    // The policy is the last comma-separated field (MATCH has
                    // type and policy only).
                    let policy = text.rsplit(',').next().unwrap_or("").trim();
                    if policy.is_empty() {
                        errors.push(format!("rules[{}] has no policy: `{}`", index, text));
                    } else if !is_known_target(
                        policy,
                        &proxy_names,
                        &group_names,
                        &provider_names,
                    ) {
                        warnings.push(format!(
                            "rules[{}] targets unknown policy `{}`",
                            index, policy
                        ));
                    }
                }
            }
            None => errors.push("`rules` must be a list of strings".to_string()),
        }
    }

    ValidationReport {
        ok: errors.is_empty(),
        errors,
        warnings,
        summary: Some(summary),
    }
}

/// Built-in policies that are valid without being declared in the config.
pub fn is_known_target(
    name: &str,
    proxies: &BTreeSet<String>,
    groups: &BTreeSet<String>,
    providers: &BTreeSet<String>,
) -> bool {
    matches!(
        name,
        "DIRECT" | "REJECT" | "REJECT-DROP" | "PASS" | "COMPATIBLE" | "GLOBAL"
    ) || proxies.contains(name)
        || groups.contains(name)
        || providers.contains(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_config_has_summary() {
        let yaml = "proxies:\n  - name: p1\n    type: ss\n    server: 1.2.3.4\n    port: 8388\nrules:\n  - MATCH,DIRECT\n";
        let report = validate(yaml);
        assert!(report.ok, "{:?}", report.errors);
        let summary = report.summary.unwrap();
        assert_eq!(summary.proxies, 1);
        assert_eq!(summary.rules, 1);
    }

    #[test]
    fn duplicate_proxy_name_is_an_error() {
        let yaml = "proxies:\n  - {name: p1, type: ss, server: a, port: 1}\n  - {name: p1, type: ss, server: b, port: 2}\n";
        let report = validate(yaml);
        assert!(!report.ok);
        assert!(report.errors.iter().any(|e| e.contains("Duplicate proxy name")));
    }

    #[test]
    fn unknown_policy_is_only_a_warning() {
        let yaml = "rules:\n  - DOMAIN,example.com,MISSING_GROUP\n";
        let report = validate(yaml);
        assert!(report.ok);
        assert!(report.warnings.iter().any(|w| w.contains("unknown policy")));
    }
}
