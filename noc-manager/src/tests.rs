use crate::{
    health::{HealthStore, Heartbeat, HeartbeatBody, HistoryQuery},
    models::{Kiosk, RdpConfig},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
fn db_path() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "noc-tests-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("health.sqlite3")
}
fn body(state: &str) -> HeartbeatBody {
    HeartbeatBody {
        version: "1.0".into(),
        build: "test".into(),
        state: state.into(),
    }
}
fn kiosk(agent: &str, display: Option<&str>) -> Kiosk {
    Kiosk {
        username: agent.into(),
        display_username: display.map(str::to_string),
        name: "Test".into(),
        url: "https://example.com".into(),
        enabled: true,
        restart_cron: None,
        rdp: RdpConfig::default(),
    }
}

#[test]
fn association_legacy_defaults_and_conflicts() {
    let legacy: Kiosk = serde_json::from_str(
        r#"{"username":"agent-a","name":"A","url":"https://example.com","enabled":true}"#,
    )
    .unwrap();
    assert_eq!(legacy.display_username(), "agent-a");
    let other = kiosk("agent-b", Some("display-b"));
    assert!(crate::models::validate(&other, &[legacy.clone()], None).is_ok());
    assert!(
        crate::models::validate(&kiosk("agent-b", Some("agent-a")), &[legacy.clone()], None)
            .is_err()
    );
    assert!(crate::models::validate(&legacy, &[legacy.clone()], Some("agent-a")).is_ok());
    assert!(crate::models::validate(&kiosk("agent-c", Some("display/b")), &[], None).is_err());
}

#[test]
fn ready_requires_both_recent_and_correct_states() {
    let mut k = kiosk("a", Some("d"));
    let mut a = Heartbeat {
        version: String::new(),
        build: String::new(),
        state: "RUNNING".into(),
        last_seen: 100,
    };
    let mut d = Heartbeat {
        state: "CONNECTED".into(),
        ..a.clone()
    };
    let status = |k: &Kiosk, a: Option<&Heartbeat>, d: Option<&Heartbeat>, now| {
        crate::dashboard::kiosk_status(k, a, d, now, 90)
    };
    assert_eq!(status(&k, Some(&a), Some(&d), 190), "ready");
    assert_eq!(status(&k, Some(&a), Some(&d), 191), "attention");
    assert_eq!(status(&k, Some(&a), None, 100), "attention");
    d.state = "RECONNECTING".into();
    assert_eq!(status(&k, Some(&a), Some(&d), 100), "attention");
    d.state = "CONNECTED".into();
    a.state = "TARGET_UNAVAILABLE".into();
    assert_eq!(status(&k, Some(&a), Some(&d), 100), "attention");
    k.enabled = false;
    assert_eq!(status(&k, Some(&a), Some(&d), 100), "disabled");
}

#[test]
fn history_preserves_each_event_filters_and_keyset_pagination() {
    let store = HealthStore::load(&db_path(), 30);
    for _ in 0..205 {
        store.record("agent", "unconfigured", body("RUNNING"));
    }
    store.record("display", "different-name", body("CONNECTED"));
    let query = HistoryQuery {
        app: "agent".into(),
        username: "unconfigured".into(),
        state: "RUNNING".into(),
        ..Default::default()
    };
    let first = store.history(&query).unwrap();
    assert_eq!(first.events.len(), 100);
    let second = store
        .history(&HistoryQuery {
            before: first.next,
            ..query.clone()
        })
        .unwrap();
    assert_eq!(second.events.len(), 100);
    let third = store
        .history(&HistoryQuery {
            before: second.next,
            ..query.clone()
        })
        .unwrap();
    assert_eq!(third.events.len(), 5);
    assert!(third.next.is_none());
    assert!(first.events.last().unwrap().id > second.events[0].id);
    assert_eq!(store.snapshot().len(), 2);
    assert!(store
        .history(&HistoryQuery {
            state: "ERROR".into(),
            ..query
        })
        .unwrap()
        .events
        .is_empty());
    assert!(store
        .history(&HistoryQuery {
            to: Some(1),
            ..Default::default()
        })
        .unwrap()
        .events
        .is_empty());
}

#[test]
fn restart_and_purge_keep_last_known_clients() {
    let path = db_path();
    {
        let s = HealthStore::load(&path, 30);
        s.record("display", "orphan", body("CONNECTED"));
    }
    let db = rusqlite::Connection::open(&path).unwrap();
    let old = crate::health::now_unix() - 31 * 86400;
    db.execute("UPDATE events SET last_seen=?1", [old]).unwrap();
    db.execute("UPDATE latest SET last_seen=?1", [old]).unwrap();
    drop(db);
    let restored = HealthStore::load(&path, 30);
    assert_eq!(restored.get("display", "orphan").unwrap().last_seen, old);
    assert!(restored
        .history(&HistoryQuery::default())
        .unwrap()
        .events
        .is_empty());
}

#[test]
fn database_failure_keeps_live_supervision_and_signals_loss() {
    let path = db_path();
    let s = HealthStore::load(&path, 30);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("DROP TABLE events;").unwrap();
    s.record("agent", "still-live", body("RUNNING"));
    assert!(s.get("agent", "still-live").is_some());
    assert!(s.warning().is_some());
    assert!(s.history(&HistoryQuery::default()).is_err());
    let missing = HealthStore::load(&path.join("missing"), 30);
    missing.record("display", "live", body("CONNECTED"));
    assert!(missing.get("display", "live").is_some());
    assert!(missing.warning().is_some());
}

#[test]
fn write_failure_does_not_change_kiosk_inventory() {
    let path = db_path().with_extension("json");
    let storage = crate::storage::Storage::load(&path).unwrap();
    let temp = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::create_dir(&temp).unwrap();
    assert!(storage.add(kiosk("a", None)).is_err());
    assert!(storage.all().is_empty());
}

#[test]
fn prometheus_remains_compatible_and_escapes_labels() {
    let s = HealthStore::new();
    s.record("agent", "a\"b", body("RUNNING"));
    let text = crate::health::render_prometheus(&s, 90);
    assert!(text.contains("noc_up{app=\"agent\",username=\"a\\\"b\"} 1"));
    assert!(text.contains("noc_last_seen_seconds"));
    assert!(text.contains("noc_info"));
}
