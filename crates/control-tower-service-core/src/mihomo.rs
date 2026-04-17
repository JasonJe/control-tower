//! Mihomo subprocess management

use crate::state::CircuitBreaker;
use std::path::PathBuf;
use std::process::{Command, Child};
use anyhow::Result;

/// Mihomo manager - handles Mihomo subprocess lifecycle
pub struct MihomoManager {
    mihomo_path: Option<PathBuf>,
    config_path: Option<PathBuf>,
    child: Option<Child>,
    /// Circuit breaker for auto-restart
    circuit_breaker: CircuitBreaker,
}

impl Default for MihomoManager {
    fn default() -> Self {
        Self::new()
    }
}

impl MihomoManager {
    pub fn new() -> Self {
        Self {
            mihomo_path: None,
            config_path: None,
            child: None,
            circuit_breaker: CircuitBreaker::new(),
        }
    }

    /// Set the Mihomo binary path
    pub fn with_mihomo_path(mut self, path: PathBuf) -> Self {
        self.mihomo_path = Some(path);
        self
    }

    /// Set the config file path
    pub fn with_config_path(mut self, path: PathBuf) -> Self {
        self.config_path = Some(path);
        self
    }

    /// Get the Mihomo binary path, or search in common locations
    pub fn get_mihomo_path(&self) -> Option<PathBuf> {
        if let Some(ref path) = self.mihomo_path {
            return Some(path.clone());
        }

        self.find_in_common_locations()
    }

    /// Check if Mihomo binary exists
    pub fn is_mihomo_available(&self) -> bool {
        self.get_mihomo_path().map(|p| p.exists()).unwrap_or(false)
    }

    /// Ensure Mihomo is available, downloading if necessary
    /// Returns the path to Mihomo binary
    pub fn ensure_mihomo(&mut self) -> Result<PathBuf> {
        // If we already have a path and it exists, return it
        if let Some(ref path) = self.mihomo_path
            && path.exists() {
                return Ok(path.clone());
            }

        // Try to find in common locations
        if let Some(path) = self.find_in_common_locations() {
            self.mihomo_path = Some(path.clone());
            return Ok(path);
        }

        // Try to auto-download
        tracing::info!("Mihomo not found, attempting to download...");
        let installer = crate::installer::MihomoInstaller::new(
            crate::installer::MihomoInstaller::default_install_dir()
        );

        match installer.install() {
            Ok(path) => {
                self.mihomo_path = Some(path.clone());
                Ok(path)
            }
            Err(e) => {
                anyhow::bail!("Failed to download Mihomo: {}. Please install Mihomo manually or provide path.", e)
            }
        }
    }

