//! Chargement de config.toml + detection de l'utilisateur Linux courant.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub manager: ManagerConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub firefox: FirefoxConfig,
    #[serde(default)]
    pub target: TargetConfig,
    #[serde(default)]
    pub log: LogConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManagerConfig {
    /// Ex: http://192.168.10.64:8080
    pub url: String,
    /// Intervalle de re-lecture de la configuration quand tout va bien.
    #[serde(default = "d_poll")]
    pub poll_seconds: u64,
    /// Attente avant nouvelle tentative apres une erreur manager.
    #[serde(default = "d_retry")]
    pub retry_seconds: u64,
    /// Intervalle de relecture des commandes distantes.
    #[serde(default = "d_command_poll")]
    pub command_poll_seconds: u64,
    /// Intervalle d'envoi du heartbeat (statut/version) a NOC Manager.
    #[serde(default = "d_heartbeat")]
    pub heartbeat_seconds: u64,
    /// Timeout des requetes HTTP vers NOC Manager.
    #[serde(default = "d_http_timeout")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UiConfig {
    #[serde(default = "d_background")]
    pub background: String,
    #[serde(default = "d_logo")]
    pub logo: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FirefoxConfig {
    #[serde(default = "d_flatpak")]
    pub flatpak: String,
    #[serde(default = "d_app_id")]
    pub app_id: String,
    /// Wrapper passe a geckodriver comme "binary".
    /// Vide => <base>/firefox-flatpak-wrapper.sh
    #[serde(default)]
    pub wrapper: String,
    /// Chemin de geckodriver. Vide => recherche dans $PATH.
    #[serde(default)]
    pub geckodriver: String,
    /// Profil Firefox persistant (cookies, session Grafana). `~` est etendu.
    #[serde(default = "d_profile_dir")]
    pub profile_dir: String,
    #[serde(default = "d_load_timeout")]
    pub load_timeout_seconds: u64,
    #[serde(default = "d_slow")]
    pub slow_loading_after_seconds: u64,
    #[serde(default = "d_retry")]
    pub retry_seconds: u64,
    /// Pause entre l'arret de Firefox et sa relance.
    #[serde(default = "d_restart_delay")]
    pub restart_delay_seconds: u64,
    /// Passer --kiosk a Firefox.
    #[serde(default = "d_true")]
    pub kiosk: bool,
    /// Dernier recours si Firefox refuse de s'arreter : `flatpak kill <app_id>`
    /// (limite aux instances Flatpak de l'utilisateur Linux courant).
    #[serde(default = "d_true")]
    pub force_kill: bool,
    /// Delai d'attente du demarrage de geckodriver.
    #[serde(default = "d_gecko_timeout")]
    pub geckodriver_timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetConfig {
    /// Accepter les certificats TLS auto-signes lors du test de la destination.
    #[serde(default)]
    pub insecure: bool,
    #[serde(default = "d_http_timeout")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogConfig {
    #[serde(default = "d_logfile")]
    pub file: String,
}

fn d_poll() -> u64 {
    30
}
fn d_retry() -> u64 {
    10
}
fn d_command_poll() -> u64 {
    5
}
fn d_heartbeat() -> u64 {
    30
}
fn d_http_timeout() -> u64 {
    10
}
fn d_load_timeout() -> u64 {
    60
}
fn d_slow() -> u64 {
    15
}
fn d_restart_delay() -> u64 {
    3
}
fn d_gecko_timeout() -> u64 {
    30
}
fn d_true() -> bool {
    true
}
fn d_background() -> String {
    "background.jpg".into()
}
fn d_logo() -> String {
    "logo.png".into()
}
fn d_flatpak() -> String {
    "/usr/bin/flatpak".into()
}
fn d_app_id() -> String {
    "org.mozilla.firefox".into()
}
fn d_profile_dir() -> String {
    "~/.var/app/org.mozilla.firefox/noc-agent-profile".into()
}
fn d_logfile() -> String {
    "noc-agent.log".into()
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            background: d_background(),
            logo: d_logo(),
        }
    }
}

impl Default for FirefoxConfig {
    fn default() -> Self {
        Self {
            flatpak: d_flatpak(),
            app_id: d_app_id(),
            wrapper: String::new(),
            geckodriver: String::new(),
            profile_dir: d_profile_dir(),
            load_timeout_seconds: d_load_timeout(),
            slow_loading_after_seconds: d_slow(),
            retry_seconds: d_retry(),
            restart_delay_seconds: d_restart_delay(),
            kiosk: true,
            force_kill: true,
            geckodriver_timeout_seconds: d_gecko_timeout(),
        }
    }
}

impl Default for TargetConfig {
    fn default() -> Self {
        Self {
            insecure: false,
            timeout_seconds: d_http_timeout(),
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self { file: d_logfile() }
    }
}

impl Config {
    /// Resout un chemin d'asset relatif au dossier de base,
    /// en essayant aussi le sous-dossier `assets/`.
    pub fn asset_path(base: &Path, name: &str) -> PathBuf {
        let direct = resolve(base, name);
        if direct.exists() {
            return direct;
        }
        let in_assets = base.join("assets").join(name);
        if in_assets.exists() {
            return in_assets;
        }
        direct
    }

    pub fn log_path(&self, base: &Path) -> PathBuf {
        resolve(base, &self.log.file)
    }
}

fn resolve(base: &Path, value: &str) -> PathBuf {
    let expanded = expand_tilde(value);
    if expanded.is_absolute() {
        expanded
    } else {
        base.join(expanded)
    }
}

pub fn expand_tilde(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(value)
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

/// Emplacements testes pour config.toml, dans l'ordre.
pub fn find_config_file() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(explicit) = std::env::var_os("NOC_AGENT_CONFIG") {
        candidates.push(PathBuf::from(explicit));
    }
    candidates.push(PathBuf::from("config.toml"));
    if let Some(home) = home_dir() {
        candidates.push(home.join(".config/noc-agent/config.toml"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("config.toml"));
        }
    }
    candidates.push(PathBuf::from("/etc/noc-agent/config.toml"));

    candidates.into_iter().find(|p| p.is_file())
}

pub struct Loaded {
    pub config: Config,
    /// Dossier de reference pour les chemins relatifs (assets, log).
    pub base: PathBuf,
    pub file: PathBuf,
}

pub fn load() -> Result<Loaded, String> {
    let file = find_config_file().ok_or_else(|| {
        "config.toml introuvable (cherché dans : $NOC_AGENT_CONFIG, ./config.toml, \
         ~/.config/noc-agent/config.toml, <dossier du binaire>/config.toml, \
         /etc/noc-agent/config.toml)"
            .to_string()
    })?;
    let raw = std::fs::read_to_string(&file)
        .map_err(|e| format!("lecture de {} impossible : {e}", file.display()))?;
    let config: Config = toml::from_str(&raw)
        .map_err(|e| format!("config.toml invalide ({}) : {e}", file.display()))?;
    let base = file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(Loaded { config, base, file })
}

/// Nom de l'utilisateur Linux courant (equivalent de `id -un`).
pub fn current_username() -> String {
    for key in ["USER", "LOGNAME", "USERNAME"] {
        if let Ok(v) = std::env::var(key) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return v;
            }
        }
    }
    if let Ok(out) = std::process::Command::new("id").arg("-un").output() {
        if out.status.success() {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !v.is_empty() {
                return v;
            }
        }
    }
    "unknown".to_string()
}
