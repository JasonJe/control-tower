//! Rule provider handlers: list, add, update, delete, refresh

use std::sync::Arc;

use actix_web::{web, HttpResponse};

use crate::ServiceState;
use control_tower_service_core::profiles::RuleProviderConfig;
use control_tower_service_core::ActiveConfigStore;

use super::{ApiResponse, get_control_tower_paths};

/// GET /api/rule-providers — List all rule providers from settings.yaml
pub async fn get_rule_providers() -> HttpResponse {
    let state = ServiceState::new();
    let settings = state.get_settings();
    let providers = settings.rule_providers.unwrap_or_default();

    // Try to get provider status from Mihomo config.yaml
    let provider_statuses = tokio::task::spawn_blocking({
        move || {
            let paths = get_control_tower_paths();
            let content = std::fs::read_to_string(&paths.active_config_path).unwrap_or_default();
            let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content).unwrap_or_default();
            let mut statuses = serde_json::Map::new();
            if let Some(map) = yaml.get("rule-providers").and_then(|v| v.as_mapping()) {
                for (key, value) in map {
                    if let Some(name) = key.as_str() {
                        let behavior = value.get("behavior").and_then(|v| v.as_str()).unwrap_or("");
                        let type_ = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        let url = value.get("url").and_then(|v| v.as_str()).unwrap_or("");
                        let interval = value.get("interval").and_then(|v| v.as_u64()).unwrap_or(0);
                        statuses.insert(name.to_string(), serde_json::json!({
                            "name": name,
                            "behavior": behavior,
                            "type": type_,
                            "url": url,
                            "interval": interval,
                            "enabled": !matches!(value.get("type"), None | Some(serde_yaml_ng::Value::Null)),
                        }));
                    }
                }
            }
            statuses
        }
    }).await.unwrap_or_default();

    let result: Vec<_> = providers.into_iter().map(|mut p| {
        if let Some(status) = provider_statuses.get(&p.name) {
            if let Some(behavior) = status.get("behavior").and_then(|v| v.as_str()) {
                p.behavior = behavior.to_string();
            }
            if let Some(type_) = status.get("type").and_then(|v| v.as_str()) {
                p.type_ = type_.to_string();
            }
            if let Some(url) = status.get("url").and_then(|v| v.as_str()) {
                p.url = Some(url.to_string());
            }
            if let Some(interval) = status.get("interval").and_then(|v| v.as_u64()) {
                p.interval = interval as u32;
            }
        }
        serde_json::json!(p)
    }).collect();

    HttpResponse::Ok().json(ApiResponse::success(result))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleProviderRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub behavior: String,
    pub url: Option<String>,
    pub path: Option<String>,
    pub interval: Option<u32>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub lazy: Option<bool>,
    pub filter: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleProviderUpdateRequest {
    pub interval: Option<u32>,
}

