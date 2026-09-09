//! Pilotage de Firefox via geckodriver / WebDriver (fantoccini).
//!
//! On ne fait JAMAIS de `sleep(5)` suivi d'un affichage optimiste :
//! l'agent interroge reellement `document.readyState` (et, si demande,
//! la presence d'un selecteur CSS) avant de laisser Firefox apparaitre.

use crate::config::Config;
use crate::firefox;
use crate::logging;
use serde_json::{json, Value};
use std::net::TcpListener;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};

#[derive(Debug)]
pub enum LaunchError {
    /// geckodriver ou le wrapper Flatpak manque : etat FIREFOX_COMPONENT_MISSING.
    MissingComponent(String),
    Other(String),
}

#[derive(Debug)]
pub enum PageError {
    /// La page n'a pas atteint l'etat "prete" dans le temps imparti.
    Timeout,
    /// Session WebDriver perdue (Firefox ferme, crash, driver disparu).
    SessionLost(String),
}

pub struct Browser {
    pub client: fantoccini::Client,
    driver: Child,
}

impl Browser {
    /// Demarre geckodriver puis Firefox (Flatpak) via le wrapper.
    pub async fn launch(cfg: &Config, base: &Path) -> Result<Browser, LaunchError> {
        if !firefox::flatpak_app_installed(&cfg.firefox) {
            return Err(LaunchError::MissingComponent(format!(
                "application Flatpak {} non installée",
                cfg.firefox.app_id
            )));
        }
        let geckodriver = firefox::resolve_geckodriver(&cfg.firefox).ok_or_else(|| {
            LaunchError::MissingComponent(
                "geckodriver introuvable (sudo apt install firefox-geckodriver)".to_string(),
            )
        })?;
        let wrapper = firefox::resolve_wrapper(&cfg.firefox, base)
            .map_err(LaunchError::MissingComponent)?;
        let profile = firefox::ensure_profile_dir(&cfg.firefox).map_err(LaunchError::Other)?;

        // Aucune instance residuelle ne doit subsister, sinon Firefox
        // repond "deja en cours d'execution" et geckodriver echoue.
        firefox::ensure_no_leftover(&cfg.firefox, &profile).await;

        let port = free_port().map_err(|e| LaunchError::Other(e.to_string()))?;

        logging::log(
            "STARTING_FIREFOX",
            &format!(
                "geckodriver={} wrapper={} port={port}",
                geckodriver.display(),
                wrapper.display()
            ),
        );

        let mut driver = Command::new(&geckodriver)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--binary")
            .arg(&wrapper)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| LaunchError::Other(format!("geckodriver ne démarre pas : {e}")))?;

        let client = match wait_and_connect(cfg, port, &wrapper, &profile).await {
            Ok(c) => c,
            Err(e) => {
                // Ne jamais laisser un geckodriver orphelin derriere nous.
                let _ = driver.kill().await;
                return Err(e);
            }
        };

        // Firefox est deja plein ecran grace a --kiosk ; il demarre
        // derriere la fenetre NOC Agent qui est always-on-top.
        Ok(Browser { client, driver })
    }

    /// Navigue vers l'URL. `pageLoadStrategy = none` : retour immediat,
    /// c'est nous qui surveillons la fin du chargement.
    pub async fn goto(&self, url: &str) -> Result<(), PageError> {
        self.client
            .goto(url)
            .await
            .map_err(|e| PageError::SessionLost(e.to_string()))
    }

    /// Interroge la page : (prete, readyState).
    pub async fn probe_ready(&self, ready_selector: &str) -> Result<(bool, String), PageError> {
        const SCRIPT: &str = r#"
            var sel = arguments[0];
            var rs = document.readyState;
            if (rs !== 'complete') { return { ready: false, readyState: rs }; }
            if (sel && sel.length > 0) {
                return { ready: document.querySelector(sel) !== null, readyState: rs, selector: true };
            }
            return { ready: true, readyState: rs };
        "#;

        let result = self
            .client
            .execute(SCRIPT, vec![json!(ready_selector)])
            .await
            .map_err(|e| PageError::SessionLost(e.to_string()))?;

        let ready = result
            .get("ready")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let state = result
            .get("readyState")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        Ok((ready, state))
    }

    /// Firefox (et sa session WebDriver) est-il toujours la ?
    pub async fn is_alive(&mut self) -> bool {
        if matches!(self.driver.try_wait(), Ok(Some(_)) | Err(_)) {
            return false;
        }
        matches!(
            tokio::time::timeout(Duration::from_secs(10), self.client.current_url()).await,
            Ok(Ok(_))
        )
    }

    pub async fn current_url(&self) -> Option<String> {
        self.client.current_url().await.ok().map(|u| u.to_string())
    }

    /// Arret propre : WebDriver d'abord, puis arret cible du driver,
    /// puis `flatpak kill` en tout dernier recours.
    pub async fn shutdown(mut self, cfg: &Config) {
        let graceful = matches!(
            tokio::time::timeout(Duration::from_secs(10), self.client.clone().close()).await,
            Ok(Ok(()))
        );
        logging::log(
            "FIREFOX_STOP",
            if graceful {
                "session WebDriver fermée proprement"
            } else {
                "fermeture WebDriver impossible, arrêt du driver"
            },
        );

        // Laisse a Firefox le temps de se terminer de lui-meme.
        for _ in 0..10 {
            if let Ok(Some(_)) = self.driver.try_wait() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Kill cible : uniquement le geckodriver lance par CET agent.
        let _ = self.driver.kill().await;
        let _ = self.driver.wait().await;

        // Le driver mort, l'application Flatpak peut survivre : on verifie
        // et on insiste si necessaire, sinon le prochain lancement echoue.
        firefox::ensure_stopped(&cfg.firefox).await;
    }
}

async fn wait_and_connect(
    cfg: &Config,
    port: u16,
    wrapper: &Path,
    profile: &Path,
) -> Result<fantoccini::Client, LaunchError> {
    // 1. Attendre que geckodriver ecoute.
    let deadline = Instant::now() + Duration::from_secs(cfg.firefox.geckodriver_timeout_seconds);
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            return Err(LaunchError::Other(
                "geckodriver n'écoute pas (délai dépassé)".to_string(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // 2. Ouvrir la session Firefox.
    let mut args: Vec<Value> = Vec::new();
    if cfg.firefox.kiosk {
        args.push(json!("--kiosk"));
    }
    // Profil persistant : cookies / session Grafana / certificats conservés.
    args.push(json!("-profile"));
    args.push(json!(profile.to_string_lossy()));

    let mut caps = serde_json::Map::new();
    caps.insert(
        "moz:firefoxOptions".to_string(),
        json!({
            "binary": wrapper.to_string_lossy(),
            "args": args,
        }),
    );
    // On veut la main immediatement apres goto() pour piloter l'attente.
    caps.insert("pageLoadStrategy".to_string(), json!("none"));
    caps.insert("acceptInsecureCerts".to_string(), json!(cfg.target.insecure));

    fantoccini::ClientBuilder::native()
        .capabilities(caps)
        .connect(&format!("http://127.0.0.1:{port}"))
        .await
        .map_err(|e| LaunchError::Other(format!("session WebDriver refusée : {e}")))
}

fn free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}
