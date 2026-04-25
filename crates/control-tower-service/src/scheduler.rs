//! Cron scheduler for profile auto-update

use std::time::Duration;

use crate::ServiceState;

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
        if minutes < 1 || minutes > 10080 {
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
}

impl ProfileCronJob {
    pub fn new(profile_id: String, file: Option<String>, url: Option<String>, schedule: Schedule) -> Self {
        let next_run = chrono::Utc::now().timestamp() + schedule.next_run_seconds();
        Self {
            profile_id,
            file,
            url,
            schedule,
            last_run: None,
            next_run,
        }
    }

    /// Check if job should run now and update last_run if so
    pub fn check_and_update(&mut self) -> bool {
        let now = chrono::Utc::now().timestamp();

        if now >= self.next_run {
            self.last_run = Some(chrono::Local::now());
            self.next_run = now + self.schedule.next_run_seconds();
            true
        } else {
            false
        }
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
                    let job = ProfileCronJob::new(item.uid.clone(), item.file.clone(), item.url.clone(), schedule.clone());
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
        let mut jobs = self.cron_jobs.write();

        for job in jobs.iter_mut() {
            if job.check_and_update() {
                let profile_id = job.profile_id.clone();
                let url = job.url.clone();
                let schedule_desc = job.schedule.description();

                tracing::info!("Triggering scheduled profile update: {} ({})", profile_id, schedule_desc);

                let exe_dir = control_tower_service_core::exe_dir();
                let profiles_dir = exe_dir.join("profiles");
                let profile_file = match job.file.as_ref() {
                    Some(f) => profiles_dir.join(f),
                    None => profiles_dir.join(format!("{}.yaml", profile_id)),
                };

                if let Some(url) = url {
                    if profile_file.exists() {
                        std::thread::spawn(move || {
                            if let Err(e) = update_profile_subscription(&url, &profile_file) {
                                tracing::error!("Failed to update profile {}: {}", profile_id, e);
                            } else {
                                tracing::info!("Profile {} updated successfully", profile_id);
                            }
                        });
                    } else {
                        tracing::warn!("Profile file not found: {:?}", profile_file);
                    }
                } else {
                    tracing::warn!("No URL configured for profile: {}", profile_id);
                }
            }
        }
    }
}

/// Update a profile subscription (download new content and write to file).
/// Used by cron job auto-update.
pub(crate) fn update_profile_subscription(url: &str, profile_file: &std::path::Path) -> Result<(), String> {
    use reqwest::blocking::Client as BlockingClient;
    use std::time::Duration as StdDuration;

    let response = BlockingClient::new()
        .get(url)
        .timeout(StdDuration::from_secs(60))
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
