//! Clash API: proxy/connection queries and proxy selection.

use std::time::Duration;

use reqwest::blocking::Client as BlockingClient;

use crate::ServiceState;
use super::logging::ConnectionMetadata;

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
        let conns = match connections.get("connections").and_then(|c| c.as_array()) {
            Some(c) => c,
            None => return,
        };

        let current_ids: std::collections::HashSet<String> = conns
            .iter()
            .filter_map(|c| c.get("id").and_then(|v| v.as_str()).map(String::from))
            .collect();

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

        // Update active connections
        for conn in conns {
            if let Some(id) = conn.get("id").and_then(|v| v.as_str()) {
                let chains: Vec<String> = conn
                    .get("chains")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();

                let meta = ConnectionMetadata {
                    id: id.to_string(),
                    source_ip: conn.get("sourceIP").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    destination: conn.get("destination").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    chains,
                    upload: conn.get("upload").and_then(|v| v.as_u64()).unwrap_or(0),
                    download: conn.get("download").and_then(|v| v.as_u64()).unwrap_or(0),
                };
                active.insert(id.to_string(), meta);
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

                let closed = crate::settings::ClosedConnection {
                    id: conn.get("id").and_then(|v| v.as_str()).unwrap_or(id).to_string(),
                    source_ip: conn.get("sourceIP").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    destination: conn.get("destination").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    chains,
                    upload: conn.get("upload").and_then(|v| v.as_u64()).unwrap_or(0),
                    download: conn.get("download").and_then(|v| v.as_u64()).unwrap_or(0),
                    closed_at: chrono::Utc::now().to_rfc3339(),
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

    /// Select a proxy via Clash API
    pub(crate) fn select_proxy(&self, name: &str) -> Result<(), String> {
        let client = BlockingClient::new();
        let url = format!("{}/proxies/GLOBAL", self.get_api_url());

        let response = client
            .put(&url)
            .json(&serde_json::json!({ "name": name }))
            .timeout(Duration::from_secs(5))
            .send()
            .map_err(|e| format!("Failed to select proxy: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("Failed to select proxy: {}", response.status()));
        }
        Ok(())
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
