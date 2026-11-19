//! Profile handlers: list, add, activate, update, refresh, delete

use actix_web::{web, HttpResponse};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task;

use crate::ServiceState;
use control_tower_service_core::{ProfilesYaml, ProfileDownloadOptions, ProfileItem};

use super::{ApiResponse, AddProfileRequest, RefreshRequest};

/// Persist a profile file atomically and owner-only (it contains proxies and
/// their credentials).
fn save_profile_file(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    control_tower_service_core::write_private_atomic(path, content)
}

/// Field updates computed from a `PATCH /api/profiles/{id}` request.
///
/// They are applied to a *freshly read* manifest entry inside
/// `update_profiles`, so two concurrent edits (e.g. rename + cron update) do
/// not overwrite each other with a stale snapshot.
#[derive(Clone)]
struct ProfileEdits {
    /// Normalized `file` value (uid[..8].yaml for long uids).
    file: Option<String>,
    /// `Some` when the field was present in the request; inner `None` clears it.
    name: Option<Option<String>>,
    cron: Option<Option<String>>,
    options: Option<Option<ProfileDownloadOptions>>,
    /// Set only when the subscription URL actually changed.
    url: Option<String>,
    mark_updated: bool,
}

fn apply_profile_edits(item: &mut ProfileItem, edits: &ProfileEdits) {
    if let Some(file) = &edits.file {
        if item.file.as_ref() != Some(file) {
            item.file = Some(file.clone());
        }
    }
    if let Some(name) = &edits.name {
        item.name = name.clone();
    }
    if let Some(cron) = &edits.cron {
        item.cron = cron.clone();
    }
    if let Some(options) = &edits.options {
        match options {
            Some(options) => {
                let mut options = options.clone();
                // `GET /api/profiles` deliberately omits header values, so a
                // missing `headers` field means "keep the stored headers".
                // Sending `"headers": {}` explicitly clears them.
                if options.headers.is_none() {
                    options.headers = item.options.as_ref().and_then(|o| o.headers.clone());
                }
                item.options = Some(options);
            }
            None => item.options = None,
        }
    }
    if let Some(url) = &edits.url {
        item.url = Some(url.clone());
    }
    if edits.mark_updated {
        item.updated_at = Some(chrono::Utc::now().timestamp());
    }
}

/// Resolve the on-disk path for a profile file.
///
/// Tries in order:
///   1. `profiles_dir/file_name` (if file_name is set)
///   2. `profiles_dir/{uid[..8]}.yaml` (canonical fallback)
///   3. `profiles_dir/{uid}.yaml` (legacy fallback for old buggy data)
fn resolve_profile_file(profiles_dir: &PathBuf, uid: &str, file_name: Option<&str>) -> PathBuf {
    if let Some(f) = file_name {
        let p = profiles_dir.join(f);
        if p.exists() {
            return p;
        }
    }
    if uid.len() > 8 {
        let short = profiles_dir.join(format!("{}.yaml", &uid[..8]));
        if short.exists() {
            return short;
        }
    }
    profiles_dir.join(format!("{}.yaml", uid))
}

