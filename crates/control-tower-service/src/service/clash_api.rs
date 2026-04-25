//! Clash API: proxy/connection queries and proxy selection.

use std::time::Duration;

use reqwest::blocking::Client as BlockingClient;

use crate::ServiceState;

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

    /// Get connections from Clash API
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
        Ok(connections)
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
}
