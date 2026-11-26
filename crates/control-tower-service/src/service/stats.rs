//! Persistent traffic statistics.
//!
//! Totals used to live only in the in-memory connection list, so every restart
//! reset the "today" numbers and there was no way to see long-term usage per
//! proxy. Stats are accumulated as connections close and written to
//! `stats.yaml` at most once every few seconds.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Keep roughly three months of daily buckets.
const MAX_DAYS: usize = 90;
/// Cap destination cardinality so a scan cannot grow the file forever.
const MAX_DESTINATIONS: usize = 1000;
/// Minimum seconds between implicit flushes on the connection path.
const FLUSH_INTERVAL_SECS: u64 = 10;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Totals {
    pub upload: u64,
    pub download: u64,
    pub connections: u64,
}

impl Totals {
    fn add(&mut self, upload: u64, download: u64) {
        self.upload = self.upload.saturating_add(upload);
        self.download = self.download.saturating_add(download);
        self.connections = self.connections.saturating_add(1);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StatsData {
    pub total: Totals,
    pub daily: BTreeMap<String, Totals>,
    pub proxies: BTreeMap<String, Totals>,
    pub destinations: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DailyEntry {
    pub date: String,
    pub upload: u64,
    pub download: u64,
    pub connections: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NamedTotals {
    pub name: String,
    pub upload: u64,
    pub download: u64,
    pub connections: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DestinationEntry {
    pub host: String,
    pub connections: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsView {
    pub total: Totals,
    pub today: Totals,
    pub daily: Vec<DailyEntry>,
    pub proxies: Vec<NamedTotals>,
    pub destinations: Vec<DestinationEntry>,
}

fn cache() -> &'static RwLock<StatsData> {
    static CACHE: OnceLock<RwLock<StatsData>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(load_from_disk()))
}

fn dirty() -> &'static AtomicBool {
    static DIRTY: AtomicBool = AtomicBool::new(false);
    &DIRTY
}

fn last_flush() -> &'static AtomicU64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    &LAST
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn stats_path() -> std::path::PathBuf {
    crate::settings::resolve_paths().config_dir.join("stats.yaml")
}

fn load_from_disk() -> StatsData {
    let path = stats_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|content| serde_yaml_ng::from_str(&content).ok())
        .unwrap_or_default()
}

/// Record one closed connection. Writes to disk at most every
/// [`FLUSH_INTERVAL_SECS`] seconds; call [`flush`] at shutdown for the tail.
pub fn record(chains: &[String], destination: &str, upload: u64, download: u64) {
    let proxy = chains
        .last()
        .map(String::as_str)
        .unwrap_or("DIRECT")
        .to_string();
    let host = destination_host(destination);
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    {
        let mut stats = cache().write();
        stats.total.add(upload, download);
        stats
            .daily
            .entry(today)
            .or_default()
            .add(upload, download);
        stats
            .proxies
            .entry(proxy)
            .or_default()
            .add(upload, download);
        let counter = stats.destinations.entry(host).or_insert(0);
        *counter = counter.saturating_add(1);
    }
    dirty().store(true, Ordering::Relaxed);

    let now = unix_now();
    let last = last_flush().load(Ordering::Relaxed);
    if now.saturating_sub(last) >= FLUSH_INTERVAL_SECS {
        flush();
    }
}

/// Persist accumulated stats if anything changed.
pub fn flush() {
    if !dirty().swap(false, Ordering::SeqCst) {
        last_flush().store(unix_now(), Ordering::Relaxed);
        return;
    }
    let snapshot = {
        let mut stats = cache().write();
        prune(&mut stats);
        stats.clone()
    };
    let path = stats_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_yaml_ng::to_string(&snapshot) {
        Ok(yaml) => match control_tower_service_core::write_private_atomic(&path, &yaml) {
            Ok(()) => last_flush().store(unix_now(), Ordering::Relaxed),
            Err(e) => {
                tracing::warn!("Failed to write stats.yaml: {}", e);
                dirty().store(true, Ordering::Relaxed);
            }
        },
        Err(e) => tracing::warn!("Failed to serialize stats: {}", e),
    }
}

fn prune(stats: &mut StatsData) {
    while stats.daily.len() > MAX_DAYS {
        // BTreeMap is date-ordered, so the first key is the oldest day.
        let oldest = stats.daily.keys().next().cloned();
        match oldest {
            Some(key) => {
                stats.daily.remove(&key);
            }
            None => break,
        }
    }
    if stats.destinations.len() > MAX_DESTINATIONS {
        let mut counts: Vec<(String, u64)> =
            stats.destinations.iter().map(|(k, v)| (k.clone(), *v)).collect();
        counts.sort_by(|a, b| b.1.cmp(&a.1));
        counts.truncate(MAX_DESTINATIONS);
        stats.destinations = counts.into_iter().collect();
    }
}

/// Current stats for the API, newest day first and traffic-heavy entries first.
pub fn view() -> StatsView {
    let stats = cache().read().clone();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    let mut daily: Vec<DailyEntry> = stats
        .daily
        .iter()
        .map(|(date, totals)| DailyEntry {
            date: date.clone(),
            upload: totals.upload,
            download: totals.download,
            connections: totals.connections,
        })
        .collect();
    daily.sort_by(|a, b| b.date.cmp(&a.date));

    let mut proxies: Vec<NamedTotals> = stats
        .proxies
        .iter()
        .map(|(name, totals)| NamedTotals {
            name: name.clone(),
            upload: totals.upload,
            download: totals.download,
            connections: totals.connections,
        })
        .collect();
    proxies.sort_by(|a, b| (b.upload + b.download).cmp(&(a.upload + a.download)));

    let mut destinations: Vec<DestinationEntry> = stats
        .destinations
        .iter()
        .map(|(host, connections)| DestinationEntry {
            host: host.clone(),
            connections: *connections,
        })
        .collect();
    destinations.sort_by(|a, b| b.connections.cmp(&a.connections));
    destinations.truncate(100);

    StatsView {
        total: stats.total.clone(),
        today: stats.daily.get(&today).cloned().unwrap_or_default(),
        daily,
        proxies,
        destinations,
    }
}

/// Clear all stats, both in memory and on disk.
pub fn reset() -> Result<(), String> {
    *cache().write() = StatsData::default();
    dirty().store(false, Ordering::SeqCst);
    let path = stats_path();
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("Failed to remove {}: {}", path.display(), e))?;
    }
    Ok(())
}

