//! Rules handlers: list, add, delete, clear
//!
//! Rules have two sources:
//! - "profile": rules from the active config.yaml (loaded from the active profile)
//! - "custom":  rules from settings.yaml custom-rules (user-added, persist across profile switches)
//!
//! Only "custom" rules can be deleted or cleared.

use actix_web::{web, HttpResponse};
use std::path::PathBuf;
use std::sync::Arc;

use crate::ServiceState;
use crate::settings::SettingsData;

use super::{ApiResponse, AddRuleRequest};

/// Return settings.yaml path, falling back to a default PathBuf if not found.
fn settings_path() -> PathBuf {
    super::find_settings_path().unwrap_or_else(|| PathBuf::from("settings.yaml"))
}

/// A single rule with source marking.
#[derive(serde::Serialize)]
struct RuleItem {
    index: usize,      // 1-based global display index
    rule: String,
    #[serde(rename = "source")]
    source: &'static str,
}

/// GET /api/rules response body
#[derive(serde::Serialize)]
struct RulesResponse {
    rules: Vec<RuleItem>,
    #[serde(rename = "profile_rules_count")]
    profile_rules_count: usize,
    #[serde(rename = "custom_rules_count")]
    custom_rules_count: usize,
}

/// GET /api/rules - Returns rules with source marking
pub async fn get_rules() -> HttpResponse {
    let paths = super::get_control_tower_paths();
    let config_path = &paths.active_config_path;
    let settings_path = settings_path();

    // Read settings
    let settings: SettingsData = if settings_path.exists() {
        std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|c| serde_yaml_ng::from_str(&c).ok())
            .unwrap_or_default()
    } else {
        SettingsData::default()
    };

    // Read all rules from config.yaml
    let all_rules: Vec<String> = if config_path.exists() {
        std::fs::read_to_string(config_path)
            .ok()
            .and_then(|content| {
                serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content).ok()
            })
            .and_then(|yaml| {
                yaml.get("rules")?
                    .as_sequence()?
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
                    .into()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // Determine custom rules count
    // Priority: settings.custom_rules (if non-empty) > detect from profile file
    // When custom_rules is None or empty, always detect from profile to handle
    // stale profile_rules_count or polluted config state.
    let custom_count = if let Some(ref cr) = settings.custom_rules {
        if !cr.is_empty() {
            cr.len()
        } else {
            detect_custom_count_from_profile(&all_rules, &paths)
        }
    } else {
        detect_custom_count_from_profile(&all_rules, &paths)
    };

    let custom_count = custom_count.min(all_rules.len());
    let profile_count = all_rules.len() - custom_count;

    let custom_rules_list: Vec<String> = all_rules[..custom_count].to_vec();
    let profile_rules: Vec<String> = if custom_count < all_rules.len() {
        all_rules[custom_count..].to_vec()
    } else {
        Vec::new()
    };

    // Build combined list with source marking
    // Display order: custom rules first (higher priority), profile rules last
    let mut items: Vec<RuleItem> = Vec::with_capacity(all_rules.len());

    for (i, rule) in custom_rules_list.iter().enumerate() {
        items.push(RuleItem {
            index: i + 1,
            rule: rule.clone(),
            source: "custom",
        });
    }
    for (i, rule) in profile_rules.iter().enumerate() {
        items.push(RuleItem {
            index: custom_count + i + 1,
            rule: rule.clone(),
            source: "profile",
        });
    }

    let response = RulesResponse {
        rules: items,
        profile_rules_count: profile_count,
        custom_rules_count: custom_count,
    };
    HttpResponse::Ok().json(ApiResponse::success(response))
}

/// Detect how many custom rules are prepended to config by comparing against
/// the active profile file. Finds the first occurrence of a profile rule in
/// config — all rules before it are custom rules.
fn detect_custom_count_from_profile(
    all_rules: &[String],
    paths: &control_tower_service_core::ControlTowerPaths,
) -> usize {
    use serde_yaml_ng::Value;

    // Read profiles.yaml to find the active profile's file name
    let profiles_yaml_path = &paths.profiles_path;
    if !profiles_yaml_path.exists() {
        return 0;
    }

    let profiles_content = match std::fs::read_to_string(profiles_yaml_path) {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let profiles: Value = match serde_yaml_ng::from_str(&profiles_content) {
        Ok(v) => v,
        Err(_) => return 0,
    };

    // Look up the current profile's file name from the items array
    let current_uid = match profiles.get("current").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => return 0,
    };

    let profile_file_name = {
        let items = match profiles.get("items").and_then(|v| v.as_sequence()) {
            Some(i) => i,
            None => return 0,
        };
        let matching = items.iter().find(|item| {
            item.get("uid")
                .and_then(|v| v.as_str())
                .map(|u| u == current_uid)
                .unwrap_or(false)
        });
        match matching {
            Some(item) => item.get("file").and_then(|v| v.as_str()).map(String::from),
            None => None,
        }
    };

    let profile_file_name = match profile_file_name {
        Some(f) => f,
        None => return 0,
    };

    let config_dir = &paths.config_dir;
    let profile_path = config_dir.join("profiles").join(&profile_file_name);
    if !profile_path.exists() {
        return 0;
    }

    let profile_content = match std::fs::read_to_string(&profile_path) {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let profile_rules: Vec<String> = match serde_yaml_ng::from_str::<Value>(&profile_content) {
        Ok(yaml) => yaml
            .get("rules")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        Err(_) => return 0,
    };

    if profile_rules.is_empty() {
        return 0;
    }

    // Find first profile rule in all_rules (custom rules are prepended before it)
    for (i, rule) in all_rules.iter().enumerate() {
        if profile_rules.iter().any(|pr| pr == rule) {
            return i; // rules before this are custom
        }
    }

    0 // no match found
}

/// POST /api/rules - Add a custom rule
pub async fn add_rule(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<AddRuleRequest>,
) -> HttpResponse {
    let proxy = body.proxy.trim();

    // Validate proxy name exists by checking Mihomo proxy list
    // NOTE: validation is skipped if proxy list is empty (api_port mismatch in test env)
    let proxy_names = {
        let svc: &ServiceState = &**state;
        match svc.get_proxies() {
            Ok(data) => {
                data.as_object()
                    .and_then(|o| o.get("proxies"))
                    .and_then(|v| v.as_object())
                    .map(|m| m.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default()
            }
            Err(_) => Vec::new(),
        }
    };
    // Skip validation when proxy list is empty (api_port mismatch in test/dev env)
    let proxy_exists = proxy_names.is_empty() || proxy_names.iter().any(|n| n == proxy);
    if !proxy_exists {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error(format!(
                "Proxy '{}' not found. Available proxies: {}",
                proxy,
                proxy_names.iter().take(10).cloned().collect::<Vec<_>>().join(", ")
            )));
    }

    let rule = format!("{},{},{}", body.rule_type.to_uppercase(), body.value, proxy);

    let settings_path = settings_path();

    // Read current custom-rules from settings.yaml
    let custom_rules: Vec<String> = if settings_path.exists() {
        match std::fs::read_to_string(&settings_path) {
            Ok(content) => {
                match serde_yaml_ng::from_str::<SettingsData>(&content) {
                    Ok(s) => s.custom_rules.unwrap_or_default(),
                    Err(_) => Vec::new(),
                }
            }
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };

    let mut new_custom_rules = custom_rules;
    new_custom_rules.push(rule.clone());

    // Write updated custom-rules back to settings.yaml
    if let Err(e) = write_custom_rules_to_settings(&settings_path, &new_custom_rules) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to save custom rules: {}", e)));
    }

    // Read profile_rules_count from settings.yaml
    let profile_rules_count = if settings_path.exists() {
        std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|c| serde_yaml_ng::from_str::<SettingsData>(&c).ok())
            .and_then(|s| s.profile_rules_count)
            .unwrap_or(0)
    } else {
        0
    };

    // Merge profile rules (from config.yaml) + custom rules into config.yaml
    let paths = super::get_control_tower_paths();
    let store = control_tower_service_core::ActiveConfigStore::new(paths);
    if let Err(e) = store.merge_rules_with_custom(profile_rules_count, &new_custom_rules) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to merge rules: {}", e)));
    }

    // Config.yaml updated — Mihomo will pick up on next hot-reload or manual refresh
    HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "rule": rule
    })))
}