/// Execute a script and return its stdout as config content
fn execute_script(script_content: &str, profile_file: &PathBuf) -> Result<String, String> {
    use std::process::Command;

    // Write script to a temp file to execute it
    let script_ext = if cfg!(windows) { ".ps1" } else { ".sh" };
    let script_file = profile_file.with_extension(script_ext);
    std::fs::write(&script_file, script_content)
        .map_err(|e| format!("Failed to write script file: {}", e))?;

    let output = if cfg!(windows) {
        Command::new("powershell")
            .args(["-ExecutionPolicy", "Bypass", "-File", script_file.to_str().unwrap_or("")])
            .output()
            .map_err(|e| format!("Failed to execute script: {}", e))?
    } else {
        Command::new("sh")
            .arg(script_file.to_str().unwrap_or(""))
            .output()
            .map_err(|e| format!("Failed to execute script: {}", e))?
    };

    // Clean up temp script file
    let _ = std::fs::remove_file(&script_file);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Script failed: {}", stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Merge multiple profiles into a single config
fn merge_profiles(profile_uids: &[String], profiles_dir: &PathBuf, profiles_yaml: &ProfilesYaml) -> Result<String, String> {
    use serde_yaml_ng::Value;

    let mut merged: Value = serde_yaml_ng::from_str("mode: rule\nproxies: []\n")
        .map_err(|e| format!("Failed to parse base config: {}", e))?;

    for uid in profile_uids {
        // Find the profile in profiles.yaml
        let profile_item = profiles_yaml.items.iter()
            .find(|p| p.uid == *uid)
            .ok_or_else(|| format!("Profile {} not found for merge", uid))?;

        // Resolve the profile file
        let profile_path = resolve_profile_file(profiles_dir, uid, profile_item.file.as_deref());

        // Read and parse the profile
        let content = std::fs::read_to_string(&profile_path)
            .map_err(|e| format!("Failed to read profile {}: {}", uid, e))?;

        let profile_yaml: Value = serde_yaml_ng::from_str(&content)
            .map_err(|e| format!("Failed to parse profile {}: {}", uid, e))?;

        // Merge proxies
        if let (Some(merged_map), Some(profile_map)) = (merged.as_mapping_mut(), profile_yaml.as_mapping()) {
            // Merge proxies arrays
            if let (Some(merged_proxies), Some(profile_proxies)) = (
                merged_map.get_mut("proxies"),
                profile_map.get("proxies")
            ) {
                if let (Some(merged_arr), Some(profile_arr)) = (merged_proxies.as_sequence_mut(), profile_proxies.as_sequence()) {
                    for proxy in profile_arr {
                        merged_arr.push(proxy.clone());
                    }
                }
            } else if let Some(profile_proxies) = profile_map.get("proxies") {
                merged_map.insert("proxies".into(), profile_proxies.clone());
            }

            // Merge proxy-groups
            if let (Some(merged_groups), Some(profile_groups)) = (
                merged_map.get_mut("proxy-groups"),
                profile_map.get("proxy-groups")
            ) {
                if let (Some(merged_arr), Some(profile_arr)) = (merged_groups.as_sequence_mut(), profile_groups.as_sequence()) {
                    for group in profile_arr {
                        merged_arr.push(group.clone());
                    }
                }
            } else if let Some(profile_groups) = profile_map.get("proxy-groups") {
                merged_map.insert("proxy-groups".into(), profile_groups.clone());
            }

            // Merge rules
            if let (Some(merged_rules), Some(profile_rules)) = (
                merged_map.get_mut("rules"),
                profile_map.get("rules")
            ) {
                if let (Some(merged_arr), Some(profile_arr)) = (merged_rules.as_sequence_mut(), profile_rules.as_sequence()) {
                    for rule in profile_arr {
                        merged_arr.push(rule.clone());
                    }
                }
            } else if let Some(profile_rules) = profile_map.get("rules") {
                merged_map.insert("rules".into(), profile_rules.clone());
            }
        }
    }

    serde_yaml_ng::to_string(&merged)
        .map_err(|e| format!("Failed to serialize merged config: {}", e))
}

/// GET /api/profiles - Returns list of profiles
pub async fn get_profiles() -> HttpResponse {
    let paths = super::get_control_tower_paths();
    let profiles_path = &paths.profiles_path;

    if !profiles_path.exists() {
        return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "items": Vec::<serde_json::Value>::new(),
            "current": serde_json::Value::Null
        })));
    }

    match std::fs::read_to_string(profiles_path) {
        Ok(content) => {
            let result = super::parse_profiles_yaml_full(&content);
            HttpResponse::Ok().json(ApiResponse::success(result))
        }
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to read profiles.yaml: {}", e))),
    }
}

