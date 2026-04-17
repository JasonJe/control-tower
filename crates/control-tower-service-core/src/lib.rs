//! Service Core - Core service functionality for TUI
//!
//! This module provides the core service functionality needed for the TUI
//! to manage Mihomo and handle IPC communication.

pub mod mihomo;
pub mod state;
pub mod config;
pub mod installer;

pub use state::{ServiceState, ServiceStatus};
pub use mihomo::MihomoManager;
pub use config::ServiceConfig;
pub use installer::MihomoInstaller;