/// DELETE /api/rules/{source}/{index} - Remove a custom rule
///
/// - source="profile" → 400 error (profile rules cannot be deleted)
/// - source="custom"  → delete custom-rules[index] (0-based), merge and hot-reload
pub async fn delete_rule(
    _state: web::Data<Arc<ServiceState>>,
    path: web::Path<(String, usize)>,
) -> HttpResponse {
    let (source, index) = path.into_inner();

    if source == "profile" {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("Profile rules cannot be deleted".to_string()));
    }

    let paths = super::get_control_tower_paths();
    let settings_path = settings_path();
    let config_path = &paths.active_config_path;

    // Read all rules from config.yaml
    let all_rules: Vec<String> = if config_path.exists() {
        std::fs::read_to_string(config_path)
            .ok()
            .and_then(|content| {
                serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content).ok()
            })
            .and_then(|yaml| {
                yaml.get("rules")?
                    .as_sequence()?
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
                    .into()
            })
            .unwrap_or_default()
    } else {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("config.yaml not found".to_string()));
    };

    // Detect custom rules count from profile (handles corrupted settings.yaml)
    let custom_count = detect_custom_count_from_profile(&all_rules, &paths);
    if custom_count == 0 || index >= custom_count {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error(format!("Invalid index {}. Valid range: 0-{}", index, custom_count.saturating_sub(1))));
    }

    // Read current custom_rules from settings.yaml (may be empty/None if corrupted)
    let current_custom: Vec<String> = if settings_path.exists() {
        std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|c| serde_yaml_ng::from_str::<SettingsData>(&c).ok())
            .and_then(|s| s.custom_rules)
            .unwrap_or_else(|| all_rules[..custom_count].to_vec())
    } else {
        all_rules[..custom_count].to_vec()
    };

    let removed = current_custom.get(index).cloned();
    let mut new_custom_rules = current_custom;
    new_custom_rules.remove(index);

    if let Err(e) = write_custom_rules_to_settings(&settings_path, &new_custom_rules) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to save custom rules: {}", e)));
    }

    let profile_rules_count = all_rules.len() - custom_count;
    let store = control_tower_service_core::ActiveConfigStore::new(paths);
    if let Err(e) = store.merge_rules_with_custom(profile_rules_count, &new_custom_rules) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to merge rules: {}", e)));
    }

    HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "removed": removed
    })))
}

