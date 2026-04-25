//! Logs handler

use actix_web::{web, HttpResponse};

use super::{ApiResponse, LogsQuery};

/// GET /api/logs - Read log files (ctsvc or mihomo)
pub async fn get_logs(
    query: web::Query<LogsQuery>,
) -> HttpResponse {
    let log_dir = std::path::PathBuf::from("/opt/ctsvc/logs");
    let lines = query.lines.unwrap_or(200).min(1000);

    let file_path = match query.source.as_deref() {
        Some("mihomo") => log_dir.join("mihomo.log"),
        _ => {
            let entries = std::fs::read_dir(&log_dir).ok()
                .into_iter().flatten().flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("ctsvc.log."))
                .max_by_key(|e| e.file_name().to_string_lossy().to_string());
            match entries {
                Some(e) => e.path(),
                None => log_dir.join("ctsvc.log"),
            }
        }
    };

    if !file_path.exists() {
        return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "items": Vec::<String>::new(),
            "source": query.source.clone().unwrap_or_else(|| "ctsvc".to_string()),
            "total_lines": 0
        })));
    }

    let content = std::fs::read_to_string(&file_path).unwrap_or_default();
    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len();
    let start = if all_lines.len() > lines { all_lines.len() - lines } else { 0 };
    let items: Vec<String> = all_lines[start..].iter().map(|s| s.to_string()).collect();

    HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
        "items": items,
        "source": query.source.clone().unwrap_or_else(|| "ctsvc".to_string()),
        "total_lines": total_lines
    })))
}
