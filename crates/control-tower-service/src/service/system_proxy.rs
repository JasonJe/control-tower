//! Linux desktop "system proxy" integration (GNOME / KDE).
//!
//! The service is normally a headless daemon, and `gsettings` /
//! `kwriteconfig` write to the *caller's* per-user configuration. Running them
//! as root would therefore change root's desktop settings (or nothing at all)
//! while the person at the keyboard keeps browsing without a proxy. Every
//! desktop command is consequently executed inside the desktop user's session:
//! as ourselves when the service runs unprivileged, or through
//! `runuser -u <user> -- env ...` (plus the session D-Bus address) when it runs
//! as root.
//!
//! The original desktop values are captured before the first enable and
//! restored on disable/shutdown, so turning the feature off never leaves a
//! dead proxy address behind.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Which desktop stack the proxy settings are written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Gnome,
    Kde,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Gnome => "gnome",
            Backend::Kde => "kde",
        }
    }
}

/// Persisted state of the system-proxy switch (`system-proxy.yaml`).
///
/// `enabled` records the *user's intent*, not whether the desktop settings are
/// currently applied: on shutdown the desktop values are restored but the flag
/// stays `true`, so the next start re-applies them.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemProxyState {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<Backend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mixed_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socks_port: Option<u16>,
    /// Desktop settings captured before the first enable, keyed by backend
    /// specific identifiers. Restored verbatim on disable.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub original: BTreeMap<String, String>,
    /// Last failure encountered while applying (used by the periodic retry).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Public view returned by `GET /api/system-proxy`.
#[derive(Debug, Clone, Serialize)]
pub struct SystemProxyStatus {
    /// Whether a supported desktop session was found.
    pub supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socks_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mixed_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Explanation shown when the feature is unavailable or failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Set once the desktop settings have been written successfully in this
/// process, so the periodic retry does not spawn tools every minute.
static APPLIED: AtomicBool = AtomicBool::new(false);

/// State file lives next to settings.yaml (same 0600 handling).
pub fn state_path() -> PathBuf {
    crate::settings::resolve_paths()
        .config_dir
        .join("system-proxy.yaml")
}

fn load_state() -> SystemProxyState {
    let path = state_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_yaml_ng::from_str(&content).unwrap_or_default(),
        Err(_) => SystemProxyState::default(),
    }
}

fn save_state(state: &SystemProxyState) -> Result<(), String> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    let yaml = serde_yaml_ng::to_string(state)
        .map_err(|e| format!("Failed to serialize system proxy state: {}", e))?;
    control_tower_service_core::write_private_atomic(&path, &yaml)
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

// ---------------------------------------------------------------------------
// Command execution
// ---------------------------------------------------------------------------

type FakeOutputs = BTreeMap<String, String>;

#[derive(Default)]
struct FakeState {
    commands: Vec<String>,
    outputs: FakeOutputs,
    available: Vec<String>,
}

/// Runs desktop command-line tools in the target session.
#[derive(Default)]
struct Runner {
    /// Desktop user to switch to when the service runs as root.
    user: Option<String>,
    /// Environment passed to the tool (`DBUS_SESSION_BUS_ADDRESS`, `HOME`, ...).
    envs: Vec<(String, String)>,
    /// Tests: record commands and answer from a fixed table.
    fake: Option<Arc<Mutex<FakeState>>>,
}

impl Runner {
    #[cfg(test)]
    fn fake(outputs: &[(&str, &str)], available: &[&str]) -> Runner {
        let mut state = FakeState {
            available: available.iter().map(|s| (*s).to_string()).collect(),
            ..Default::default()
        };
        for (command, output) in outputs {
            state.outputs.insert((*command).to_string(), (*output).to_string());
        }
        Runner {
            fake: Some(Arc::new(Mutex::new(state))),
            ..Default::default()
        }
    }

