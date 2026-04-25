//! Delay / latency test handlers

use actix_web::{web, HttpResponse};
use futures::future::join_all;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use std::sync::Arc;
use tokio::time::{timeout, Duration};

use crate::ServiceState;

use super::{ApiResponse, ProxyDelayRequest, ProxyDelayPostRequest, ProxyDelayAllRequest, FastestResponse, LatencyResultDto};

/// GET /api/proxies/{name}/delay - Get proxy delay
pub async fn proxy_delay(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
    query: web::Query<ProxyDelayRequest>,
) -> HttpResponse {
    let name = path.into_inner();

    // Call Mihomo's delay API
    let api_url = state.get_api_url();
    let url = format!(
        "{}/proxies/{}/delay?timeout={}",
        api_url, name, query.timeout
    );

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(query.timeout + 1000))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to create HTTP client: {}", e)))
        }
    };

    match client.get(&url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(data) => HttpResponse::Ok().json(ApiResponse::success(data)),
                    Err(e) => HttpResponse::InternalServerError()
                        .json(ApiResponse::<()>::error(format!("Failed to parse response: {}", e))),
                }
            } else {
                HttpResponse::BadRequest()
                    .json(ApiResponse::<()>::error(format!("Delay check failed: {}", response.status())))
            }
        }
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to check proxy delay: {}", e))),
    }
}

/// POST /api/proxies/delay - Get proxy delay (JSON body)
pub async fn proxy_delay_post(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<ProxyDelayPostRequest>,
) -> HttpResponse {
    let name_or_idx = &body.name;
    let timeout_ms = body.timeout;
    let mode = body.mode.clone().unwrap_or_else(|| state.get_latency_test_mode());

    // Get the API URL from state
    let api_url = state.get_api_url();

    // Build a single HTTP client for all requests
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms + 2000))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to create HTTP client: {}", e)))
        }
    };

    // Step 1: Resolve name (index -> proxy name) and get current GLOBAL selection
    let proxies_url = format!("{}/proxies", api_url);
    let proxies_response = match client.get(&proxies_url).send().await {
        Ok(r) => r,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to fetch proxies: {}", e)))
        }
    };

    if !proxies_response.status().is_success() {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error(format!("Failed to get proxies: {}", proxies_response.status())));
    }

    let proxies_data: serde_json::Value = match proxies_response.json().await {
        Ok(d) => d,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to parse proxies response: {}", e)))
        }
    };

    // Get GLOBAL.all for index resolution
    let global_all = match proxies_data
        .get("proxies")
        .and_then(|p| p.get("GLOBAL"))
        .and_then(|g| g.get("all"))
        .and_then(|a| a.as_array())
    {
        Some(a) => a,
        None => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error("GLOBAL.all not found".to_string()))
        }
    };

    // Resolve proxy name from index or use directly
    let target_proxy = if name_or_idx.chars().all(|c| c.is_ascii_digit()) {
        let idx: usize = match name_or_idx.parse() {
            Ok(i) => i,
            Err(_) => {
                return HttpResponse::BadRequest()
                    .json(ApiResponse::<()>::error(format!("Invalid index: {}", name_or_idx)))
            }
        };
        if idx == 0 || idx > global_all.len() {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error(format!("Index {} out of range (1-{})", idx, global_all.len())));
        }
        global_all[idx - 1].as_str().unwrap_or(name_or_idx).to_string()
    } else {
        name_or_idx.clone()
    };

    tracing::debug!("Latency test: target={}, mode={}", target_proxy, mode);

    // All modes use Mihomo's /proxies/{name}/delay API (no GLOBAL switching needed).
    // - ping mode: ?timeout=...  (TCP ping, may not work for VLESS nodes)
    // - http mode: ?url=...&timeout=...  (HTTP test via proxy, works for all proxy types)
    let test_url = if mode == "ping" {
        format!(
            "{}/proxies/{}/delay?timeout={}",
            api_url,
            utf8_percent_encode(&*target_proxy, NON_ALPHANUMERIC),
            timeout_ms
        )
    } else {
        format!(
            "{}/proxies/{}/delay?url={}&timeout={}",
            api_url,
            utf8_percent_encode(&*target_proxy, NON_ALPHANUMERIC),
            "http%3A%2F%2Fcp.cloudflare.com%2Fgenerate_204",
            timeout_ms
        )
    };

    match client.get(&test_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(data) => {
                        let delay = data.get("delay").and_then(|v| v.as_i64()).unwrap_or(-1);
                        tracing::debug!("Latency test result: {}ms for {} (mode={})", delay, target_proxy, mode);
                        let delay_val = if delay >= 0 { serde_json::Value::from(delay) } else { serde_json::Value::Null };
                        return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({ "delay": delay_val })));
                    }
                    Err(e) => {
                        return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({ "delay": null, "error": format!("Parse error: {}", e) })));
                    }
                }
            } else {
                return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({ "delay": null, "error": format!("Mihomo error: {}", response.status()) })));
            }
        }
        Err(e) => {
            return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({ "delay": null, "error": e.to_string() })));
        }
    }
}

