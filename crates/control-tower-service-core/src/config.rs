//! Service configuration

use std::path::PathBuf;

/// Service configuration
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

    #[test]
    fn test_service_config_default() {
        let config = ServiceConfig::default();
        assert!(config.config_dir.to_string_lossy().contains("clash-verge"));
        assert_eq!(config.socket_path, PathBuf::from("/tmp/verge/clash-verge-service.sock"));
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