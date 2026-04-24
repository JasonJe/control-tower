//! Service Core - Core service functionality for TUI
//!
//! This module provides the core service functionality needed for the TUI
//! to manage Mihomo and handle IPC communication.

pub mod mihomo;
pub mod state;
pub mod config;
pub mod installer;
pub mod active_config;
pub mod profiles;

pub use state::{ServiceState, ServiceStatus};
pub use mihomo::MihomoManager;
#[allow(deprecated)]
pub use config::{ServiceConfig, ControlTowerPaths, exe_dir};
pub use installer::MihomoInstaller;
pub use active_config::ActiveConfigStore;
pub use profiles::{ProfileItem, ProfilesYaml};