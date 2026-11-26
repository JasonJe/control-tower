//! View service and Mihomo log files directly from disk.
//!
//! Reading files instead of asking the daemon means logs remain available
//! when `ctsvc` is down or the IPC socket is stale.

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn log_dir() -> Result<PathBuf> {
    Ok(crate::settings::shared_paths()?.log_dir)
}

/// Newest `ctsvc.log*` file by modification time (active file or rotation).
fn service_log(dir: &Path) -> Result<PathBuf> {
    let newest = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("ctsvc.log")
        })
        .max_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok());

    match newest {
        Some(entry) => Ok(entry.path()),
        None => anyhow::bail!("No service log found in {}. Is ctsvc running?", dir.display()),
    }
}

fn log_file(dir: &Path, source: Option<&str>) -> Result<PathBuf> {
    match source.unwrap_or("service") {
        "mihomo" => {
            let path = dir.join("mihomo.log");
            if !path.exists() {
                anyhow::bail!("Mihomo log not found: {}", path.display());
            }
            Ok(path)
        }
        "service" => service_log(dir),
        other => anyhow::bail!("Unknown log source: {}", other),
    }
}

fn tail(path: &Path, lines: usize) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
    let all: Vec<&str> = content.lines().collect();
    let start = all.len().saturating_sub(lines);
    Ok(all[start..].iter().map(|line| line.to_string()).collect())
}

pub fn handle(lines: Option<usize>, source: Option<&str>, follow: bool) -> Result<()> {
    let dir = log_dir()?;
    let lines = lines.unwrap_or(100);
    let mut path = log_file(&dir, source)?;

    for line in tail(&path, lines)? {
        println!("{}", line);
    }

    if !follow {
        return Ok(());
    }

    println!("\n== Following {} (Ctrl+C to exit) ==", path.display());
    let mut position = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    loop {
        std::thread::sleep(Duration::from_millis(500));

        // Handle daily rotation: if the active file shrank or a newer file
        // appeared, restart from the beginning of the new file.
        if source != Some("mihomo") {
            if let Ok(newest) = service_log(&dir) {
                if newest != path {
                    path = newest;
                    position = 0;
                    println!("\n== Switched to {} ==", path.display());
                }
            }
        }

        let Ok(meta) = fs::metadata(&path) else { continue };
        if meta.len() < position {
            position = 0;
        }
        if meta.len() == position {
            continue;
        }
        let content = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if content.len() > position as usize {
            let added = &content[position as usize..];
            let text = String::from_utf8_lossy(added);
            print!("{}", text);
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
        position = meta.len();
    }
}
