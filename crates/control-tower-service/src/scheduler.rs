//! Cron scheduler for profile auto-update

use std::time::Duration;

use crate::ServiceState;
use crate::settings::consts::MAX_CRON_INTERVAL_MINS;

/// Simplified cron schedule (minutes only)
#[derive(Debug, Clone)]
pub struct Schedule {
    /// Minutes between updates
    pub minutes: u32,
}

impl Schedule {
    /// Parse a simplified schedule string
    /// Format: just a number representing minutes
    /// Examples:
    /// - "5" -> Every 5 minutes
    /// - "1" -> Every 1 minute
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();

        if s.is_empty() {
            return None;
        }

        let minutes: u32 = s.parse().ok()?;
        if minutes < 1 || minutes > MAX_CRON_INTERVAL_MINS {
            return None;
        }

        Some(Schedule { minutes })
    }

    /// Calculate next run time from now (in seconds)
    pub fn next_run_seconds(&self) -> i64 {
        (self.minutes as i64) * 60
    }

    /// Get interval until next run
    #[allow(dead_code)]
    pub fn interval_until_next(&self) -> Duration {
        Duration::from_secs(self.next_run_seconds().max(60) as u64)
    }

    /// Get human readable description
    pub fn description(&self) -> String {
        if self.minutes == 1 {
            "Every 1 minute".to_string()
        } else {
            format!("Every {} minutes", self.minutes)
        }
    }
}

/// Maximum backoff interval in seconds (6 hours)
const MAX_BACKOFF_SECS: i64 = 6 * 60 * 60;

/// Profile cron entry
#[derive(Debug)]
pub struct ProfileCronJob {
    pub profile_id: String,
    /// Resolved profile file path (relative filename, e.g. "302db1eb.yaml")
    pub file: Option<String>,
    pub url: Option<String>,
    pub schedule: Schedule,
    pub last_run: Option<chrono::DateTime<chrono::Local>>,
    /// Next scheduled run timestamp
    next_run: i64,
    /// Number of consecutive failures since last success
    failure_count: u32,
    /// Last error message from subscription fetch
    pub last_error: Option<String>,
    /// Download options (user_agent, timeout, etc.)
    pub options: Option<control_tower_service_core::ProfileDownloadOptions>,
}

impl ProfileCronJob {
    pub fn new(profile_id: String, file: Option<String>, url: Option<String>, schedule: Schedule, options: Option<control_tower_service_core::ProfileDownloadOptions>) -> Self {
        let next_run = chrono::Utc::now().timestamp() + schedule.next_run_seconds();
        Self {
            profile_id,
            file,
            url,
            schedule,
            last_run: None,
            next_run,
            failure_count: 0,
            last_error: None,
            options,
        }
    }

    /// Called when a scheduled run fails. Applies exponential backoff and records the error.
    fn mark_failure(&mut self, error: String) {
        self.last_error = Some(error);
        self.failure_count += 1;
        let base = self.schedule.next_run_seconds();
        let backoff = base * (2_i64.pow(self.failure_count.min(20) as u32));
        self.next_run = chrono::Utc::now().timestamp() + backoff.min(MAX_BACKOFF_SECS);
    }

    /// Called when a scheduled run succeeds. Resets failure state.
    fn mark_success(&mut self) {
        self.last_error = None;
        self.failure_count = 0;
        self.last_run = Some(chrono::Local::now());
        self.next_run = chrono::Utc::now().timestamp() + self.schedule.next_run_seconds();
    }

    /// Check if job should run now.
    /// Note: caller must call mark_success() or mark_failure() after the update attempt
    /// to properly update last_run / next_run / failure_count.
    pub fn is_due(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        now >= self.next_run
    }

    #[allow(dead_code)]
    /// Legacy alias for is_due — kept to avoid breaking callers.
    pub fn check_and_update(&mut self) -> bool {
        self.is_due()
    }
}

