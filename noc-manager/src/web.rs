use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;

use crate::commands;
use crate::dashboard;
use crate::models::{Kiosk, PasswordAction, RdpConfig};
use crate::AppState;

const BUILD_TIME: &str = env!("NOC_MANAGER_BUILD_TIME");

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(dashboard::home))
        .route("/kiosks", get(dashboard::kiosks))
        .route("/supervision", get(dashboard::supervision))
        .route("/rdp-servers", get(crate::rdp_servers::list))
        .route(
            "/rdp-servers/new",
            get(crate::rdp_servers::new_form).post(crate::rdp_servers::create),
        )
        .route(
            "/rdp-servers/:id/edit",
            get(crate::rdp_servers::edit).post(crate::rdp_servers::update),
        )
        .route("/ui/rdp-servers", post(crate::rdp_servers::inline_create))
        .route("/ui/status", get(dashboard::status))
        .route("/ui/history", get(dashboard::history))
        .route("/assets/manager.css", get(dashboard::css))
        .route("/assets/manager.js", get(dashboard::js))
        .route("/kiosk/new", get(new_form).post(create))
        .route("/kiosk/:username/edit", get(edit_form).post(update))
        .route("/kiosk/:username/delete", post(delete))
        .route("/kiosk/:username/restart-browser", post(restart_browser))
        .route("/kiosk/:username/restart-agent", post(restart_agent))
}

// ---------------------------------------------------------------- form input

#[derive(Debug, Deserialize)]
pub struct KioskForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    display_username: String,
    #[serde(default)]
    url: String,
    /// Checkboxes are only submitted when checked.
    #[serde(default)]
    enabled: Option<String>,
    #[serde(default)]
    restart_cron: String,
    #[serde(default)]
    rdp_enabled: Option<String>,
    #[serde(default)]
    rdp_server: String,
    #[serde(default)]
    rdp_server_id: String,
    #[serde(default)]
    rdp_port: String,
    #[serde(default)]
    rdp_ignore_certificate_errors: Option<String>,
    /// Blank = unchanged on edit (see `PasswordAction`); used directly on create.
    #[serde(default)]
    rdp_password: String,
    #[serde(default)]
    rdp_password_clear: Option<String>,
    #[serde(default)]
    rdp_use_local: Option<String>,
}

impl KioskForm {
    /// Resolves what to do with the centrally-stored RDP password on an
    /// update: an empty field alone never clears it, only the explicit
    /// checkbox does — see `PasswordAction`.
    fn password_action(&self) -> PasswordAction {
        if self.rdp_password_clear.is_some() {
            PasswordAction::Clear
        } else if !self.rdp_password.trim().is_empty() {
            PasswordAction::Set(self.rdp_password.trim().to_string())
        } else {
            PasswordAction::Keep
        }
    }
}

impl From<&KioskForm> for Kiosk {
    fn from(form: &KioskForm) -> Self {
        let mut kiosk = Kiosk {
            username: form.username.clone(),
            display_username: Some(form.display_username.clone()),
            name: form.name.clone(),
            url: form.url.clone(),
            enabled: form.enabled.is_some(),
            restart_cron: Some(form.restart_cron.clone()),
            rdp: RdpConfig {
                server_id: (!form.rdp_server_id.is_empty() && form.rdp_server_id != "__manual")
                    .then(|| form.rdp_server_id.clone()),
                enabled: form.rdp_enabled.is_some(),
                server: form.rdp_server.clone(),
                port: if form.rdp_port.trim().is_empty() {
                    3389
                } else {
                    form.rdp_port.trim().parse().unwrap_or(0)
                },
                // Authoritative on create (Storage::add takes the Kiosk as-is);
                // overwritten by Storage::update's PasswordAction merge on edit.
                password: (!form.rdp_password.trim().is_empty())
                    .then(|| form.rdp_password.trim().to_string()),
                ignore_certificate_errors: form.rdp_ignore_certificate_errors.is_some(),
            },
        };
        crate::models::normalize(&mut kiosk);
        kiosk
    }
}

