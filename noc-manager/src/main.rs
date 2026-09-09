// The binary is named "NOC Manager"; the crate name inherits it.
#![allow(non_snake_case)]

mod api;
mod commands;
mod config;
mod models;
mod storage;
mod web;
#[path = "../../shared/update.rs"]
mod startup_update;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;

use commands::CommandStore;
use storage::Storage;

pub struct AppState {
    pub storage: Storage,
    pub commands: CommandStore,
    pub api_token: String,
}

#[tokio::main]
async fn main() {
    if startup_update::run("noc-manager", "noc-manager.exe") {
        return;
    }
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("NOC Manager {} — Norfair Operation Center", env!("CARGO_PKG_VERSION"));
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
        storage,
        commands,
        api_token: config.server.api_token.trim().to_string(),
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
