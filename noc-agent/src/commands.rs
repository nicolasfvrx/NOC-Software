//! Commandes distantes envoyees par NOC Manager
//! (`restart_browser`, `restart_agent`), persistance du dernier id traite
//! et boucle de polling legere.
//!
//! L'endpoint peut ne pas encore exister cote Manager : dans ce cas on
//! journalise une fois et l'agent continue normalement.

use crate::config::Config;
use crate::logging;
use crate::state::KioskState;
use crate::ui::UiHandle;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

// ---------------------------------------------------------------- actions

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingAction {
    RestartBrowser,
    RestartAgent,
}

/// File d'attente (au plus une commande) partagee entre le poller et
/// l'orchestrateur. `restart_agent` est prioritaire sur tout le reste.
pub struct RemoteControl {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    pending: Option<PendingAction>,
    /// Un redemarrage de navigateur est deja en cours : une nouvelle
    /// commande restart_browser ne doit pas en declencher un second.
    browser_restart_in_progress: bool,
}

impl RemoteControl {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Met une commande en file. `false` = commande ignoree (un
    /// redemarrage est deja en cours) ; son id reste marque comme traite.
    pub fn queue(&self, action: PendingAction) -> bool {
        let mut inner = self.lock();
        match action {
            // Priorite maximale : ecrase tout ce qui attend.
            PendingAction::RestartAgent => {
                inner.pending = Some(PendingAction::RestartAgent);
                true
            }
            PendingAction::RestartBrowser => {
                if inner.pending == Some(PendingAction::RestartAgent) {
                    return false;
                }
                if inner.browser_restart_in_progress
                    || inner.pending == Some(PendingAction::RestartBrowser)
                {
                    return false;
                }
                inner.pending = Some(PendingAction::RestartBrowser);
                true
            }
        }
    }

    /// Recupere et consomme la commande en attente.
    pub fn take(&self) -> Option<PendingAction> {
        self.lock().pending.take()
    }

    pub fn begin_browser_restart(&self) {
        self.lock().browser_restart_in_progress = true;
    }

    pub fn end_browser_restart(&self) {
        self.lock().browser_restart_in_progress = false;
    }

    pub fn browser_restart_in_progress(&self) -> bool {
        self.lock().browser_restart_in_progress
    }
}

impl Default for RemoteControl {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------ persistance

#[derive(Debug, Default, Serialize, Deserialize)]
struct StateFile {
    #[serde(default)]
    last_command_id: u64,
}

/// Petite abstraction sur ~/.local/state/kiosk-agent/state.json.
/// Ecriture atomique (fichier .tmp puis rename) pour ne jamais corrompre
/// le fichier en cas d'arret brutal.
pub struct CommandStore {
    path: PathBuf,
    last_command_id: u64,
}

impl CommandStore {
    pub fn load() -> Self {
        let path = state_file_path();
        let last_command_id = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<StateFile>(&raw).ok())
            .map(|s| s.last_command_id)
            .unwrap_or(0);
        logging::log(
            "REMOTE_COMMAND",
            &format!(
                "état local {} (last_command_id={last_command_id})",
                path.display()
            ),
        );
        Self {
            path,
            last_command_id,
        }
    }

    pub fn last_command_id(&self) -> u64 {
        self.last_command_id
    }

    /// Marque une commande comme traitee, immediatement et durablement.
    pub fn mark_processed(&mut self, id: u64) -> Result<(), String> {
        self.last_command_id = id;

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("création de {} impossible : {e}", parent.display()))?;
        }
        let payload = serde_json::to_string_pretty(&StateFile {
            last_command_id: id,
        })
        .map_err(|e| e.to_string())?;

        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, payload.as_bytes())
            .map_err(|e| format!("écriture de {} impossible : {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|e| format!("renommage vers {} impossible : {e}", self.path.display()))?;
        Ok(())
    }
}