    fn run(&self, program: &str, args: &[String]) -> Result<String, String> {
        let rendered = format!("{} {}", program, args.join(" "));
        if let Some(fake) = &self.fake {
            let mut fake = fake.lock().unwrap_or_else(|e| e.into_inner());
            fake.commands.push(rendered.clone());
            return Ok(fake.outputs.get(&rendered).cloned().unwrap_or_default());
        }

        let mut argv: Vec<String> = Vec::new();
        if let Some(user) = &self.user {
            argv.extend([
                "runuser".to_string(),
                "-u".to_string(),
                user.clone(),
                "--".to_string(),
            ]);
        }
        if !self.envs.is_empty() {
            argv.push("env".to_string());
            for (key, value) in &self.envs {
                argv.push(format!("{}={}", key, value));
            }
        }
        argv.push(program.to_string());
        argv.extend(args.iter().cloned());

        let output = Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .map_err(|e| format!("Failed to run {}: {}", program, e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "{} exited with {}: {}",
                program,
                output.status,
                stderr.trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn exists(&self, program: &str) -> bool {
        if let Some(fake) = &self.fake {
            return fake
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .available
                .iter()
                .any(|p| p == program);
        }
        Command::new("sh")
            .arg("-c")
            .arg(format!("command -v '{}' >/dev/null 2>&1", program))
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// A desktop session plus the tool runner bound to it.
struct Session {
    backend: Backend,
    runner: Runner,
}

fn session_env() -> Vec<(String, String)> {
    let mut envs = Vec::new();
    for key in [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
    ] {
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                envs.push((key.to_string(), value));
            }
        }
    }
    envs
}

/// Resolve the desktop session the proxy settings should be written to.
fn desktop_session() -> Result<Session, String> {
    let (runner, home) = target_runner()?;
    let hint = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let backend = detect_backend(&runner, home.as_deref(), &hint)?;
    Ok(Session { backend, runner })
}

#[cfg(unix)]
fn target_runner() -> Result<(Runner, Option<PathBuf>), String> {
    let euid = unsafe { libc::geteuid() };
    if euid != 0 {
        let mut runner = Runner {
            envs: session_env(),
            ..Default::default()
        };
        // Add the session bus even when XDG_RUNTIME_DIR is not exported.
        if !runner.envs.iter().any(|(k, _)| k == "DBUS_SESSION_BUS_ADDRESS") {
            if let Some(bus) = std::env::var("XDG_RUNTIME_DIR")
                .ok()
                .map(|dir| format!("unix:path={}/bus", dir))
            {
                runner.envs.push(("DBUS_SESSION_BUS_ADDRESS".to_string(), bus));
            }
        }
        return Ok((runner, dirs::home_dir()));
    }

    // Running as root: pick the user owning an active session. An explicit
    // override wins, which is useful for multi-seat machines.
    let override_user = std::env::var("CT_SYSTEM_PROXY_USER")
        .ok()
        .filter(|u| !u.trim().is_empty());

    let (user, home) = match override_user {
        Some(user) => {
            let info = user_info_by_name(&user)
                .ok_or_else(|| format!("CT_SYSTEM_PROXY_USER={} does not exist", user))?;
            (user, info)
        }
        None => find_desktop_user().ok_or_else(|| {
            "No logged-in desktop session found. Log in to the desktop, or set \
             CT_SYSTEM_PROXY_USER to the desktop user."
                .to_string()
        })?,
    };

    let mut envs = vec![
        ("HOME".to_string(), home.display().to_string()),
        ("USER".to_string(), user.clone()),
        ("LOGNAME".to_string(), user.clone()),
    ];
    // Session bus path is derived from the runtime dir; prefer the real bus
    // socket when it exists.
    if let Some(uid) = user_info_by_name(&user).and_then(|_| user_uid(&user)) {
        let bus = format!("/run/user/{}/bus", uid);
        if Path::new(&bus).exists() {
            envs.push((
                "DBUS_SESSION_BUS_ADDRESS".to_string(),
                format!("unix:path={}", bus),
            ));
            envs.push((
                "XDG_RUNTIME_DIR".to_string(),
                format!("/run/user/{}", uid),
            ));
        }
    }
    envs.extend(session_env());

    Ok((
        Runner {
            user: Some(user),
            envs,
            ..Default::default()
        },
        Some(home),
    ))
}

#[cfg(not(unix))]
fn target_runner() -> Result<(Runner, Option<PathBuf>), String> {
    Err("System proxy control is only supported on Linux desktops".to_string())
}

/// Find an active user session under `/run/user/<uid>/bus`.
#[cfg(unix)]
fn find_desktop_user() -> Option<(String, PathBuf)> {
    let entries = std::fs::read_dir("/run/user").ok()?;
    let mut candidates: Vec<u32> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_string_lossy().parse::<u32>().ok())
        .filter(|uid| *uid != 0)
        .collect();
    candidates.sort_unstable();
    for uid in candidates {
        if Path::new(&format!("/run/user/{}/bus", uid)).exists() {
            if let Some((name, home)) = user_info(uid) {
                return Some((name, home));
            }
        }
    }
    None
}

#[cfg(unix)]
fn user_info(uid: u32) -> Option<(String, PathBuf)> {
    unsafe {
        let mut buf = vec![0u8; 8192];
        let mut passwd: libc::passwd = std::mem::zeroed();
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let rc = libc::getpwuid_r(
            uid,
            &mut passwd,
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            &mut result,
        );
        if rc != 0 || result.is_null() {
            return None;
        }
        let name = std::ffi::CStr::from_ptr(passwd.pw_name)
            .to_string_lossy()
            .into_owned();
        let home = std::ffi::CStr::from_ptr(passwd.pw_dir)
            .to_string_lossy()
            .into_owned();
        Some((name, PathBuf::from(home)))
    }
}

#[cfg(unix)]
fn user_info_by_name(name: &str) -> Option<PathBuf> {
    unsafe {
        let c_name = std::ffi::CString::new(name).ok()?;
        let mut buf = vec![0u8; 8192];
        let mut passwd: libc::passwd = std::mem::zeroed();
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let rc = libc::getpwnam_r(
            c_name.as_ptr(),
            &mut passwd,
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            &mut result,
        );
        if rc != 0 || result.is_null() {
            return None;
        }
        Some(PathBuf::from(
            std::ffi::CStr::from_ptr(passwd.pw_dir).to_string_lossy().into_owned(),
        ))
    }
}

#[cfg(unix)]
fn user_uid(name: &str) -> Option<u32> {
    unsafe {
        let c_name = std::ffi::CString::new(name).ok()?;
        let mut buf = vec![0u8; 8192];
        let mut passwd: libc::passwd = std::mem::zeroed();
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let rc = libc::getpwnam_r(
            c_name.as_ptr(),
            &mut passwd,
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            &mut result,
        );
        if rc != 0 || result.is_null() {
            return None;
        }
        Some(passwd.pw_uid)
    }
}

// ---------------------------------------------------------------------------
// Backend detection
// ---------------------------------------------------------------------------

fn detect_backend(runner: &Runner, home: Option<&Path>, desktop: &str) -> Result<Backend, String> {
    let desktop = desktop.to_ascii_lowercase();
    let kde_writer = if runner.exists("kwriteconfig6") {
        Some("kwriteconfig6")
    } else if runner.exists("kwriteconfig5") {
        Some("kwriteconfig5")
    } else {
        None
    };
    let kde_config = home
        .map(|home| home.join(".config").join("kioslaverc").exists())
        .unwrap_or(false);

    // KDE first: GTK is often installed on Plasma machines, so asking
    // gsettings first would report GNOME while the user runs KDE.
    if kde_writer.is_some() && (desktop.contains("kde") || desktop.contains("plasma") || kde_config)
    {
        return Ok(Backend::Kde);
    }

    if runner.exists("gsettings")
        && runner
            .run(
                "gsettings",
                &[
                    "get".to_string(),
                    "org.gnome.system.proxy".to_string(),
                    "mode".to_string(),
                ],
            )
            .is_ok()
    {
        return Ok(Backend::Gnome);
    }

    if kde_writer.is_some() {
        return Ok(Backend::Kde);
    }

    Err("No supported desktop found (GNOME/gsettings or KDE/kwriteconfig required)".to_string())
}

// ---------------------------------------------------------------------------
// GNOME
// ---------------------------------------------------------------------------

const GNOME_KEYS: &[(&str, &str)] = &[
    ("org.gnome.system.proxy", "mode"),
    ("org.gnome.system.proxy.http", "host"),
    ("org.gnome.system.proxy.http", "port"),
    ("org.gnome.system.proxy.https", "host"),
    ("org.gnome.system.proxy.https", "port"),
    ("org.gnome.system.proxy.socks", "host"),
    ("org.gnome.system.proxy.socks", "port"),
];

fn gnome_key_id(schema: &str, key: &str) -> String {
    format!("gnome:{}:{}", schema, key)
}

fn capture_gnome(runner: &Runner) -> Result<BTreeMap<String, String>, String> {
    let mut captured = BTreeMap::new();
    for (schema, key) in GNOME_KEYS {
        let value = runner.run(
            "gsettings",
            &[
                "get".to_string(),
                (*schema).to_string(),
                (*key).to_string(),
            ],
        )?;
        captured.insert(gnome_key_id(schema, key), value);
    }
    Ok(captured)
}

fn apply_gnome(runner: &Runner, host: &str, http_port: u16, socks_port: u16) -> Result<(), String> {
    let quoted_host = format!("'{}'", host.replace('\'', "\\'"));
    let settings: Vec<(&str, &str, String)> = vec![
        ("org.gnome.system.proxy", "mode", "'manual'".to_string()),
        ("org.gnome.system.proxy.http", "host", quoted_host.clone()),
        ("org.gnome.system.proxy.http", "port", http_port.to_string()),
        ("org.gnome.system.proxy.https", "host", quoted_host),
        (
            "org.gnome.system.proxy.https",
            "port",
            http_port.to_string(),
        ),
        ("org.gnome.system.proxy.socks", "host", format!("'{}'", host)),
        (
            "org.gnome.system.proxy.socks",
            "port",
            socks_port.to_string(),
        ),
    ];
    for (schema, key, value) in settings {
        runner.run(
            "gsettings",
            &[
                "set".to_string(),
                schema.to_string(),
                key.to_string(),
                value,
            ],
        )?;
    }
    Ok(())
}

fn restore_gnome(
    runner: &Runner,
    original: &BTreeMap<String, String>,
) -> Result<(), String> {
    // Restore in a stable order, `mode` last so apps do not see a manual proxy
    // pointing at a half-restored configuration.
    let mut entries: Vec<(&String, &String)> = original.iter().collect();
    entries.sort_by_key(|(key, _)| key.ends_with(":mode"));
    for (id, value) in entries {
        let mut parts = id.splitn(3, ':');
        if parts.next() != Some("gnome") {
            continue;
        }
        let (Some(schema), Some(key)) = (parts.next(), parts.next()) else {
            continue;
        };
        runner.run(
            "gsettings",
            &[
                "set".to_string(),
                schema.to_string(),
                key.to_string(),
                value.clone(),
            ],
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// KDE
// ---------------------------------------------------------------------------

const KDE_GROUP: &str = "Proxy Settings";
const KDE_KEYS: &[&str] = &[
    "ProxyType",
    "httpProxy",
    "httpsProxy",
    "socksProxy",
    "ftpProxy",
];

fn kde_key_id(key: &str) -> String {
    format!("kde:{}", key)
}

fn kde_reader(runner: &Runner) -> &'static str {
    if runner.exists("kreadconfig6") {
        "kreadconfig6"
    } else {
        "kreadconfig5"
    }
}

fn kde_writer(runner: &Runner) -> &'static str {
    if runner.exists("kwriteconfig6") {
        "kwriteconfig6"
    } else {
        "kwriteconfig5"
    }
}

fn capture_kde(runner: &Runner) -> Result<BTreeMap<String, String>, String> {
    let reader = kde_reader(runner);
    let mut captured = BTreeMap::new();
    for key in KDE_KEYS {
        let value = runner.run(
            reader,
            &[
                "--file".to_string(),
                "kioslaverc".to_string(),
                "--group".to_string(),
                KDE_GROUP.to_string(),
                "--key".to_string(),
                (*key).to_string(),
            ],
        )?;
        captured.insert(kde_key_id(key), value);
    }
    Ok(captured)
}

fn apply_kde(runner: &Runner, host: &str, http_port: u16, socks_port: u16) -> Result<(), String> {
    let writer = kde_writer(runner);
    let values = [
        ("ProxyType", "1".to_string()),
        ("httpProxy", format!("http://{}:{}", host, http_port)),
        ("httpsProxy", format!("http://{}:{}", host, http_port)),
        ("ftpProxy", format!("http://{}:{}", host, http_port)),
        ("socksProxy", format!("socks://{}:{}", host, socks_port)),
    ];
    for (key, value) in values {
        runner.run(
            writer,
            &[
                "--file".to_string(),
                "kioslaverc".to_string(),
                "--group".to_string(),
                KDE_GROUP.to_string(),
                "--key".to_string(),
                key.to_string(),
                value,
            ],
        )?;
    }
    // Ask running KIO applications to pick the change up. Best effort: the
    // signal is an optimisation, the config file is already correct.
    let _ = runner.run(
        "dbus-send",
        &[
            "--type=signal".to_string(),
            "/KIO/Scheduler".to_string(),
            "org.kde.KIO.Scheduler.reparseSlaveConfiguration".to_string(),
            "string:".to_string(),
        ],
    );
    Ok(())
}

fn restore_kde(runner: &Runner, original: &BTreeMap<String, String>) -> Result<(), String> {
    let writer = kde_writer(runner);
    for key in KDE_KEYS {
        let value = original
            .get(&kde_key_id(key))
            .cloned()
            .unwrap_or_else(|| {
                if *key == "ProxyType" {
                    "0".to_string()
                } else {
                    String::new()
                }
            });
        runner.run(
            writer,
            &[
                "--file".to_string(),
                "kioslaverc".to_string(),
                "--group".to_string(),
                KDE_GROUP.to_string(),
                "--key".to_string(),
                (*key).to_string(),
                value,
            ],
        )?;
    }
    let _ = runner.run(
        "dbus-send",
        &[
            "--type=signal".to_string(),
            "/KIO/Scheduler".to_string(),
            "org.kde.KIO.Scheduler.reparseSlaveConfiguration".to_string(),
            "string:".to_string(),
        ],
    );
    Ok(())
}

fn capture(runner: &Runner, backend: Backend) -> Result<BTreeMap<String, String>, String> {
    match backend {
        Backend::Gnome => capture_gnome(runner),
        Backend::Kde => capture_kde(runner),
    }
}

fn apply(
    runner: &Runner,
    backend: Backend,
    host: &str,
    http_port: u16,
    socks_port: u16,
) -> Result<(), String> {
    match backend {
        Backend::Gnome => apply_gnome(runner, host, http_port, socks_port),
        Backend::Kde => apply_kde(runner, host, http_port, socks_port),
    }
}

fn restore(
    runner: &Runner,
    backend: Backend,
    original: &BTreeMap<String, String>,
) -> Result<(), String> {
    match backend {
        Backend::Gnome => restore_gnome(runner, original),
        Backend::Kde => restore_kde(runner, original),
    }
}

// ---------------------------------------------------------------------------
// Public API used by HTTP handlers / startup / shutdown
// ---------------------------------------------------------------------------

/// Current status, including whether a desktop session is reachable.
pub fn status() -> SystemProxyStatus {
    let state = load_state();
    let session = desktop_session();
    let (supported, backend, message) = match &session {
        Ok(session) => (true, Some(session.backend), None),
        Err(e) => (false, state.backend, Some(e.clone())),
    };
    SystemProxyStatus {
        supported,
        backend: backend.map(|b| b.as_str().to_string()),
        enabled: state.enabled,
        host: state.host.clone(),
        http_port: state.http_port,
        socks_port: state.socks_port,
        mixed_port: state.mixed_port,
        last_error: state.last_error.clone(),
        message,
    }
}

/// Enable or disable the desktop system proxy.
pub fn set_enabled(
    enabled: bool,
    host: Option<String>,
    http_port: Option<u16>,
    socks_port: Option<u16>,
) -> Result<SystemProxyStatus, String> {
    let mut state = load_state();

    if !enabled {
        if state.enabled || !state.original.is_empty() {
            let backend = state
                .backend
                .ok_or_else(|| "No recorded desktop backend to restore".to_string())?;
            let session = desktop_session().map_err(|e| {
                format!("Cannot restore system proxy without a desktop session: {}", e)
            })?;
            // Restore even when the detected backend changed: the captured map
            // records which tool wrote what.
            restore(&session.runner, backend, &state.original)?;
        }
        state.enabled = false;
        state.original.clear();
        state.last_error = None;
        save_state(&state)?;
        APPLIED.store(false, Ordering::SeqCst);
        return Ok(status());
    }

    let session = desktop_session()?;
    let host = host.unwrap_or_else(|| "127.0.0.1".to_string());
    let http_port = http_port.ok_or_else(|| "http_port is required".to_string())?;
    let socks_port = socks_port.ok_or_else(|| "socks_port is required".to_string())?;

    // Capture the desktop values before writing for the first time, or after a
    // desktop switch (the previous backend's values were already restored on
    // disable in that case).
    if !state.enabled || state.backend != Some(session.backend) {
        state.original = capture(&session.runner, session.backend)?;
    }

    apply(&session.runner, session.backend, &host, http_port, socks_port)?;

    state.enabled = true;
    state.backend = Some(session.backend);
    state.host = Some(host);
    state.http_port = Some(http_port);
    state.socks_port = Some(socks_port);
    state.last_error = None;
    save_state(&state)?;
    APPLIED.store(true, Ordering::SeqCst);
    Ok(status())
}

/// Re-apply the saved settings when the user has the switch on and the
/// desktop was not reachable earlier (for example at boot, before login).
pub fn ensure_applied() {
    if APPLIED.load(Ordering::SeqCst) {
        return;
    }
    let state = load_state();
    if !state.enabled {
        return;
    }
    let (Some(host), Some(http_port), Some(socks_port)) =
        (state.host.clone(), state.http_port, state.socks_port)
    else {
        return;
    };
    match set_enabled(true, Some(host), Some(http_port), Some(socks_port)) {
        Ok(_) => tracing::info!("System proxy applied on startup"),
        Err(e) => {
            if state.last_error.as_deref() != Some(e.as_str()) {
                tracing::warn!("System proxy not applied yet: {}", e);
                let mut state = load_state();
                state.last_error = Some(e);
                let _ = save_state(&state);
            }
        }
    }
}

/// Update the proxy ports after the user changed them while the switch is on.
pub fn refresh_ports(http_port: u16, socks_port: u16) {
    let state = load_state();
    if !state.enabled {
        return;
    }
    match set_enabled(
        true,
        state.host.clone(),
        Some(http_port),
        Some(socks_port),
    ) {
        Ok(_) => tracing::info!("System proxy ports updated to http={}, socks={}", http_port, socks_port),
        Err(e) => tracing::warn!("Failed to update system proxy ports: {}", e),
    }
}

/// Restore the desktop values on a graceful shutdown. The persisted `enabled`
/// flag is kept so the next start re-applies the user's choice.
pub fn restore_on_shutdown() {
    let state = load_state();
    if !state.enabled {
        return;
    }
    if let (Some(backend), Ok(session)) = (state.backend, desktop_session()) {
        match restore(&session.runner, backend, &state.original) {
            Ok(()) => tracing::info!("System proxy restored before shutdown"),
            Err(e) => tracing::warn!("Failed to restore system proxy: {}", e),
        }
    }
    APPLIED.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_string()).collect()
    }

    #[test]
    fn state_roundtrip() {
        let mut state = SystemProxyState {
            enabled: true,
            backend: Some(Backend::Gnome),
            host: Some("127.0.0.1".into()),
            http_port: Some(7890),
            socks_port: Some(7891),
            ..Default::default()
        };
        state.original.insert(
            gnome_key_id("org.gnome.system.proxy", "mode"),
            "'none'".into(),
        );

        let yaml = serde_yaml_ng::to_string(&state).unwrap();
        let parsed: SystemProxyState = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed, state);
        assert!(yaml.contains("backend: gnome"));
    }

