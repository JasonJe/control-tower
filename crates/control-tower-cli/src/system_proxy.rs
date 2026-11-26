//! Desktop system-proxy control through the service daemon.
//!
//! The daemon applies changes as the service user, which is the user with a
//! desktop session, so the CLI does not need to discover DBus/GSettings itself.

use anyhow::Result;
use serde_json::Value;

use crate::SystemProxyAction;

fn print_status(status: &Value) {
    let enabled = status.get("enabled").and_then(Value::as_bool).unwrap_or(false);
    let supported = status.get("supported").and_then(Value::as_bool).unwrap_or(false);
    println!(
        "System proxy: {}",
        if enabled { "enabled" } else { "disabled" }
    );
    println!(
        "Desktop session: {}",
        if supported { "supported" } else { "unsupported" }
    );
    if let Some(backend) = status.get("backend").and_then(Value::as_str) {
        println!("Backend: {}", backend);
    }
    if let Some(host) = status.get("host").and_then(Value::as_str) {
        println!("Host: {}", host);
    }
    if let Some(port) = status.get("http_port").and_then(Value::as_u64) {
        println!("HTTP port: {}", port);
    }
    if let Some(port) = status.get("socks_port").and_then(Value::as_u64) {
        println!("SOCKS port: {}", port);
    }
    if let Some(error) = status.get("last_error").and_then(Value::as_str) {
        if !error.is_empty() {
            println!("Last error: {}", error);
        }
    }
    if let Some(message) = status.get("message").and_then(Value::as_str) {
        if !message.is_empty() {
            println!("Message: {}", message);
        }
    }
}

pub fn handle(action: SystemProxyAction) -> Result<()> {
    match action {
        SystemProxyAction::Status => {
            print_status(&crate::service::system_proxy_status()?);
            Ok(())
        }
        SystemProxyAction::On => {
            let status = crate::service::set_system_proxy(true)?;
            print_status(&status);
            Ok(())
        }
        SystemProxyAction::Off => {
            let status = crate::service::set_system_proxy(false)?;
            print_status(&status);
            Ok(())
        }
    }
}
