//! Filesystem helpers shared by the service and the CLI.
//!
//! The configuration directory contains data that is security-sensitive:
//! `settings.yaml` (password verifier), `profiles.yaml` (subscription URLs and
//! custom headers/tokens) and the generated `config.yaml` (proxy credentials).
//! All of them must be written atomically and must not be readable by other
//! local users, otherwise an attacker on the same machine can steal the
//! credentials or forge a login.

use std::io;
use std::path::Path;

/// Restrict a file to its owner (best effort; no-op on non-Unix builds).
pub fn restrict_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Restrict a directory to its owner (best effort; no-op on non-Unix builds).
pub fn restrict_dir_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Atomically replace `path` with `content`, owner-readable only.
///
/// Content is written to a sibling temporary file first, so readers never see
/// a truncated file (which for `settings.yaml` would parse as defaults and
/// temporarily disable authentication), then renamed over the target.
pub fn write_private_atomic(path: &Path, content: &str) -> io::Result<()> {
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, content)?;
    restrict_permissions(&temp);
    std::fs::rename(&temp, path)?;
    restrict_permissions(path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_private_atomic_replaces_content() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("profiles.yaml");

        write_private_atomic(&target, "current: a\nitems: []\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "current: a\nitems: []\n"
        );

        write_private_atomic(&target, "current: b\nitems: []\n").unwrap();
        assert!(std::fs::read_to_string(&target).unwrap().contains("current: b"));

        // The temporary file must not be left behind.
        assert!(!dir.path().join("profiles.tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn write_private_atomic_restricts_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("settings.yaml");
        write_private_atomic(&target, "auth: {}\n").unwrap();

        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
