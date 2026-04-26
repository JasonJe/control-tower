//! CLI entry point helpers: argument parsing, signal handlers, and service port resolution.

use crate::settings::consts::DEFAULT_SERVICE_PORT;
#[allow(unused_imports)]
use clap::Parser;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

pub use crate::ipc_types::SHUTDOWN;

/// CLI argument parser for the ctsvc binary.
#[derive(clap::Parser)]
#[command(version = "0.1.0")]
#[command(about = "Control Tower Service - IPC server for proxy management")]
pub struct Args {
    /// Socket path for IPC (default: ctsvc.sock in executable directory)
    #[arg(short, long)]
    pub socket: Option<PathBuf>,

    /// Log level (default: info)
    #[arg(short, long, default_value = "info")]
    pub log_level: String,

    /// Run in foreground (don't daemonize)
    #[arg(short, long, default_value = "false")]
    pub foreground: bool,
}

impl Args {
    /// Parse CLI arguments using clap
    pub fn parse() -> Self {
        <Self as clap::Parser>::parse()
    }
}

/// Setup signal handlers for SIGTERM and SIGINT
pub fn setup_signal_handlers() {
    SHUTDOWN.store(false, Ordering::SeqCst);

    unsafe {
        signal_hook::low_level::register(signal_hook::consts::SIGTERM, || {
            SHUTDOWN.store(true, Ordering::SeqCst);
        }).ok();
        signal_hook::low_level::register(signal_hook::consts::SIGINT, || {
            SHUTDOWN.store(true, Ordering::SeqCst);
        }).ok();
    }

    tracing::info!("Signal handlers registered (SIGTERM, SIGINT)");
}

/// Get the service HTTP port from settings.yaml, defaulting to DEFAULT_SERVICE_PORT
pub fn get_service_port() -> u16 {
    let settings_path = control_tower_service_core::exe_dir().join("settings.yaml");

    if settings_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&settings_path) {
            if let Ok(settings) = serde_yaml_ng::from_str::<Settings>(&content) {
                return settings.service_port.unwrap_or(DEFAULT_SERVICE_PORT);
            }
        }
    }
    DEFAULT_SERVICE_PORT
}

#[derive(serde::Deserialize)]
struct Settings {
    service_port: Option<u16>,
}
