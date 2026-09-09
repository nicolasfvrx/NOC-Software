//! NOC Agent — agent kiosque Linux (Zorin OS / XFCE / xRDP).
//!
//! Thread principal : fenetre de statut plein ecran (eframe/egui).
//! Thread worker    : runtime tokio (API NOC Manager, test HTTP,
//!                    geckodriver / WebDriver, cron, surveillance).

mod app;
mod commands;
mod config;
mod firefox;
mod logging;
mod manager;
mod scheduler;
mod state;
mod ui;
mod webdriver;
#[path = "../../shared/update.rs"]
mod startup_update;

use eframe::egui;
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    startup_update::run("noc-agent", "noc-agent-ubuntu-22.04-x64");
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("NOC Agent {} — Norfair Operation Center", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let loaded = match config::load() {
        Ok(loaded) => loaded,
        Err(err) => {
            eprintln!("NOC Agent: {err}");
            std::process::exit(2);
        }
    };
    let config::Loaded { config, base, file } = loaded;

    let username = config::current_username();
    logging::init(&config.log_path(&base), &username);
    logging::log("STARTING", "application started");
    logging::log("STARTING", &format!("configuration : {}", file.display()));

    let bridge: ui::UiHandle = Arc::new(ui::UiBridge::new(&username));

    // ------------------------------------------------------------- worker
    {
        let bridge = bridge.clone();
        let config = config.clone();
        let base = base.clone();
        let username = username.clone();
        std::thread::Builder::new()
            .name("kiosk-worker".to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        logging::log("STARTING", &format!("runtime tokio indisponible : {e}"));
                        return;
                    }
                };
                runtime.block_on(app::run(config, base, username, bridge));
            })
            .expect("impossible de démarrer le thread worker");
    }

    // ----------------------------------------------------------------- UI
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("noc-agent")
            .with_title("NOC Agent")
            .with_fullscreen(true)
            .with_decorations(false)
            .with_resizable(false)
            .with_always_on_top()
            .with_active(true),
        // Pas de restauration d'etat : la fenetre doit toujours repartir
        // en plein ecran, quelle que soit la resolution xRDP.
        persist_window: false,
        ..Default::default()
    };

    eframe::run_native(
        "NOC Agent",
        options,
        Box::new(move |cc| {
            Ok(Box::new(ui::KioskApp::new(cc, bridge, &config, &base)) as Box<dyn eframe::App>)
        }),
    )
}
