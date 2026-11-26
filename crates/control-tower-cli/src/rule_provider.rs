//! Rule provider inspection and refresh.

use anyhow::Result;
use serde_json::Value;
use std::time::Duration;

use crate::RuleProviderAction;

const CLASH_API_HOST: &str = "127.0.0.1";

/// Read `rule-providers` from settings.yaml and enrich with config.yaml status.
fn list_providers() -> Result<Vec<Value>> {
    let paths = crate::settings::shared_paths()?;
    let settings: Value = std::fs::read_to_string(&paths.settings_path)
        .ok()
        .and_then(|content| serde_yaml_ng::from_str(&content).ok())
        .unwrap_or(Value::Null);
    let configured = settings
        .get("rule-providers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let config: Value = std::fs::read_to_string(&paths.active_config_path)
        .ok()
        .and_then(|content| serde_yaml_ng::from_str(&content).ok())
        .unwrap_or(Value::Null);

    Ok(configured
        .into_iter()
        .map(|mut item| {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            if let Some(status) = config
                .get("rule-providers")
                .and_then(Value::as_object)
                .and_then(|map| map.get(&name))
            {
                for key in ["behavior", "type", "url", "interval"] {
                    if let Some(value) = status.get(key) {
                        item[key] = value.clone();
                    }
                }
            }
            item
        })
        .collect())
}

fn print_providers(providers: &[Value]) {
    if providers.is_empty() {
        println!("No rule providers configured.");
        return;
    }
    println!("Rule Providers ({}):", providers.len());
    println!("{:<28}  {:<10}  {:<12}  {:>8}  {}", "Name", "Type", "Behavior", "Interval", "URL");
    println!("{}", "-".repeat(110));
    for provider in providers {
        println!(
            "{:<28}  {:<10}  {:<12}  {:>8}  {}",
            provider.get("name").and_then(Value::as_str).unwrap_or("?"),
            provider.get("type").and_then(Value::as_str).unwrap_or("?"),
            provider.get("behavior").and_then(Value::as_str).unwrap_or("?"),
            provider.get("interval").and_then(Value::as_u64).unwrap_or(0),
            provider.get("url").and_then(Value::as_str).unwrap_or("")
        );
    }
}

/// Trigger a refresh through Mihomo's external controller.
async fn mihomo_request(method: reqwest::Method, path: &str) -> Result<()> {
    let url = format!(
        "http://{}:{}{}",
        CLASH_API_HOST,
        crate::settings::get_api_port(),
        path
    );
    let response = reqwest::Client::new()
        .request(method, &url)
        .json(&serde_json::json!({}))
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to contact Mihomo API: {}", e))?;
    if !response.status().is_success() {
        anyhow::bail!("Mihomo API returned {}", response.status());
    }
    Ok(())
}

pub async fn handle(action: RuleProviderAction) -> Result<()> {
    match action {
        RuleProviderAction::List { json } => {
            let providers = list_providers()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&providers)?);
            } else {
                print_providers(&providers);
            }
            Ok(())
        }
        RuleProviderAction::Refresh { name, all } => {
            if all {
                // Same path the Web UI uses for "Refresh all": force a config
                // reload, which makes Mihomo re-fetch every provider.
                mihomo_request(reqwest::Method::PUT, "/configs?force=true").await?;
                println!("All rule providers refreshed.");
                return Ok(());
            }
            let name = name.ok_or_else(|| anyhow::anyhow!("Specify a provider name or use --all"))?;
            let encoded = path_segment(&name);
            mihomo_request(
                reqwest::Method::PUT,
                &format!("/providers/rules/{}", encoded),
            )
            .await?;
            println!("Rule provider refreshed: {}", name);
            Ok(())
        }
    }
}

/// Minimal percent-encoding for a URL path segment (Mihomo group/provider
/// names are often Chinese). Avoids pulling another dependency into the CLI.
fn path_segment(input: &str) -> String {
    let mut out = String::new();
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_utf8_name() {
        assert_eq!(path_segment("中文"), "%E4%B8%AD%E6%96%87");
    }
}
