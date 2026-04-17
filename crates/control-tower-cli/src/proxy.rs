//! Proxy management module

use anyhow::Result;
use std::time::{Duration, Instant};

use crate::service::{get_clash_proxies, select_proxy as service_select_proxy};
use crate::ProxyAction;

const CLASH_API_HOST: &str = "127.0.0.1";
const CLASH_PROXY_PORT: u16 = 7890;  // Mixed proxy port for HTTP/SOCKS5

/// Truncate string to max_width characters, showing start and end with "..." in middle if truncated
fn truncate_str(s: &str, max_width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let char_count = chars.len();
    if char_count <= max_width {
        return s.to_string();
    }

    // Need at least 5 chars: start + "..." + end (3 dots = 3)
    if max_width < 5 {
        return chars[..max_width].iter().collect();
    }

    // Calculate: total = start_len + 3 (dots) + end_len
    // So: start_len + end_len = max_width - 3
    let remaining = max_width - 3;
    let start_len = remaining / 2;
    let end_len = remaining - start_len;

    // Start: first start_len characters
    let start: String = chars[..start_len].iter().collect();
    // End: last end_len characters
    let end: String = chars[char_count - end_len..].iter().collect();

    format!("{}...{}", start, end)
}

pub async fn handle(action: ProxyAction) -> Result<()> {
    match action {
        ProxyAction::List => list_proxies().await?,
        ProxyAction::Select { name } => select_proxy_node(&name).await?,
        ProxyAction::Test { name } => test_proxy(name.as_deref()).await?,
        ProxyAction::Stats => show_stats().await?,
    }
    Ok(())
}

/// Get all proxy names from GLOBAL group
async fn get_global_proxy_list() -> Result<Vec<String>> {
    let proxies = get_clash_proxies().await?;

    if let Some(global) = proxies.get("proxies").and_then(|p| p.get("GLOBAL")) {
        if let Some(all) = global.get("all").and_then(|a| a.as_array()) {
            return Ok(all.iter().filter_map(|v| v.as_str().map(String::from)).collect());
        }
    }

    Ok(vec![])
}

async fn list_proxies() -> Result<()> {
    let proxies = get_clash_proxies().await?;

    // Collect all proxy names from GLOBAL group for indexing
    let proxy_names = get_global_proxy_list().await?;

    println!("Proxy Nodes ({} total):", proxy_names.len());
    println!("{:<6}  {:<35}  {:<12}  {:<10}", "Index", "Name", "Type", "Status");
    println!("{}", "-".repeat(68));

    // Parse and display proxies with index
    if let Some(proxies_obj) = proxies.get("proxies") {
        if let Some(proxies_map) = proxies_obj.as_object() {
            for (idx, proxy_name) in proxy_names.iter().enumerate() {
                if let Some(proxy_info) = proxies_map.get(proxy_name) {
                    let ptype = proxy_info.get("type").and_then(|v| v.as_str()).unwrap_or("?");
                    let alive = proxy_info.get("alive").and_then(|v| v.as_bool()).unwrap_or(false);
                    let status = if alive { "●" } else { "✗" };

                    println!("{:<6}  {:<35}  {:<12}  {:<10}",
                        format!("[{}]", idx + 1),
                        truncate_str(proxy_name, 35),
                        ptype,
                        status
                    );
                } else {
                    println!("{:<6}  {:<35}", format!("[{}]", idx + 1), truncate_str(proxy_name, 35));
                }
            }
        }
    }

    println!();
    println!("Tip: Use index number to select: clash proxy select 5");
    println!("     Or use name directly: clash proxy select \"香港 101\"");

    Ok(())
}

async fn select_proxy_node(selector: &str) -> Result<()> {
    let proxy_names = get_global_proxy_list().await?;

    let proxy_name = if let Some(stripped) = selector.strip_prefix('#') {
        // Index selection: #1, #2, etc.
        let idx: usize = stripped
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid index format. Use #1, #2, etc."))?;

        if idx == 0 || idx > proxy_names.len() {
            anyhow::bail!("Index {} out of range (1-{})", idx, proxy_names.len());
        }

        tracing::info!("Selecting proxy by index {}: {}", idx, proxy_names[idx - 1]);
        proxy_names[idx - 1].clone()
    } else if selector.chars().all(|c| c.is_ascii_digit()) {
        // Direct index selection: 1, 2, 58, 59, etc. (without # prefix)
        let idx: usize = selector
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid index format"))?;

        if idx == 0 || idx > proxy_names.len() {
            anyhow::bail!("Index {} out of range (1-{})", idx, proxy_names.len());
        }

        tracing::info!("Selecting proxy by index {}: {}", idx, proxy_names[idx - 1]);
        proxy_names[idx - 1].clone()
    } else {
        // Name selection - check if it's a partial match
        let selector_lower = selector.to_lowercase();
        let matches: Vec<_> = proxy_names.iter()
            .filter(|name| name.to_lowercase().contains(&selector_lower))
            .collect();

        match matches.len() {
            0 => anyhow::bail!("No proxy found matching: {}", selector),
            1 => {
                let name = (*matches[0]).clone();
                tracing::info!("Selecting proxy (matched): {}", name);
                name
            }
            _ => {
                // Multiple matches - pick the first one that's an exact match, otherwise the first
                let exact_idx = matches.iter().position(|n| n.to_lowercase() == selector_lower);
                match exact_idx {
                    Some(idx) => {
                        let name = (*matches[idx]).clone();
                        tracing::info!("Selecting proxy (exact match): {}", name);
                        name
                    }
                    None => {
                        println!("Multiple matches found, selecting first one:");
                        for (i, m) in matches.iter().enumerate() {
                            println!("  {}. {}", i + 1, m);
                        }
                        println!();
                        let name = (*matches[0]).clone();
                        tracing::info!("Selecting proxy (first match): {}", name);
                        name
                    }
                }
            }
        }
    };

    service_select_proxy(&proxy_name).await?;
    crate::settings::set_selected_proxy(proxy_name.clone());
    println!("Selected proxy: {}", proxy_name);
    Ok(())
}