/// POST /api/profiles - Add a new profile (remote, local, script, or merge type)
pub async fn add_profile(
    body: web::Json<AddProfileRequest>,
) -> HttpResponse {
    let paths = super::get_control_tower_paths();
    let profiles_path = &paths.profiles_path;
    let profiles_dir = paths.config_dir.join("profiles");

    // Ensure profiles directory exists
    if let Err(e) = std::fs::create_dir_all(&profiles_dir) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to create profiles dir: {}", e)));
    }

    let profile_type = body.type_.as_deref().unwrap_or("remote");
    let uid = uuid::Uuid::new_v4().to_string();
    let file_name = format!("{}.yaml", &uid[..8]);
    let profile_file = profiles_dir.join(&file_name);

    match profile_type {
        "local" => {
            // Local profile: just create empty file, user provides content later
            // or copy from existing file
            if let Some(ref src_file) = body.file {
                let src_path = std::path::Path::new(src_file);
                if src_path.exists() {
                    if let Err(e) = std::fs::copy(src_path, &profile_file) {
                        return HttpResponse::InternalServerError()
                            .json(ApiResponse::<()>::error(format!("Failed to copy file: {}", e)));
                    }
                }
            } else {
                // Create empty file
                if let Err(e) = save_profile_file(&profile_file, "mode: rule\nproxies: []\n") {
                    return HttpResponse::InternalServerError()
                        .json(ApiResponse::<()>::error(format!("Failed to create profile file: {}", e)));
                }
            }
        }
        "script" => {
            // Script profile: store script content, execute at activation
            let script_content = body.script.as_deref().unwrap_or("");
            if let Err(e) = save_profile_file(&profile_file, script_content) {
                return HttpResponse::InternalServerError()
                    .json(ApiResponse::<()>::error(format!("Failed to write script: {}", e)));
            }
        }
        "merge" => {
            // Merge profile: just store references, merge at activation
            // Create a placeholder file
            if let Err(e) = save_profile_file(&profile_file, "# Merge profile - content generated at activation\nmode: rule\n") {
                return HttpResponse::InternalServerError()
                    .json(ApiResponse::<()>::error(format!("Failed to create merge profile: {}", e)));
            }
        }
        _ => {
            // Remote/subscription profile: download from URL
            let url = match body.url.as_ref() {
                Some(u) if !u.trim().is_empty() => u.clone(),
                _ => {
                    return HttpResponse::BadRequest()
                        .json(ApiResponse::<()>::error("URL is required for remote profiles".to_string()));
                }
            };

            let options = body.options.as_ref();
            let timeout_secs = options.map(|o| o.timeout_or(30)).unwrap_or(30);
            let with_proxy = options.and_then(|o| o.with_proxy).unwrap_or(false);
            let danger_accept_invalid_certs = options
                .and_then(|o| o.danger_accept_invalid_certs)
                .unwrap_or(false);

            let mut builder = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(timeout_secs))
                .danger_accept_invalid_certs(danger_accept_invalid_certs);
            if with_proxy {
                let proxy_url = format!("http://127.0.0.1:{}", super::get_mihomo_http_port());
                match reqwest::Proxy::http(&proxy_url) {
                    Ok(proxy) => builder = builder.proxy(proxy),
                    Err(e) => {
                        return HttpResponse::InternalServerError()
                            .json(ApiResponse::<()>::error(format!("Failed to create proxy: {}", e)))
                    }
                }
            }
            let client = match builder.build() {
                Ok(c) => c,
                Err(e) => {
                    return HttpResponse::InternalServerError()
                        .json(ApiResponse::<()>::error(format!("Failed to create HTTP client: {}", e)))
                }
            };

            let mut request = client.get(&url);
            let has_custom_user_agent = options
                .map(|o| o.has_header("user-agent"))
                .unwrap_or(false);
            if !has_custom_user_agent {
                request = request.header(
                    "User-Agent",
                    options
                        .map(|o| o.user_agent_or("clash-verge/v2.4.7"))
                        .unwrap_or("clash-verge/v2.4.7"),
                );
            }
            if let Some(options) = options {
                for (name, value) in options.header_pairs() {
                    request = request.header(name, value);
                }
            }

            let response = match request.send().await {
                Ok(r) => r,
                Err(e) => {
                    return HttpResponse::BadRequest()
                        .json(ApiResponse::<()>::error(format!("Failed to download profile: {}", e)))
                }
            };

            if !response.status().is_success() {
                return HttpResponse::BadRequest()
                    .json(ApiResponse::<()>::error(format!("Download failed: {}", response.status())));
            }

            let content = match response.text().await {
                Ok(c) => c,
                Err(e) => {
                    return HttpResponse::InternalServerError()
                        .json(ApiResponse::<()>::error(format!("Failed to read download: {}", e)))
                }
            };

            // Validate it's YAML (not HTML error page)
            if content.trim().starts_with('<') {
                return HttpResponse::BadRequest()
                    .json(ApiResponse::<()>::error("Downloaded content looks like HTML, not a valid subscription".to_string()));
            }

            // Save the profile file
            if let Err(e) = save_profile_file(&profile_file, &content) {
                return HttpResponse::InternalServerError()
                    .json(ApiResponse::<()>::error(format!("Failed to save profile: {}", e)));
            }
        }
    }

    // Get the profile name
    let name = body.name.clone().unwrap_or_else(|| {
        if profile_type == "remote" {
            body.url.as_ref()
                .and_then(|u| u.split('/').last())
                .map(|s| s.trim_end_matches(".yaml").to_string())
                .unwrap_or_else(|| format!("Profile {}", &uid[..8]))
        } else {
            format!("{} profile", profile_type)
        }
    });

    // Update profiles.yaml. The read-modify-write happens inside
    // `update_profiles`, so a concurrent cron/manifest update is not lost.
    let new_item = ProfileItem {
        uid: uid.clone(),
        name: Some(name),
        file: Some(file_name.clone()),
        url: body.url.clone(),
        cron: None,
        updated_at: Some(chrono::Utc::now().timestamp()),
        options: body.options.clone(),
        type_: Some(profile_type.to_string()),
        script: body.script.clone(),
        merge: body.merge.clone().unwrap_or_default(),
    };

    if let Err(e) = control_tower_service_core::update_profiles(profiles_path, |yaml| {
        yaml.items.push(new_item);
        Ok(())
    }) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to update profiles.yaml: {}", e)));
    }

    HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
        "uid": uid,
        "name": body.name,
        "type": profile_type,
        "file": file_name
    })))
}