/// POST /api/rule-providers — Add or update a rule provider
pub async fn add_rule_provider(
    shared_state: web::Data<Arc<ServiceState>>,
    body: web::Json<RuleProviderRequest>,
) -> HttpResponse {
    let req = body.into_inner();

    if req.name.is_empty() {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("Provider name is required".to_string()));
    }
    if req.type_ != "http" && req.type_ != "file" {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("Type must be 'http' or 'file'".to_string()));
    }
    if req.behavior != "domain" && req.behavior != "ipcidr" && req.behavior != "classical" {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("Behavior must be 'domain', 'ipcidr', or 'classical'".to_string()));
    }
    if req.type_ == "http" && req.url.is_none() {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("URL is required for HTTP providers".to_string()));
    }

    let provider = RuleProviderConfig {
        name: req.name.clone(),
        type_: req.type_.clone(),
        behavior: req.behavior.clone(),
        url: req.url.clone(),
        path: req.path.clone(),
        interval: req.interval.unwrap_or(86400),
        enabled: req.enabled.unwrap_or(true),
        lazy: req.lazy.unwrap_or(true),
        filter: req.filter.clone(),
    };

    let paths = get_control_tower_paths();
    let config_store = ActiveConfigStore::new(paths.clone());
    let state = ServiceState::new();
    state.load_settings();

    let result = tokio::task::spawn_blocking(move || {
        let mut settings = state.get_settings();
        let mut providers = settings.rule_providers.take().unwrap_or_default();

        // Remove existing with same name (update case)
        providers.retain(|p| p.name != provider.name);
        providers.push(provider.clone());
        settings.rule_providers = Some(providers.clone());

        // Auto-add RULE-SET rule to custom_rules for this provider
        let rule_set_rule = format!("RULE-SET,{},REJECT", provider.name);
        let mut custom_rules = settings.custom_rules.take().unwrap_or_default();
        if !custom_rules.contains(&rule_set_rule) {
            custom_rules.insert(0, rule_set_rule.clone());
            settings.custom_rules = Some(custom_rules);
        }

        state.save_settings(&settings)
            .map_err(|e| format!("Failed to save settings: {}", e))?;

        // Write to config.yaml: build rule-providers section and inject
        config_store.set_rule_providers(&providers)
            .map_err(|e| format!("Failed to set rule-providers in config: {}", e))?;

        // Also prepend the RULE-SET rule to config.yaml's rules array so Mihomo picks it up immediately
        config_store.prepend_rule(&rule_set_rule)
            .map_err(|e| format!("Failed to prepend rule to config: {}", e))?;

        Ok::<(), String>(())
    }).await;

    match result {
        Ok(Ok(())) => {
            // Reload using the shared state's api_port
            let api_port = *shared_state.api_port.read();
            let url = format!("http://127.0.0.1:{}/configs?force=true", api_port);
            match reqwest::Client::new().put(&url).json(&serde_json::json!({})).send().await {
                Ok(res) if res.status().is_success() => {
                    tracing::info!("Mihomo config hot-reloaded");
                }
                Ok(res) => {
                    tracing::warn!("Reload returned status: {}", res.status());
                }
                Err(e) => {
                    tracing::warn!("Reload failed: {}", e);
                }
            }
            tracing::info!("Rule provider '{}' added/updated", req.name);
            HttpResponse::Ok().json(ApiResponse::<()>::success(()))
        }
        Ok(Err(e)) => {
            HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(e))
        }
        Err(e) => {
            HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Task error: {}", e)))
        }
    }
}

/// PUT /api/rule-providers/{name} — Update interval for a rule provider
pub async fn update_rule_provider(
    shared_state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
    body: web::Json<RuleProviderUpdateRequest>,
) -> HttpResponse {
    let name = path.into_inner();
    let req = body.into_inner();
    let name_for_task = name.clone();

    let paths = get_control_tower_paths();
    let config_store = ActiveConfigStore::new(paths.clone());
    let state = ServiceState::new();
    state.load_settings();

    let result = tokio::task::spawn_blocking(move || {
        let mut settings = state.get_settings();
        let mut providers = settings.rule_providers.take().unwrap_or_default();

        let provider = match providers.iter_mut().find(|p| p.name == name_for_task) {
            Some(p) => p,
            None => return Err("Provider not found".to_string()),
        };

        if let Some(interval) = req.interval {
            provider.interval = interval;
        }

        settings.rule_providers = Some(providers.clone());
        state.save_settings(&settings)
            .map_err(|e| format!("Failed to save settings: {}", e))?;

        config_store.set_rule_providers(&providers)
            .map_err(|e| format!("Failed to update config: {}", e))?;

        Ok::<(), String>(())
    }).await;

    match result {
        Ok(Ok(())) => {
            // Use the shared state's api_port for the reload URL
            let api_port = *shared_state.api_port.read();
            let url = format!("http://127.0.0.1:{}/configs?force=true", api_port);
            match reqwest::Client::new().put(&url).json(&serde_json::json!({})).send().await {
                Ok(res) if res.status().is_success() => {
                    tracing::info!("Mihomo config hot-reloaded");
                }
                Ok(res) => {
                    tracing::warn!("Reload returned status: {}", res.status());
                }
                Err(e) => {
                    tracing::warn!("Reload failed: {}", e);
                }
            }
            tracing::info!("Rule provider '{}' updated", name);
            HttpResponse::Ok().json(ApiResponse::<()>::success(()))
        }
        Ok(Err(e)) => {
            if e == "Provider not found" {
                HttpResponse::NotFound().json(ApiResponse::<()>::error(e))
            } else {
                HttpResponse::InternalServerError()
                    .json(ApiResponse::<()>::error(e))
            }
        }
        Err(e) => {
            HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Task error: {}", e)))
        }
    }
}

