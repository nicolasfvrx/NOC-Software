use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kiosk {
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_username: Option<String>,
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

impl Kiosk {
    pub fn display_username(&self) -> &str {
        self.display_username
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&self.username)
    }
}

/// Centralized RDP connection settings, served to NOC Display via
/// `GET /api/kiosk/{display_username}/rdp`. The returned login username is
/// `Kiosk.username` (Linux), which can differ from the Display lookup identity.
#[derive(Clone, Serialize, Deserialize)]
pub struct RdpConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
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
            server_id: None,
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
        return Err("La session Agent est obligatoire".to_string());
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(
            "Session Agent : seuls lettres, chiffres, tirets, points et underscores sont acceptés"
                .to_string(),
        );
    }

    let is_rename = previous_username.map(|p| p != username).unwrap_or(true);
    if is_rename && existing.iter().any(|k| k.username == username) {
        return Err(format!("La session Agent '{username}' est déjà configurée"));
    }

    if kiosk.name.trim().is_empty() {
        return Err("Le nom du kiosque est obligatoire".to_string());
    }

    let display = kiosk.display_username();
    if !display
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(
            "Nom Display : seuls lettres, chiffres, tirets, points et underscores sont acceptés"
                .into(),
        );
    }
    if existing
        .iter()
        .any(|k| Some(k.username.as_str()) != previous_username && k.display_username() == display)
    {
        return Err(format!(
            "Le Display '{display}' est déjà associé à un kiosque"
        ));
    }

    let url = kiosk.url.trim();
    if url.is_empty() {
        return Err("L’URL est obligatoire".to_string());
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("L’URL doit commencer par http:// ou https://".to_string());
    }

    if let Some(cron) = kiosk.restart_cron.as_deref() {
        if !cron.trim().is_empty() {
            validate_cron(cron)?;
        }
    }

    if kiosk.rdp.enabled {
        let server = kiosk.rdp.server.trim();
        if server.is_empty() || kiosk.rdp.port == 0 {
            return Err("RDP activé : indiquez un serveur et un port entre 1 et 65535".to_string());
        }
        if server.contains("://") || server.contains('@') || server.contains('/') {
            return Err(
                "Le serveur RDP doit être un nom DNS ou une adresse IP, sans schéma URL ni identifiants"
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
            "La planification exige 5 champs (minute heure jour mois jour-de-semaine), {} reçu(s)",
            fields.len()
        ));
    }
    for field in fields {
        if !field
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '*' | '/' | ',' | '-'))
        {
            return Err(format!("Caractère invalide dans le champ cron '{field}'"));
        }
    }
    Ok(())
}

/// Normalizes user input coming from the web form.
pub fn normalize(kiosk: &mut Kiosk) {
    kiosk.username = kiosk.username.trim().to_string();
    kiosk.display_username = kiosk
        .display_username
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    kiosk.name = kiosk.name.trim().to_string();
    kiosk.url = kiosk.url.trim().to_string();
    kiosk.restart_cron = match kiosk.restart_cron.as_deref() {
        Some(c) if !c.trim().is_empty() => Some(c.split_whitespace().collect::<Vec<_>>().join(" ")),
        _ => None,
    };
    kiosk.rdp.server = kiosk.rdp.server.trim().to_string();
}