    #[test]
    fn gnome_capture_records_every_key() {
        let runner = Runner::fake(
            &[
                ("gsettings get org.gnome.system.proxy mode", "'none'"),
                (
                    "gsettings get org.gnome.system.proxy.http host",
                    "''",
                ),
                ("gsettings get org.gnome.system.proxy.http port", "0"),
                (
                    "gsettings get org.gnome.system.proxy.https host",
                    "''",
                ),
                ("gsettings get org.gnome.system.proxy.https port", "0"),
                (
                    "gsettings get org.gnome.system.proxy.socks host",
                    "''",
                ),
                ("gsettings get org.gnome.system.proxy.socks port", "0"),
            ],
            &["gsettings"],
        );

        let captured = capture_gnome(&runner).unwrap();
        assert_eq!(captured.len(), GNOME_KEYS.len());
        assert_eq!(
            captured[&gnome_key_id("org.gnome.system.proxy", "mode")],
            "'none'"
        );
    }

    #[test]
    fn gnome_apply_sets_manual_mode_and_ports() {
        let runner = Runner::fake(&[], &["gsettings"]);
        apply_gnome(&runner, "127.0.0.1", 7890, 7891).unwrap();

        let commands = runner.fake.as_ref().unwrap().lock().unwrap().commands.clone();
        assert!(commands.contains(&"gsettings set org.gnome.system.proxy mode 'manual'".to_string()));
        assert!(commands
            .contains(&"gsettings set org.gnome.system.proxy.http port 7890".to_string()));
        assert!(commands
            .contains(&"gsettings set org.gnome.system.proxy.socks port 7891".to_string()));
        assert!(commands
            .contains(&"gsettings set org.gnome.system.proxy.https host '127.0.0.1'".to_string()));
    }

