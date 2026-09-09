use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kiosk {
    pub username: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub enabled: bool,
    /// `None` (or JSON `null`) means: no scheduled restart.
    #[serde(default)]
    pub restart_cron: Option<String>,
    /// RDP connection settings for the NOC Display instance(s) showing this
    /// kiosk. Absent in older kiosks.json entries: defaults to disabled.
    #[serde(default)]
    pub rdp: RdpConfig,
}

/// Centralized RDP connection settings, served to NOC Display via
/// `GET /api/kiosk/{username}/rdp`. `username` is not duplicated here:
/// the same `Kiosk.username` is both the Linux/browser-kiosk account and
/// the RDP login username, since they're the same account.
#[derive(Clone, Serialize, Deserialize)]
pub struct RdpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub server: String,
    #[serde(default = "default_rdp_port")]
    pub port: u16,
    /// `None` = no centrally-managed password; NOC Display falls back to
    /// its local DPAPI-encrypted secret for this kiosk.
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub ignore_certificate_errors: bool,
}

fn default_rdp_port() -> u16 {
    3389
}

/// What to do with the centrally-stored RDP password on an update, decided
/// by the web form: an empty password input never means "clear" by itself
/// (that needs the explicit checkbox), to avoid an admin accidentally
/// wiping a password by leaving the field blank.
#[derive(Debug, Clone)]
pub enum PasswordAction {
    Keep,
    Set(String),
    Clear,
}

impl Default for RdpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server: String::new(),
            port: default_rdp_port(),
            password: None,
            ignore_certificate_errors: false,
        }
    }
}

impl std::fmt::Debug for RdpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RdpConfig")
            .field("enabled", &self.enabled)
            .field("server", &self.server)
            .field("port", &self.port)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .field("ignore_certificate_errors", &self.ignore_certificate_errors)
            .finish()
    }
}

/// Validates a kiosk. `existing` is the full list, used for the uniqueness check.
/// `previous_username` is the username being edited (allowed to keep its own name).
pub fn validate(
    kiosk: &Kiosk,
    existing: &[Kiosk],
    previous_username: Option<&str>,
) -> Result<(), String> {
    let username = kiosk.username.trim();
    if username.is_empty() {
        return Err("username is required".to_string());
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err("username may only contain letters, digits, '-', '_' and '.'".to_string());
    }

    let is_rename = previous_username.map(|p| p != username).unwrap_or(true);
    if is_rename && existing.iter().any(|k| k.username == username) {
        return Err(format!("username '{username}' already exists"));
    }

    if kiosk.name.trim().is_empty() {
        return Err("name is required".to_string());
    }

    let url = kiosk.url.trim();
    if url.is_empty() {
        return Err("url is required".to_string());
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("url must start with http:// or https://".to_string());
    }

    if let Some(cron) = kiosk.restart_cron.as_deref() {
        if !cron.trim().is_empty() {
            validate_cron(cron)?;
        }
    }

    if kiosk.rdp.enabled {
        let server = kiosk.rdp.server.trim();
        if server.is_empty() || kiosk.rdp.port == 0 {
            return Err("rdp.server and rdp.port are required when RDP is enabled".to_string());
        }
        if server.contains("://") || server.contains('@') || server.contains('/') {
            return Err(
                "rdp.server must be a DNS name or IP address, without a URL scheme or credentials"
                    .to_string(),
            );
        }
    }

    Ok(())
}

/// Minimal 5-field cron check: `minute hour day-of-month month day-of-week`.
pub fn validate_cron(expr: &str) -> Result<(), String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(format!(
            "restart_cron must have 5 fields (minute hour day-of-month month day-of-week), got {}",
            fields.len()
        ));
    }
    for field in fields {
        if !field
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '*' | '/' | ',' | '-'))
        {
            return Err(format!("invalid character in cron field '{field}'"));
        }
    }
    Ok(())
}

/// Normalizes user input coming from the web form.
pub fn normalize(kiosk: &mut Kiosk) {
    kiosk.username = kiosk.username.trim().to_string();
    kiosk.name = kiosk.name.trim().to_string();
    kiosk.url = kiosk.url.trim().to_string();
    kiosk.restart_cron = match kiosk.restart_cron.as_deref() {
        Some(c) if !c.trim().is_empty() => Some(c.split_whitespace().collect::<Vec<_>>().join(" ")),
        _ => None,
    };
    kiosk.rdp.server = kiosk.rdp.server.trim().to_string();
}