/// GET /api/proxies/fastest - Get auto speed test results
pub async fn get_fastest(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let auto_test = state.auto_test.read();
    let interval_minutes = (auto_test.interval_secs / 60).max(1) as u32;

    let results: Vec<LatencyResultDto> = auto_test.results.iter().map(|r| {
        LatencyResultDto {
            name: r.name.clone(),
            latency: r.latency,
            error: r.error.clone(),
        }
    }).collect();

    let fastest = auto_test.fastest.as_ref().map(|r| {
        LatencyResultDto {
            name: r.name.clone(),
            latency: r.latency,
            error: r.error.clone(),
        }
    });

    HttpResponse::Ok().json(ApiResponse::success(FastestResponse {
        enabled: auto_test.enabled,
        interval_minutes,
        last_test_at: auto_test.last_test_at,
        fastest,
        results,
    }))
}

/// POST /api/proxies/delay-all - Batch delay test for all proxies
pub async fn proxy_delay_all(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<ProxyDelayAllRequest>,
) -> HttpResponse {
    let timeout_ms = body.timeout.unwrap_or(5000);
    let mode = body.mode.clone().unwrap_or_else(|| state.get_latency_test_mode());

    let api_url = state.get_api_url();

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to create HTTP client: {}", e)))
        }
    };

    // Get all proxies
    let proxies_url = format!("{}/proxies", api_url);
    let proxies_response = match timeout(Duration::from_secs(15), client.get(&proxies_url).send()).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to fetch proxies: {}", e)))
        }
        Err(_) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error("Timeout fetching proxies list".to_string()))
        }
    };

    if !proxies_response.status().is_success() {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error(format!("Failed to get proxies: {}", proxies_response.status())));
    }

    let proxies_data: serde_json::Value = match timeout(Duration::from_secs(15), proxies_response.json()).await {
        Ok(Ok(d)) => d,
        Ok(Err(e)) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to parse proxies response: {}", e)))
        }
        Err(_) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error("Timeout parsing proxies response".to_string()))
        }
    };

    let proxies = match proxies_data.get("proxies") {
        Some(p) => p,
        None => {
            return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({ "results": [] })));
        }
    };

    // Get GLOBAL.all list (skip first 3: DIRECT, REJECT, FALLBACK)
    let global_all: Vec<String> = proxies
        .get("GLOBAL")
        .and_then(|g| g.get("all"))
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .skip(3) // Skip DIRECT, REJECT, FALLBACK
                .collect()
        })
        .unwrap_or_default();

    // Test each proxy concurrently with async reqwest
    use tokio::sync::Semaphore;
    let semaphore = Arc::new(Semaphore::new(30)); // Max 30 concurrent
    let client = Arc::new(client);

    let test_and_collect = |proxy_name: String| {
        let client = client.clone();
        let semaphore = semaphore.clone();
        let api_url = api_url.clone();
        let mode = mode.clone();

        async move {
            let _permit = semaphore.acquire().await.unwrap();
            let test_url = if mode == "ping" {
                format!(
                    "{}/proxies/{}/delay?timeout={}",
                    api_url,
                    utf8_percent_encode(&proxy_name, NON_ALPHANUMERIC),
                    timeout_ms
                )
            } else {
                format!(
                    "{}/proxies/{}/delay?url={}&timeout={}",
                    api_url,
                    utf8_percent_encode(&proxy_name, NON_ALPHANUMERIC),
                    "http%3A%2F%2Fcp.cloudflare.com%2Fgenerate_204",
                    timeout_ms
                )
            };

            match client.get(&test_url).send().await {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        match response.json::<serde_json::Value>().await {
                            Ok(data) => {
                                let delay = data.get("delay").and_then(|v| v.as_i64()).unwrap_or(-1);
                                if delay >= 0 {
                                    serde_json::json!({ "name": proxy_name, "delay": delay })
                                } else {
                                    serde_json::json!({ "name": proxy_name, "delay": null, "error": format!("BadResp: {:?}", data) })
                                }
                            }
                            Err(e) => serde_json::json!({ "name": proxy_name, "delay": null, "error": format!("Parse: {}", e) }),
                        }
                    } else {
                        serde_json::json!({ "name": proxy_name, "delay": null, "error": format!("HTTP {}", status) })
                    }
                }
                Err(e) => serde_json::json!({ "name": proxy_name, "delay": null, "error": e.to_string() }),
            }
        }
    };

    let futures: Vec<_> = global_all.into_iter().map(test_and_collect).collect();
    let results: Vec<serde_json::Value> = join_all(futures).await;

    // Compute fastest (lowest delay) from results
    let fastest = results
        .iter()
        .filter_map(|r| {
            let delay = r.get("delay")?.as_i64()?;
            if delay > 0 { Some((r.get("name")?.as_str()?.to_string(), delay)) } else { None }
        })
        .min_by_key(|(_, delay)| *delay)
        .map(|(name, delay)| serde_json::json!({ "name": name, "delay": delay }));

    HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
        "results": results,
        "fastest": fastest
    })))
}