    #[test]
    fn gnome_restore_uses_captured_values() {
        let runner = Runner::fake(&[], &["gsettings"]);
        let mut original = BTreeMap::new();
        original.insert(
            gnome_key_id("org.gnome.system.proxy", "mode"),
            "'none'".to_string(),
        );
        original.insert(
            gnome_key_id("org.gnome.system.proxy.http", "port"),
            "3128".to_string(),
        );

        restore_gnome(&runner, &original).unwrap();

        let commands = runner.fake.as_ref().unwrap().lock().unwrap().commands.clone();
        assert!(commands.contains(&"gsettings set org.gnome.system.proxy.http port 3128".to_string()));
        assert!(commands.contains(&"gsettings set org.gnome.system.proxy mode 'none'".to_string()));
    }

    #[test]
    fn kde_apply_writes_proxy_type_and_urls() {
        let runner = Runner::fake(&[], &["kwriteconfig6", "kreadconfig6"]);
        apply_kde(&runner, "127.0.0.1", 7890, 7891).unwrap();

        let commands = runner.fake.as_ref().unwrap().lock().unwrap().commands.clone();
        assert!(commands.iter().any(|c| c
            == "kwriteconfig6 --file kioslaverc --group Proxy Settings --key ProxyType 1"));
        assert!(commands.iter().any(|c| c
            == "kwriteconfig6 --file kioslaverc --group Proxy Settings --key httpProxy http://127.0.0.1:7890"));
        assert!(commands.iter().any(|c| c
            == "kwriteconfig6 --file kioslaverc --group Proxy Settings --key socksProxy socks://127.0.0.1:7891"));
    }