    /// Find Mihomo in common system locations
    fn find_in_common_locations(&self) -> Option<PathBuf> {
        // First try executable's directory/mihomo/ (working directory structure from build.sh)
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let path = exe_dir.join("mihomo").join("mihomo");
                if path.exists() {
                    return Some(path);
                }
                // Also check exe_dir/mihomo (backup for alternative structure)
                let alt_path = exe_dir.join("mihomo");
                if alt_path.exists() && alt_path.is_file() {
                    return Some(alt_path);
                }
            }
        }

        let possible_paths = [
            PathBuf::from("./mihomo/mihomo"),
            PathBuf::from("./mihomo"),
            PathBuf::from("/usr/local/bin/mihomo"),
            PathBuf::from("/usr/bin/mihomo"),
        ];

        for path in possible_paths {
            if path.exists() {
                return Some(path);
            }
        }

        None
    }

    /// Validate that Mihomo is executable
    pub fn validate_mihomo(&self) -> Result<()> {
        let path = self.get_mihomo_path()
            .ok_or_else(|| anyhow::anyhow!("Mihomo binary not found"))?;

        if !path.exists() {
            anyhow::bail!("Mihomo binary not found at {:?}", path);
        }

        // Check if executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&path)?;
            let mode = metadata.permissions().mode();
            if mode & 0o111 == 0 {
                anyhow::bail!("Mihomo binary is not executable");
            }
        }

        Ok(())
    }

    /// Build Mihomo start command
    pub fn build_start_command(&self, config_path: &PathBuf) -> Result<Command> {
        let mihomo_path = self.get_mihomo_path()
            .ok_or_else(|| anyhow::anyhow!("Mihomo binary not found"))?;

        let mut cmd = Command::new(mihomo_path);
        cmd.arg("-f").arg(config_path);
        Ok(cmd)
    }

    /// Check if Mihomo is currently running
    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            return child.try_wait().ok().flatten().is_none();
        }
        false
    }

    /// Get the process ID of Mihomo if running
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(|child| child.id())
    }

    /// Start Mihomo with the given config file
    /// This will automatically download Mihomo if not found
    /// This respects the circuit breaker for auto-restart
    pub fn start(&mut self, config_path: &PathBuf) -> Result<()> {
        if self.is_running() {
            self.circuit_breaker.record_success();
            return Ok(());
        }

        // Check circuit breaker
        if !self.circuit_breaker.is_allowed() {
            let remaining = self.circuit_breaker.remaining_cooldown_secs()
                .unwrap_or(0);
            anyhow::bail!(
                "Circuit breaker is active, {} seconds remaining until retry",
                remaining
            );
        }

        // Ensure Mihomo is available (download if necessary)
        let mihomo_path = match self.ensure_mihomo() {
            Ok(path) => path,
            Err(e) => {
                self.circuit_breaker.record_failure();
                return Err(e);
            }
        };
        tracing::info!("Starting Mihomo from {:?}", mihomo_path);

        // Verify config file exists before starting
        if !config_path.exists() {
            self.circuit_breaker.record_failure();
            anyhow::bail!("Config file not found: {:?}", config_path);
        }

        // Create logs directory and mihomo log file
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let log_dir = exe_dir.join("logs");
                let _ = std::fs::create_dir_all(&log_dir);
                let log_file = log_dir.join("mihomo.log");

                let mut cmd = Command::new(&mihomo_path);
                cmd.arg("-f").arg(config_path);

                // Copy geoip to ~/.config/mihomo/ if it exists locally
                let local_geoip = exe_dir.join("mihomo").join("geoip.metadb");
                if local_geoip.exists() {
                    let config_dir = dirs::config_dir()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("mihomo");
                    let target_geoip = config_dir.join("geoip.metadb");
                    if !target_geoip.exists() {
                        let _ = std::fs::create_dir_all(&config_dir);
                        if let Err(e) = std::fs::copy(&local_geoip, &target_geoip) {
                            tracing::warn!("Failed to copy geoip to ~/.config/mihomo/: {}", e);
                        } else {
                            tracing::info!("Copied geoip to: {}", target_geoip.display());
                        }
                    }
                }

                // Copy geosite to ~/.config/mihomo/ if it exists locally
                let local_geosite = exe_dir.join("mihomo").join("geosite.db");
                if local_geosite.exists() {
                    let config_dir = dirs::config_dir()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("mihomo");
                    let target_geosite = config_dir.join("geosite.db");
                    if !target_geosite.exists() {
                        let _ = std::fs::create_dir_all(&config_dir);
                        if let Err(e) = std::fs::copy(&local_geosite, &target_geosite) {
                            tracing::warn!("Failed to copy geosite to ~/.config/mihomo/: {}", e);
                        } else {
                            tracing::info!("Copied geosite to: {}", target_geosite.display());
                        }
                    }
                }

                // Redirect stdout and stderr to log file
                let log_fd = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_file)?;
                cmd.stdout(log_fd.try_clone()?);
                cmd.stderr(log_fd);

                tracing::info!("Mihomo logging to: {}", log_file.display());

                let child = match cmd.spawn() {
                    Ok(c) => c,
                    Err(e) => {
                        self.circuit_breaker.record_failure();
                        anyhow::bail!("Failed to spawn Mihomo: {}", e)
                    }
                };
                self.child = Some(child);
                self.config_path = Some(config_path.clone());
            }
        }

        // Wait a brief moment to verify process stays running
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Check if process is still running (it might have exited due to config error)
        if !self.is_running() {
            if let Some(ref mut child) = self.child
                && let Ok(exit_status) = child.try_wait()
                    && let Some(status) = exit_status {
                        self.child = None;
                        self.circuit_breaker.record_failure();
                        anyhow::bail!("Mihomo exited immediately with status: {}", status);
                    }
            self.child = None;
            self.circuit_breaker.record_failure();
            anyhow::bail!("Mihomo exited immediately after start");
        }

        // Success!
        self.circuit_breaker.record_success();
        Ok(())
    }

    /// Check if circuit breaker allows restart attempts
    pub fn can_restart(&self) -> bool {
        self.circuit_breaker.is_allowed()
    }

    /// Get remaining cooldown seconds
    pub fn remaining_cooldown_secs(&self) -> Option<u64> {
        self.circuit_breaker.remaining_cooldown_secs()
    }

    /// Get failure count
    pub fn failure_count(&self) -> u32 {
        self.circuit_breaker.failure_count()
    }

    /// Stop Mihomo if running
    pub fn stop(&mut self) -> Result<()> {
        if let Some(ref mut child) = self.child {
            child.kill()?;
            let _ = child.wait();
        }
        self.child = None;
        Ok(())
    }
}