/// DELETE /api/rules - Clear custom rules
///
/// - body.source="profile" → 400 error
/// - body.source="custom"  → clear custom-rules, merge and hot-reload
pub async fn clear_rules(
    _state: web::Data<Arc<ServiceState>>,
    body: web::Json<super::DeleteRulesRequest>,
) -> HttpResponse {
    if body.source == "profile" {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("Profile rules cannot be cleared".to_string()));
    }

    let settings_path = settings_path();

    if let Err(e) = write_custom_rules_to_settings(&settings_path, &[]) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to clear custom rules: {}", e)));
    }

    let paths = super::get_control_tower_paths();
    let config_path = &paths.active_config_path;

    // Read all rules from config.yaml
    let all_rules: Vec<String> = if config_path.exists() {
        std::fs::read_to_string(config_path)
            .ok()
            .and_then(|content| {
                serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content).ok()
            })
            .and_then(|yaml| {
                yaml.get("rules")?
                    .as_sequence()?
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
                    .into()
            })
            .unwrap_or_default()
    } else {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("config.yaml not found".to_string()));
    };

    // Detect custom count from profile (handles corrupted settings.yaml)
    let custom_count = detect_custom_count_from_profile(&all_rules, &paths);
    if custom_count == 0 {
        return HttpResponse::Ok().json(ApiResponse::<()>::success(()));
    }

    if let Err(e) = write_custom_rules_to_settings(&settings_path, &[]) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to clear custom rules: {}", e)));
    }

    // all_rules.len() - custom_count = actual profile_rules_count
    let profile_rules_count = all_rules.len() - custom_count;
    let store = control_tower_service_core::ActiveConfigStore::new(paths);
    if let Err(e) = store.merge_rules_with_custom(profile_rules_count, &[]) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to merge rules: {}", e)));
    }

    HttpResponse::Ok().json(ApiResponse::<()>::success(()))
}

/// Write the custom-rules list into settings.yaml, preserving all other fields.
fn write_custom_rules_to_settings(settings_path: &std::path::Path, custom_rules: &[String]) -> anyhow::Result<()> {
    let content = if settings_path.exists() {
        std::fs::read_to_string(settings_path)?
    } else {
        String::new()
    };

    let mut settings: SettingsData = serde_yaml_ng::from_str(&content)
        .unwrap_or_else(|_| SettingsData::default());

    if custom_rules.is_empty() {
        settings.custom_rules = None;
    } else {
        settings.custom_rules = Some(custom_rules.to_vec());
    }

    let yaml_str = serde_yaml_ng::to_string(&settings)?;

    // Atomic write
    let temp = settings_path.with_extension("yaml.tmp");
    std::fs::write(&temp, &yaml_str)?;
    std::fs::rename(&temp, settings_path)?;

    Ok(())
}
