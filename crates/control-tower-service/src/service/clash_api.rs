//! Clash API: proxy/connection queries and proxy selection.

use std::time::Duration;

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::blocking::Client as BlockingClient;

use crate::ServiceState;
use super::logging::ConnectionMetadata;

/// Location of the per-group proxy selections.
fn selections_path() -> std::path::PathBuf {
    crate::settings::resolve_paths()
        .config_dir
        .join("proxy-selections.yaml")
}

fn load_proxy_selections() -> std::collections::BTreeMap<String, String> {
    match std::fs::read_to_string(selections_path()) {
        Ok(content) => serde_yaml_ng::from_str(&content).unwrap_or_default(),
        Err(_) => std::collections::BTreeMap::new(),
    }
}

fn save_proxy_selection(group: &str, name: &str) -> Result<(), String> {
    // Selection writes come from independent HTTP requests; serialize them so
    // two groups selected at the same time do not overwrite each other.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let mut selections = load_proxy_selections();
    selections.insert(group.to_string(), name.to_string());
    let yaml = serde_yaml_ng::to_string(&selections)
        .map_err(|e| format!("Failed to serialize proxy selections: {}", e))?;
    let path = selections_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    control_tower_service_core::write_private_atomic(&path, &yaml)
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}


impl ServiceState {
    /// Get the Mihomo API base URL
    pub fn get_api_url(&self) -> String {
        let host = self.api_host.read().clone();
        let port = *self.api_port.read();
        format!("http://{}:{}", host, port)
    }

    /// Get proxies from Clash API
    pub fn get_proxies(&self) -> Result<serde_json::Value, String> {
        drop(self.manager.read());
        let url = format!("{}/proxies", self.get_api_url());
        let response = BlockingClient::new()
            .get(&url)
            .send()
            .map_err(|e| format!("Failed to query Clash API: {}", e))?;
        if !response.status().is_success() {
            return Err(format!("Clash API returned error: {}", response.status()));
        }
        let text = response.text().map_err(|e| format!("Failed to read response body: {}", e))?;
        let proxies: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse Clash response: {}", e))?;
        Ok(proxies)
    }

    /// Get configs from Clash API (includes current mode)
    pub fn get_configs(&self) -> Result<serde_json::Value, String> {
        drop(self.manager.read());
        let url = format!("{}/configs", self.get_api_url());
        let response = BlockingClient::new()
            .get(&url)
            .send()
            .map_err(|e| format!("Failed to query Clash API: {}", e))?;
        if !response.status().is_success() {
            return Err(format!("Clash API returned error: {}", response.status()));
        }
        let text = response.text().map_err(|e| format!("Failed to read response body: {}", e))?;
        let configs: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse Clash response: {}", e))?;
        Ok(configs)
    }

    /// Get connections from Clash API and track for history recording
    pub fn get_connections(&self) -> Result<serde_json::Value, String> {
        drop(self.manager.read());
        let url = format!("{}/connections", self.get_api_url());
        let response = BlockingClient::new()
            .get(&url)
            .send()
            .map_err(|e| format!("Failed to query Clash API: {}", e))?;
        if !response.status().is_success() {
            return Err(format!("Clash API returned error: {}", response.status()));
        }
        let text = response.text().map_err(|e| format!("Failed to read response body: {}", e))?;
        let connections: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse Clash response: {}", e))?;

        // Track connections and detect closed ones
        self.track_connections(&connections);

        Ok(connections)
    }

