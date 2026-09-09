use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub data: DataConfig,
    #[serde(default)]
    pub health: HealthConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Empty = no authentication.
    #[serde(default)]
    pub api_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataConfig {
    #[serde(default = "default_data_file")]
    pub file: String,
    #[serde(default = "default_commands_file")]
    pub commands_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthConfig {
    /// `noc_up` reports 0 in `/metrics` once a heartbeat is older than this.
    /// Keep it a few times the agents'/displays' own heartbeat interval.
    #[serde(default = "default_stale_after_seconds")]
    pub stale_after_seconds: u64,
}

fn default_listen() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8080
}

fn default_data_file() -> String {
    "kiosks.json".to_string()
}

fn default_commands_file() -> String {
    "commands.json".to_string()
}

fn default_stale_after_seconds() -> u64 {
    90
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            port: default_port(),
            api_token: String::new(),
        }
    }
}

impl Default for DataConfig {
    fn default() -> Self {
        Self {
            file: default_data_file(),
            commands_file: default_commands_file(),
        }
    }
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            stale_after_seconds: default_stale_after_seconds(),
        }
    }
}

const DEFAULT_CONFIG_TOML: &str = r#"[server]
listen = "0.0.0.0"
port = 8080
api_token = ""

[data]
file = "kiosks.json"
commands_file = "commands.json"

[health]
stale_after_seconds = 90
"#;

/// Loads config.toml, creating it with default values if it does not exist.
pub fn load(path: &Path) -> Result<Config, String> {
    if !path.exists() {
        std::fs::write(path, DEFAULT_CONFIG_TOML)
            .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
        println!("[NOC Manager] created default {}", path.display());
    }

    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    toml::from_str(&raw).map_err(|e| format!("invalid {}: {e}", path.display()))
}