// ----------------------------------------------------------------- handlers

#[derive(Default, Deserialize)]
pub struct Prefill {
    #[serde(default)]
    agent: String,
    #[serde(default)]
    display: String,
}

async fn new_form(State(state): State<Arc<AppState>>, Query(query): Query<Prefill>) -> Response {
    let display = query.display.trim().to_string();
    if !display.is_empty() {
        if let Some(existing) = state
            .storage
            .all()
            .iter()
            .find(|k| k.display_username() == display)
        {
            return Redirect::to(&format!("/kiosk/{}/edit", existing.username)).into_response();
        }
    }
    let from_display = !display.is_empty();
    let kiosk = Kiosk {
        username: query.agent,
        display_username: Some(display.clone()),
        name: display,
        url: String::new(),
        enabled: true,
        restart_cron: None,
        rdp: RdpConfig {
            enabled: from_display,
            ..RdpConfig::default()
        },
    };
    Html(page(
        if from_display {
            "Créer ce kiosque"
        } else {
            "Ajouter un kiosque"
        },
        &form_html(
            if from_display {
                "Créer ce kiosque"
            } else {
                "Ajouter un kiosque"
            },
            "/kiosk/new",
            Some(&kiosk),
            None,
            false,
            &state.rdp_servers.all(),
        ),
    ))
    .into_response()
}

async fn create(State(state): State<Arc<AppState>>, Form(form): Form<KioskForm>) -> Response {
    let mut kiosk = Kiosk::from(&form);
    if form.rdp_use_local.is_some() {
        kiosk.rdp.password = None;
    }
    let result =
        if kiosk.rdp.enabled && kiosk.rdp.password.is_none() && form.rdp_use_local.is_none() {
            Err(
                "Saisissez le mot de passe RDP ou choisissez les identifiants locaux du Display."
                    .into(),
            )
        } else {
            resolve_server(&state, &mut kiosk).and_then(|_| state.storage.add(kiosk.clone()))
        };
    match result {
        Ok(()) => Redirect::to("/kiosks").into_response(),
        Err(error) => Html(page(
            "Ajouter un kiosque",
            &form_html(
                "Ajouter un kiosque",
                "/kiosk/new",
                Some(&kiosk),
                Some(&error),
                form.rdp_use_local.is_some(),
                &state.rdp_servers.all(),
            ),
        ))
        .into_response(),
    }
}

async fn edit_form(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Query(query): Query<Prefill>,
) -> Response {
    match state.storage.get(&username).map(|mut k| {
        if !query.display.is_empty() {
            k.display_username = Some(query.display);
        }
        if !query.agent.is_empty() {
            // Associating a new Agent must not silently change the existing Display.
            if k.display_username.is_none() {
                k.display_username = Some(k.username.clone());
            }
            k.username = query.agent;
        }
        k
    }) {
        Some(kiosk) => Html(page(
            "Modifier le kiosque",
            &form_html(
                "Modifier le kiosque",
                &format!("/kiosk/{}/edit", esc(&username)),
                Some(&kiosk),
                None,
                false,
                &state.rdp_servers.all(),
            ),
        ))
        .into_response(),
        None => not_found(&username),
    }
}

async fn update(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Form(form): Form<KioskForm>,
) -> Response {
    let mut kiosk = Kiosk::from(&form);
    match resolve_server(&state, &mut kiosk).and_then(|_| {
        state
            .storage
            .update(&username, kiosk.clone(), form.password_action())
    }) {
        Ok(()) => Redirect::to("/kiosks").into_response(),
        Err(error) => Html(page(
            "Modifier le kiosque",
            &form_html(
                "Modifier le kiosque",
                &format!("/kiosk/{}/edit", esc(&username)),
                Some(&kiosk),
                Some(&error),
                false,
                &state.rdp_servers.all(),
            ),
        ))
        .into_response(),
    }
}

