//! Rules handlers: list, add, delete, clear

use actix_web::{web, HttpResponse};

use super::{ApiResponse, AddRuleRequest};

/// GET /api/config - Returns verge.yaml as JSON
pub async fn get_config() -> HttpResponse {
    let paths = super::get_control_tower_paths();
    let verge_path = &paths.verge_config_path;

    if !verge_path.exists() {
        // Return empty config instead of 404 so the UI handles it gracefully
        return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({})));
    }

    match std::fs::read_to_string(verge_path) {
        Ok(content) => {
            // Parse YAML to JSON
            match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content) {
                Ok(json) => HttpResponse::Ok().json(ApiResponse::success(json)),
                Err(e) => HttpResponse::InternalServerError()
                    .json(ApiResponse::<()>::error(format!("Failed to parse verge.yaml: {}", e))),
            }
        }
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to read verge.yaml: {}", e))),
    }
}

/// GET /api/rules - Returns list of rules from config.yaml
pub async fn get_rules() -> HttpResponse {
    let paths = super::get_control_tower_paths();
    let config_path = &paths.active_config_path;

    if !config_path.exists() {
        return HttpResponse::Ok().json(ApiResponse::success(Vec::<String>::new()));
    }

    match std::fs::read_to_string(config_path) {
        Ok(content) => {
            match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content) {
                Ok(yaml) => {
                    let rules: Vec<String> = yaml
                        .get("rules")
                        .and_then(|v| v.as_sequence())
                        .map(|seq| {
                            seq.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    HttpResponse::Ok().json(ApiResponse::success(rules))
                }
                Err(_) => HttpResponse::Ok().json(ApiResponse::success(Vec::<String>::new())),
            }
        }
        Err(_) => HttpResponse::Ok().json(ApiResponse::success(Vec::<String>::new())),
    }
}

/// POST /api/rules - Add a new rule
pub async fn add_rule(body: web::Json<AddRuleRequest>) -> HttpResponse {
    let paths = super::get_control_tower_paths();
    let store = control_tower_service_core::ActiveConfigStore::new(paths);

    let rule = format!("{},{},{}", body.rule_type.to_uppercase(), body.value, body.proxy);

    match store.append_rule(&rule) {
        Ok(()) => HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "rule": rule
        }))),
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to add rule: {}", e))),
    }
}

/// DELETE /api/rules/{index} - Remove a rule by 1-based index
pub async fn delete_rule(path: web::Path<usize>) -> HttpResponse {
    let index = path.into_inner();
    let paths = super::get_control_tower_paths();
    let store = control_tower_service_core::ActiveConfigStore::new(paths);

    match store.remove_rule(index) {
        Ok(removed) => HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "removed": removed
        }))),
        Err(e) => HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error(format!("Failed to remove rule: {}", e))),
    }
}

/// DELETE /api/rules - Clear all rules
pub async fn clear_rules() -> HttpResponse {
    let paths = super::get_control_tower_paths();
    let store = control_tower_service_core::ActiveConfigStore::new(paths);

    match store.clear_rules() {
        Ok(()) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to clear rules: {}", e))),
    }
}