/// Resolve proxy name or index to actual proxy name
async fn resolve_proxy_name(selector: &str) -> Result<String> {
    let proxy_names = get_global_proxy_list().await?;

    if let Some(stripped) = selector.strip_prefix('#') {
        // Index selection: #1, #2, etc.
        let idx: usize = stripped
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid index format. Use #1, #2, etc."))?;

        if idx == 0 || idx > proxy_names.len() {
            anyhow::bail!("Index {} out of range (1-{})", idx, proxy_names.len());
        }
        Ok(proxy_names[idx - 1].clone())
    } else if selector.chars().all(|c| c.is_ascii_digit()) {
        // Direct index selection: 1, 2, 58, 59, etc. (without # prefix)
        let idx: usize = selector
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid index format"))?;

        if idx == 0 || idx > proxy_names.len() {
            anyhow::bail!("Index {} out of range (1-{})", idx, proxy_names.len());
        }
        Ok(proxy_names[idx - 1].clone())
    } else {
        // Name selection - check if it's a partial match
        let selector_lower = selector.to_lowercase();
        let matches: Vec<_> = proxy_names.iter()
            .filter(|name| name.to_lowercase().contains(&selector_lower))
            .collect();

        match matches.len() {
            0 => anyhow::bail!("No proxy found matching: {}", selector),
            1 => Ok((*matches[0]).clone()),
            _ => {
                let exact_idx = matches.iter().position(|n| n.to_lowercase() == selector_lower);
                match exact_idx {
                    Some(idx) => Ok((*matches[idx]).clone()),
                    None => {
                        println!("Multiple matches found, selecting first one:");
                        for (i, m) in matches.iter().enumerate() {
                            println!("  {}. {}", i + 1, m);
                        }
                        Ok((*matches[0]).clone())
                    }
                }
            }
        }
    }
}

