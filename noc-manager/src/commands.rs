use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::storage::Storage;

/// The only actions an agent will ever be asked to run. Never a free-form string.
pub const ALLOWED_ACTIONS: [&str; 2] = ["restart_browser", "restart_agent"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KioskCommand {
    pub id: u64,
    pub action: String,
    /// Unix seconds. Optional so a hand-written commands.json still loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommandState {
    #[serde(default = "first_id")]
    next_id: u64,
    #[serde(default)]
    pending: BTreeMap<String, KioskCommand>,
}

fn first_id() -> u64 {
    1
}

impl Default for CommandState {
    fn default() -> Self {
        Self {
            next_id: first_id(),
            pending: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub enum CommandError {
    KioskNotFound,
    InvalidAction,
    CommandNotFound,
    IdMismatch,
    Storage(String),
}

impl CommandError {
    /// Machine-readable code used in JSON error bodies.
    pub fn code(&self) -> &'static str {
        match self {
            CommandError::KioskNotFound => "kiosk_not_found",
            CommandError::InvalidAction => "invalid_action",
            CommandError::CommandNotFound => "command_not_found",
            CommandError::IdMismatch => "command_id_mismatch",
            CommandError::Storage(_) => "storage_error",
        }
    }

    pub fn message(&self) -> String {
        match self {
            CommandError::KioskNotFound => "kiosk not found".to_string(),
            CommandError::InvalidAction => {
                format!("action must be one of {}", ALLOWED_ACTIONS.join(", "))
            }
            CommandError::CommandNotFound => "no pending command".to_string(),
            CommandError::IdMismatch => "command id does not match the pending command".to_string(),
            CommandError::Storage(e) => e.clone(),
        }
    }
}

/// One pending command per kiosk, backed by a JSON file.
pub struct CommandStore {
    path: PathBuf,
    state: Mutex<CommandState>,
}

impl CommandStore {
    /// Loads commands.json, creating an empty one if it does not exist.
    /// An unreadable file is kept aside instead of being silently overwritten.
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            let store = Self {
                path: path.to_path_buf(),
                state: Mutex::new(CommandState::default()),
            };
            Self::write(&store.path, &store.state.lock().unwrap())?;
            println!("[NOC Manager] created empty {}", path.display());
            return Ok(store);
        }

        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

        let state = if raw.trim().is_empty() {
            CommandState::default()
        } else {
            match serde_json::from_str::<CommandState>(&raw) {
                Ok(state) => state,
                Err(error) => {
                    // Do not crash and do not destroy the file: move it aside.
                    let mut backup = path.as_os_str().to_os_string();
                    backup.push(".invalid");
                    let backup = PathBuf::from(backup);
                    eprintln!("[NOC Manager] invalid {}: {error}", path.display());
                    match std::fs::rename(path, &backup) {
                        Ok(()) => eprintln!(
                            "[NOC Manager] kept the broken file as {}, starting with an empty command state",
                            backup.display()
                        ),
                        Err(e) => {
                            return Err(format!(
                                "invalid {} and cannot move it to {}: {e}",
                                path.display(),
                                backup.display()
                            ))
                        }
                    }
                    CommandState::default()
                }
            }
        };

        let store = Self {
            path: path.to_path_buf(),
            state: Mutex::new(state),
        };
        if !store.path.exists() {
            // The previous file was moved aside: recreate an empty one.
            Self::write(&store.path, &store.state.lock().unwrap())?;
        }
        Ok(store)
    }

    /// Pending command for one kiosk, if any.
    pub fn pending(&self, username: &str) -> Option<KioskCommand> {
        self.state.lock().unwrap().pending.get(username).cloned()
    }

    /// Snapshot of every pending command, for the web list.
    pub fn all_pending(&self) -> BTreeMap<String, KioskCommand> {
        self.state.lock().unwrap().pending.clone()
    }

    /// Queues a command, replacing any previous pending one for that kiosk.
    /// The caller has already validated the kiosk and the action.
    fn queue(&self, username: &str, action: &str) -> Result<KioskCommand, CommandError> {
        let mut state = self.state.lock().unwrap();

        let id = state.next_id;
        state.next_id = state.next_id.saturating_add(1);

        let command = KioskCommand {
            id,
            action: action.to_string(),
            created_at: now_unix(),
        };

        if let Some(previous) = state.pending.get(username) {
            println!(
                "COMMAND replaced username={username} old_id={} new_id={id}",
                previous.id
            );
        }
        state.pending.insert(username.to_string(), command.clone());

        Self::write(&self.path, &state).map_err(CommandError::Storage)?;
        println!("COMMAND queued username={username} id={id} action={action}");

        Ok(command)
    }

    /// Removes the pending command once the agent confirms it ran.
    /// An ack for an older id never deletes a newer command.
    pub fn ack(&self, username: &str, id: u64) -> Result<(), CommandError> {
        let mut state = self.state.lock().unwrap();

        let pending = match state.pending.get(username) {
            Some(command) => command.clone(),
            None => return Err(CommandError::CommandNotFound),
        };

        if pending.id != id {
            println!(
                "COMMAND ack mismatch username={username} requested={id} pending={}",
                pending.id
            );
            return Err(CommandError::IdMismatch);
        }

        state.pending.remove(username);
        Self::write(&self.path, &state).map_err(CommandError::Storage)?;
        println!("COMMAND ack username={username} id={id}");

        Ok(())
    }

    /// Atomic-ish write, same approach as kiosks.json.
    fn write(path: &Path, state: &CommandState) -> Result<(), String> {
        let json = serde_json::to_string_pretty(state)
            .map_err(|e| format!("cannot serialize commands: {e}"))?;

        let mut tmp = path.as_os_str().to_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);

        std::fs::write(&tmp, json.as_bytes())
            .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| format!("cannot replace {}: {e}", path.display()))?;
        Ok(())
    }
}

fn now_unix() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

pub fn is_allowed_action(action: &str) -> bool {
    ALLOWED_ACTIONS.contains(&action)
}

/// Entry point used by both the web UI and the admin API.
///
/// 1. the kiosk must exist in kiosks.json
/// 2. the action must be one of ALLOWED_ACTIONS
/// 3. a fresh id is allocated, the previous pending command is replaced
/// 4. commands.json is rewritten
pub fn queue_command(
    kiosks: &Storage,
    commands: &CommandStore,
    username: &str,
    action: &str,
) -> Result<KioskCommand, CommandError> {
    if kiosks.get(username).is_none() {
        return Err(CommandError::KioskNotFound);
    }
    if !is_allowed_action(action) {
        return Err(CommandError::InvalidAction);
    }
    commands.queue(username, action)
}
