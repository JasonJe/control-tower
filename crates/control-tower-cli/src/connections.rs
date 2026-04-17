//! Connections viewer module

use anyhow::Result;
use serde_json::Value;
use std::io::{self, Write};
use std::time::Duration;

use crate::service::{close_connection_via_ipc, get_connections_via_ipc};
use crate::ConnectionsAction;

pub async fn handle(action: ConnectionsAction) -> Result<()> {
    match action {
        ConnectionsAction::List => list_connections().await?,
        ConnectionsAction::Close { index } => close_by_index(index).await?,
        ConnectionsAction::Detail { index } => show_detail(index).await?,
        ConnectionsAction::Top { interval } => top_connections(interval).await?,
    }
    Ok(())
}

pub async fn list_connections() -> Result<()> {
    println!("Connections:");
    println!("(Use web UI at http://127.0.0.1:8080 for full connections view)");
    println!();

    // Try to get connections from Clash API
    match get_connections_via_ipc().await {
        Ok(conns) => {
            // Show traffic info
            if let Some(upload) = conns.get("uploadTotal").and_then(|v| v.as_u64()) {
                if let Some(download) = conns.get("downloadTotal").and_then(|v| v.as_u64()) {
                    println!("Total Traffic:");
                    println!("  Upload:   {}", format_bytes(upload));
                    println!("  Download: {}", format_bytes(download));
                    println!();
                }
            }

            // Get connection details
            if let Some(connections) = conns.get("connections") {
                if connections.is_null() {
                    println!("No active connections");
                    return Ok(());
                }

                if let Some(arr) = connections.as_array() {
                    if arr.is_empty() {
                        println!("No active connections");
                        return Ok(());
                    }

                    println!("Active connections: {}", arr.len());
                    println!("{}", "=".repeat(120));
                    println!("{:<6}  {:<50}  {:<12}  {:<15}  {:<40}", "ID", "Destination", "Type", "DL/UL", "Proxy");
                    println!("{}", "-".repeat(120));

                    for (idx, conn) in arr.iter().enumerate() {
                        let _id = conn.get("id").and_then(|v| v.as_str()).unwrap_or("-");
                        // Mihomo uses "metadata" not "meta"
                        let meta = conn.get("metadata").and_then(|v| v.as_object());

                        let dest = if let Some(host) = meta.and_then(|m| m.get("host")).and_then(|v| v.as_str()) {
                            host.to_string()
                        } else {
                            let ip = meta.and_then(|m| m.get("destinationIP")).and_then(|v| v.as_str()).unwrap_or("-");
                            let port = meta.and_then(|m| m.get("destinationPort")).and_then(|v| v.as_str()).unwrap_or("");
                            if port.is_empty() { ip.to_string() } else { format!("{}:{}", ip, port) }
                        };
                        let dest_display = truncate_str(&dest, 50);

                        let ptype = meta.and_then(|m| m.get("type")).and_then(|v| v.as_str()).unwrap_or("-");

                        // Use the stored selected proxy name
                        let proxy = crate::settings::get_selected_proxy().unwrap_or_else(|| "-".to_string());
                        let proxy_display = truncate_str(&proxy, 40);

                        // Calculate speed if available
                        let dl_bytes = conn.get("download").and_then(|v| v.as_u64()).unwrap_or(0);
                        let ul_bytes = conn.get("upload").and_then(|v| v.as_u64()).unwrap_or(0);
                        let dl_str = format_bytes(dl_bytes);
                        let ul_str = format_bytes(ul_bytes);
                        let speed_str = format!("{}/{}", dl_str, ul_str);

                        println!("{:<6}  {:<50}  {:<12}  {:<15}  {:<40}",
                            format!("{}.", idx + 1),
                            dest_display,
                            ptype,
                            speed_str,
                            proxy_display
                        );
                    }

                    println!();
                    println!("Tip: Use 'connections close <index>' to close a connection");
                    println!("     Use 'connections detail <index>' for detailed information");
                }
            }
        }
        Err(e) => {
            println!("Note: Could not fetch connections (service may not be running): {}", e);
        }
    }

    Ok(())
}

/// Close a connection by its display index (1-based)
async fn close_by_index(index: usize) -> Result<()> {
    let conns = get_connections_via_ipc().await?;

    let connections = conns.get("connections")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("No connections found"))?;

    if index == 0 || index > connections.len() {
        anyhow::bail!("Invalid index {}. Valid range: 1-{}", index, connections.len());
    }

    let conn = &connections[index - 1];
    let id = conn.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Connection has no ID"))?;

    let meta = conn.get("meta").and_then(|v| v.as_object());
    let dest = meta.and_then(|m| m.get("destination")).and_then(|v| v.as_str()).unwrap_or("unknown");

    println!("Closing connection {} (destination: {})...", id, dest);

    close_connection_via_ipc(id).await?;

    println!("Connection closed successfully");
    Ok(())
}

