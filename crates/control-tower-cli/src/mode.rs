//! Mode management module

use anyhow::Result;
use serde_json::Value;

use crate::service::{get_mode, set_mode};
use crate::ModeAction;

const CLASH_API_HOST: &str = "127.0.0.1";

pub async fn handle(action: ModeAction) -> Result<()> {
    match action {
        ModeAction::Get => get_current_mode().await?,
        ModeAction::Set { mode } => set_current_mode(&mode).await?,
        ModeAction::Tun { action } => handle_tun(action).await?,
    }
    Ok(())
}

async fn get_current_mode() -> Result<()> {
    let mode = get_mode().await?;
    println!("Current mode: {}", mode);
    Ok(())
}

async fn set_current_mode(mode: &str) -> Result<()> {
    let valid_modes = ["rule", "global", "direct"];
    if !valid_modes.contains(&mode) {
        anyhow::bail!("Invalid mode: {}. Must be one of: rule, global, direct", mode);
    }

    set_mode(mode).await?;
    println!("Mode set to: {}", mode);
    Ok(())
}

async fn handle_tun(action: crate::TunAction) -> Result<()> {
    match action {
        crate::TunAction::Status => get_tun_status().await?,
        crate::TunAction::Enable => enable_tun().await?,
        crate::TunAction::Disable => disable_tun().await?,
    }
    Ok(())
}

async fn get_tun_config() -> Result<Value> {
    let url = format!("http://{}:{}/configs", CLASH_API_HOST, crate::settings::get_api_port());
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get config: {}", e))?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to get config: {}", response.status());
    }

    let json: Value = response.json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))?;

    Ok(json)
}

async fn put_config(config: &serde_json::Value) -> Result<()> {
    let url = format!("http://{}:{}/configs", CLASH_API_HOST, crate::settings::get_api_port());
    let client = reqwest::Client::new();
    let response = client
        .put(&url)
        .json(config)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to update config: {}", e))?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to update config: {}", response.status());
    }

    Ok(())
}

async fn get_tun_status() -> Result<()> {
    let config = get_tun_config().await?;

    if let Some(tun) = config.get("tun") {
        let enable = tun.get("enable").and_then(|v| v.as_bool()).unwrap_or(false);
        let stack = tun.get("stack").and_then(|v| v.as_str()).unwrap_or("unknown");
        let auto_route = tun.get("auto-route").and_then(|v| v.as_bool()).unwrap_or(false);

        println!("TUN Mode Status:");
        println!("  Enabled: {}", if enable { "Yes" } else { "No" });
        println!("  Stack: {}", stack);
        println!("  Auto-route: {}", if auto_route { "Yes" } else { "No" });

        if enable {
            println!("\nTUN mode is active. All system traffic is being routed through the proxy.");
        } else {
            println!("\nTUN mode is disabled.");
            println!("To enable TUN mode, use: ctctl mode tun enable");
            println!("Note: Enabling TUN may require service restart.");
        }
    } else {
        println!("TUN configuration not found in config.");
    }

    Ok(())
}

async fn enable_tun() -> Result<()> {
    let mut config = get_tun_config().await?;

    // Get or create tun section
    let tun = if let Some(v) = config.get_mut("tun") {
        v
    } else {
        let default_tun = serde_json::json!({
            "enable": true,
            "stack": "gVisor",
            "auto-route": true,
            "auto-detect-interface": true
        });
        config.as_object_mut().unwrap().insert("tun".to_string(), default_tun.clone());
        config.get_mut("tun").unwrap()
    };

    if let Some(obj) = tun.as_object_mut() {
        obj.insert("enable".to_string(), serde_json::Value::Bool(true));
        // Set recommended defaults if not already set
        obj.entry("stack".to_string()).or_insert(serde_json::Value::String("gVisor".to_string()));
        obj.entry("auto-route".to_string()).or_insert(serde_json::Value::Bool(true));
        obj.entry("auto-detect-interface".to_string()).or_insert(serde_json::Value::Bool(true));
    }

    put_config(&config).await?;

    println!("TUN mode enabled. Use 'ctctl service restart' to apply changes.");
    Ok(())
}

async fn disable_tun() -> Result<()> {
    let mut config = get_tun_config().await?;

    if let Some(tun) = config.get_mut("tun") {
        if let Some(obj) = tun.as_object_mut() {
            obj.insert("enable".to_string(), serde_json::Value::Bool(false));
        }
    }

    put_config(&config).await?;

    println!("TUN mode disabled. Use 'ctctl service restart' to apply changes.");
    Ok(())
}
