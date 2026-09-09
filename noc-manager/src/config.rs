use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub data: DataConfig,
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

const DEFAULT_CONFIG_TOML: &str = r#"[server]
listen = "0.0.0.0"
port = 8080
api_token = ""

[data]
file = "kiosks.json"
commands_file = "commands.json"
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
