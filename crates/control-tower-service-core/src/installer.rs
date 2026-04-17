//! Mihomo installer - downloads and installs Mihomo binary

use std::path::PathBuf;
use anyhow::{Result, Context};

/// Mihomo version to download
const MIHOMO_VERSION: &str = "1.19.0";

/// Mihomo installer - handles downloading and installing Mihomo binary
pub struct MihomoInstaller {
    install_dir: PathBuf,
}

impl MihomoInstaller {
    /// Create a new installer with the given install directory
    pub fn new(install_dir: PathBuf) -> Self {
        Self { install_dir }
    }

    /// Get the default install directory (next to executable)
    pub fn default_install_dir() -> PathBuf {
        // Use executable's directory as working directory
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// Get the target binary path
    pub fn binary_path(&self) -> PathBuf {
        self.install_dir.join("mihomo")
    }

    /// Get the download URL for the current platform
    fn get_download_url(&self) -> Result<String> {
        let (os, arch, extension) = self.get_platform_info()?;

        let filename = match (os.as_str(), extension.as_str()) {
            ("linux", "gz") => format!("mihomo-linux-{}-v{}.gz", arch, MIHOMO_VERSION),
            ("darwin", "gz") => format!("mihomo-darwin-{}-v{}.gz", arch, MIHOMO_VERSION),
            ("windows", "zip") => format!("mihomo-windows-{}-v{}.zip", arch, MIHOMO_VERSION),
            _ => anyhow::bail!("Unsupported platform: {}-{}", os, arch),
        };

        Ok(format!(
            "https://github.com/MetaCubeX/mihomo/releases/download/v{}/{}",
            MIHOMO_VERSION, filename
        ))
    }

    /// Get platform information for download URL
    fn get_platform_info(&self) -> Result<(String, String, String)> {
        #[cfg(target_os = "linux")]
        {
            let arch = self.get_arch()?;
            Ok(("linux".to_string(), arch, "gz".to_string()))
        }

        #[cfg(target_os = "macos")]
        {
            let arch = self.get_arch()?;
            Ok(("darwin".to_string(), arch, "gz".to_string()))
        }

        #[cfg(target_os = "windows")]
        {
            let arch = self.get_arch()?;
            Ok(("windows".to_string(), arch, "zip".to_string()))
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            anyhow::bail!("Unsupported operating system")
        }
    }

    /// Get architecture string for download URL
    fn get_arch(&self) -> Result<String> {
        #[cfg(target_arch = "x86_64")]
        return Ok("amd64".to_string());

        #[cfg(target_arch = "aarch64")]
        return Ok("arm64".to_string());

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        anyhow::bail!("Unsupported architecture: {}", std::env::consts::ARCH)
    }

    /// Check if Mihomo is already installed
    pub fn is_installed(&self) -> bool {
        self.binary_path().exists()
    }

    /// Download and install Mihomo
    pub fn install(&self) -> Result<PathBuf> {
        if self.is_installed() {
            tracing::info!("Mihomo already installed at {:?}", self.binary_path());
            return Ok(self.binary_path());
        }

        // Create install directory if it doesn't exist
        if !self.install_dir.exists() {
            std::fs::create_dir_all(&self.install_dir)
                .context("Failed to create install directory")?;
        }

        let url = self.get_download_url()?;
        tracing::info!("Downloading Mihomo from: {}", url);

        // Download the archive
        let response = reqwest::blocking::get(&url)
            .with_context(|| format!("Failed to download Mihomo from: {}", url))?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to download Mihomo: HTTP {}", response.status());
        }

        let bytes = response.bytes()
            .context("Failed to read response body")?;

        // Decompress based on platform
        let binary_data = self.decompress(&bytes)?;

        // Write the binary
        let binary_path = self.binary_path();
        std::fs::write(&binary_path, &binary_data)
            .context("Failed to write Mihomo binary")?;

        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&binary_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&binary_path, perms)?;
        }

        tracing::info!("Mihomo installed successfully to {:?}", binary_path);
        Ok(binary_path)
    }

    /// Decompress the downloaded archive
    fn decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::io::Read;
            let mut decoder = flate2::read::GzDecoder::new(data);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed)
                .context("Failed to decompress gzip archive")?;
            Ok(decompressed)
        }

        #[cfg(target_os = "windows")]
        {
            use std::io::Read;
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(data))
                .context("Failed to read zip archive")?;

            let mut file = archive.by_index(0)
                .context("Failed to read file from zip archive")?;

            let mut decompressed = Vec::new();
            file.read_to_end(&mut decompressed)
                .context("Failed to extract file from zip")?;

            Ok(decompressed)
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        anyhow::bail!("Unsupported platform for decompression")
    }

    /// Verify the installed binary
    pub fn verify(&self) -> Result<()> {
        let path = self.binary_path();

        if !path.exists() {
            anyhow::bail!("Mihomo binary not found at {:?}", path);
        }

        // Try to run version command
        let output = std::process::Command::new(&path)
            .arg("-v")
            .output()
            .context("Failed to run Mihomo version command")?;

        if !output.status.success() {
            anyhow::bail!("Mihomo version check failed");
        }

        let version = String::from_utf8_lossy(&output.stdout);
        tracing::info!("Mihomo version: {}", version.trim());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_install_dir() {
        let dir = MihomoInstaller::default_install_dir();
        assert!(dir.to_str().unwrap().contains("clash-verge"));
    }

    #[test]
    fn test_installer_creation() {
        let installer = MihomoInstaller::new(PathBuf::from("/tmp/test"));
        assert_eq!(installer.binary_path(), PathBuf::from("/tmp/test/mihomo"));
    }

    #[test]
    fn test_get_platform_info() {
        let installer = MihomoInstaller::new(PathBuf::from("/tmp"));
        let result = installer.get_platform_info();
        // This test just verifies the method works, actual value depends on platform
        assert!(result.is_ok());
        let (os, arch, ext) = result.unwrap();
        assert!(!os.is_empty());
        assert!(!arch.is_empty());
        assert!(!ext.is_empty());
    }

    #[test]
    fn test_get_download_url() {
        let installer = MihomoInstaller::new(PathBuf::from("/tmp"));
        let url = installer.get_download_url().unwrap();
        assert!(url.contains("github.com"));
        assert!(url.contains("mihomo"));
        assert!(url.contains(MIHOMO_VERSION));
    }

    #[test]
    fn test_is_installed_when_not_exists() {
        let installer = MihomoInstaller::new(PathBuf::from("/nonexistent/path"));
        assert!(!installer.is_installed());
    }
}