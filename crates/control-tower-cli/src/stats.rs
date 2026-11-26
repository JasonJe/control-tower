//! Persistent traffic statistics (same data as the Web UI dashboard).

use anyhow::Result;
use serde_json::Value;

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let value = bytes as f64;
    if value >= GIB {
        format!("{:.2} GiB", value / GIB)
    } else if value >= MIB {
        format!("{:.2} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.1} KiB", value / KIB)
    } else {
        format!("{} B", bytes)
    }
}

fn totals_line(label: &str, obj: &Value) {
    let upload = obj.get("upload").and_then(Value::as_u64).unwrap_or(0);
    let download = obj.get("download").and_then(Value::as_u64).unwrap_or(0);
    let connections = obj.get("connections").and_then(Value::as_u64).unwrap_or(0);
    println!(
        "{:<10} ↑ {}  ↓ {}  {} connections",
        label,
        format_bytes(upload),
        format_bytes(download),
        connections
    );
}

pub fn handle(json: bool, reset: bool) -> Result<()> {
    if reset {
        crate::service::reset_stats()?;
        println!("Statistics cleared.");
    }

    let stats = crate::service::get_stats()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
        return Ok(());
    }

    println!("Traffic Statistics");
    println!("{}", "=".repeat(60));
    totals_line("Total", stats.get("total").unwrap_or(&Value::Null));
    totals_line("Today", stats.get("today").unwrap_or(&Value::Null));

    if let Some(daily) = stats.get("daily").and_then(Value::as_array) {
        println!("\nRecent days:");
        println!("{:<12}  {:>12}  {:>12}  {:>8}", "Date", "Upload", "Download", "Conns");
        println!("{}", "-".repeat(50));
        for day in daily.iter().take(7) {
            println!(
                "{:<12}  {:>12}  {:>12}  {:>8}",
                day.get("date").and_then(Value::as_str).unwrap_or("?"),
                format_bytes(day.get("upload").and_then(Value::as_u64).unwrap_or(0)),
                format_bytes(day.get("download").and_then(Value::as_u64).unwrap_or(0)),
                day.get("connections").and_then(Value::as_u64).unwrap_or(0)
            );
        }
    }

    if let Some(proxies) = stats.get("proxies").and_then(Value::as_array) {
        if !proxies.is_empty() {
            println!("\nTop proxies:");
            println!("{:<30}  {:>12}  {:>12}  {:>8}", "Proxy", "Upload", "Download", "Conns");
            println!("{}", "-".repeat(68));
            for proxy in proxies.iter().take(10) {
                println!(
                    "{:<30}  {:>12}  {:>12}  {:>8}",
                    proxy.get("name").and_then(Value::as_str).unwrap_or("?"),
                    format_bytes(proxy.get("upload").and_then(Value::as_u64).unwrap_or(0)),
                    format_bytes(proxy.get("download").and_then(Value::as_u64).unwrap_or(0)),
                    proxy.get("connections").and_then(Value::as_u64).unwrap_or(0)
                );
            }
        }
    }

    if let Some(destinations) = stats.get("destinations").and_then(Value::as_array) {
        if !destinations.is_empty() {
            println!("\nTop destinations:");
            println!("{:<40}  {:>8}", "Host", "Conns");
            println!("{}", "-".repeat(52));
            for entry in destinations.iter().take(10) {
                println!(
                    "{:<40}  {:>8}",
                    entry.get("host").and_then(Value::as_str).unwrap_or("?"),
                    entry.get("connections").and_then(Value::as_u64).unwrap_or(0)
                );
            }
        }
    }

    Ok(())
}