impl Drop for MihomoManager {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mihomo_manager_default() {
        let manager = MihomoManager::default();
        assert!(manager.mihomo_path.is_none());
        assert!(manager.config_path.is_none());
    }

    #[test]
    fn test_mihomo_manager_with_paths() {
        let mihomo_path = PathBuf::from("/usr/bin/mihomo");
        let config_path = PathBuf::from("/tmp/config.yaml");

        let manager = MihomoManager::new()
            .with_mihomo_path(mihomo_path.clone())
            .with_config_path(config_path.clone());

        assert_eq!(manager.mihomo_path, Some(mihomo_path));
        assert_eq!(manager.config_path, Some(config_path));
    }

    #[test]
    fn test_get_mihomo_path_with_explicit_path() {
        let manager = MihomoManager::new()
            .with_mihomo_path(PathBuf::from("/custom/path/mihomo"));

        assert_eq!(manager.get_mihomo_path(), Some(PathBuf::from("/custom/path/mihomo")));
    }

    #[test]
    fn test_build_start_command_requires_mihomo() {
        // With the new auto-download behavior, build_start_command only requires
        // that get_mihomo_path returns Some (it may not exist yet, download happens later)
        // But when no path is set AND no Mihomo found in common locations, it returns None
        let manager = MihomoManager::new();
        // Without explicit path set, get_mihomo_path searches common locations
        // which may or may not find one depending on the system
        let mihomo_path = manager.get_mihomo_path();

        // If no Mihomo found, build_start_command should fail
        if mihomo_path.is_none() {
            let result = manager.build_start_command(&PathBuf::from("/tmp/config.yaml"));
            assert!(result.is_err(), "build_start_command should fail when no Mihomo found");
        } else {
            // If Mihomo found, build_start_command should succeed
            let result = manager.build_start_command(&PathBuf::from("/tmp/config.yaml"));
            assert!(result.is_ok(), "build_start_command should succeed when Mihomo found");
        }
    }

    #[test]
    fn test_validate_mihomo_not_found() {
        let manager = MihomoManager::new()
            .with_mihomo_path(PathBuf::from("/nonexistent/mihomo"));

        let result = manager.validate_mihomo();
        assert!(result.is_err());
    }

    #[test]
    fn test_is_running_when_not_started() {
        let mut manager = MihomoManager::new();
        assert!(!manager.is_running());
    }

    #[test]
    fn test_pid_when_not_started() {
        let manager = MihomoManager::new();
        assert_eq!(manager.pid(), None);
    }