    /// Track active connections, detect and record closed ones to history
    fn track_connections(&self, connections: &serde_json::Value) {
        let conns = connections.get("connections").and_then(|c| c.as_array());

        let current_ids: std::collections::HashSet<String> = conns
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| c.get("id").and_then(|v| v.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // Get previous connections
        let mut active = self.active_connections.write();
        let previous_ids: std::collections::HashSet<String> = active.keys().cloned().collect();

        // Find newly closed connections (in previous but not in current)
        let closed_ids: Vec<String> = previous_ids.difference(&current_ids).cloned().collect();

        for id in &closed_ids {
            if let Some(meta) = active.remove(id) {
                let closed = crate::settings::ClosedConnection {
                    id: meta.id.clone(),
                    source_ip: meta.source_ip,
                    destination: meta.destination,
                    chains: meta.chains,
                    upload: meta.upload,
                    download: meta.download,
                    closed_at: chrono::Utc::now().to_rfc3339(),
                };
                tracing::info!("Recording closed connection to history: {}", closed.id);
                // Record to history
                if let Err(e) = self.record_closed_connection(closed) {
                    tracing::warn!("Failed to record closed connection: {}", e);
                }
            }
        }

        // Update active connections - iterate over the array if it exists
        if let Some(arr) = conns {
            for conn in arr {
            if let Some(id) = conn.get("id").and_then(|v| v.as_str()) {
                let chains: Vec<String> = conn
                    .get("chains")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();

                let metadata = conn.get("metadata");
                let source_ip = metadata.and_then(|m| m.get("sourceIP")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let dest_ip = metadata.and_then(|m| m.get("destinationIP")).and_then(|v| v.as_str()).unwrap_or("");
                let dest_port = metadata.and_then(|m| m.get("destinationPort")).and_then(|v| v.as_str()).unwrap_or("");
                let host = metadata.and_then(|m| m.get("host")).and_then(|v| v.as_str()).unwrap_or("");
                let destination = if !dest_ip.is_empty() {
                    format!("{}:{}", dest_ip, dest_port)
                } else if !host.is_empty() {
                    host.to_string()
                } else {
                    String::new()
                };
                let meta = ConnectionMetadata {
                    id: id.to_string(),
                    source_ip,
                    destination,
                    chains,
                    upload: conn.get("upload").and_then(|v| v.as_u64()).unwrap_or(0),
                    download: conn.get("download").and_then(|v| v.as_u64()).unwrap_or(0),
                };
                active.insert(id.to_string(), meta);
            }
        }
        }
    }

    /// Get connection metadata by ID (for history recording before close)
    pub fn get_connection_info(&self, id: &str) -> Result<Option<crate::settings::ClosedConnection>, String> {
        let connections = self.get_connections()?;
        let conns = connections.get("connections")
            .and_then(|c| c.as_array())
            .ok_or("Invalid connections response")?;

        for conn in conns {
            if conn.get("id").and_then(|v| v.as_str()) == Some(id) {
                let chains = conn.get("chains")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();

                let closed = {
                    let metadata = conn.get("metadata");
                    let source_ip = metadata.and_then(|m| m.get("sourceIP")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let dest_ip = metadata.and_then(|m| m.get("destinationIP")).and_then(|v| v.as_str()).unwrap_or("");
                    let dest_port = metadata.and_then(|m| m.get("destinationPort")).and_then(|v| v.as_str()).unwrap_or("");
                    let host = metadata.and_then(|m| m.get("host")).and_then(|v| v.as_str()).unwrap_or("");
                    let destination = if !dest_ip.is_empty() {
                        format!("{}:{}", dest_ip, dest_port)
                    } else if !host.is_empty() {
                        host.to_string()
                    } else {
                        String::new()
                    };
                    crate::settings::ClosedConnection {
                        id: conn.get("id").and_then(|v| v.as_str()).unwrap_or(id).to_string(),
                        source_ip,
                        destination,
                        chains,
                        upload: conn.get("upload").and_then(|v| v.as_u64()).unwrap_or(0),
                        download: conn.get("download").and_then(|v| v.as_u64()).unwrap_or(0),
                        closed_at: chrono::Utc::now().to_rfc3339(),
                    }
                };
                return Ok(Some(closed));
            }
        }
        Ok(None)
    }

    /// Close a specific connection by ID
    pub fn close_connection(&self, id: &str) -> Result<(), String> {
        drop(self.manager.read());
        let url = format!("{}/connections/{}", self.get_api_url(), id);
        let response = BlockingClient::new()
            .delete(&url)
            .send()
            .map_err(|e| format!("Failed to close connection: {}", e))?;
        if !response.status().is_success() {
            return Err(format!("Failed to close connection: {}", response.status()));
        }
        tracing::info!("Connection {} closed successfully", id);
        Ok(())
    }

    /// Select a proxy in the GLOBAL group.
    pub(crate) fn select_proxy(&self, name: &str) -> Result<(), String> {
        self.select_proxy_in("GLOBAL", name)
    }

    /// Select a proxy inside a specific proxy group (e.g. "节点选择").
    ///
    /// Mihomo exposes manual selection per selector group: `PUT
    /// /proxies/{group}`. The previous implementation always targeted GLOBAL,
    /// which is not the group most subscriptions route through, so clicking a
    /// node in the UI could leave the effective route unchanged.
    pub(crate) fn select_proxy_in(&self, group: &str, name: &str) -> Result<(), String> {
        let group = if group.trim().is_empty() {
            "GLOBAL"
        } else {
            group
        };
        let client = BlockingClient::new();
        let url = format!(
            "{}/proxies/{}",
            self.get_api_url(),
            utf8_percent_encode(group, NON_ALPHANUMERIC)
        );

        let response = client
            .put(&url)
            .json(&serde_json::json!({ "name": name }))
            .timeout(Duration::from_secs(5))
            .send()
            .map_err(|e| format!("Failed to select proxy in {}: {}", group, e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Failed to select proxy '{}' in group '{}': {}",
                name,
                group,
                response.status()
            ));
        }

        if let Err(e) = save_proxy_selection(group, name) {
            tracing::warn!("Failed to persist proxy selection: {}", e);
        }
        Ok(())
    }

    /// Re-apply per-group selections after a Mihomo start/restart.
    ///
    /// Selections are stored in `proxy-selections.yaml` because putting them in
    /// settings.yaml would make every settings save rewrite them, and the UI
    /// PUT handler has to copy dozens of unrelated fields.
    pub(crate) fn restore_proxy_selections(&self) {
        let mut selections = load_proxy_selections();
        // Compatibility with the old single `selected_proxy` key.
        if let Some(legacy) = self.load_selected_proxy() {
            selections.entry("GLOBAL".to_string()).or_insert(legacy);
        }
        for (group, name) in selections {
            match self.select_proxy_in(&group, &name) {
                Ok(()) => tracing::info!("Restored proxy selection in {}: {}", group, name),
                Err(e) => tracing::warn!("Failed to restore proxy selection in {}: {}", group, e),
            }
        }
    }

    /// Trigger Mihomo config hot-reload via PUT /configs?force=true
    pub fn reload_config(&self) -> Result<(), String> {
        drop(self.manager.read());
        let url = format!("{}/configs?force=true", self.get_api_url());
        let response = BlockingClient::new()
            .put(&url)
            .json(&serde_json::json!({}))
            .send()
            .map_err(|e| format!("Reload failed: {}", e))?;
        if !response.status().is_success() {
            return Err(format!("Reload failed: {}", response.status()));
        }
        tracing::info!("Mihomo config hot-reloaded successfully");
        Ok(())
    }
}