/// PUT /api/profiles/{id}/activate - Activate a profile
pub async fn activate_profile(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
) -> HttpResponse {
    let uid = path.into_inner();
    let paths = super::get_control_tower_paths();
    let profiles_path = &paths.profiles_path;

    if !profiles_path.exists() {
        return HttpResponse::NotFound().json(ApiResponse::<()>::error("profiles.yaml not found"));
    }

    // Read profiles.yaml to find the profile's file path
    let content = match std::fs::read_to_string(profiles_path) {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to read profiles.yaml: {}", e)))
        }
    };

    let yaml: ProfilesYaml = match serde_yaml_ng::from_str(&content) {
        Ok(y) => y,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error(format!("Invalid profiles.yaml: {}", e)))
        }
    };

    // Find the profile by uid
    let profile_item = match yaml.items.iter().find(|p| p.uid == uid) {
        Some(item) => item,
        None => {
            return HttpResponse::NotFound().json(ApiResponse::<()>::error(format!("Profile {} not found in profiles.yaml", uid)));
        }
    };

    let profiles_dir = paths.config_dir.join("profiles");
    let actual_file = resolve_profile_file(&profiles_dir, &uid, profile_item.file.as_deref());

    // Get profile content based on type
    let profile_content = match profile_item.type_.as_deref().unwrap_or("remote") {
        "script" => {
            // Execute script to generate config
            let script_content = profile_item.script.as_deref().unwrap_or("");
            match execute_script(script_content, &actual_file) {
                Ok(content) => {
                    // Write generated content to profile file for later use
                    if let Err(e) = save_profile_file(&actual_file, &content) {
                        tracing::warn!("Failed to write script output to profile file: {}", e);
                    }
                    content
                }
                Err(e) => {
                    return HttpResponse::BadRequest()
                        .json(ApiResponse::<()>::error(format!("Script execution failed: {}", e)))
                }
            }
        }
        "merge" => {
            // Merge referenced profiles
            if profile_item.merge.is_empty() {
                return HttpResponse::BadRequest()
                    .json(ApiResponse::<()>::error("Merge profile has no profiles to merge".to_string()));
            }
            match merge_profiles(&profile_item.merge, &profiles_dir, &yaml) {
                Ok(content) => {
                    // Write merged content to profile file for later use
                    if let Err(e) = save_profile_file(&actual_file, &content) {
                        tracing::warn!("Failed to write merged content to profile file: {}", e);
                    }
                    content
                }
                Err(e) => {
                    return HttpResponse::BadRequest()
                        .json(ApiResponse::<()>::error(format!("Merge failed: {}", e)))
                }
            }
        }
        _ => {
            // Remote or local: read directly from file
            match std::fs::read_to_string(&actual_file) {
                Ok(c) => c,
                Err(e) => {
                    return HttpResponse::BadRequest()
                        .json(ApiResponse::<()>::error(format!("Failed to read profile file: {}", e)))
                }
            }
        }
    };

    // Parse and validate the profile YAML has required fields
    let yaml: serde_yaml_ng::Value = match serde_yaml_ng::from_str(&profile_content) {
        Ok(y) => y,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error(format!("Invalid YAML in profile: {}", e)))
        }
    };

    // Check for required fields: proxies, a listener port, or proxy providers
    let has_proxies = yaml.get("proxies").is_some();
    let has_http_port = yaml.get("port").is_some();
    let has_mixed_port = yaml.get("mixed-port").is_some();
    let has_proxy_providers = yaml.get("proxy-providers").is_some();
    let has_proxy_groups = yaml.get("proxy-groups").is_some();

    if !has_proxies
        && !has_http_port
        && !has_mixed_port
        && !has_proxy_providers
        && !has_proxy_groups
    {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("Profile does not contain valid proxy configuration (missing: proxies, port, mixed-port, proxy-providers, or proxy-groups)".to_string()));
    }

    // Mark the profile as current. Re-read under the profiles.yaml lock so a
    // concurrent edit is not overwritten by this stale snapshot.
    let result = control_tower_service_core::update_profiles(profiles_path, |yaml| {
        if !yaml.items.iter().any(|item| item.uid == uid) {
            anyhow::bail!("Profile {} not found in profiles.yaml", uid);
        }
        yaml.current = Some(uid.clone());
        Ok(())
    });
    if let Err(e) = result {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to update profiles.yaml: {}", e)));
    }

    // Replace active config from profile, preserving custom user rules
    let settings = state.get_settings();
    let custom_rules: Vec<String> = settings.custom_rules.unwrap_or_default();

    // Build port overrides from settings.yaml (highest priority)
    // HTTP and mixed proxies are independent ports: mixed_port is only used
    // when it is explicitly configured, never derived from http_port.
    let port_overrides = control_tower_service_core::active_config::PortOverrides {
        mixed_port: settings.mixed_port,
        socks_port: settings.socks_port,
        http_port: settings.http_port,
        external_controller: settings.api_host.zip(settings.api_port),
    };

    let store = control_tower_service_core::ActiveConfigStore::new(paths.clone());
    let profile_rules_count = match store.replace_from_profile(&actual_file, &custom_rules, port_overrides) {
        Ok(count) => count,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to activate profile: {}", e)));
        }
    };

    // Store profile_rules_count in settings.yaml so add/delete rules knows the boundary
    if let Err(e) = super::update_profile_rules_count(profile_rules_count) {
        tracing::warn!("Failed to update profile_rules_count in settings: {}", e);
    }

    // Inject rule_providers from settings.yaml into the new config.yaml
    // (rule-providers is NOT preserved through replace_from_profile to avoid
    // carrying over corrupted data from old config)
    if let Some(ref providers) = settings.rule_providers {
        if !providers.is_empty() {
            if let Err(e) = store.set_rule_providers(providers) {
                tracing::warn!("Failed to inject rule-providers into config: {}", e);
            }
        }
    }

    // Restart Mihomo to load new config
    let config_path = paths.active_config_path.clone();
    let state = state.clone();
    match task::spawn_blocking(move || state.restart_with_config(&config_path)).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to restart Mihomo: {}", e))),
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to restart Mihomo: {}", e.to_string()))),
    }
}

