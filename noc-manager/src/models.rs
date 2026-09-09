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
}