fn state_file_path() -> PathBuf {
    let dir = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::config::home_dir().map(|h| h.join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    dir.join("kiosk-agent").join("state.json")
}

// -------------------------------------------------------------------- API

#[derive(Debug, Deserialize)]
struct CommandEnvelope {
    #[serde(default)]
    command: Option<RemoteCommand>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteCommand {
    pub id: u64,
    pub action: String,
}

enum PollResult {
    Command(RemoteCommand),
    Empty,
    /// Endpoint absent (404), injoignable ou reponse illisible :
    /// jamais bloquant pour le fonctionnement normal de l'agent.
    Unavailable(String),
}

async fn fetch_command(
    client: &reqwest::Client,
    manager_url: &str,
    username: &str,
) -> PollResult {
    let endpoint = format!(
        "{}/api/kiosk/{}/command",
        manager_url.trim_end_matches('/'),
        username
    );

    let response = match client.get(&endpoint).send().await {
        Ok(r) => r,
        Err(e) => return PollResult::Unavailable(crate::manager::short_reqwest_error(&e)),
    };
    let status = response.status();
    if status.as_u16() == 404 {
        return PollResult::Unavailable("endpoint de commandes absent (HTTP 404)".to_string());
    }
    if !status.is_success() {
        return PollResult::Unavailable(format!("HTTP {}", status.as_u16()));
    }
    let body = match response.text().await {
        Ok(b) => b,
        Err(e) => return PollResult::Unavailable(crate::manager::short_reqwest_error(&e)),
    };
    match serde_json::from_str::<CommandEnvelope>(&body) {
        Ok(envelope) => match envelope.command {
            Some(command) => PollResult::Command(command),
            None => PollResult::Empty,
        },
        Err(e) => PollResult::Unavailable(format!("réponse JSON invalide : {e}")),
    }
}

/// POST {manager}/api/kiosk/{username}/command/{id}/ack
async fn ack_command(
    client: &reqwest::Client,
    manager_url: &str,
    username: &str,
    id: u64,
) -> Result<(), String> {
    let endpoint = format!(
        "{}/api/kiosk/{}/command/{}/ack",
        manager_url.trim_end_matches('/'),
        username,
        id
    );
    let response = client
        .post(&endpoint)
        .send()
        .await
        .map_err(|e| crate::manager::short_reqwest_error(&e))?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", status.as_u16()))
    }
}

// ---------------------------------------------------------------- polling

/// Boucle de polling des commandes. Volontairement legere : une requete
/// GET toutes les `manager.command_poll_seconds` secondes.
pub async fn poll_loop(
    cfg: Config,
    username: String,
    ui: UiHandle,
    remote: std::sync::Arc<RemoteControl>,
) {
    // Timeout court : l'ACK ne doit jamais retarder l'affichage.
    let client = crate::manager::build_client(cfg.manager.timeout_seconds.min(5).max(1), false);
    let mut store = CommandStore::load();
    let interval = Duration::from_secs(cfg.manager.command_poll_seconds.max(1));

    // Evite de saturer le journal quand l'endpoint n'existe pas encore.
    let mut endpoint_ok = true;
    let mut last_ignored_logged = 0u64;

    loop {
        tokio::time::sleep(interval).await;

        match fetch_command(&client, &cfg.manager.url, &username).await {
            PollResult::Unavailable(reason) => {
                if endpoint_ok {
                    endpoint_ok = false;
                    logging::log(
                        "REMOTE_COMMAND",
                        &format!("polling indisponible : {reason} (fonctionnement normal maintenu)"),
                    );
                }
            }
            PollResult::Empty => {
                if !endpoint_ok {
                    endpoint_ok = true;
                    logging::log("REMOTE_COMMAND", "polling à nouveau disponible");
                }
            }
            PollResult::Command(command) => {
                if !endpoint_ok {
                    endpoint_ok = true;
                    logging::log("REMOTE_COMMAND", "polling à nouveau disponible");
                }
                handle_command(command, &cfg, &username, &client, &ui, &remote, &mut store, &mut last_ignored_logged)
                    .await;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_command(
    command: RemoteCommand,
    cfg: &Config,
    username: &str,
    client: &reqwest::Client,
    ui: &UiHandle,
    remote: &RemoteControl,
    store: &mut CommandStore,
    last_ignored_logged: &mut u64,
) {
    // 1. Anti-double-execution : state.json fait foi.
    if command.id <= store.last_command_id() {
        if *last_ignored_logged != command.id {
            *last_ignored_logged = command.id;
            logging::log(
                "REMOTE_COMMAND",
                &format!(
                    "id={} déjà traité (last_command_id={}), ignorée",
                    command.id,
                    store.last_command_id()
                ),
            );
        }
        return;
    }

    logging::log(
        "REMOTE_COMMAND",
        &format!("id={} action={} received", command.id, command.action),
    );

    // 2. Marquage local AVANT toute action destructive.
    match store.mark_processed(command.id) {
        Ok(()) => logging::log(
            "REMOTE_COMMAND",
            &format!("id={} saved locally", command.id),
        ),
        Err(e) => logging::log(
            "REMOTE_COMMAND",
            &format!("id={} échec d'écriture de state.json : {e}", command.id),
        ),
    }

    // 3. ACK. Un echec n'empeche jamais l'execution ni ne provoque de
    //    re-execution : state.json protege contre les doublons.
    match ack_command(client, &cfg.manager.url, username, command.id).await {
        Ok(()) => logging::log("REMOTE_COMMAND", &format!("id={} ack success", command.id)),
        Err(e) => logging::log(
            "REMOTE_COMMAND",
            &format!("id={} ack failed: {e}", command.id),
        ),
    }

    // 4. Execution.
    match command.action.as_str() {
        "restart_browser" => {
            ui.show();
            ui.set(KioskState::RemoteRestartBrowser);
            if !remote.queue(PendingAction::RestartBrowser) {
                logging::log(
                    "REMOTE_COMMAND",
                    &format!(
                        "id={} restart_browser ignorée : redémarrage déjà en cours",
                        command.id
                    ),
                );
            }
        }
        "restart_agent" => {
            ui.show();
            ui.set(KioskState::RemoteRestartAgent);
            remote.queue(PendingAction::RestartAgent);
        }
        unknown => {
            // Marquee et acquittee : pas de boucle permanente.
            logging::log("REMOTE_COMMAND", &format!("unknown action={unknown}"));
        }
    }
}
