//! ServiceState extension methods — split into focused submodules.
//!
//! Main entry: `service_state_ext.rs` which does `pub use service::*;`

mod lifecycle;
pub(crate) mod settings_ops;
pub(crate) mod config_ops;
mod clash_api;
pub mod logging;
pub mod system_proxy;
pub mod watchdog;
pub mod stats;
pub(crate) mod config_store;
mod latency_test;
