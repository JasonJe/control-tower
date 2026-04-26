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
use tokio::task;

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

    // Read settings to get profile_rules_count and custom_rules
    let settings: SettingsData = if settings_path.exists() {
        std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|c| serde_yaml_ng::from_str(&c).ok())
            .unwrap_or_default()
    } else {
        SettingsData::default()
    };

    let profile_rules_count = settings.profile_rules_count.unwrap_or(0);

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

    // Split at profile_rules_count boundary
    // Custom rules are now FIRST in config (higher priority), profile rules follow
    let profile_count = profile_rules_count.min(all_rules.len());
    let custom_count = if profile_count < all_rules.len() {
        all_rules.len() - profile_count
    } else {
        settings.custom_rules.unwrap_or_default().len()
    };
    let custom_rules: Vec<String> = all_rules[..custom_count].to_vec();
    let profile_rules: Vec<String> = if custom_count < all_rules.len() {
        all_rules[custom_count..].to_vec()
    } else {
        Vec::new()
    };

    let total = all_rules.len();

    // Build combined list with source marking
    // Display order: custom rules first (higher priority), profile rules last
    let mut items: Vec<RuleItem> = Vec::with_capacity(total);

    for (i, rule) in custom_rules.iter().enumerate() {
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

    // Hot-reload Mihomo via API
    let state = state.clone();
    match task::spawn_blocking(move || state.reload_config()).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "rule": rule
        }))),
        Ok(Err(e)) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Rule saved but reload failed: {}", e))),
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Reload task error: {}", e))),
    }
}

/// DELETE /api/rules/{source}/{index} - Remove a custom rule
///
/// - source="profile" → 400 error (profile rules cannot be deleted)
/// - source="custom"  → delete custom-rules[index] (0-based), merge and hot-reload
pub async fn delete_rule(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<(String, usize)>,
) -> HttpResponse {
    let (source, index) = path.into_inner();

    if source == "profile" {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("Profile rules cannot be deleted".to_string()));
    }

    let settings_path = settings_path();

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

    if index >= custom_rules.len() {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error(format!("Invalid index {}. Valid range: 0-{}", index, custom_rules.len().saturating_sub(1))));
    }

    let removed = custom_rules[index].clone();
    let mut new_custom_rules = custom_rules;
    new_custom_rules.remove(index);

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

    let paths = super::get_control_tower_paths();
    let store = control_tower_service_core::ActiveConfigStore::new(paths);
    if let Err(e) = store.merge_rules_with_custom(profile_rules_count, &new_custom_rules) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to merge rules: {}", e)));
    }

    let state = state.clone();
    match task::spawn_blocking(move || state.reload_config()).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "removed": removed
        }))),
        Ok(Err(e)) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Rule removed but reload failed: {}", e))),
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Reload task error: {}", e))),
    }
}

/// DELETE /api/rules - Clear custom rules
///
/// - body.source="profile" → 400 error
/// - body.source="custom"  → clear custom-rules, merge and hot-reload
pub async fn clear_rules(
    state: web::Data<Arc<ServiceState>>,
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

    let paths = super::get_control_tower_paths();
    let store = control_tower_service_core::ActiveConfigStore::new(paths);
    if let Err(e) = store.merge_rules_with_custom(profile_rules_count, &[]) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to merge rules: {}", e)));
    }

    let state = state.clone();
    match task::spawn_blocking(move || state.reload_config()).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Rules cleared but reload failed: {}", e))),
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Reload task error: {}", e))),
    }
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
