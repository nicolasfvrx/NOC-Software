// The binary is named "NOC Manager"; the crate name inherits it.
#![allow(non_snake_case)]

mod api;
mod commands;
mod config;
mod dashboard;
mod health;
mod models;
mod rdp_servers;
mod storage;
#[path = "../../shared/version.rs"]
mod suite_version;
#[cfg(test)]
mod tests;
mod web;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;

use commands::CommandStore;
use health::HealthStore;
use storage::Storage;

pub struct AppState {
    pub rdp_servers: rdp_servers::ServerStore,
    pub storage: Storage,
    pub commands: CommandStore,
    pub health: HealthStore,
    pub api_token: String,
    pub health_stale_after_seconds: u64,
}

#[tokio::main]
async fn main() {
    // Startup auto-update is disabled: it assumed one binary per running
    // instance, but this binary is shared by several concurrent sessions.
    // See doc/updates.md before re-enabling shared::update.
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!(
            "NOC Manager {} — Norfair Operation Center",
            suite_version::suite_version()
        );
        return;
    }
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn SetConsoleTitleW(title: *const u16) -> i32;
        }
        let title: Vec<u16> = "NOC Manager".encode_utf16().chain(Some(0)).collect();
        SetConsoleTitleW(title.as_ptr());
    }
    if let Err(error) = run().await {
        eprintln!("[NOC Manager] fatal: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    // Files are resolved next to the executable's working directory.
    let config_path = PathBuf::from("config.toml");
    let config = config::load(&config_path)?;

    let data_path = PathBuf::from(&config.data.file);
    let storage = Storage::load(&data_path)?;

    let commands_path = PathBuf::from(&config.data.commands_file);
    let commands = CommandStore::load(&commands_path)?;

    let state = Arc::new(AppState {
        rdp_servers: rdp_servers::ServerStore::load(std::path::Path::new(
            &config.data.rdp_servers_file,
        ))?,
        storage,
        commands,
        health: HealthStore::load(
            std::path::Path::new(&config.health.history_file),
            config.health.retention_days,
        ),
        api_token: config.server.api_token.trim().to_string(),
        health_stale_after_seconds: config.health.stale_after_seconds,
    });

    let maintenance = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            let state = maintenance.clone();
            let _ =
                tokio::task::spawn_blocking(move || state.health.purge(health::now_unix())).await;
        }
    });

    let app = Router::new()
        .merge(web::router())
        .merge(api::router(state.clone()))
        .with_state(state.clone());

    let address = format!("{}:{}", config.server.listen, config.server.port);
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .map_err(|e| format!("cannot listen on {address}: {e}"))?;

    println!("[NOC Manager] data file: {}", data_path.display());
    println!("[NOC Manager] commands file: {}", commands_path.display());
    println!(
        "[NOC Manager] api token: {}",
        if state.api_token.is_empty() {
            "disabled"
        } else {
            "enabled"
        }
    );
    println!("[NOC Manager] listening on http://{address}");

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {e}"))
}
