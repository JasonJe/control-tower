//! ServiceState extension methods — split into focused submodules.
//!
//! Main entry: `service_state_ext.rs` which does `pub use service::*;`

mod lifecycle;
mod settings_ops;
mod config_ops;
mod clash_api;
mod logging;
mod latency_test;
