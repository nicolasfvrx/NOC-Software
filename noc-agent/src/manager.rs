//! Client HTTP : lecture de la configuration sur NOC Manager
//! et test d'accessibilite de la destination.

use serde::Deserialize;
use std::time::Duration;

/// Configuration d'un kiosque telle que renvoyee par NOC Manager.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct KioskConfig {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub name: String,
    pub url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub restart_cron: Option<String>,
    /// Optionnel : selecteur CSS a attendre en plus de readyState == "complete".
    #[serde(default)]
    pub ready_selector: Option<String>,
}

fn default_true() -> bool {
    true
}

impl KioskConfig {
    pub fn ready_selector(&self) -> &str {
        self.ready_selector.as_deref().unwrap_or("").trim()
    }

    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            &self.username
        } else {
            &self.name
        }
    }
}

pub enum FetchOutcome {
    Found(Box<KioskConfig>),
    /// Le manager repond mais ne connait pas ce kiosque (404).
    NotFound,
    /// Manager injoignable, en erreur, ou reponse illisible.
    Unavailable(String),
}

pub enum TargetCheck {
    /// Service joignable (2xx, 3xx, 401, 403).
    Reachable(u16),
    /// Service joignable mais reponse inutilisable, ou injoignable.
    /// Le message est directement affichable a l'utilisateur.
    Unusable(String),
}

pub fn build_client(timeout_seconds: u64, insecure: bool) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds.max(1)))
        .user_agent(concat!("noc-agent/", env!("CARGO_PKG_VERSION")));
    if insecure {
        builder = builder.danger_accept_invalid_certs(true);
    }
    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

/// GET {manager_url}/api/kiosk/{username}
pub async fn fetch_kiosk(
    client: &reqwest::Client,
    manager_url: &str,
    username: &str,
) -> FetchOutcome {
    let endpoint = format!(
        "{}/api/kiosk/{}",
        manager_url.trim_end_matches('/'),
        username
    );

    let response = match client.get(&endpoint).send().await {
        Ok(r) => r,
        Err(e) => return FetchOutcome::Unavailable(short_reqwest_error(&e)),
    };

    let status = response.status();
    if status.as_u16() == 404 {
        return FetchOutcome::NotFound;
    }
    if !status.is_success() {
        return FetchOutcome::Unavailable(format!("HTTP {}", status.as_u16()));
    }

    let body = match response.text().await {
        Ok(b) => b,
        Err(e) => return FetchOutcome::Unavailable(short_reqwest_error(&e)),
    };

    match serde_json::from_str::<KioskConfig>(&body) {
        Ok(cfg) => {
            if cfg.url.trim().is_empty() {
                FetchOutcome::Unavailable("configuration sans URL".to_string())
            } else {
                FetchOutcome::Found(Box::new(cfg))
            }
        }
        Err(e) => FetchOutcome::Unavailable(format!("réponse JSON invalide : {e}")),
    }
}

/// Test HTTP de la destination avant de lancer Firefox.
pub async fn check_target(client: &reqwest::Client, url: &str) -> TargetCheck {
    let response = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            return TargetCheck::Unusable(unreachable_message(&e));
        }
    };

    let code = response.status().as_u16();
    match code {
        // 2xx / 3xx : service accessible.
        200..=399 => TargetCheck::Reachable(code),
        // Authentification requise : Firefox dispose peut-etre d'un cookie
        // que NOC Agent n'a pas -> on considere le service accessible.
        401 | 403 => TargetCheck::Reachable(code),
        404 => TargetCheck::Unusable("Page introuvable — HTTP 404".to_string()),
        410 => TargetCheck::Unusable("Page supprimée — HTTP 410".to_string()),
        500..=599 => TargetCheck::Unusable(format!("Erreur du service — HTTP {code}")),
        other => TargetCheck::Unusable(format!("Réponse inattendue — HTTP {other}")),
    }
}

fn unreachable_message(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "Le serveur ne répond pas (délai dépassé)".to_string()
    } else if e.is_connect() {
        "Impossible de joindre le serveur".to_string()
    } else if e.is_request() {
        "Adresse du service invalide".to_string()
    } else {
        "Impossible de joindre le serveur".to_string()
    }
}

/// Message court pour les logs (jamais d'URL avec identifiants).
pub fn short_reqwest_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "timeout".to_string()
    } else if e.is_connect() {
        "connexion refusée / DNS".to_string()
    } else {
        "erreur réseau".to_string()
    }
}
