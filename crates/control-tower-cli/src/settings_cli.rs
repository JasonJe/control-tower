//! Read and write top-level keys in settings.yaml.
//!
//! The CLI deliberately edits the same YAML the service reads. Keys that
//! affect listeners (api_port/http_port/socks_port/service_port), TUN or
//! HTTPS require a daemon restart: `sudo systemctl restart ctsvc`.

use anyhow::Result;
use serde_json::Value;

use crate::SettingsAction;

fn settings_path() -> Result<std::path::PathBuf> {
    crate::settings::get_settings_path()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine settings.yaml path"))
}

fn read_yaml() -> Result<Value> {
    let path = settings_path()?;
    let content = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
    Ok(serde_yaml_ng::from_str(&content).unwrap_or(Value::Null))
}

pub fn handle(action: SettingsAction) -> Result<()> {
    match action {
        SettingsAction::Get { key } => {
            let path = settings_path()?;
            let yaml = read_yaml()?;
            match key {
                Some(key) => {
                    let value = yaml.get(&key).unwrap_or(&Value::Null);
                    if value.is_null() {
                        println!("{}: <not set>", key);
                    } else {
                        println!("{}", serde_yaml_ng::to_string(value)?.trim_end());
                    }
                }
                None => {
                    print!("{}", std::fs::read_to_string(&path)?);
                    println!("\nNote: values containing credentials are shown here; be careful with shell history/logs.");
                }
            }
            Ok(())
        }
        SettingsAction::Set { key, value } => {
            if key.is_empty() {
                anyhow::bail!("Key must not be empty");
            }
            let path = settings_path()?;
            let mut yaml = read_yaml()?;
            let object = yaml
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("settings.yaml root must be a mapping"))?;

            // Parse as YAML first so numbers/bools/lists work; fall back to a
            // plain string (e.g. "rule", "info", a name with spaces).
            let parsed: Value = serde_yaml_ng::from_str(&value)
                .unwrap_or_else(|_| Value::String(value.clone()));
            object.insert(key.clone(), parsed.clone());

            let content = serde_yaml_ng::to_string(&yaml)?;
            control_tower_service_core::write_private_atomic(&path, &content)?;
            let _ = crate::settings::reload_settings();

            println!("Set {}={} in {}", key, serde_yaml_ng::to_string(&parsed)?.trim_end(), path.display());
            if matches!(
                key.as_str(),
                "api_port" | "http_port" | "socks_port" | "service_port" | "tun_enabled" | "https"
            ) {
                println!("Restart the daemon to apply: sudo systemctl restart ctsvc");
            }
            Ok(())
        }
    }
}
