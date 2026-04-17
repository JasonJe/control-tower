//! Cron scheduler for profile auto-update

use std::time::Duration;

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
    pub url: Option<String>,
    pub schedule: Schedule,
    pub last_run: Option<chrono::DateTime<chrono::Local>>,
    /// Next scheduled run timestamp
    next_run: i64,
}

impl ProfileCronJob {
    pub fn new(profile_id: String, url: Option<String>, schedule: Schedule) -> Self {
        let next_run = chrono::Utc::now().timestamp() + schedule.next_run_seconds();
        Self {
            profile_id,
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