/// PATCH /api/profiles/{id} - Update name, URL, and/or cron schedule for a profile.
/// If the URL is changed, the subscription content is re-downloaded and the profile
/// file is overwritten. If the profile is currently active, Mihomo is restarted.
pub async fn update_profile(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
    body: web::Json<serde_json::Value>,
) -> HttpResponse {
    let uid = path.into_inner();
    let paths = super::get_control_tower_paths();
    let profiles_path = &paths.profiles_path;
    let profiles_dir = paths.config_dir.join("profiles");

    if !profiles_path.exists() {
        return HttpResponse::NotFound().json(ApiResponse::<()>::error("profiles.yaml not found"));
    }

    // Parse body as serde_json::Value to distinguish "key absent" from "key = null" from "key = ''"
    let obj = match body.as_object() {
        Some(o) => o,
        None => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error("Request body must be a JSON object".to_string()))
        }
    };

    // Returns (value_str, was_explicitly_provided)
    fn get_string_field(obj: &serde_json::Map<String, serde_json::Value>, key: &str) -> (Option<String>, bool) {
        if let Some(v) = obj.get(key) {
            if v.is_null() {
                return (None, true); // explicitly null → clear
            }
            if let Some(s) = v.as_str() {
                return (Some(s.to_string()), true); // present with value
            }
        }
        (None, false) // absent or wrong type
    }

    let (name, name_provided) = get_string_field(obj, "name");
    let (url, url_provided) = get_string_field(obj, "url");
    let (cron, cron_provided) = get_string_field(obj, "cron");

    // Parse options field if provided
    let new_options = match obj.get("options") {
        None => None,
        Some(v) if v.is_null() => Some(None),
        Some(v) => match serde_json::from_value::<ProfileDownloadOptions>(v.clone()) {
            Ok(options) => Some(Some(options)),
            // Never treat a malformed options object as "clear the options":
            // that silently drops headers / proxy settings on a typo.
            Err(e) => {
                return HttpResponse::BadRequest()
                    .json(ApiResponse::<()>::error(format!("Invalid options: {}", e)))
            }
        },
    };

    // Validate URL if explicitly provided (even as empty string → clear not allowed for URL)
    if url_provided && url.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true) {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("URL cannot be empty".to_string()));
    }

    // Read profiles.yaml (read-only snapshot: used to resolve the profile file
    // and to decide whether the URL changed. The actual mutation re-reads the
    // manifest under the profiles.yaml lock below.)
    let content = match std::fs::read_to_string(profiles_path) {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to read profiles.yaml: {}", e)))
        }
    };

    let yaml = match serde_yaml_ng::from_str::<ProfilesYaml>(&content) {
        Ok(y) => y,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error(format!("Invalid profiles.yaml: {}", e)))
        }
    };

    // Find the profile item
    let item = match yaml.items.iter().find(|i| i.uid == uid) {
        Some(i) => i,
        None => {
            return HttpResponse::NotFound()
                .json(ApiResponse::<()>::error(format!("Profile {} not found", uid)))
        }
    };

    let is_active = yaml.current.as_ref() == Some(&uid);
    let old_url = item.url.clone();
    let url_changed = url_provided && url.as_ref() != old_url.as_ref();

    // Normalize file field to uid[..8].yaml so all code paths find the same file.
    // Historical bug: item.file may contain full uid instead of uid[..8].
    let normalized_file = if uid.len() > 8 {
        Some(format!("{}.yaml", &uid[..8]))
    } else {
        None
    };

    // Validate and normalize cron: cron_provided means the key was present
    // (even if null/empty → clear).
    let cron_edit = if cron_provided {
        let trimmed = cron.as_ref().map(|s| s.trim()).unwrap_or("");
        if trimmed.is_empty() {
            Some(None)
        } else {
            match trimmed.parse::<u32>() {
                Ok(n) if n >= 1 => Some(Some(trimmed.to_string())),
                _ => {
                    return HttpResponse::BadRequest().json(ApiResponse::<()>::error(
                        "Cron must be a positive integer (minutes)".to_string(),
                    ))
                }
            }
        }
    } else {
        None
    };

    let edits = ProfileEdits {
        file: normalized_file.clone(),
        // null or "" → clear, otherwise update
        name: if name_provided {
            Some(name.filter(|s| !s.trim().is_empty()))
        } else {
            None
        },
        cron: cron_edit,
        // null → clear, otherwise update (headers fallback happens when applied)
        options: new_options,
        url: if url_changed { url.clone() } else { None },
        mark_updated: url_changed,
    };

    let profile_file = resolve_profile_file(
        &profiles_dir,
        &uid,
        normalized_file.as_deref().or(item.file.as_deref()),
    );

    // Options to use for the download: the requested ones when the PATCH also
    // updates `options`, otherwise the stored ones.
    let download_options = match &edits.options {
        Some(Some(options)) => {
            let mut options = options.clone();
            if options.headers.is_none() {
                options.headers = item.options.as_ref().and_then(|o| o.headers.clone());
            }
            Some(options)
        }
        Some(None) => None,
        None => item.options.clone(),
    };

    // If URL changed, download new content in a blocking task.
    if url_changed {
        let download_url = url.clone().unwrap(); // url_changed means url_provided && Some
        let profile_file_clone = profile_file.clone();
        let profiles_path_clone = profiles_path.clone();
        let options_clone = download_options.clone();
        let edits_clone = edits.clone();
        let uid_clone = uid.clone();

        let timeout_secs = options_clone.as_ref().map(|o| o.timeout_or(30)).unwrap_or(30);
        let with_proxy = options_clone
            .as_ref()
            .and_then(|o| o.with_proxy)
            .unwrap_or(false);
        let danger_accept_invalid_certs = options_clone
            .as_ref()
            .and_then(|o| o.danger_accept_invalid_certs)
            .unwrap_or(false);
        let proxy_url = format!("http://127.0.0.1:{}", super::get_mihomo_http_port());

        let result = tokio::task::spawn_blocking(move || {
            let mut builder = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(timeout_secs))
                .danger_accept_invalid_certs(danger_accept_invalid_certs);
            if with_proxy {
                builder = builder.proxy(
                    reqwest::Proxy::http(&proxy_url)
                        .map_err(|e| anyhow::anyhow!("Failed to create proxy: {}", e))?,
                );
            }
            let client = builder
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;

            let mut request = client.get(&download_url);
            let has_custom_user_agent = options_clone
                .as_ref()
                .map(|o| o.has_header("user-agent"))
                .unwrap_or(false);
            if !has_custom_user_agent {
                request = request.header(
                    "User-Agent",
                    options_clone
                        .as_ref()
                        .map(|o| o.user_agent_or("clash-verge/v2.4.7"))
                        .unwrap_or("clash-verge/v2.4.7"),
                );
            }
            if let Some(options) = options_clone.as_ref() {
                for (name, value) in options.header_pairs() {
                    request = request.header(name, value);
                }
            }

            let response = request
                .send()
                .map_err(|e| anyhow::anyhow!("Failed to download: {}", e))?;

            if !response.status().is_success() {
                anyhow::bail!("Download failed: {}", response.status());
            }

            let new_content = response
                .text()
                .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

            // Validate it's YAML (not HTML error page)
            if new_content.trim().starts_with('<') {
                anyhow::bail!("Downloaded content looks like HTML, not a valid subscription");
            }

            // Write new content to profile file
            control_tower_service_core::write_private_atomic(&profile_file_clone, &new_content)?;

            // Apply the edits to the freshly read manifest (name/cron/options/
            // url/updated_at) so a concurrent change is not overwritten.
            control_tower_service_core::update_profiles(&profiles_path_clone, |yaml| {
                let item = yaml
                    .items
                    .iter_mut()
                    .find(|item| item.uid == uid_clone)
                    .ok_or_else(|| anyhow::anyhow!("Profile {} not found", uid_clone))?;
                apply_profile_edits(item, &edits_clone);
                Ok(())
            })?;

            Ok::<(), anyhow::Error>(())
        })
        .await;

        match result {
            Ok(Ok(())) => {
                tracing::info!("Profile {} URL updated and subscription re-downloaded", uid);
            }
            Ok(Err(e)) => {
                tracing::error!("Failed to update subscription for profile {}: {}", uid, e);
                return HttpResponse::BadRequest()
                    .json(ApiResponse::<()>::error(format!("Failed to download new subscription: {}", e)));
            }
            Err(e) => {
                return HttpResponse::InternalServerError()
                    .json(ApiResponse::<()>::error(format!("Task error: {}", e)));
            }
        }

        // If this profile is currently active, replace active config and restart Mihomo
        if is_active {
            let settings = state.get_settings();
            let custom_rules: Vec<String> = settings.custom_rules.unwrap_or_default();

            // Build port overrides from settings.yaml (highest priority)
            // HTTP and mixed proxies are independent ports: mixed_port is only used
            // when it is explicitly configured, never derived from http_port.
            let port_overrides = control_tower_service_core::active_config::PortOverrides {
                mixed_port: settings.mixed_port,
                socks_port: settings.socks_port,
                http_port: settings.http_port,
                external_controller: settings.api_host.zip(settings.api_port),
            };

            let store = control_tower_service_core::ActiveConfigStore::new(paths.clone());
            let profile_rules_count = match store.replace_from_profile(&profile_file, &custom_rules, port_overrides) {
                Ok(count) => count,
                Err(e) => {
                    return HttpResponse::InternalServerError()
                        .json(ApiResponse::<()>::error(format!("Failed to update active config: {}", e)));
                }
            };

            // Store profile_rules_count in settings.yaml
            if let Err(e) = super::update_profile_rules_count(profile_rules_count) {
                tracing::warn!("Failed to update profile_rules_count in settings: {}", e);
            }

            let config_path = paths.active_config_path.clone();
            let state = state.clone();
            match task::spawn_blocking(move || state.restart_with_config(&config_path)).await {
                Ok(Ok(())) => {
                    tracing::info!("Profile {} activated with new URL, Mihomo restarted", uid);
                }
                Ok(Err(e)) => {
                    return HttpResponse::InternalServerError()
                        .json(ApiResponse::<()>::error(format!("Mihomo restart failed: {}", e)));
                }
                Err(e) => {
                    return HttpResponse::InternalServerError()
                        .json(ApiResponse::<()>::error(format!("Task error: {}", e)));
                }
            }
        }
    } else {
        // No URL change — just apply name/cron/options updates
        if let Err(e) = control_tower_service_core::update_profiles(profiles_path, |yaml| {
            let item = yaml
                .items
                .iter_mut()
                .find(|item| item.uid == uid)
                .ok_or_else(|| anyhow::anyhow!("Profile {} not found", uid))?;
            apply_profile_edits(item, &edits);
            Ok(())
        }) {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to update profiles.yaml: {}", e)));
        }
    }

    // Reload cron jobs so the scheduler picks up any new schedule
    state.load_cron_jobs();

    tracing::info!("Profile {} updated", uid);
    HttpResponse::Ok().json(ApiResponse::<()>::success(()))
}