    #[test]
    fn test_stop_when_not_running() {
        let mut manager = MihomoManager::new();
        let result = manager.stop();
        assert!(result.is_ok());
    }

    #[test]
    fn test_start_with_nonexistent_mihomo() {
        // When Mihomo path is explicitly set but doesn't exist,
        // ensure_mihomo will try to download instead of failing immediately
        let mut manager = MihomoManager::new()
            .with_mihomo_path(PathBuf::from("/nonexistent/mihomo"));

        // This will try to auto-download Mihomo
        // The test fails if download also fails (no network), but the behavior
        // is that it DOES try to download, not immediate failure
        let result = manager.start(&PathBuf::from("/tmp/config.yaml"));

        // If network is available, it might succeed (or fail for other reasons)
        // If network is not available, it will fail with download error
        // Either way, it should NOT fail with "binary not found" without trying
        if result.is_err() {
            let err = result.unwrap_err().to_string();
            // Should not be the old error "Mihomo binary not found"
            assert!(!err.contains("Mihomo binary not found"),
                "Should try to download before failing, but got: {}", err);
        }
    }

    #[test]
    fn test_start_twice_is_idempotent() {
        use std::os::unix::fs::PermissionsExt;

        // Clean up any existing fake mihomo processes and files from previous runs
        let fake_mihomo = PathBuf::from("/tmp/fake_mihomo");
        let fake_config = PathBuf::from("/tmp/fake_mihomo_config.yaml");
        let _ = std::process::Command::new("pkill")
            .arg("-f")
            .arg("/tmp/fake_mihomo")
            .output();
        let _ = std::fs::remove_file(&fake_mihomo);
        let _ = std::fs::remove_file(&fake_config);

        // This test creates a fake mihomo script for testing
        // The script runs in background with sleep to keep process alive
        std::fs::write(&fake_mihomo, "#!/bin/bash\nsleep 300\n").ok();

        // Create a minimal valid config file
        std::fs::write(&fake_config, "port: 7890\n").ok();

        // Make it executable
        let mut perms = std::fs::metadata(&fake_mihomo).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake_mihomo, perms).ok();

        let mut manager = MihomoManager::new()
            .with_mihomo_path(fake_mihomo.clone());

        // First start should succeed
        let result = manager.start(&fake_config);
        if result.is_err() {
            eprintln!("First start error: {:?}", result);
        }
        assert!(result.is_ok(), "First start failed: {:?}", result);
        assert!(manager.is_running());

        // Second start should be no-op (already running)
        let result2 = manager.start(&fake_config);
        assert!(result2.is_ok());

        // Clean up
        manager.stop().ok();
        let _ = std::process::Command::new("pkill")
            .arg("-f")
            .arg("/tmp/fake_mihomo")
            .output();
        std::fs::remove_file(fake_mihomo).ok();
        std::fs::remove_file(fake_config).ok();
    }

    // ============ Circuit Breaker Integration Tests ============

    #[test]
    fn test_mihomo_manager_circuit_breaker_default_allows() {
        let manager = MihomoManager::new();
        assert!(manager.can_restart());
        assert_eq!(manager.failure_count(), 0);
        assert!(manager.remaining_cooldown_secs().is_none());
    }

    #[test]
    fn test_mihomo_manager_circuit_breaker_failure_tracking() {
        let mut manager = MihomoManager::new()
            .with_mihomo_path(PathBuf::from("/nonexistent/mihomo"));

        // First start will try to download and fail, recording a failure
        let result = manager.start(&PathBuf::from("/tmp/config.yaml"));
        // It might succeed in downloading or fail, either way it records failures properly
        // Just verify the manager tracks failures correctly

        // If it fails, failure count should be > 0
        if result.is_err() {
            // Failure was recorded
            assert!(manager.failure_count() > 0 || !manager.can_restart(),
                "Should record failure or circuit breaker should be active");
        }
    }
}