/// Aggregate destinations by host: strips a trailing numeric port.
fn destination_host(destination: &str) -> String {
    let dest = destination.trim();
    if let Some(rest) = dest.strip_prefix('[') {
        if let Some((host, _)) = rest.split_once(']') {
            return host.to_string();
        }
    }
    if dest.parse::<std::net::IpAddr>().is_ok() {
        return dest.to_string();
    }
    if let Some((host, port)) = dest.rsplit_once(':') {
        if !host.contains(':') && port.parse::<u16>().is_ok() {
            return host.to_string();
        }
    }
    dest.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_accumulate() {
        let mut t = Totals::default();
        t.add(10, 20);
        t.add(1, 2);
        assert_eq!(t.upload, 11);
        assert_eq!(t.download, 22);
        assert_eq!(t.connections, 2);
    }

    #[test]
    fn destination_host_strips_ports() {
        assert_eq!(destination_host("example.com:443"), "example.com");
        assert_eq!(destination_host("1.1.1.1:443"), "1.1.1.1");
        assert_eq!(destination_host("2001:db8::1"), "2001:db8::1");
        assert_eq!(destination_host("[2001:db8::1]:443"), "2001:db8::1");
    }

    #[test]
    fn prune_keeps_newest_days() {
        let mut stats = StatsData::default();
        for i in 0..(MAX_DAYS + 10) {
            let date = format!("2025-{:02}-{:02}", 1 + i / 28, 1 + i % 28);
            stats.daily.insert(date, Totals::default());
        }
        assert!(stats.daily.len() > MAX_DAYS);
        prune(&mut stats);
        assert_eq!(stats.daily.len(), MAX_DAYS);
        assert!(!stats.daily.contains_key("2025-01-01"));
    }
}