/// POST /api/profiles/{id}/refresh - Manually refresh a subscription profile
pub async fn refresh_profile(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
    body: Option<web::Json<RefreshRequest>>,
) -> HttpResponse {
    let uid = path.into_inner();
    let paths = super::get_control_tower_paths();
    let profiles_path = &paths.profiles_path;
    let profiles_dir = paths.config_dir.join("profiles");

    // Read profiles.yaml to get URL and file path
    let content = match std::fs::read_to_string(profiles_path) {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to read profiles.yaml: {}", e)))
        }
    };

    use control_tower_service_core::ProfilesYaml as ProfilesYamlRead;
    let yaml: ProfilesYamlRead = match serde_yaml_ng::from_str(&content) {
        Ok(y) => y,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to parse profiles.yaml: {}", e)))
        }
    };

    let profile_item = match yaml.items.iter().find(|i| i.uid == uid) {
        Some(item) => item,
        None => {
            return HttpResponse::NotFound()
                .json(ApiResponse::<()>::error("Profile not found".to_string()))
        }
    };

    let url = match &profile_item.url {
        Some(u) => u.clone(),
        None => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error("Profile has no subscription URL".to_string()))
        }
    };

    // Determine profile file path using helper
    let profile_file = resolve_profile_file(&profiles_dir, &uid, profile_item.file.as_deref());

    if !profile_file.exists() {
        return HttpResponse::NotFound()
            .json(ApiResponse::<()>::error("Profile file not found".to_string()));
    }

    // Get current profile info for reload
    let is_current = yaml.current.as_ref() == Some(&uid);

    // Get settings for port overrides and custom rules
    let settings = state.get_settings();
    let custom_rules: Vec<String> = settings.custom_rules.unwrap_or_default();
    // HTTP and mixed proxies are independent ports: mixed_port is only used
    // when it is explicitly configured, never derived from http_port.
    let port_overrides = control_tower_service_core::active_config::PortOverrides {
        mixed_port: settings.mixed_port,
        socks_port: settings.socks_port,
        http_port: settings.http_port,
        external_controller: settings.api_host.zip(settings.api_port),
    };

    // Clone for blocking task
    let profile_file_clone = profile_file.clone();
    let profiles_path_clone = profiles_path.clone();
    let use_proxy = body
        .as_ref()
        .and_then(|b| b.use_proxy)
        .or_else(|| profile_item.options.as_ref().and_then(|o| o.with_proxy))
        .unwrap_or(false);
    let http_port = super::get_mihomo_http_port();
    let paths_clone = paths.clone();
    let custom_rules_clone = custom_rules.clone();
    let port_overrides_clone = port_overrides.clone();
    let settings_clone = state.get_settings();
    let uid_clone = uid.clone();
    let download_options = profile_item.options.clone();
    let download_timeout = download_options.as_ref().map(|o| o.timeout_or(60)).unwrap_or(60);
    let danger_accept_invalid_certs = download_options
        .as_ref()
        .and_then(|o| o.danger_accept_invalid_certs)
        .unwrap_or(false);

    let result = tokio::task::spawn_blocking(move || {
        // Download subscription (optionally through Mihomo proxy)
        let timeout = std::time::Duration::from_secs(download_timeout);
        let mut builder = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .danger_accept_invalid_certs(danger_accept_invalid_certs);
        if use_proxy {
            // Route through Mihomo HTTP proxy
            let proxy_url = format!("http://127.0.0.1:{}", http_port);
            let proxy = reqwest::Proxy::http(proxy_url)
                .map_err(|e| anyhow::anyhow!("Failed to create proxy: {}", e))?;
            builder = builder.proxy(proxy);
        }
        let client = builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;

        let mut request = client.get(&url);
        let has_custom_user_agent = download_options
            .as_ref()
            .map(|o| o.has_header("user-agent"))
            .unwrap_or(false);
        if !has_custom_user_agent {
            request = request.header(
                "User-Agent",
                download_options
                    .as_ref()
                    .map(|o| o.user_agent_or("clash-verge/v2.4.7"))
                    .unwrap_or("clash-verge/v2.4.7"),
            );
        }
        if let Some(options) = download_options.as_ref() {
            for (name, value) in options.header_pairs() {
                request = request.header(name, value);
            }
        }

        let response = request
            .send()
            .map_err(|e| anyhow::anyhow!("Failed to download: {}", e))?;

        if !response.status().is_success() {
            anyhow::bail!("Download failed: {}", response.status());
        }

        let new_content = response
            .text()
            .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

        // Update profile file
        control_tower_service_core::write_private_atomic(&profile_file_clone, &new_content)?;

        // Update updated_at in profiles.yaml (re-read under the manifest lock).
        control_tower_service_core::update_profiles(&profiles_path_clone, |yaml| {
            let now = chrono::Utc::now().timestamp();
            if let Some(item) = yaml.items.iter_mut().find(|item| item.uid == uid_clone) {
                item.updated_at = Some(now);
            }
            Ok(())
        })?;

        // If this is the current profile, update active config.yaml so Mihomo reload picks up new proxies
        if is_current {
            let store = control_tower_service_core::ActiveConfigStore::new(paths_clone.clone());
            match store.replace_from_profile(&profile_file_clone, &custom_rules_clone, port_overrides_clone) {
                Ok(_) => tracing::info!("Active config updated with refreshed profile content"),
                Err(e) => tracing::warn!("Failed to update active config after refresh: {}", e),
            }
            // Inject rule-providers from settings (same as activate_profile)
            drop(store);
            let store2 = control_tower_service_core::ActiveConfigStore::new(paths_clone.clone());
            if let Some(ref providers) = settings_clone.rule_providers {
                if !providers.is_empty() {
                    if let Err(e) = store2.set_rule_providers(providers) {
                        tracing::warn!("Failed to inject rule-providers into config: {}", e);
                    }
                }
            }
        }

        Ok::<(), anyhow::Error>(())
    })
    .await;

    match result {
        Ok(Ok(())) => {
            tracing::info!("Profile {} refreshed manually (use_proxy={})", uid, use_proxy);
            // Try hot-reload first (via Mihomo PUT /configs) in a blocking task
            let state_for_reload = state.clone();
            match task::spawn_blocking(move || state_for_reload.reload_config()).await {
                Ok(Ok(())) => {
                    tracing::info!("Profile {} hot-reloaded successfully", uid);
                    HttpResponse::Ok().json(ApiResponse::success(()))
                }
                Ok(Err(e)) => {
                    tracing::warn!("Hot-reload failed for profile {}, falling back to restart: {}", uid, e);
                    tracing::info!("Calling restart_with_config for profile {}", uid);
                    let config_path = paths.active_config_path.clone();
                    let state = state.clone();
                    match task::spawn_blocking(move || state.restart_with_config(&config_path)).await {
                        Ok(Ok(())) => {
                            tracing::info!("Profile {} restarted successfully (hot-reload had failed)", uid);
                            HttpResponse::Ok().json(ApiResponse::success(()))
                        }
                        Ok(Err(e)) => {
                            tracing::error!("Failed to restart Mihomo after refresh: {}", e);
                            HttpResponse::InternalServerError().json(ApiResponse::<()>::error(format!("Restart failed: {}", e)))
                        }
                        Err(e) => HttpResponse::InternalServerError()
                            .json(ApiResponse::<()>::error(format!("Task error: {}", e))),
                    }
                }
                Err(e) => HttpResponse::InternalServerError()
                    .json(ApiResponse::<()>::error(format!("Task error: {}", e))),
            }
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to refresh profile {}: {}", uid, e);
            HttpResponse::BadRequest().json(ApiResponse::<()>::error(e.to_string()))
        }
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Task error: {}", e))),
    }
}

