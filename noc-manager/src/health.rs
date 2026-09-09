//! Heartbeats sent periodically by NOC Agent and NOC Display, and their
//! rendering as Prometheus metrics (`GET /metrics`) for Grafana.
//!
//! Purely in-memory: a restart of NOC Manager loses history, which is fine
//! for a liveness signal — each instance re-announces itself within its own
//! heartbeat interval.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

pub const APPS: [&str; 2] = ["agent", "display"];

#[derive(Debug, Clone)]
pub struct Heartbeat {
    pub version: String,
    pub build: String,
    pub state: String,
    pub last_seen: u64,
}

#[derive(Debug, Default, Deserialize)]
pub struct HeartbeatBody {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub build: String,
    #[serde(default)]
    pub state: String,
}

pub struct HealthStore {
    inner: Mutex<HashMap<(String, String), Heartbeat>>,
}

impl HealthStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn record(&self, app: &str, username: &str, body: HeartbeatBody) {
        let heartbeat = Heartbeat {
            version: body.version,
            build: body.build,
            state: body.state,
            last_seen: now_unix(),
        };
        self.inner
            .lock()
            .unwrap()
            .insert((app.to_string(), username.to_string()), heartbeat);
    }

    /// Latest heartbeat for one (app, username) pair, if any was ever received.
    pub fn get(&self, app: &str, username: &str) -> Option<Heartbeat> {
        self.inner
            .lock()
            .unwrap()
            .get(&(app.to_string(), username.to_string()))
            .cloned()
    }

    fn snapshot(&self) -> Vec<((String, String), Heartbeat)> {
        let mut entries: Vec<_> = self
            .inner
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }
}

impl Default for HealthStore {
    fn default() -> Self {
        Self::new()
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `true` when a heartbeat was received recently enough to be considered alive.
pub fn is_up(heartbeat: &Heartbeat, now: u64, stale_after_seconds: u64) -> bool {
    now.saturating_sub(heartbeat.last_seen) <= stale_after_seconds
}

/// Prometheus text exposition format (version 0.0.4), consumable directly by
/// a Prometheus scrape job and from there by Grafana.
pub fn render_prometheus(store: &HealthStore, stale_after_seconds: u64) -> String {
    let now = now_unix();
    let mut out = String::new();

    out.push_str("# HELP noc_up 1 if a heartbeat arrived within the staleness window, 0 otherwise.\n");
    out.push_str("# TYPE noc_up gauge\n");
    for ((app, username), heartbeat) in store.snapshot() {
        let up = if is_up(&heartbeat, now, stale_after_seconds) { 1 } else { 0 };
        out.push_str(&format!(
            "noc_up{{app=\"{}\",username=\"{}\"}} {up}\n",
            escape(&app),
            escape(&username)
        ));
    }

    out.push_str("# HELP noc_last_seen_seconds Unix timestamp of the last heartbeat received.\n");
    out.push_str("# TYPE noc_last_seen_seconds gauge\n");
    for ((app, username), heartbeat) in store.snapshot() {
        out.push_str(&format!(
            "noc_last_seen_seconds{{app=\"{}\",username=\"{}\"}} {}\n",
            escape(&app),
            escape(&username),
            heartbeat.last_seen
        ));
    }

    out.push_str("# HELP noc_info Build metadata of the last heartbeat received. Value is always 1.\n");
    out.push_str("# TYPE noc_info gauge\n");
    for ((app, username), heartbeat) in store.snapshot() {
        out.push_str(&format!(
            "noc_info{{app=\"{}\",username=\"{}\",version=\"{}\",build=\"{}\",state=\"{}\"}} 1\n",
            escape(&app),
            escape(&username),
            escape(&heartbeat.version),
            escape(&heartbeat.build),
            escape(&heartbeat.state)
        ));
    }

    out
}

pub fn is_allowed_app(app: &str) -> bool {
    APPS.contains(&app)
}

/// Escapes a Prometheus label value: backslash, double quote and newline.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}
