//! Heartbeats sent periodically by NOC Agent and NOC Display, and their
//! rendering as Prometheus metrics (`GET /metrics`) for Grafana.
//!
//! Live inventory in memory, with an independent, bounded SQLite event history.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub const APPS: [&str; 2] = ["agent", "display"];

#[derive(Debug, Clone, Serialize)]
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
    database: Mutex<Option<Connection>>,
    warning: Mutex<Option<String>>,
    retention_days: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub id: i64,
    pub app: String,
    pub username: String,
    #[serde(flatten)]
    pub heartbeat: Heartbeat,
}

#[derive(Default, Debug, Clone, Deserialize)]
pub struct HistoryQuery {
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub state: String,
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub before: Option<i64>,
}

#[derive(Serialize)]
pub struct HistoryPage {
    pub events: Vec<Event>,
    pub next: Option<i64>,
}

impl HealthStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            database: Mutex::new(None),
            warning: Mutex::new(None),
            retention_days: 30,
        }
    }

    pub fn load(path: &Path, retention_days: u64) -> Self {
        let mut store = Self::new();
        store.retention_days = retention_days.clamp(1, 365);
        let result = (|| -> Result<Connection, rusqlite::Error> {
            let db = Connection::open(path)?;
            db.busy_timeout(std::time::Duration::from_secs(3))?;
            db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
                CREATE TABLE IF NOT EXISTS events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT, app TEXT NOT NULL, username TEXT NOT NULL,
                    version TEXT NOT NULL, build TEXT NOT NULL, state TEXT NOT NULL, last_seen INTEGER NOT NULL);
                CREATE INDEX IF NOT EXISTS events_client ON events(app, username, id);
                CREATE INDEX IF NOT EXISTS events_time ON events(last_seen);
                CREATE INDEX IF NOT EXISTS events_app ON events(app, id);
                CREATE INDEX IF NOT EXISTS events_user ON events(username, id);
                CREATE INDEX IF NOT EXISTS events_state ON events(state, id);
                CREATE TABLE IF NOT EXISTS latest (
                    app TEXT NOT NULL, username TEXT NOT NULL, version TEXT NOT NULL,
                    build TEXT NOT NULL, state TEXT NOT NULL, last_seen INTEGER NOT NULL,
                    PRIMARY KEY(app, username));")?;
            {
                let mut stmt = db.prepare(
                    "SELECT app, username, version, build, state, last_seen FROM latest",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok((
                        (r.get(0)?, r.get(1)?),
                        Heartbeat {
                            version: r.get(2)?,
                            build: r.get(3)?,
                            state: r.get(4)?,
                            last_seen: r.get(5)?,
                        },
                    ))
                })?;
                let mut live = store.inner.lock().unwrap();
                for row in rows {
                    let (key, value) = row?;
                    live.insert(key, value);
                }
            }
            Ok(db)
        })();
        match result {
            Ok(db) => *store.database.lock().unwrap() = Some(db),
            Err(e) => store.report_error(&e.to_string()),
        }
        store.purge(now_unix());
        store
    }

    fn report_error(&self, error: &str) {
        eprintln!("[NOC Manager] health history: {error}");
        // Sticky: successful subsequent writes cannot repair a missing event.
        *self.warning.lock().unwrap() = Some("Historique incomplet : une erreur de stockage est survenue. Consultez les journaux du Manager.".into());
    }

    pub fn warning(&self) -> Option<String> {
        self.warning.lock().unwrap().clone()
    }

    pub fn retention_days(&self) -> u64 {
        self.retention_days
    }

    pub fn record(&self, app: &str, username: &str, body: HeartbeatBody) {
        let heartbeat = Heartbeat {
            version: body.version,
            build: body.build,
            state: body.state,
            last_seen: now_unix(),
        };
        // Serialize writes to preserve reception order across live and persistent views.
        let mut database = self.database.lock().unwrap();
        self.inner
            .lock()
            .unwrap()
            .insert((app.to_string(), username.to_string()), heartbeat.clone());
        if let Some(db) = database.as_mut() {
            let result = (|| -> rusqlite::Result<()> {
                let tx = db.transaction()?;
                let values = params![
                    app,
                    username,
                    heartbeat.version,
                    heartbeat.build,
                    heartbeat.state,
                    heartbeat.last_seen
                ];
                tx.execute("INSERT INTO events(app,username,version,build,state,last_seen) VALUES (?1,?2,?3,?4,?5,?6)", values)?;
                tx.execute("INSERT INTO latest(app,username,version,build,state,last_seen) VALUES (?1,?2,?3,?4,?5,?6)
                    ON CONFLICT(app,username) DO UPDATE SET version=excluded.version,build=excluded.build,state=excluded.state,last_seen=excluded.last_seen", values)?;
                tx.commit()
            })();
            if let Err(e) = result {
                self.report_error(&e.to_string());
            }
        }
    }

    /// Latest heartbeat for one (app, username) pair, if any was ever received.
    pub fn get(&self, app: &str, username: &str) -> Option<Heartbeat> {
        self.inner
            .lock()
            .unwrap()
            .get(&(app.to_string(), username.to_string()))
            .cloned()
    }

    pub fn snapshot(&self) -> Vec<((String, String), Heartbeat)> {
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

    pub fn purge(&self, now: u64) {
        let db = self.database.lock().unwrap();
        if let Some(db) = db.as_ref() {
            if let Err(e) = db.execute(
                "DELETE FROM events WHERE last_seen < ?1",
                [now.saturating_sub(self.retention_days * 86400)],
            ) {
                self.report_error(&e.to_string());
            }
        }
    }

    pub fn history(&self, query: &HistoryQuery) -> Result<HistoryPage, String> {
        let database = self.database.lock().unwrap();
        let db = database
            .as_ref()
            .ok_or("Historique indisponible. Vérifiez le stockage puis redémarrez Manager.")?;
        let mut sql = String::from(
            "SELECT id,app,username,version,build,state,last_seen FROM events WHERE last_seen >= ?",
        );
        let mut values = vec![rusqlite::types::Value::Integer(
            now_unix().saturating_sub(self.retention_days * 86400) as i64,
        )];
        for (column, value) in [
            ("app", &query.app),
            ("username", &query.username),
            ("state", &query.state),
        ] {
            if !value.is_empty() {
                sql.push_str(&format!(" AND {column} = ?"));
                values.push(value.clone().into());
            }
        }
        for (condition, value) in [
            (
                "last_seen >=",
                query.from.map(|v| v.min(i64::MAX as u64) as i64),
            ),
            (
                "last_seen <=",
                query.to.map(|v| v.min(i64::MAX as u64) as i64),
            ),
            ("id <", query.before),
        ] {
            if let Some(value) = value {
                sql.push_str(&format!(" AND {condition} ?"));
                values.push(value.into());
            }
        }
        sql.push_str(" ORDER BY id DESC LIMIT 101");
        let result = (|| -> rusqlite::Result<HistoryPage> {
            let mut stmt = db.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(values), |r| {
                Ok(Event {
                    id: r.get(0)?,
                    app: r.get(1)?,
                    username: r.get(2)?,
                    heartbeat: Heartbeat {
                        version: r.get(3)?,
                        build: r.get(4)?,
                        state: r.get(5)?,
                        last_seen: r.get(6)?,
                    },
                })
            })?;
            let mut events = rows.collect::<rusqlite::Result<Vec<_>>>()?;
            let next = if events.len() > 100 {
                events.truncate(100);
                events.last().map(|e| e.id)
            } else {
                None
            };
            Ok(HistoryPage { events, next })
        })();
        result.map_err(|e| {
            self.report_error(&e.to_string());
            "Lecture de l’historique impossible.".into()
        })
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

    out.push_str(
        "# HELP noc_up 1 if a heartbeat arrived within the staleness window, 0 otherwise.\n",
    );
    out.push_str("# TYPE noc_up gauge\n");
    for ((app, username), heartbeat) in store.snapshot() {
        let up = if is_up(&heartbeat, now, stale_after_seconds) {
            1
        } else {
            0
        };
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

    out.push_str(
        "# HELP noc_info Build metadata of the last heartbeat received. Value is always 1.\n",
    );
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
