//! Auto latency testing: run_auto_latency_test + async helpers.

use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::Client as ReqwestClient;
use tokio::sync::Semaphore;
use tokio::runtime::Runtime;

use crate::{LatencyResult, ServiceState};

impl ServiceState {
    /// Run automatic latency test on all proxies.
    /// Returns the number of proxies tested.
    pub fn run_auto_latency_test(&self) -> usize {
        let settings = self.get_settings();
        let auto_test_cfg = match settings.auto_test {
            Some(cfg) => cfg,
            None => {
                tracing::debug!("Auto test config not found, skipping");
                return 0;
            }
        };

        if !auto_test_cfg.enabled {
            return 0;
        }

        {
            let mut st = self.auto_test.write();
            st.last_test_at = Some(chrono::Utc::now().timestamp());
        }

        if !self.is_running() {
            tracing::warn!("Mihomo not running, skipping auto latency test");
            return 0;
        }

        let api_url = self.get_api_url();

        let client = match reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to create HTTP client: {}", e);
                return 0;
            }
        };

        let proxies_url = format!("{}/proxies", api_url);
        let proxies_response = match client.get(&proxies_url).send() {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Failed to fetch proxies for latency test: {}", e);
                return 0;
            }
        };

        if !proxies_response.status().is_success() {
            tracing::warn!("Failed to get proxies: {}", proxies_response.status());
            return 0;
        }

        let proxies_data: serde_json::Value = match proxies_response.json() {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Failed to parse proxies response: {}", e);
                return 0;
            }
        };

        let global_all = match proxies_data
            .get("proxies")
            .and_then(|p| p.get("GLOBAL"))
            .and_then(|g| g.get("all"))
            .and_then(|a| a.as_array())
        {
            Some(a) => a,
            None => {
                tracing::warn!("GLOBAL.all not found in proxies response");
                return 0;
            }
        };

        let skip_count = 3;
        let all_names: Vec<String> = global_all
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();

        let nodes_to_test: Vec<&str> = all_names
            .iter()
            .skip(skip_count)
            .map(|s| s.as_str())
            .collect();

        if nodes_to_test.is_empty() {
            tracing::debug!("No proxy nodes to test");
            let mut state = self.auto_test.write();
            state.enabled = true;
            state.interval_secs = (auto_test_cfg.interval_minutes as u64) * 60;
            state.last_test_at = Some(chrono::Utc::now().timestamp());
            state.fastest = None;
            state.results.clear();
            return 0;
        }

        let timeout_ms = 8000;
        let latency_mode = auto_test_cfg.latency_test_mode.as_deref().unwrap_or("http");
        let total_count = nodes_to_test.len();
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("Failed to create tokio runtime for latency test: {}", e);
                return 0;
            }
        };
        let results: Vec<LatencyResult> = rt.block_on(test_concurrent_latency(
            api_url,
            nodes_to_test,
            timeout_ms,
            latency_mode,
        ));

        let fastest_name = results.iter()
            .filter(|r| r.latency.is_some())
            .min_by_key(|r| r.latency.unwrap())
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "N/A".to_string());

        let mut state = self.auto_test.write();
        state.enabled = true;
        state.interval_secs = (auto_test_cfg.interval_minutes as u64) * 60;
        state.last_test_at = Some(chrono::Utc::now().timestamp());
        state.fastest = results.iter()
            .filter(|r| r.latency.is_some())
            .min_by_key(|r| r.latency.unwrap())
            .cloned();
        state.results = results;

        tracing::info!("Auto latency test completed, tested {} nodes, fastest: {}",
            total_count, fastest_name);
        self.append_log(format!("Auto test: {} nodes tested, fastest={}", total_count, fastest_name));
        total_count
    }
}

/// Test a single proxy node's latency asynchronously.
#[allow(dead_code)]
pub(crate) async fn test_node_latency(
    client: &ReqwestClient,
    api_url: &str,
    node_name: &str,
    timeout_ms: u64,
    latency_mode: &str,
) -> LatencyResult {
    let delay_url = if latency_mode == "http" {
        format!(
            "{}/proxies/{}/delay?url={}&timeout={}",
            api_url,
            utf8_percent_encode(node_name, NON_ALPHANUMERIC),
            "http%3A%2F%2Fcp.cloudflare.com%2Fgenerate_204",
            timeout_ms
        )
    } else {
        format!(
            "{}/proxies/{}/delay?timeout={}",
            api_url,
            utf8_percent_encode(node_name, NON_ALPHANUMERIC),
            timeout_ms
        )
    };

    match client.get(&delay_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(data) => {
                        let delay = data.get("delay").and_then(|v| v.as_i64()).unwrap_or(-1);
                        if delay >= 0 {
                            LatencyResult {
                                name: node_name.to_string(),
                                latency: Some(delay),
                                error: None,
                            }
                        } else {
                            LatencyResult {
                                name: node_name.to_string(),
                                latency: None,
                                error: Some("Negative delay".into()),
                            }
                        }
                    }
                    Err(_) => LatencyResult {
                        name: node_name.to_string(),
                        latency: None,
                        error: Some("Parse error".into()),
                    },
                }
            } else {
                LatencyResult {
                    name: node_name.to_string(),
                    latency: None,
                    error: Some(format!("Mihomo error (HTTP {})", response.status().as_u16())),
                }
            }
        }
        Err(_) => LatencyResult {
            name: node_name.to_string(),
            latency: None,
            error: Some("Timeout".into()),
        },
    }
}

/// Run concurrent latency tests on all proxy nodes.
/// Semaphore limits concurrency to MAX_CONCURRENT (avoids overwhelming Mihomo).
pub(crate) async fn test_concurrent_latency(
    api_url: String,
    nodes_to_test: Vec<&str>,
    timeout_ms: u64,
    latency_mode: &str,
) -> Vec<LatencyResult> {
    const MAX_CONCURRENT: usize = 20;
    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT));

    let client = match ReqwestClient::builder()
        .timeout(Duration::from_millis(timeout_ms as u64 + 3000))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to create async HTTP client: {}", e);
            return nodes_to_test
                .iter()
                .map(|name| LatencyResult {
                    name: (*name).to_string(),
                    latency: None,
                    error: Some("Client init failed".into()),
                })
                .collect();
        }
    };

    let api_url_for_task = api_url.clone();
    let latency_mode_for_task = latency_mode.to_string();
    let timeout_ms_for_task = timeout_ms;

    let results: Vec<LatencyResult> = stream::iter(nodes_to_test)
        .map(|node_name| {
            let sem = sem.clone();
            let client = client.clone();
            let api_url = api_url_for_task.clone();
            let latency_mode = latency_mode_for_task.clone();
            let node_name = node_name.to_string();
            async move {
                let _permit = sem.acquire().await.expect("semaphore not closed");
                test_node_latency(
                    &client,
                    &api_url,
                    &node_name,
                    timeout_ms_for_task,
                    &latency_mode,
                )
                .await
            }
        })
        .buffer_unordered(MAX_CONCURRENT)
        .collect()
        .await;

    results
}