/// DELETE /api/profiles/{id} - Delete a profile
pub async fn delete_profile(
    path: web::Path<String>,
) -> HttpResponse {
    let uid = path.into_inner();
    let paths = super::get_control_tower_paths();
    let profiles_path = &paths.profiles_path;
    let profiles_dir = paths.config_dir.join("profiles");

    if !profiles_path.exists() {
        return HttpResponse::NotFound().json(ApiResponse::<()>::error("profiles.yaml not found"));
    }

    // Remove the entry and clear the current marker atomically under the
    // profiles.yaml lock, so a concurrent manifest update is not lost.
    let result = control_tower_service_core::update_profiles(profiles_path, |yaml| {
        let Some(item) = yaml.items.iter().find(|item| item.uid == uid) else {
            return Ok(None);
        };
        let file = item.file.clone();
        yaml.items.retain(|item| item.uid != uid);
        if yaml.current.as_deref() == Some(uid.as_str()) {
            yaml.current = None;
        }
        Ok(Some(file))
    });

    let file_name = match result {
        Ok(Some(file)) => file,
        Ok(None) => {
            return HttpResponse::NotFound().json(ApiResponse::<()>::error(format!(
                "Profile {} not found",
                uid
            )))
        }
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to update profiles.yaml: {}", e)))
        }
    };

    // Determine actual file path using helper
    let profile_file = resolve_profile_file(&profiles_dir, &uid, file_name.as_deref());

    // Delete the profile file
    if profile_file.exists() {
        if let Err(e) = std::fs::remove_file(&profile_file) {
            tracing::warn!("Failed to delete profile file {:?}: {}", profile_file, e);
        }
    }

    HttpResponse::Ok().json(ApiResponse::<()>::success(()))
}

/// POST /api/profiles/check-updates - Check and update all profile subscriptions
pub async fn check_all_profiles() -> HttpResponse {
    let results = tokio::task::spawn_blocking(|| {
        crate::scheduler::check_and_update_all_profiles()
    }).await;

    match results {
        Ok(results) => {
            let updated_count = results.iter().filter(|(_, success)| *success).count();
            let total_count = results.len();
            tracing::info!("Profile update check complete: {}/{} updated", updated_count, total_count);
            HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
                "total": total_count,
                "updated": updated_count,
                "results": results,
            })))
        }
        Err(e) => {
            HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to check profiles: {}", e)))
        }
    }
}