/// Show detailed information about a connection
async fn show_detail(index: usize) -> Result<()> {
    let conns = get_connections_via_ipc().await?;

    let connections = conns.get("connections")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("No connections found"))?;

    if index == 0 || index > connections.len() {
        anyhow::bail!("Invalid index {}. Valid range: 1-{}", index, connections.len());
    }

    let conn = &connections[index - 1];

    println!("{}", "=".repeat(60));
    println!("Connection Details (Index #{})", index);
    println!("{}", "=".repeat(60));

    // ID
    if let Some(id) = conn.get("id").and_then(|v| v.as_str()) {
        println!("ID: {}", id);
    }

    // Metadata information
    if let Some(meta) = conn.get("metadata").and_then(|v| v.as_object()) {
        println!("\nMetadata Information:");
        if let Some(v) = meta.get("destinationIP").and_then(|v| v.as_str()) {
            println!("  Destination IP: {}", v);
        }
        if let Some(v) = meta.get("destinationPort").and_then(|v| v.as_str()) {
            println!("  Destination Port: {}", v);
        }
        if let Some(v) = meta.get("sourceIP").and_then(|v| v.as_str()) {
            println!("  Source IP: {}", v);
        }
        if let Some(v) = meta.get("sourcePort").and_then(|v| v.as_str()) {
            println!("  Source Port: {}", v);
        }
        if let Some(v) = meta.get("type").and_then(|v| v.as_str()) {
            println!("  Type: {}", v);
        }
        if let Some(v) = meta.get("host").and_then(|v| v.as_str()) {
            println!("  Host: {}", v);
        }
        if let Some(v) = meta.get("dnsMode").and_then(|v| v.as_str()) {
            println!("  DNS Mode: {}", v);
        }
        if let Some(v) = meta.get("processPath").and_then(|v| v.as_str()) {
            println!("  Process: {}", v);
        }
        if let Some(v) = meta.get("user").and_then(|v| v.as_str()) {
            println!("  User: {}", v);
        }
        if let Some(v) = meta.get("proxy").and_then(|v| v.as_str()) {
            println!("  Proxy: {}", v);
        }
    }

    // Proxy (from connection metadata)
    if let Some(proxy) = conn.get("metadata").and_then(|v| v.as_object()).and_then(|m| m.get("proxy")).and_then(|v| v.as_str()) {
        println!("\nProxy: {}", proxy);
    }

    // Traffic information
    println!("\nTraffic:");
    if let Some(upload) = conn.get("upload").and_then(|v| v.as_u64()) {
        println!("  Upload:   {}", format_bytes(upload));
    }
    if let Some(download) = conn.get("download").and_then(|v| v.as_u64()) {
        println!("  Download: {}", format_bytes(download));
    }

    // Speed
    let speed = calculate_speed(conn);
    if speed > 0 {
        println!("  Speed:    {}", format_speed(speed));
    }

    // Chains
    if let Some(chains) = conn.get("chains").and_then(|v| v.as_array()) {
        println!("\nChains ({} hops):", chains.len());
        for (i, chain) in chains.iter().enumerate() {
            if let Some(name) = chain.as_str() {
                println!("  {}. {}", i + 1, name);
            }
        }
    }

    // Rule
    if let Some(rule) = conn.get("rule").and_then(|v| v.as_str()) {
        println!("\nRule: {}", rule);
    }

    // Time
    if let Some(time) = conn.get("time").and_then(|v| v.as_str()) {
        println!("\nTime: {}", time);
    }

    println!("{}", "=".repeat(60));

    Ok(())
}

/// Calculate total speed (upload + download) from connection metadata
fn calculate_speed(conn: &Value) -> u64 {
    let download_speed = conn.get("downloadSpeed").and_then(|v| v.as_u64()).unwrap_or(0);
    let upload_speed = conn.get("uploadSpeed").and_then(|v| v.as_u64()).unwrap_or(0);
    download_speed.saturating_add(upload_speed)
}

/// Format bytes into human readable format
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format speed in bytes per second
fn format_speed(bytes_per_sec: u64) -> String {
    let speed_bps = bytes_per_sec as f64;
    if speed_bps >= 1_073_741_824.0 {
        format!("{:.2} GB/s", speed_bps / 1_073_741_824.0)
    } else if speed_bps >= 1_048_576.0 {
        format!("{:.2} MB/s", speed_bps / 1_048_576.0)
    } else if speed_bps >= 1024.0 {
        format!("{:.2} KB/s", speed_bps / 1024.0)
    } else {
        format!("{:.2} B/s", speed_bps)
    }
}