/// Reload cron jobs from profiles.yaml content string
#[allow(dead_code)]
pub fn reload_cron_jobs_from_yaml(content: &str) {
    #[derive(serde::Deserialize)]
    struct ProfilesYaml {
        items: Vec<ProfileItem>,
    }
    #[derive(serde::Deserialize)]
    struct ProfileItem {
        uid: String,
        url: Option<String>,
        cron: Option<String>,
    }

    if let Ok(yaml) = serde_yaml_ng::from_str::<ProfilesYaml>(content) {
        for item in yaml.items {
            if let Some(cron_str) = item.cron {
                if let Some(schedule) = Schedule::parse(&cron_str) {
                    tracing::info!("Cron job [{}]: {} — {}", item.uid, cron_str, schedule.description());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minutes() {
        let s = Schedule::parse("5").unwrap();
        assert_eq!(s.minutes, 5);

        let s = Schedule::parse("1").unwrap();
        assert_eq!(s.minutes, 1);
    }

    #[test]
    fn test_next_run_seconds() {
        let s = Schedule { minutes: 5 };
        assert_eq!(s.next_run_seconds(), 300);
    }

    #[test]
    fn test_description() {
        let s = Schedule { minutes: 5 };
        assert_eq!(s.description(), "Every 5 minutes");

        let s = Schedule { minutes: 1 };
        assert_eq!(s.description(), "Every 1 minute");
    }
}

impl ServiceState {
    /// Load cron jobs from profiles.yaml
    pub fn load_cron_jobs(&self) {
        let mut jobs = self.cron_jobs.write();
        let exe_dir = control_tower_service_core::exe_dir();
        let profiles_path = exe_dir.join("profiles.yaml");

        if !profiles_path.exists() {
            tracing::info!("No profiles.yaml found, no cron jobs to load");
            return;
        }

        let content = match std::fs::read_to_string(&profiles_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read profiles.yaml: {}", e);
                return;
            }
        };

        #[derive(serde::Deserialize)]
        struct ProfilesYaml {
            items: Vec<ProfileItem>,
        }

        #[derive(serde::Deserialize)]
        struct ProfileItem {
            uid: String,
            file: Option<String>,
            url: Option<String>,
            cron: Option<String>,
            #[serde(rename = "options", default)]
            options: Option<control_tower_service_core::ProfileDownloadOptions>,
        }

        let yaml: ProfilesYaml = match serde_yaml_ng::from_str(&content) {
            Ok(y) => y,
            Err(e) => {
                tracing::warn!("Failed to parse profiles.yaml: {}", e);
                return;
            }
        };

        jobs.clear();

        for item in yaml.items {
            if let Some(cron_str) = item.cron {
                if let Some(schedule) = Schedule::parse(&cron_str) {
                    let job = ProfileCronJob::new(item.uid.clone(), item.file.clone(), item.url.clone(), schedule.clone(), item.options);
                    tracing::info!("Loaded cron job: {} - {}", item.uid, schedule.description());
                    jobs.push(job);
                } else {
                    tracing::warn!("Invalid cron expression for profile {}: {}", item.uid, cron_str);
                }
            }
        }

        tracing::info!("Loaded {} cron jobs", jobs.len());
    }

    /// Check and run due cron jobs
    pub fn check_and_run_crons(&self) {
        // Collect due jobs first to avoid holding the write lock during HTTP requests.
        let due_jobs: Vec<(usize, String, Option<String>, String, Option<control_tower_service_core::ProfileDownloadOptions>)> = {
            let mut jobs = self.cron_jobs.write();
            let exe_dir = control_tower_service_core::exe_dir();
            let profiles_dir = exe_dir.join("profiles");

            let due: Vec<_> = jobs.iter_mut().enumerate().filter(|(_, j)| j.is_due()).collect();

            due.into_iter().map(|(idx, job)| {
                let profile_id = job.profile_id.clone();
                let url = job.url.clone();
                let options = job.options.clone();
                let schedule_desc = job.schedule.description();
                tracing::info!("Triggering scheduled profile update: {} ({})", profile_id, schedule_desc);

                // Resolve profile file path: try job.file first, then fallback to uid[..8].yaml
                let profile_file = if let Some(f) = job.file.as_ref() {
                    let p = profiles_dir.join(f);
                    if p.exists() {
                        p
                    } else if profile_id.len() > 8 {
                        profiles_dir.join(format!("{}.yaml", &profile_id[..8]))
                    } else {
                        p
                    }
                } else if profile_id.len() > 8 {
                    profiles_dir.join(format!("{}.yaml", &profile_id[..8]))
                } else {
                    profiles_dir.join(format!("{}.yaml", profile_id))
                };
                (idx, profile_id, url, profile_file.to_string_lossy().into_owned(), options)
            }).collect()
        };

        // Execute HTTP requests outside the lock.
        let results: Vec<(usize, Result<(), String>)> = due_jobs
            .into_iter()
            .map(|(idx, _profile_id, url, profile_file, options)| {
                let result = if let Some(url) = url {
                    let path = std::path::PathBuf::from(&profile_file);
                    update_profile_subscription(&url, &path, options.as_ref())
                } else {
                    Err("No URL configured".to_string())
                };
                (idx, result)
            })
            .collect();

        // Update job states under a single write lock.
        let mut jobs = self.cron_jobs.write();
        for (idx, result) in results {
            if let Some(job) = jobs.get_mut(idx) {
                match result {
                    Ok(()) => {
                        tracing::info!("Profile {} updated successfully", job.profile_id);
                        job.mark_success();
                    }
                    Err(e) => {
                        tracing::error!("Failed to update profile {}: {}", job.profile_id, e);
                        job.mark_failure(e);
                    }
                }
            }
        }
    }
}

impl ServiceState {
    /// Check and update ALL profiles with subscription URLs (used for startup auto-update).
    /// Returns a list of (profile_uid, success) for each profile that was checked.
    pub fn check_and_update_all_profiles(&self) -> Vec<(String, bool)> {
        check_and_update_all_profiles()
    }
}

/// Check and update ALL profiles with subscription URLs (used for startup auto-update).
/// Returns a list of (profile_uid, success) for each profile that was updated.
pub fn check_and_update_all_profiles() -> Vec<(String, bool)> {
    let exe_dir = control_tower_service_core::exe_dir();
    let profiles_path = exe_dir.join("profiles.yaml");
    let profiles_dir = exe_dir.join("profiles");

    if !profiles_path.exists() {
        tracing::info!("No profiles.yaml found, skipping startup profile check");
        return vec![];
    }

    let content = match std::fs::read_to_string(&profiles_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to read profiles.yaml for startup check: {}", e);
            return vec![];
        }
    };

    #[derive(serde::Deserialize)]
    struct ProfilesYaml {
        items: Vec<ProfileItem>,
    }

    #[derive(serde::Deserialize)]
    struct ProfileItem {
        uid: String,
        file: Option<String>,
        url: Option<String>,
        #[serde(rename = "options", default)]
        options: Option<control_tower_service_core::ProfileDownloadOptions>,
    }

    let yaml: ProfilesYaml = match serde_yaml_ng::from_str(&content) {
        Ok(y) => y,
        Err(e) => {
            tracing::warn!("Failed to parse profiles.yaml for startup check: {}", e);
            return vec![];
        }
    };

    let mut results = vec![];

    for item in yaml.items {
        let url = match &item.url {
            Some(u) if !u.is_empty() => u.clone(),
            _ => {
                // No URL, skip
                results.push((item.uid, false));
                continue;
            }
        };

        // Resolve profile file path
        let profile_file = if let Some(f) = item.file.as_ref() {
            let p = profiles_dir.join(f);
            if p.exists() || !f.is_empty() {
                p
            } else if item.uid.len() > 8 {
                profiles_dir.join(format!("{}.yaml", &item.uid[..8]))
            } else {
                profiles_dir.join(format!("{}.yaml", item.uid))
            }
        } else if item.uid.len() > 8 {
            profiles_dir.join(format!("{}.yaml", &item.uid[..8]))
        } else {
            profiles_dir.join(format!("{}.yaml", item.uid))
        };

        tracing::info!("Checking profile {} ({}) for updates", item.uid, url);

        let path = std::path::PathBuf::from(&profile_file);
        match update_profile_subscription(&url, &path, item.options.as_ref()) {
            Ok(()) => {
                tracing::info!("Profile {} updated successfully", item.uid);
                results.push((item.uid, true));
            }
            Err(e) => {
                tracing::warn!("Failed to update profile {}: {}", item.uid, e);
                results.push((item.uid, false));
            }
        }
    }

    results
}

/// Update a profile subscription (download new content and write to file).
/// Used by cron job auto-update.
pub(crate) fn update_profile_subscription(url: &str, profile_file: &std::path::Path, options: Option<&control_tower_service_core::ProfileDownloadOptions>) -> Result<(), String> {
    use reqwest::blocking::Client as BlockingClient;
    use std::time::Duration as StdDuration;

    let timeout_secs = options
        .and_then(|o| o.timeout_seconds)
        .unwrap_or(60);
    let user_agent = options
        .and_then(|o| o.user_agent.clone())
        .unwrap_or_else(|| "clash-verge/v2.4.7".to_string());

    let response = BlockingClient::new()
        .get(url)
        .header("User-Agent", user_agent)
        .timeout(StdDuration::from_secs(timeout_secs))
        .send()
        .map_err(|e| format!("Failed to fetch subscription: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Subscription fetch failed: {}", response.status()));
    }

    let new_content = response.text()
        .map_err(|e| format!("Failed to read subscription content: {}", e))?;

    // Basic validation: reject HTML responses (common for errors / CAPTCHAs)
    let trimmed = new_content.trim_start();
    if trimmed.starts_with('<') || trimmed.starts_with("<!") {
        return Err("Subscription returned HTML — likely a block page".to_string());
    }

    // Basic YAML structure check
    if !new_content.contains("proxies:")
       && !new_content.contains("proxy-providers:")
       && !new_content.contains("mixed-port:")
    {
        return Err("Subscription content does not look like a Clash config".to_string());
    }

    std::fs::write(profile_file, &new_content)
        .map_err(|e| format!("Failed to write profile file: {}", e))?;

    Ok(())
}