/// DELETE /api/rule-providers/{name} — Delete a rule provider
pub async fn delete_rule_provider(
    shared_state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
) -> HttpResponse {
    let name = path.into_inner();
    let name_for_task = name.clone();

    let paths = get_control_tower_paths();
    let config_store = ActiveConfigStore::new(paths.clone());
    let state = ServiceState::new();
    state.load_settings();

    let result = tokio::task::spawn_blocking(move || {
        let mut settings = state.get_settings();
        let mut providers = settings.rule_providers.take().unwrap_or_default();
        let original_len = providers.len();
        providers.retain(|p| p.name != name_for_task);
        let deleted = providers.len() != original_len;

        if !deleted {
            return Err("Provider not found".to_string());
        }

        settings.rule_providers = Some(providers);

        // Remove the RULE-SET rule for this provider from custom_rules
        let rule_set_rule = format!("RULE-SET,{},REJECT", name_for_task);
        if let Some(mut custom_rules) = settings.custom_rules.take() {
            custom_rules.retain(|r| r != &rule_set_rule);
            settings.custom_rules = Some(custom_rules);
        }

        state.save_settings(&settings)
            .map_err(|e| format!("Failed to save settings: {}", e))?;

        config_store.remove_rule_provider(&name_for_task)
            .map_err(|e| format!("Failed to remove provider from config: {}", e))?;

        let rule_set_rule = format!("RULE-SET,{},REJECT", name_for_task);
        if let Err(e) = config_store.remove_rule_by_content(&rule_set_rule) {
            tracing::warn!("Failed to remove RULE-SET rule from config: {}", e);
        }

        Ok::<(), String>(())
    }).await;

    match result {
        Ok(Ok(())) => {
            // Use the shared state's api_port for the reload URL
            let api_port = *shared_state.api_port.read();
            let url = format!("http://127.0.0.1:{}/configs?force=true", api_port);
            match reqwest::Client::new().put(&url).json(&serde_json::json!({})).send().await {
                Ok(res) if res.status().is_success() => {
                    tracing::info!("Mihomo config hot-reloaded");
                }
                Ok(res) => {
                    tracing::warn!("Reload returned status: {}", res.status());
                }
                Err(e) => {
                    tracing::warn!("Reload failed: {}", e);
                }
            }
            tracing::info!("Rule provider '{}' deleted", name);
            HttpResponse::Ok().json(ApiResponse::<()>::success(()))
        }
        Ok(Err(e)) => {
            if e == "Provider not found" {
                HttpResponse::NotFound().json(ApiResponse::<()>::error(e))
            } else {
                HttpResponse::InternalServerError()
                    .json(ApiResponse::<()>::error(e))
            }
        }
        Err(e) => {
            HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Task error: {}", e)))
        }
    }
}

/// POST /api/rule-providers/refresh-all — Force refresh all rule providers
pub async fn refresh_all_rule_providers(state: web::Data<Arc<ServiceState>>) -> HttpResponse {
    let port = *state.api_port.read();
    let url = format!("http://127.0.0.1:{}/configs?force=true", port);
    match reqwest::Client::new().put(&url).json(&serde_json::json!({})).send().await {
        Ok(res) if res.status().is_success() => {
            tracing::info!("All rule providers refreshed via API");
            HttpResponse::Ok().json(ApiResponse::<()>::success(()))
        }
        Ok(res) => {
            HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Reload failed: {}", res.status())))
        }
        Err(e) => {
            HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Reload failed: {}", e)))
        }
    }
}