/// Real-time connection monitor (like 'top')
async fn top_connections(interval: u64) -> Result<()> {
    use tokio::signal;

    // ANSI color codes
    const RESET: &str = "\x1B[0m";
    const BOLD: &str = "\x1B[1m";
    const DIM: &str = "\x1B[2m";
    const CYAN: &str = "\x1B[36m";
    const GREEN: &str = "\x1B[32m";
    const YELLOW: &str = "\x1B[33m";
    const RED: &str = "\x1B[31m";
    const MAGENTA: &str = "\x1B[35m";
    const BG_BLUE: &str = "\x1B[44m";

    println!();
    println!("{}[══════════════════════════════════════════════════════════════════════════════]{}", CYAN, RESET);
    println!("{}  {}⚡ Control Tower - Connection Monitor{}                        ", CYAN, BOLD, RESET);
    println!("{}[══════════════════════════════════════════════════════════════════════════════]{}\n", CYAN, RESET);

    let interval_duration = Duration::from_secs(interval);

    loop {
        // Clear screen
        print!("\x1B[2J\x1B[1;1H");
        io::stdout().flush().ok();

        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");

        // Try to get connections from Clash API
        match get_connections_via_ipc().await {
            Ok(conns) => {
                // Show traffic info
                let upload = conns.get("uploadTotal").and_then(|v| v.as_u64()).unwrap_or(0);
                let download = conns.get("downloadTotal").and_then(|v| v.as_u64()).unwrap_or(0);
                let memory = conns.get("memory").and_then(|v| v.as_u64()).unwrap_or(0);

                println!("{}[{}] {}更新: {}{}", DIM, now, RESET, CYAN, now);
                println!();

                // Stats boxes
                print!("  {}[ Traffic ]{}  ", BG_BLUE, RESET);
                print!("  {}[ RX ]{} {:>10}  ", GREEN, RESET, format_bytes(download));
                print!("  {}[ TX ]{} {:>10}  ", MAGENTA, RESET, format_bytes(upload));
                print!("  {}[ Memory ]{} {:>10}", YELLOW, RESET, format_bytes(memory));
                println!();
                println!();

                // Get connection details
                if let Some(connections) = conns.get("connections") {
                    if connections.is_null() || connections.as_array().map(|a| a.is_empty()).unwrap_or(true) {
                        println!("  {}✨ No active connections{}", DIM, RESET);
                    } else if let Some(arr) = connections.as_array() {
                        println!("  {}[ Active Connections: {} ]{}", CYAN, arr.len(), RESET);
                        println!();
                        println!("  {:<4}  {:<55}  {:<10}  {:<15}  {:<40}", "IDX", "Destination", "Type", "DL/UL", "Proxy");
                        println!("  {}", "-".repeat(125));

                        for (idx, conn) in arr.iter().enumerate() {
                            // Mihomo uses "metadata" not "meta"
                            let meta = conn.get("metadata").and_then(|v| v.as_object());

                            // Build destination from host or IP+port
                            let dest = if let Some(host) = meta.and_then(|m| m.get("host")).and_then(|v| v.as_str()) {
                                host.to_string()
                            } else {
                                let ip = meta.and_then(|m| m.get("destinationIP")).and_then(|v| v.as_str()).unwrap_or("-");
                                let port = meta.and_then(|m| m.get("destinationPort")).and_then(|v| v.as_str()).unwrap_or("");
                                if port.is_empty() {
                                    ip.to_string()
                                } else {
                                    format!("{}:{}", ip, port)
                                }
                            };
                            let dest_display = truncate_str(&dest, 55);

                            let ptype = meta.and_then(|m| m.get("type")).and_then(|v| v.as_str()).unwrap_or("-");

                            // Use the stored selected proxy name
                            let proxy = crate::settings::get_selected_proxy().unwrap_or_else(|| "-".to_string());
                            let proxy_short = truncate_str(&proxy, 40);

                            // Mihomo provides total bytes (download/upload), not real-time speed
                            let dl_bytes = conn.get("download").and_then(|v| v.as_u64()).unwrap_or(0);
                            let ul_bytes = conn.get("upload").and_then(|v| v.as_u64()).unwrap_or(0);
                            let dl_str = if dl_bytes > 0 { format_bytes(dl_bytes) } else { "-".to_string() };
                            let ul_str = if ul_bytes > 0 { format_bytes(ul_bytes) } else { "-".to_string() };

                            println!("  {:<4}  {:<55}  {:<10}  {:<15}  {:<40}",
                                idx + 1,
                                dest_display,
                                ptype,
                                format!("{}/{}", dl_str, ul_str),
                                proxy_short
                            );
                        }
                        println!("  {}", "-".repeat(125));
                    }
                } else {
                    println!("  {}✨ No active connections{}", DIM, RESET);
                }

                println!();
                println!("  {}⟳ Refresh: {}s  •  {}Ctrl+C to exit{}  •  {}Enter index to close{}", DIM, interval, DIM, RESET, CYAN, RESET);
            }
            Err(e) => {
                println!("  {}⚠ Error:{} {}", RED, e, RESET);
                println!("  Make sure the service is running.");
            }
        }

        io::stdout().flush().ok();

        // Wait for interval or Ctrl+C
        match tokio::time::timeout(interval_duration, signal::ctrl_c()).await {
            Ok(Ok(_)) => {
                println!("\n\n{}✓ Exiting monitor...{}\n", GREEN, RESET);
                break;
            }
            Ok(Err(_)) => {
                break;
            }
            Err(_) => {
                // Timeout - continue loop
            }
        }
    }

    Ok(())
}

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
