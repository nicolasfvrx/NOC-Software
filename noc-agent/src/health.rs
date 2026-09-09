//! Heartbeat periodique envoye a NOC Manager (statut/version/build),
//! consomme par son endpoint `/metrics` (Prometheus / Grafana).
//!
//! Volontairement leger, sur le meme modele que `commands::poll_loop` :
//! l'endpoint peut etre absent cote Manager sans casser le fonctionnement
//! normal de l'agent.

use crate::config::Config;
use crate::logging;
use crate::ui::UiHandle;
use serde::Serialize;
use std::time::Duration;

#[derive(Serialize)]
struct HeartbeatBody {
    version: &'static str,
    build: &'static str,
    state: &'static str,
}

pub async fn heartbeat_loop(cfg: Config, username: String, ui: UiHandle) {
    let client = crate::manager::build_client(cfg.manager.timeout_seconds.min(5).max(1), false);
    let interval = Duration::from_secs(cfg.manager.heartbeat_seconds.max(1));
    let endpoint = format!(
        "{}/api/heartbeat/agent/{}",
        cfg.manager.url.trim_end_matches('/'),
        username
    );

    // Evite de saturer le journal quand l'endpoint n'existe pas encore.
    let mut endpoint_ok = true;

    loop {
        tokio::time::sleep(interval).await;

        let body = HeartbeatBody {
            version: env!("CARGO_PKG_VERSION"),
            build: env!("NOC_AGENT_BUILD_TIME"),
            state: ui.snapshot().state.code(),
        };

        let result = client.post(&endpoint).json(&body).send().await;
        match result {
            Ok(response) if response.status().is_success() => {
                if !endpoint_ok {
                    endpoint_ok = true;
                    logging::log("HEARTBEAT", "à nouveau disponible");
                }
            }
            Ok(response) => {
                if endpoint_ok {
                    endpoint_ok = false;
                    logging::log(
                        "HEARTBEAT",
                        &format!("indisponible : HTTP {} (fonctionnement normal maintenu)", response.status().as_u16()),
                    );
                }
            }
            Err(e) => {
                if endpoint_ok {
                    endpoint_ok = false;
                    logging::log(
                        "HEARTBEAT",
                        &format!(
                            "indisponible : {} (fonctionnement normal maintenu)",
                            crate::manager::short_reqwest_error(&e)
                        ),
                    );
                }
            }
        }
    }
}