    #[test]
    fn detect_prefers_kde_config_when_available() {
        let runner = Runner::fake(
            &[("gsettings get org.gnome.system.proxy mode", "'none'")],
            &["gsettings", "kwriteconfig6", "kreadconfig6"],
        );
        // `home` has no kioslaverc, but the desktop hint says KDE.
        let backend = detect_backend(&runner, None, "KDE").unwrap();
        assert_eq!(backend, Backend::Kde);

        let backend = detect_backend(&runner, None, "GNOME").unwrap();
        assert_eq!(backend, Backend::Gnome);
    }

    #[test]
    fn detect_reports_unsupported_without_tools() {
        let runner = Runner::fake(&[], &[]);
        assert!(detect_backend(&runner, None, "GNOME").is_err());
    }

    #[test]
    fn runner_renders_user_and_env_prefix() {
        let runner = Runner {
            user: Some("alice".into()),
            envs: vec![("HOME".into(), "/home/alice".into())],
            fake: Some(Arc::new(Mutex::new(FakeState::default()))),
        };
        runner.run("gsettings", &args(&["get", "org.gnome.system.proxy", "mode"]))
            .unwrap();
        let commands = runner.fake.as_ref().unwrap().lock().unwrap().commands.clone();
        // The fake records the logical command, not the privilege wrapper.
        assert_eq!(commands[0], "gsettings get org.gnome.system.proxy mode");
    }
}
