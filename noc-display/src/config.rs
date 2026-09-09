use serde::Deserialize;
use std::{io::Read, path::PathBuf};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub rdp: RdpSettings,
    pub ui: UiSettings,
    pub manager: ManagerSettings,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManagerSettings {
    /// Heartbeat desactive par defaut : NOC Display n'a historiquement
    /// aucune dependance a NOC Manager (voir README).
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub heartbeat_seconds: u32,
    /// Si vrai, `[rdp] server/port/password/ignore_certificate_errors` sont
    /// ignores : recuperes depuis Manager a chaque tentative de connexion,
    /// via le nom du compte Windows courant (voir README).
    pub provides_rdp: bool,
}
impl Default for ManagerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: String::new(),
            port: 8080,
            heartbeat_seconds: 30,
            provides_rdp: false,
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RdpSettings {
    pub enabled: bool,
    pub server: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub ignore_certificate_errors: bool,
    pub domain: String,
    pub retry_seconds: u32,
}
impl std::fmt::Debug for RdpSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RdpSettings")
            .field("enabled", &self.enabled)
            .field("password", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}
impl Default for RdpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            server: String::new(),
            port: 3389,
            username: String::new(),
            password: None,
            ignore_certificate_errors: false,
            domain: String::new(),
            retry_seconds: 10,
        }
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiSettings {
    pub connecting_text: String,
    pub disconnected_text: String,
    pub error_text: String,
    pub reconnecting_text: String,
    pub development_exit_enabled: bool,
}
impl Default for UiSettings {
    fn default() -> Self {
        Self {
            connecting_text: "Connexion en cours…".into(),
            disconnected_text: "Connexion interrompue".into(),
            error_text: "Connexion impossible".into(),
            reconnecting_text: "Nouvelle tentative dans {seconds} secondes".into(),
            development_exit_enabled: true,
        }
    }
}
impl Config {
    pub fn path() -> std::io::Result<PathBuf> {
        // Each Windows session uses its own profile, even with a shared shell EXE.
        let profile = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Profil utilisateur Windows indisponible",
                )
            })?;
        Ok(profile.join("config.toml"))
    }
    pub fn read() -> Result<Self, String> {
        let path = Self::path().map_err(|_| "Dossier de configuration indisponible")?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| "config.toml absent ou illisible")?
            .take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|_| "Lecture config.toml impossible")?;
        if bytes.len() > 65_536 {
            return Err("config.toml dépasse 64 Kio".into());
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| "config.toml doit être UTF-8")?;
        Self::parse(text)
    }
    fn parse(text: &str) -> Result<Self, String> {
        // Do not propagate TOML errors: their source excerpts can contain rejected secrets.
        let config: Self = toml::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|_| "config.toml invalide (syntaxe, type ou clé inconnue)")?;
        if config.rdp.port == 0 || !(1..=3600).contains(&config.rdp.retry_seconds) {
            return Err("Port ou intervalle de reconnexion invalide".into());
        }
        if let Some(password) = &config.rdp.password {
            if password.is_empty()
                || password.encode_utf16().count() > 512
                || password.contains('\0')
            {
                return Err("Mot de passe RDP invalide (vide, trop long ou caractère nul)".into());
            }
        }
        for field in [&config.rdp.server, &config.rdp.username, &config.rdp.domain] {
            if field.len() > 1024 || field.chars().any(char::is_control) {
                return Err("Paramètre RDP invalide".into());
            }
        }
        // When Manager provides the RDP connection settings, server/username
        // arrive at connect time (looked up by the Windows account name) and
        // are not required locally.
        if config.rdp.enabled
            && !config.manager.provides_rdp
            && (config.rdp.server.trim().is_empty() || config.rdp.username.trim().is_empty())
        {
            return Err("Serveur et utilisateur RDP requis".into());
        }
        if config.rdp.server.contains("://")
            || config.rdp.server.contains('@')
            || config.rdp.server.contains('/')
        {
            return Err(
                "rdp.server doit être un nom DNS ou une adresse IP, sans URL ni identifiants"
                    .into(),
            );
        }
        for text in [
            &config.ui.connecting_text,
            &config.ui.disconnected_text,
            &config.ui.error_text,
            &config.ui.reconnecting_text,
        ] {
            if text.is_empty() || text.len() > 2048 || text.contains('\0') {
                return Err("Texte UI invalide".into());
            }
        }
        if config.manager.enabled || config.manager.provides_rdp {
            if config.manager.host.trim().is_empty() || config.manager.port == 0 {
                return Err("Hôte et port NOC Manager requis (heartbeat ou RDP centralisé activé)".into());
            }
            if config.manager.host.len() > 1024 || config.manager.host.chars().any(char::is_control) {
                return Err("Hôte NOC Manager invalide".into());
            }
        }
        Ok(config)
    }
    pub fn details(&self, name: &str) -> String {
        format!(
            "Utilisateur Windows : {}\nRDP — utilisateur : {}   •   Serveur : {}:{}",
            name, self.rdp.username, self.rdp.server, self.rdp.port
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toml_accepts_password_without_exposing_it_in_errors_or_debug() {
        let config = Config::parse(include_str!("../config.example.toml")).unwrap();
        assert!(config.rdp.enabled);
        assert_eq!(config.rdp.retry_seconds, 10);
        let with_password = Config::parse("[rdp]\npassword='DO_NOT_ECHO'").unwrap();
        assert_eq!(with_password.rdp.password.as_deref(), Some("DO_NOT_ECHO"));
        assert!(!format!("{with_password:?}").contains("DO_NOT_ECHO"));
        let error = Config::parse("[rdp]\npassword='DO_NOT_ECHO'\nunknown=true").unwrap_err();
        assert!(!error.contains("DO_NOT_ECHO"));
        assert!(Config::parse("[rdp]\npassword=''").is_err());
        assert!(Config::parse("[rdp]\nenabled=true").is_err());
        assert!(Config::parse("[rdp]\nretry_seconds=0").is_err());
        assert!(Config::parse("bad toml!").is_err());
        assert!(!Config::parse("[rdp]\nenabled=false").unwrap().rdp.enabled);
    }
}