async fn delete(State(state): State<Arc<AppState>>, Path(username): Path<String>) -> Response {
    match state.storage.delete(&username) {
        Ok(()) => Redirect::to("/kiosks").into_response(),
        Err(error) => Html(page(
            "Erreur",
            &format!(
                r#"<h1>Erreur</h1><p class="error">{}</p><p><a class="btn" href="/">Retour</a></p>"#,
                esc(&error)
            ),
        ))
        .into_response(),
    }
}

async fn restart_browser(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Response {
    queue(&state, &username, "restart_browser")
}

async fn restart_agent(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Response {
    queue(&state, &username, "restart_agent")
}

/// Queues a command from the web UI, then back to the list with a flash message.
fn queue(state: &Arc<AppState>, username: &str, action: &str) -> Response {
    match commands::queue_command(&state.storage, &state.commands, username, action) {
        Ok(_) => Redirect::to(&format!("/kiosks?sent={action}")).into_response(),
        Err(error) => Html(page(
            "Erreur",
            &format!(
                r#"<h1>Erreur</h1><p class="error">{}</p><p><a class="btn" href="/">Retour</a></p>"#,
                esc(&error.message())
            ),
        ))
        .into_response(),
    }
}

fn not_found(username: &str) -> Response {
    (
        axum::http::StatusCode::NOT_FOUND,
        Html(page(
            "Introuvable",
            &format!(
                r#"<h1>Introuvable</h1><p>Aucun kiosque nommé <code>{}</code>.</p><p><a class="btn" href="/">Retour</a></p>"#,
                esc(username)
            ),
        )),
    )
        .into_response()
}

// ----------------------------------------------------------------- rendering

fn resolve_server(state: &AppState, kiosk: &mut Kiosk) -> Result<(), String> {
    if let Some(id) = &kiosk.rdp.server_id {
        let server = state
            .rdp_servers
            .get(id)
            .ok_or("Le serveur sélectionné n’existe plus. Choisissez un serveur du catalogue.")?;
        kiosk.rdp.server = server.address;
        kiosk.rdp.port = server.port;
        kiosk.rdp.ignore_certificate_errors = server.ignore_certificate_errors;
    }
    Ok(())
}

fn form_html(
    title: &str,
    action: &str,
    kiosk: Option<&Kiosk>,
    error: Option<&str>,
    use_local: bool,
    servers: &[crate::rdp_servers::RdpServer],
) -> String {
    let is_new = action == "/kiosk/new";
    let intro = if is_new {
        r#"<p class="notice">Sélectionnez un serveur RDP ou créez-en un ici, puis saisissez le compte Linux, l’URL et les identifiants RDP. L’Agent démarrera après l’ouverture de la session RDP.</p>"#
    } else {
        ""
    };
    let url_hint = if is_new && kiosk.map(|k| k.url.is_empty()).unwrap_or(true) {
        " — À compléter"
    } else {
        ""
    };
    let server_hint = if is_new && kiosk.map(|k| k.rdp.server.is_empty()).unwrap_or(true) {
        " — À compléter"
    } else {
        ""
    };
    let password_placeholder = if is_new {
        "Saisir le mot de passe du compte Linux"
    } else {
        "Laisser vide pour ne pas changer"
    };
    let local_choice = if is_new {
        format!(
            r#"<label class="check"><input type="checkbox" name="rdp_use_local" value="1"{}> Utiliser les identifiants déjà enregistrés sur le Display</label><p class="muted">Sans cette option, le mot de passe du compte Linux est à compléter.</p>"#,
            if use_local { " checked" } else { "" }
        )
    } else {
        String::new()
    };
    let name = kiosk.map(|k| esc(&k.name)).unwrap_or_default();
    let username = kiosk.map(|k| esc(&k.username)).unwrap_or_default();
    let display_username = kiosk
        .and_then(|k| k.display_username.as_deref())
        .map(esc)
        .unwrap_or_default();
    let url = kiosk.map(|k| esc(&k.url)).unwrap_or_default();
    let checked = match kiosk {
        Some(k) if k.enabled => " checked",
        Some(_) => "",
        // New kiosks are enabled by default.
        None => " checked",
    };
    let cron = kiosk
        .and_then(|k| k.restart_cron.as_deref())
        .map(esc)
        .unwrap_or_default();

    let rdp_checked = if kiosk.map(|k| k.rdp.enabled).unwrap_or(false) {
        " checked"
    } else {
        ""
    };
    let rdp_server = kiosk.map(|k| esc(&k.rdp.server)).unwrap_or_default();
    let selected_server = kiosk.and_then(|k| k.rdp.server_id.as_deref());
    let manual_selected = selected_server.is_none() && !rdp_server.is_empty();
    let mut server_options = String::from("<option value=\"\">Choisir un serveur…</option>");
    for server in servers {
        server_options.push_str(&format!(
            r#"<option value="{}"{}>{} — {}:{}</option>"#,
            esc(&server.id),
            if selected_server == Some(server.id.as_str()) {
                " selected"
            } else {
                ""
            },
            esc(&server.name),
            esc(&server.address),
            server.port
        ));
    }
    if let Some(id) = selected_server {
        if !servers.iter().any(|s| s.id == id) {
            server_options.push_str(&format!(r#"<option value="{}" selected>Serveur introuvable — choisissez une destination</option>"#,esc(id)));
        }
    }
    server_options.push_str(&format!(
        r#"<option value="__manual"{}>Adresse spécifique à ce kiosque</option>"#,
        if manual_selected { " selected" } else { "" }
    ));
    let rdp_port = kiosk.map(|k| k.rdp.port).unwrap_or(3389);
    let rdp_ignore_checked = if kiosk
        .map(|k| k.rdp.ignore_certificate_errors)
        .unwrap_or(false)
    {
        " checked"
    } else {
        ""
    };
    let has_password = !is_new && kiosk.map(|k| k.rdp.password.is_some()).unwrap_or(false);
    let password_status = if is_new && !use_local {
        r#"<span class="badge pending">À compléter</span>"#
    } else if has_password {
        r#"<span class="badge on">mot de passe centralisé défini</span>"#
    } else {
        r#"<span class="badge off">aucun — repli sur DPAPI local</span>"#
    };
    let clear_row = if has_password {
        r#"<label class="check">
    <input type="checkbox" name="rdp_password_clear" value="1"> Supprimer le mot de passe centralisé
  </label>"#
    } else {
        ""
    };

    let error_html = match error {
        Some(e) => format!(r#"<p class="error">{}</p>"#, esc(e)),
        None => String::new(),
    };

    format!(
        r#"<h1>{title}</h1>
{intro}
{error_html}
<form method="post" action="{action}" class="card">
  <fieldset>
    <legend>Général</legend>
    <label>Nom du kiosque
      <input type="text" name="name" value="{name}" required>
    </label>
    <label>Session Agent (compte Linux)
      <input type="text" name="username" value="{username}" placeholder="Compte Linux à saisir" required>
    </label>
    <label>Session Display (compte Windows)
      <input type="text" name="display_username" value="{display_username}" list="display-names" placeholder="Même nom que la session Agent si vide">
    </label>
    <datalist id="display-names"></datalist>
    <p class="muted">Le Display utilise son compte Windows pour récupérer la configuration. La connexion RDP utilise le compte Linux de l’Agent.</p>
    <label>URL à afficher{url_hint}
      <input type="text" name="url" value="{url}" placeholder="https://example.com" required>
    </label>
    <label class="check">
      <input type="checkbox" name="enabled" value="1"{checked}> Activé
    </label>
  </fieldset>
  <fieldset>
    <legend>Planification du redémarrage</legend>
    <label>Expression cron (facultative)
      <input type="text" name="restart_cron" value="{cron}" placeholder="0 4 * * *">
    </label>
    <div class="help">
      <code>0 4 * * *</code> = chaque jour à 04:00<br>
      <code>30 3 * * 1</code> = chaque lundi à 03:30<br>
      <code>0 */6 * * *</code> = toutes les 6 heures<br>
      Laisser vide pour désactiver la planification. Seul Firefox est redémarré.
    </div>
  </fieldset>
  <fieldset>
    <legend>RDP (NOC Display)</legend>
    <label class="check">
      <input type="checkbox" name="rdp_enabled" value="1"{rdp_checked}> Gérer la connexion RDP depuis Manager
    </label>
    <label>Serveur RDP<select name="rdp_server_id" id="rdp-server-select">{server_options}</select></label>
    <p class="muted"><a href="/rdp-servers" target="_blank" rel="noopener">Gérer les serveurs RDP</a></p>
    <details id="inline-server"><summary class="btn">+ Créer un serveur ici</summary>
      <div class="help">
        <label>Nom du serveur<input type="text" id="inline-server-name" placeholder="Linux NOC principal"></label>
        <label>Adresse IP ou nom DNS<input type="text" id="inline-server-address" placeholder="192.168.10.50"></label>
        <label>Port<input type="text" id="inline-server-port" value="3389"></label>
        <label class="check"><input type="checkbox" id="inline-server-certificate"> Ignorer les erreurs de certificat</label>
        <button type="button" class="btn" id="inline-server-save">Enregistrer et sélectionner ce serveur</button>
        <p id="inline-server-status" role="status"></p>
      </div>
    </details>
    <div id="rdp-manual-fields">
    <label>Serveur Linux (RDP){server_hint}
      <input type="text" name="rdp_server" value="{rdp_server}" placeholder="192.168.10.50">
    </label>
    <label>Port
      <input type="text" name="rdp_port" value="{rdp_port}" placeholder="3389">
    </label>
    <label class="check">
      <input type="checkbox" name="rdp_ignore_certificate_errors" value="1"{rdp_ignore_checked}> Ignorer les erreurs de certificat
    </label>
    </div>
    <label>Mot de passe {password_status}
      <input type="password" name="rdp_password" placeholder="{password_placeholder}" autocomplete="new-password">
    </label>
    {local_choice}
    {clear_row}
    <div class="help">
      Le compte de connexion RDP est la session Agent ci-dessus. Sans mot de passe
      centralisé, Display utilise son identifiant DPAPI local
      (<code>--set-credentials</code>) pour ce kiosque.
    </div>
  </fieldset>
  <div class="actions">
    <button class="btn primary" type="submit">Enregistrer</button>
    <a class="btn" href="/kiosks">Annuler</a>
  </div>
</form>"#
    )
}

pub(crate) fn page(title: &str, body: &str) -> String {
    let version = crate::suite_version::suite_version();
    format!(
        r#"<!DOCTYPE html><html lang="fr"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>{title} · NOC Manager</title>
<link rel="stylesheet" href="/assets/manager.css"><script defer src="/assets/manager.js"></script></head>
<body><header class="topbar"><a class="brand" href="/"><span class="brand-mark">N</span><span>NOC <strong>Manager</strong></span></a>
<nav aria-label="Navigation principale"><a href="/">Accueil</a><a href="/kiosks">Kiosques</a><a href="/supervision">Supervision</a><a href="/rdp-servers">Serveurs RDP</a></nav>
<span class="version">v{version}<small>Build {BUILD_TIME}</small></span></header>
<div class="summary-wrap"><div id="summary" class="summary" aria-label="Vue d’ensemble"></div></div>
<main><div class="sync-line"><span class="live-dot"></span><span id="sync-status" role="status">Connexion au Manager…</span></div>
<div id="storage-warning" role="alert"></div>{body}</main>
<footer>Norfair Operation Center <span>Supervision Agent &amp; Display</span></footer></body></html>"#
    )
}

/// Minimal HTML escaping for user-provided values.
pub(crate) fn esc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}