async fn test_proxy(name: Option<&str>) -> Result<()> {
    let test_url = "http://cp.cloudflare.com/generate_204";
    let timeout_ms = 10000;

    if let Some(name) = name {
        // Resolve index to proxy name (same logic as select_proxy_node)
        let proxy_name = resolve_proxy_name(name).await?;
        println!("Testing proxy: {}", proxy_name);
        match test_single_proxy(&proxy_name, test_url, timeout_ms).await {
            Ok(delay) => {
                if delay > 0 {
                    println!("  Delay: {}ms", delay);
                } else {
                    println!("  Error: Timeout or unreachable");
                }
            }
            Err(e) => println!("  Error: {}", e),
        }
    } else {
        println!("Testing GLOBAL group proxies...");
        println!("Test URL: {}", test_url);
        println!("Timeout: {}ms", timeout_ms);
        println!();

        let proxies = get_clash_proxies().await?;

        if let Some(global) = proxies.get("proxies").and_then(|p| p.get("GLOBAL")) {
            if let Some(all) = global.get("all").and_then(|a| a.as_array()) {
                println!("{:<35}  {:<12}", "Name", "Delay");
                println!("{}", "-".repeat(52));

                for item in all {
                    if let Some(proxy_name) = item.as_str() {
                        match test_single_proxy(proxy_name, test_url, timeout_ms).await {
                            Ok(delay) => {
                                let delay_str = if delay > 0 { format!("{}ms", delay) } else { "Error".to_string() };
                                println!("{:<35}  {:<12}", truncate_str(proxy_name, 35), delay_str);
                            }
                            Err(_) => {
                                println!("{:<35}  {:<12}", truncate_str(proxy_name, 35), "Error");
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Get the currently selected proxy in GLOBAL group
async fn get_current_global_proxy() -> Result<String> {
    let proxies = get_clash_proxies().await?;

    if let Some(global) = proxies.get("proxies").and_then(|p| p.get("GLOBAL")) {
        if let Some(now) = global.get("now").and_then(|v| v.as_str()) {
            return Ok(now.to_string());
        }
    }

    anyhow::bail!("Could not find current GLOBAL proxy")
}

/// Test a single proxy by temporarily setting it as GLOBAL and making a request
async fn test_single_proxy(name: &str, test_url: &str, timeout_ms: u64) -> Result<u64> {
    // Get current GLOBAL selection to restore later
    let original_proxy = get_current_global_proxy().await?;

    // Temporarily select the proxy to test
    service_select_proxy(name).await?;

    // Make the test request through the system proxy (which now routes through our target)
    let delay = measure_http_delay(test_url, timeout_ms).await;

    // Restore original proxy selection
    if let Err(e) = service_select_proxy(&original_proxy).await {
        tracing::warn!("Failed to restore original proxy {}: {}", original_proxy, e);
    }

    delay
}

/// Measure HTTP request delay through the system proxy
async fn measure_http_delay(url: &str, timeout_ms: u64) -> Result<u64> {
    let start = Instant::now();

    // We need to go through the Clash HTTP proxy at 127.0.0.1:9090
    // The system proxy is already configured to use Clash, so we just need
    // to make a request to an external URL

    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::http(format!("http://{}:{}", CLASH_API_HOST, CLASH_PROXY_PORT))?)
        .proxy(reqwest::Proxy::https(format!("http://{}:{}", CLASH_API_HOST, CLASH_PROXY_PORT))?)
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Request failed: {}", e))?;

    if !response.status().is_success() && response.status().as_u16() != 204 {
        anyhow::bail!("HTTP error: {}", response.status());
    }

    let elapsed = start.elapsed().as_millis() as u64;
    Ok(elapsed)
}

/// Show statistics for all proxies
async fn show_stats() -> Result<()> {
    use std::collections::HashMap;
    use crate::service::get_connections;

    println!("Proxy Statistics");
    println!("{}", "=".repeat(70));

    // Get all proxy names
    let proxy_names = get_global_proxy_list().await?;

    // Get current connections to count per-proxy usage
    let mut conn_count: HashMap<String, usize> = HashMap::new();
    if let Ok(conns) = get_connections().await {
        if let Some(arr) = conns.get("connections").and_then(|v| v.as_array()) {
            for conn in arr {
                // Mihomo API uses "chains" array, first element is the actual exit proxy
                if let Some(proxy) = conn.get("chains")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.as_str())
                {
                    *conn_count.entry(proxy.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    // Get proxy information and calculate statistics
    let proxies = get_clash_proxies().await?;

    println!("\n{:<6}  {:<30}  {:<10}  {:<12}  {:<12}",
        "Index", "Name", "Type", "Health", "Connections");
    println!("{}", "-".repeat(76));

    let mut stats: Vec<(usize, String, String, bool, usize)> = Vec::new();

    if let Some(proxies_obj) = proxies.get("proxies") {
        if let Some(proxies_map) = proxies_obj.as_object() {
            for (idx, proxy_name) in proxy_names.iter().enumerate() {
                if let Some(proxy_info) = proxies_map.get(proxy_name) {
                    let ptype = proxy_info.get("type").and_then(|v| v.as_str()).unwrap_or("?");
                    let alive = proxy_info.get("alive").and_then(|v| v.as_bool()).unwrap_or(false);
                    let conns = conn_count.get(proxy_name).copied().unwrap_or(0);

                    stats.push((idx + 1, proxy_name.clone(), ptype.to_string(), alive, conns));
                } else {
                    stats.push((idx + 1, proxy_name.clone(), "?".to_string(), false, 0));
                }
            }
        }
    }

    // Sort by connection count (most used first), then by health
    stats.sort_by(|a, b| {
        let a_score = if a.3 { 1 } else { 0 } * 10000 + a.4;
        let b_score = if b.3 { 1 } else { 0 } * 10000 + b.4;
        b_score.cmp(&a_score)
    });

    for (idx, name, ptype, alive, conns) in &stats {
        let health = if *alive { "● Healthy" } else { "✗ Dead" };

        println!("{:<6}  {:<30}  {:<10}  {:<12}  {:<12}",
            format!("[{}]", idx),
            truncate_str(name, 30),
            ptype,
            health,
            conns
        );
    }

    // Summary statistics
    let total_proxies = proxy_names.len();
    let healthy_count = stats.iter().filter(|s| s.3).count();
    let total_connections: usize = stats.iter().map(|s| s.4).sum();

    println!("\n{}", "-".repeat(70));
    println!("Summary:");
    println!("  Total proxies: {}", total_proxies);
    println!("  Healthy: {}", healthy_count);
    println!("  Dead: {}", total_proxies - healthy_count);
    println!("  Active connections: {}", total_connections);

    println!("\nTip: Use 'ctctl proxy test' for latency details");

    Ok(())
}
