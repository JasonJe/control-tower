//! Update module - handles mihomo and geoip database updates

use anyhow::Result;
use std::path::PathBuf;

use crate::service;

/// Update action types
#[derive(Debug, clap::Subcommand)]
pub enum UpdateAction {
    /// Update mihomo binary and geoip/geosite databases
    ///
    /// Downloads the latest versions from GitHub releases.
    /// Automatically restarts the service after update.
    ///
    /// Examples:
    ///   ctctl update all        # Update everything
    ///   ctctl update mihomo    # Update mihomo binary only
    ///   ctctl update geoip     # Update geoip database only
    ///   ctctl update geosite   # Update geosite database only
    All,
    /// Update mihomo binary only
    ///
    /// Downloads the latest mihomo binary from MetaCubeX GitHub releases.
    /// Automatically restarts the service after update.
    Mihomo,
    /// Update geoip database only
    ///
    /// Downloads the latest geoip.metadb database.
    /// Automatically restarts the service after update.
    Geoip,
    /// Update geosite database only
    ///
    /// Downloads the latest geosite.db database.
    /// Automatically restarts the service after update.
    Geosite,
}

const MIHOMO_VERSION: &str = "v1.19.0";

/// Handle update command
pub async fn handle(action: UpdateAction) -> Result<()> {
    match action {
        UpdateAction::All => {
            update_mihomo().await?;
            update_geoip().await?;
            update_geosite().await?;
            println!("\nRestarting service to apply updates...");
            service::restart_service().await?;
        }
        UpdateAction::Mihomo => {
            update_mihomo().await?;
            println!("\nRestarting service to apply update...");
            service::restart_service().await?;
        }
        UpdateAction::Geoip => {
            update_geoip().await?;
            println!("\nRestarting service to apply update...");
            service::restart_service().await?;
        }
        UpdateAction::Geosite => {
            update_geosite().await?;
            println!("\nRestarting service to apply update...");
            service::restart_service().await?;
        }
    }
    Ok(())
}

/// Get mihomo directory
fn get_mihomo_dir() -> Result<PathBuf> {
    let exe_path = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("Failed to get current exe: {}", e))?;
    let exe_dir = exe_path.parent()
        .ok_or_else(|| anyhow::anyhow!("Failed to get parent dir"))?;
    Ok(exe_dir.join("mihomo"))
}

/// Get architecture suffix for downloading
fn get_arch_suffix() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64-compatible",
        "aarch64" => "arm64",
        _ => "amd64-compatible",
    }
}

/// Download a file from URL to destination
async fn download_file(url: &str, dest: &PathBuf) -> Result<()> {
    println!("Downloading: {}", url);

    let response = reqwest::Client::new()
        .get(url)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to download: {}", e))?;

    if !response.status().is_success() {
        anyhow::bail!("Download failed with status: {}", response.status());
    }

    let bytes = response.bytes().await
        .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

    // Create parent directory if needed
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(dest, &bytes)?;

    Ok(())
}

/// Update mihomo binary
async fn update_mihomo() -> Result<()> {
    let mihomo_dir = get_mihomo_dir()?;
    let arch = get_arch_suffix();
    let gz_url = format!(
        "https://github.com/MetaCubeX/mihomo/releases/download/{}/mihomo-linux-{}-{}.gz",
        MIHOMO_VERSION, arch, MIHOMO_VERSION
    );
    let temp_file = std::env::temp_dir().join("mihomo.gz");

    println!("\n[Updating mihomo binary]");
    println!("Version: {}", MIHOMO_VERSION);
    println!("Architecture: {}", arch);

    // Download to temp file
    download_file(&gz_url, &temp_file).await?;

    // Decompress
    println!("Decompressing...");
    let decompressed = std::env::temp_dir().join("mihomo_temp");

    use std::process::Command;
    let output = Command::new("gunzip")
        .arg("-c")
        .arg(&temp_file)
        .output()?;

    std::fs::write(&decompressed, output.stdout)?;

    // Make executable and move to destination
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&decompressed)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&decompressed, perms)?;
    }

    std::fs::rename(&decompressed, mihomo_dir.join("mihomo"))?;

    // Cleanup
    let _ = std::fs::remove_file(temp_file);

    println!("mihomo binary updated successfully!");
    Ok(())
}

/// Update geoip database
async fn update_geoip() -> Result<()> {
    let mihomo_dir = get_mihomo_dir()?;
    let dest = mihomo_dir.join("geoip.metadb");
    let url = "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.metadb";

    println!("\n[Updating geoip database]");

    download_file(url, &dest).await?;

    println!("geoip database updated successfully!");
    Ok(())
}

/// Update geosite database
async fn update_geosite() -> Result<()> {
    let mihomo_dir = get_mihomo_dir()?;
    let dest = mihomo_dir.join("geosite.db");
    let url = "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.db";

    println!("\n[Updating geosite database]");

    download_file(url, &dest).await?;

    println!("geosite database updated successfully!");
    Ok(())
}
