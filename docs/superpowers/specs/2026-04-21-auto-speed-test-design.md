# Auto Speed Test Design

## Overview

Add backend scheduled latency testing of all proxy nodes, store results, and display the fastest node on Dashboard and Proxies pages. Users can enable/configure the schedule in Settings.

## Goals

- Backend runs latency tests on a configurable interval (5/15/30/60 min)
- Results stored in ServiceState; exposed via new API endpoint
- Dashboard shows current fastest node with latency and last-test time
- Proxies list highlights the fastest node; latency column shows scheduled-test results
- Manual test can override a single node's result; fastest marker persists until next scheduled test

## Out of Scope

- Auto-switch GLOBAL to fastest node (future work)
- Historical trending / charts

## Configuration

Extend `SettingsData`:
```rust
auto_test_enabled: bool,      // default false
auto_test_interval: u32,      // minutes: 5, 15, 30, 60 — default 15
```

Stored in `settings.yaml` under `latency_test_mode` / `auto_test_enabled` / `auto_test_interval`.

## Data Structures

```rust
struct LatencyResult {
    name: String,
    latency: Option<i64>,   // None = error / N/A
    error: Option<String>,
}

struct AutoTestState {
    enabled: bool,
    interval_secs: u64,
    last_test_at: Option<i64>,
    fastest: Option<LatencyResult>,
    results: Vec<LatencyResult>,
}
```

## API Endpoints

### GET /api/proxies/fastest

Returns current auto-test state and fastest node.

Response:
```json
{
  "enabled": true,
  "interval_minutes": 15,
  "last_test_at": 1745214725,
  "fastest": { "name": "vmess-hk-01", "latency": 127, "error": null },
  "results": [
    { "name": "vmess-hk-01", "latency": 127, "error": null },
    { "name": "vmess-sg-02", "latency": null, "error": "Not supported (HTTP 503)" }
  ]
}
```

### POST /api/proxies/delay-all

Existing endpoint already reads `latency_test_mode` from settings. No change needed — the scheduled job will call it directly.

## Backend Scheduling

Reuse the existing cron infrastructure in `main.rs` (`check_and_run_crons`). Add a new `auto_latency_test()` method on `ServiceState`:

1. Read `auto_test_enabled` and `auto_test_interval` from settings
2. If disabled, skip
3. Call the batch delay logic (reuse `proxy_delay_all` path but in-process, not over HTTP)
4. Store results + compute fastest into `AutoTestState`
5. Update `last_test_at`

The main loop already checks cron every 60s. The interval is tracked in `AutoTestState.interval_secs`.

## Frontend Changes

### Settings Page

Extend the existing Latency Test Mode block:
```
[ HTTP ▼ ]  [✓] Auto Test  Interval: [15 min ▼]
```

One row below with status:
```
Current: HTTP mode | Auto Test: On (15min) | Last: 10:32:05
```

### Dashboard

Replace/enhance the current proxy delay card:
```
┌─────────────────────────────┐
│ Current Fastest              │
│ vmess-hk-01                 │
│ 127ms ✓                     │
│ Last test: 10:32:05 (2min ago) │
└─────────────────────────────┘
```

### Proxies List

- Latency column: show scheduled-test result (or manual override)
- Fastest node row: green highlight (border or background tint)
- Top summary bar: `Fastest: vmess-hk-01 (127ms)`
- Manual "Test All" updates that node's result but does not change the fastest highlight until next scheduled run

## File Changes

| File | Change |
|------|--------|
| `settings.rs` | Add `auto_test_enabled`, `auto_test_interval` to `SettingsData` |
| `api.rs` | Add `GET /api/proxies/fastest` handler; add `proxy_fastest` struct |
| `main.rs` | Add `AutoTestState` to `ServiceState`; add `auto_latency_test()` method; call it from main loop |
| `http_server.rs` | Route `GET /api/proxies/fastest` |
| `html.rs` | Settings: add auto-test toggle + interval dropdown; Dashboard: add fastest card; Proxies: highlight + summary bar |

## Boundary Cases

### No test ever run
- `GET /api/proxies/fastest` returns `fastest: null`, `last_test_at: null`
- Dashboard card: shows "No test data" placeholder, no node name/latency
- Proxies list: no highlight, latency column shows "-" or "Not tested"

### All nodes failed (all N/A or timeout)
- `fastest: null` since no valid latency
- `last_test_at` is set (test did run, just all failed)
- Dashboard: shows "All nodes failed" or "No available nodes"
- Proxies list: all rows show "N/A", no highlight

### Empty proxy list (no nodes)
- Handle gracefully: `results: []`, `fastest: null`
- Dashboard: "No proxy nodes found"
- No error thrown

### Partial failure (some nodes fail, some succeed)
- `results` includes all nodes (failed ones have `latency: null, error: "..."`)
- `fastest` is the node with minimum non-null latency
- Proxies list: failed nodes show "N/A", fastest highlighted

### Mihomo not running
- `proxy_delay_all` returns 500 or times out
- Auto-test skips update: `last_test_at` stays unchanged
- No error shown to user until next manual test
- Log warning: "Auto latency test failed: Mihomo not responding"

### Network timeout during test
- Individual node timeout → `latency: null, error: "Timeout"`
- `fastest` computed from remaining valid results
- If ALL timeout → same as "all failed" case

## Testing

- Manual test via "Test All" button still works as before
- With auto-test enabled: verify `GET /api/proxies/fastest` returns after scheduled interval
- Verify fastest node highlight appears in proxies list
- Toggle off auto-test: verify no background requests fire
- No test run yet: verify placeholder state
- All nodes failed: verify "All nodes failed" state
