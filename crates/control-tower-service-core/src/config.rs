//! Service configuration

use std::path::PathBuf;

/// Unified paths model for Control Tower.
///
/// All entry points (CLI, Web, service) should derive their paths from this
/// type so that config directories, socket paths, and file locations are
/// consistent across the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlTowerPaths {
    /// Directory containing all mutable runtime files (config.yaml, profiles.yaml, …)
    pub config_dir: PathBuf,
    /// Path to settings.yaml
    pub settings_path: PathBuf,
    /// Path to profiles.yaml
    pub profiles_path: PathBuf,
    /// Path to the active Mihomo config (config.yaml)
    pub active_config_path: PathBuf,
    /// Path to verge.yaml
    pub verge_config_path: PathBuf,
    /// Directory for log files
    pub log_dir: PathBuf,
    /// PID file for the service daemon
    pub pid_file: PathBuf,
    /// Unix socket for IPC
    pub socket_path: PathBuf,
}

impl ControlTowerPaths {
    /// Build paths from a settings.yaml path and an optional working directory override.
    ///
    /// If `working_dir` is `None`, the config directory is derived from the parent
    /// of `settings_path`.  This matches the layout where `settings.yaml` lives next
    /// to (or inside) the config directory.
    pub fn from_settings(settings_path: PathBuf, working_dir: Option<PathBuf>) -> Self {
        let config_dir = working_dir.unwrap_or_else(|| {
            settings_path
                .parent()
                .map(|p| p.to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| PathBuf::from("."))
        });

        Self {
            config_dir: config_dir.clone(),
            settings_path,
            profiles_path: config_dir.join("profiles.yaml"),
            active_config_path: config_dir.join("config.yaml"),
            verge_config_path: config_dir.join("verge.yaml"),
            log_dir: config_dir.join("logs"),
            pid_file: config_dir.join("ctsvc.pid"),
            socket_path: PathBuf::from("/tmp/ctsvc.sock"),
        }
    }
}

/// Service configuration — retained for backwards compatibility
///
/// **Deprecated:** Use `ControlTowerPaths` instead.
#[deprecated(since = "0.2.0", note = "Use ControlTowerPaths instead")]
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Configuration directory
    pub config_dir: PathBuf,

    /// Socket path for IPC
    pub socket_path: PathBuf,

    /// Log directory
    pub log_dir: PathBuf,

    /// PID file path
    pub pid_file: PathBuf,
}

#[allow(deprecated)]
impl Default for ServiceConfig {
    fn default() -> Self {
        // Use executable's directory as working directory
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        Self {
            config_dir: exe_dir.clone(),
            socket_path: PathBuf::from("/tmp/ctsvc.sock"),
            log_dir: exe_dir.join("logs"),
            pid_file: PathBuf::from("/tmp/ctsvc.pid"),
        }
    }
}

#[allow(deprecated)]
impl ServiceConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config_dir(mut self, dir: PathBuf) -> Self {
        self.config_dir = dir.clone();
        self.log_dir = dir.join("logs");
        self.pid_file = dir.join("ctsvc.pid");
        self
    }

    /// Get profiles.yaml path
    pub fn profiles_path(&self) -> PathBuf {
        self.config_dir.join("profiles.yaml")
    }

    /// Get verge.yaml path
    pub fn verge_config_path(&self) -> PathBuf {
        self.config_dir.join("verge.yaml")
    }

    /// Get Clash config path
    pub fn clash_config_path(&self) -> PathBuf {
        self.config_dir.join("config.yaml")
    }

    /// Ensure config directories exist
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.log_dir)?;
        std::fs::create_dir_all(self.socket_path.parent().unwrap_or(&PathBuf::from("/tmp")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === ControlTowerPaths tests ===

    #[test]
    fn test_paths_use_working_dir_from_settings() {
        let settings_path = PathBuf::from("/tmp/control-tower/settings.yaml");
        let paths = ControlTowerPaths::from_settings(
            settings_path.clone(),
            Some(PathBuf::from("/srv/control-tower")),
        );

        assert_eq!(paths.config_dir, PathBuf::from("/srv/control-tower"));
        assert_eq!(paths.settings_path, settings_path);
        assert_eq!(paths.profiles_path, PathBuf::from("/srv/control-tower/profiles.yaml"));
        assert_eq!(paths.active_config_path, PathBuf::from("/srv/control-tower/config.yaml"));
        assert_eq!(paths.verge_config_path, PathBuf::from("/srv/control-tower/verge.yaml"));
        assert_eq!(paths.log_dir, PathBuf::from("/srv/control-tower/logs"));
        assert_eq!(paths.socket_path, PathBuf::from("/tmp/ctsvc.sock"));
    }

    #[test]
    fn test_paths_fall_back_to_settings_parent() {
        let settings_path = PathBuf::from("/opt/control-tower/settings.yaml");
        let paths = ControlTowerPaths::from_settings(settings_path, None);

        assert_eq!(paths.config_dir, PathBuf::from("/opt/control-tower"));
        assert_eq!(paths.active_config_path, PathBuf::from("/opt/control-tower/config.yaml"));
    }

    #[test]
    fn test_paths_defaults_to_dot_for_missing_parent() {
        // When settings path has no parent component, config_dir falls back to ".".
        // This is an unusual edge case; normal usage always has a directory.
        let settings_path = PathBuf::from("settings.yaml");
        let paths = ControlTowerPaths::from_settings(settings_path, None);
        // parent() of "settings.yaml" is None → fallback is "." → PathBuf(".")
        assert_eq!(paths.config_dir, PathBuf::from("."));
    }

    // === ServiceConfig tests (corrected) ===

    #[test]
    fn test_service_config_default() {
        let config = ServiceConfig::default();
        // Default does NOT hard-code clash-verge; it uses exe_dir.
        assert!(config.config_dir.exists() || config.config_dir == PathBuf::from("."));
    }

    #[test]
    fn test_service_config_with_config_dir() {
        let config = ServiceConfig::new()
            .with_config_dir(PathBuf::from("/tmp/test-config"));

        assert_eq!(config.config_dir, PathBuf::from("/tmp/test-config"));
        assert_eq!(config.log_dir, PathBuf::from("/tmp/test-config/logs"));
        assert_eq!(config.pid_file, PathBuf::from("/tmp/test-config/ctsvc.pid"));
    }

    #[test]
    fn test_profiles_path() {
        let config = ServiceConfig::new();
        let profiles = config.profiles_path();
        assert!(profiles.to_string_lossy().ends_with("profiles.yaml"));
    }

    #[test]
    fn test_verge_config_path() {
        let config = ServiceConfig::new();
        let verge = config.verge_config_path();
        assert!(verge.to_string_lossy().ends_with("verge.yaml"));
    }
}