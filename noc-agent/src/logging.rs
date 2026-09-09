//! Journalisation fichier + stdout au format :
//! `2026-09-09 01:00:00 [kiosk-noc-1] STATE message`

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

struct Sink {
    path: Option<PathBuf>,
    username: String,
}

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

pub fn init(path: &Path, username: &str) {
    // On verifie tout de suite qu'on peut ecrire ; sinon on se rabat sur /tmp.
    let mut target = Some(path.to_path_buf());
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .is_err()
    {
        let fallback = std::env::temp_dir().join(format!("kiosk-agent-{username}.log"));
        eprintln!(
            "NOC Agent: impossible d'écrire {} — repli sur {}",
            path.display(),
            fallback.display()
        );
        target = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&fallback)
            .ok()
            .map(|_| fallback);
    }

    let _ = SINK.set(Mutex::new(Sink {
        path: target,
        username: username.to_string(),
    }));
}

/// Ecrit une ligne de log. Ne jamais y passer de secret.
pub fn log(state_code: &str, message: &str) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let (username, path) = match SINK.get().and_then(|m| m.lock().ok()) {
        Some(sink) => (sink.username.clone(), sink.path.clone()),
        None => ("unknown".to_string(), None),
    };

    let line = if message.is_empty() {
        format!("{ts} [{username}] {state_code}")
    } else {
        format!("{ts} [{username}] {state_code} {message}")
    };

    println!("{line}");
    if let Some(path) = path {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(f, "{line}");
        }
    }
}
