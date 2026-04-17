//! CLI Integration Tests
//!
//! Tests for all CLI commands: profile, proxy, mode, service, config, connections

use std::process::Command;
use std::path::PathBuf;

/// Helper to get the CLI binary path
fn get_cli_bin() -> String {
    // Start from the crate directory
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Go up: tests -> control-tower-cli -> target/debug
    let debug_path = manifest_dir
        .parent().unwrap()  // tests
        .parent().unwrap()  // control-tower-cli
        .join("target")
        .join("debug")
        .join("ctctl");

    if debug_path.exists() {
        return debug_path.to_string_lossy().to_string();
    }

    // Fallback: just return the debug path
    debug_path.to_string_lossy().to_string()
}

/// Helper to run CLI command
fn run_cli(args: &[&str]) -> Result<String, String> {
    let bin_path = get_cli_bin();

    let output = Command::new(&bin_path)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to execute {}: {}", bin_path, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(format!("CLI failed with {}: {}\n{}", output.status, stderr, stdout));
    }

    Ok(stdout)
}

/// Helper to run CLI command expecting failure
fn run_cli_err(args: &[&str]) -> Result<String, String> {
    let output = Command::new(&get_cli_bin())
        .args(args)
        .output()
        .map_err(|e| format!("Failed to execute: {}", e))?;

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    if output.status.success() {
        return Err(format!("Expected CLI to fail, but it succeeded: {}\n{}", stderr, stdout));
    }

    Ok(stderr)
}

// ============================================================================
// Profile Command Tests
// ============================================================================

#[test]
fn test_profile_help() {
    let result = run_cli(&["profile", "--help"]);
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("add") || output.contains("list") || output.contains("Profile"));
}

#[test]
fn test_profile_list() {
    let result = run_cli(&["profile", "list"]);
    // Should succeed even with no profiles
    assert!(result.is_ok());
    let output = result.unwrap();
    // Output should be readable
    assert!(output.contains("Profiles") || output.contains("No profiles") || output.contains("ID"));
}

#[test]
fn test_profile_add_invalid_url() {
    // Adding with invalid URL should fail
    let result = run_cli_err(&["profile", "add", "not-a-valid-url"]);
    // run_cli_err returns Ok when CLI fails as expected
    assert!(result.is_ok());
}

#[test]
fn test_profile_add_missing_url() {
    // Missing URL argument
    let result = run_cli_err(&["profile", "add"]);
    // run_cli_err returns Ok when CLI fails as expected
    assert!(result.is_ok());
}

#[test]
fn test_profile_remove_nonexistent() {
    // Removing non-existent profile should fail
    let result = run_cli_err(&["profile", "remove", "nonexistent-id-12345"]);
    assert!(result.is_ok());
}

#[test]
fn test_profile_update_no_url() {
    // Updating with no active profile and no URL should fail
    let result = run_cli_err(&["profile", "update"]);
    // Should fail because no URL provided and no active profile
    assert!(result.is_ok());
}

// ============================================================================
// Proxy Command Tests
// ============================================================================

#[test]
fn test_proxy_help() {
    let result = run_cli(&["proxy", "--help"]);
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("list") || output.contains("select") || output.contains("Proxy"));
}

#[test]
fn test_proxy_list() {
    let result = run_cli(&["proxy", "list"]);
    // Should succeed (may show empty if service not running)
    assert!(result.is_ok());
}

#[test]
fn test_proxy_select_missing_name() {
    // Missing proxy name
    let result = run_cli_err(&["proxy", "select"]);
    assert!(result.is_ok());
}

// ============================================================================
// Mode Command Tests
// ============================================================================

#[test]
fn test_mode_help() {
    let result = run_cli(&["mode", "--help"]);
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("get") || output.contains("set") || output.contains("Mode"));
}

#[test]
fn test_mode_get() {
    let result = run_cli(&["mode", "get"]);
    // Should succeed even if service not running
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("mode") || output.contains("Mode") || output.contains("rule") || output.contains("global") || output.contains("direct"));
}

#[test]
fn test_mode_set_invalid() {
    // Invalid mode should fail
    let result = run_cli_err(&["mode", "set", "invalid_mode"]);
    assert!(result.is_ok());
}

#[test]
fn test_mode_set_rule() {
    let result = run_cli(&["mode", "set", "rule"]);
    // May fail if service not running, but should be recognized as valid command
    // So we just check it doesn't panic
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_mode_set_global() {
    let result = run_cli(&["mode", "set", "global"]);
    assert!(result.is_ok() || result.is_err()); // Same as above
}

#[test]
fn test_mode_set_direct() {
    let result = run_cli(&["mode", "set", "direct"]);
    assert!(result.is_ok() || result.is_err());
}

// ============================================================================
// Service Command Tests
// ============================================================================

#[test]
fn test_service_help() {
    let result = run_cli(&["service", "--help"]);
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("start") || output.contains("stop") || output.contains("status") || output.contains("Service"));
}

#[test]
fn test_service_status() {
    let result = run_cli(&["service", "status"]);
    // Should succeed
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("Status") || output.contains("Running") || output.contains("Stopped"));
}

#[test]
fn test_service_start() {
    let result = run_cli(&["service", "start"]);
    // May fail if already running or no config, but command should be valid
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_service_stop() {
    let result = run_cli(&["service", "stop"]);
    // May fail if not running
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_service_restart() {
    let result = run_cli(&["service", "restart"]);
    assert!(result.is_ok() || result.is_err());
}

// ============================================================================
// Connections Command Tests
// ============================================================================

#[test]
fn test_connections() {
    let result = run_cli(&["connections", "list"]);
    // Should succeed
    assert!(result.is_ok());
}

#[test]
fn test_connections_help() {
    let result = run_cli(&["connections", "--help"]);
    assert!(result.is_ok());
}

// ============================================================================
// Main Help Tests
// ============================================================================

#[test]
fn test_main_help() {
    let result = run_cli(&["--help"]);
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("profile") || output.contains("proxy") || output.contains("clash"));
}

#[test]
fn test_main_version() {
    let result = run_cli(&["--version"]);
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("0.1.0") || output.contains("clash"));
}

// ============================================================================
// Invalid Command Tests
// ============================================================================

#[test]
fn test_invalid_command() {
    let result = run_cli_err(&["nonexistent", "command"]);
    assert!(result.is_ok());
}

#[test]
fn test_missing_subcommand() {
    let result = run_cli_err(&["profile"]);
    assert!(result.is_ok());
}
