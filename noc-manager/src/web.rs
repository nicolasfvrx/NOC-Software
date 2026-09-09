use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;

use crate::commands;
use crate::health::{self, Heartbeat};
use crate::models::{Kiosk, PasswordAction, RdpConfig};
use crate::AppState;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const BUILD_TIME: &str = env!("NOC_MANAGER_BUILD_TIME");

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(index))
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
    rdp_port: String,
    #[serde(default)]
    rdp_ignore_certificate_errors: Option<String>,
    /// Blank = unchanged on edit (see `PasswordAction`); used directly on create.
    #[serde(default)]
    rdp_password: String,
    #[serde(default)]
    rdp_password_clear: Option<String>,
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
            name: form.name.clone(),
            url: form.url.clone(),
            enabled: form.enabled.is_some(),
            restart_cron: Some(form.restart_cron.clone()),
            rdp: RdpConfig {
                enabled: form.rdp_enabled.is_some(),
                server: form.rdp_server.clone(),
                port: form.rdp_port.trim().parse().unwrap_or(3389),
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

/// `?sent=restart_browser` after a command has been queued.
#[derive(Debug, Deserialize)]
pub struct IndexQuery {
    #[serde(default)]
    sent: Option<String>,
}

async fn index(
    State(state): State<Arc<AppState>>,
    Query(query): Query<IndexQuery>,
) -> Html<String> {
    let kiosks = state.storage.all();
    let pending = state.commands.all_pending();

    let rows = if kiosks.is_empty() {
        r#"<tr><td colspan="9" class="empty">No kiosk yet.</td></tr>"#.to_string()
    } else {
        kiosks
            .iter()
            .map(|k| {
                let user = esc(&k.username);
                let enabled = if k.enabled {
                    r#"<span class="badge on">enabled</span>"#
                } else {
                    r#"<span class="badge off">disabled</span>"#
                };
                let cron = match k.restart_cron.as_deref() {
                    Some(c) if !c.is_empty() => format!("<code>{}</code>", esc(c)),
                    _ => r#"<span class="muted">-</span>"#.to_string(),
                };
                let command = match pending.get(&k.username) {
                    Some(c) => format!(
                        r#"<span class="badge cmd">{} (#{})</span>"#,
                        esc(&c.action),
                        c.id
                    ),
                    None => r#"<span class="muted">aucune</span>"#.to_string(),
                };
                let agent = health_badge(state.health.get("agent", &k.username), state.health_stale_after_seconds);
                let display = health_badge(state.health.get("display", &k.username), state.health_stale_after_seconds);
                format!(
                    r#"<tr>
      <td>{name}</td>
      <td><code>{user}</code></td>
      <td class="url"><a href="{url}" target="_blank" rel="noreferrer">{url}</a></td>
      <td>{enabled}</td>
      <td>{cron}</td>
      <td>{command}</td>
      <td>{agent}</td>
      <td>{display}</td>
      <td class="actions">
        <a class="btn" href="/kiosk/{user}/edit">Edit</a>
        <form method="post" action="/kiosk/{user}/restart-browser">
          <button class="btn" type="submit">Redémarrer Firefox</button>
        </form>
        <form method="post" action="/kiosk/{user}/restart-agent" onsubmit="return confirm('Voulez-vous vraiment redémarrer complètement l&#39;agent de ce kiosk ?')">
          <button class="btn" type="submit">Redémarrer l&#39;agent</button>
        </form>
        <form method="post" action="/kiosk/{user}/delete" onsubmit="return confirm('Delete this kiosk?')">
          <button class="btn danger" type="submit">Delete</button>
        </form>
      </td>
    </tr>"#,
                    name = esc(&k.name),
                    url = esc(&k.url),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let flash = match query.sent.as_deref() {
        Some(action) if commands::is_allowed_action(action) => format!(
            r#"<p class="flash">Commande envoyée : <code>{}</code></p>"#,
            esc(action)
        ),
        _ => String::new(),
    };

    let body = format!(
        r#"<div class="head">
  <h1>Kiosks</h1>
  <a class="btn primary" href="/kiosk/new">Add kiosk</a>
</div>
{flash}
<div class="table-wrap">
<table>
  <thead>
    <tr>
      <th colspan="5" class="group">Kiosque</th>
      <th colspan="3" class="group">Supervision</th>
      <th rowspan="2"></th>
    </tr>
    <tr>
      <th>Name</th><th>Username</th><th>URL</th><th>Enabled</th><th>Restart schedule</th>
      <th>Commande</th><th>Agent</th><th>Display</th>
    </tr>
  </thead>
  <tbody>
{rows}
  </tbody>
</table>
</div>"#
    );

    Html(page("Kiosks", &body))
}

async fn new_form() -> Html<String> {
    Html(page(
        "Add kiosk",
        &form_html("Add kiosk", "/kiosk/new", None, None),
    ))
}

async fn create(State(state): State<Arc<AppState>>, Form(form): Form<KioskForm>) -> Response {
    let kiosk = Kiosk::from(&form);
    match state.storage.add(kiosk.clone()) {
        Ok(()) => Redirect::to("/").into_response(),
        Err(error) => Html(page(
            "Add kiosk",
            &form_html("Add kiosk", "/kiosk/new", Some(&kiosk), Some(&error)),
        ))
        .into_response(),
    }
}

async fn edit_form(State(state): State<Arc<AppState>>, Path(username): Path<String>) -> Response {
    match state.storage.get(&username) {
        Some(kiosk) => Html(page(
            "Edit kiosk",
            &form_html(
                "Edit kiosk",
                &format!("/kiosk/{}/edit", esc(&username)),
                Some(&kiosk),
                None,
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
    let kiosk = Kiosk::from(&form);
    match state.storage.update(&username, kiosk.clone(), form.password_action()) {
        Ok(()) => Redirect::to("/").into_response(),
        Err(error) => Html(page(
            "Edit kiosk",
            &form_html(
                "Edit kiosk",
                &format!("/kiosk/{}/edit", esc(&username)),
                Some(&kiosk),
                Some(&error),
            ),
        ))
        .into_response(),
    }
}

async fn delete(State(state): State<Arc<AppState>>, Path(username): Path<String>) -> Response {
    match state.storage.delete(&username) {
        Ok(()) => Redirect::to("/").into_response(),
        Err(error) => Html(page(
            "Error",
            &format!(
                r#"<h1>Error</h1><p class="error">{}</p><p><a class="btn" href="/">Back</a></p>"#,
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
        Ok(_) => Redirect::to(&format!("/?sent={action}")).into_response(),
        Err(error) => Html(page(
            "Error",
            &format!(
                r#"<h1>Error</h1><p class="error">{}</p><p><a class="btn" href="/">Back</a></p>"#,
                esc(&error.message())
            ),
        ))
        .into_response(),
    }
}

/// Renders a compact "last seen" badge from the latest heartbeat, if any.
fn health_badge(heartbeat: Option<Heartbeat>, stale_after_seconds: u64) -> String {
    let Some(heartbeat) = heartbeat else {
        return r#"<span class="badge off">jamais vu</span>"#.to_string();
    };
    let now = health::now_unix();
    let age = now.saturating_sub(heartbeat.last_seen);
    let class = if health::is_up(&heartbeat, now, stale_after_seconds) {
        "on"
    } else {
        "off"
    };
    format!(
        r#"<span class="badge {class}" title="v{version} — Build {build} — {state}">{age}s</span>"#,
        version = esc(&heartbeat.version),
        build = esc(&heartbeat.build),
        state = esc(&heartbeat.state),
    )
}

fn not_found(username: &str) -> Response {
    (
        axum::http::StatusCode::NOT_FOUND,
        Html(page(
            "Not found",
            &format!(
                r#"<h1>Not found</h1><p>No kiosk named <code>{}</code>.</p><p><a class="btn" href="/">Back</a></p>"#,
                esc(username)
            ),
        )),
    )
        .into_response()
}

// ----------------------------------------------------------------- rendering

fn form_html(title: &str, action: &str, kiosk: Option<&Kiosk>, error: Option<&str>) -> String {
    let name = kiosk.map(|k| esc(&k.name)).unwrap_or_default();
    let username = kiosk.map(|k| esc(&k.username)).unwrap_or_default();
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
    let rdp_port = kiosk.map(|k| k.rdp.port).unwrap_or(3389);
    let rdp_ignore_checked = if kiosk.map(|k| k.rdp.ignore_certificate_errors).unwrap_or(false) {
        " checked"
    } else {
        ""
    };
    let has_password = kiosk.map(|k| k.rdp.password.is_some()).unwrap_or(false);
    let password_status = if has_password {
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
{error_html}
<form method="post" action="{action}" class="card">
  <fieldset>
    <legend>Général</legend>
    <label>Name
      <input type="text" name="name" value="{name}" required>
    </label>
    <label>Username (Linux account)
      <input type="text" name="username" value="{username}" required>
    </label>
    <label>URL
      <input type="text" name="url" value="{url}" placeholder="https://example.com" required>
    </label>
    <label class="check">
      <input type="checkbox" name="enabled" value="1"{checked}> Enabled
    </label>
  </fieldset>
  <fieldset>
    <legend>Planification du redémarrage</legend>
    <label>Restart cron (optional)
      <input type="text" name="restart_cron" value="{cron}" placeholder="0 4 * * *">
    </label>
    <div class="help">
      <code>0 4 * * *</code> = every day at 04:00<br>
      <code>30 3 * * 1</code> = every Monday at 03:30<br>
      <code>0 */6 * * *</code> = every 6 hours<br>
      Leave empty for no scheduled restart. Only Firefox is restarted.
    </div>
  </fieldset>
  <fieldset>
    <legend>RDP (NOC Display)</legend>
    <label class="check">
      <input type="checkbox" name="rdp_enabled" value="1"{rdp_checked}> Gérer la connexion RDP depuis Manager
    </label>
    <label>Serveur RDP
      <input type="text" name="rdp_server" value="{rdp_server}" placeholder="192.168.10.50">
    </label>
    <label>Port
      <input type="text" name="rdp_port" value="{rdp_port}" placeholder="3389">
    </label>
    <label class="check">
      <input type="checkbox" name="rdp_ignore_certificate_errors" value="1"{rdp_ignore_checked}> Ignorer les erreurs de certificat
    </label>
    <label>Mot de passe {password_status}
      <input type="password" name="rdp_password" placeholder="Laisser vide pour ne pas changer" autocomplete="new-password">
    </label>
    {clear_row}
    <div class="help">
      L'utilisateur RDP est le même que le username ci-dessus. Le compte
      Windows du poste NOC Display doit porter exactement ce nom pour que
      la configuration soit retrouvée automatiquement. Sans mot de passe
      centralisé, Display utilise son identifiant DPAPI local
      (<code>--set-credentials</code>) pour ce kiosque.
    </div>
  </fieldset>
  <div class="actions">
    <button class="btn primary" type="submit">Save</button>
    <a class="btn" href="/">Cancel</a>
  </div>
</form>"#
    )
}

fn page(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="application-name" content="NOC Manager">
<meta name="description" content="Central kiosk administration for Norfair Operation Center">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - NOC Manager</title>
<style>{CSS}</style>
</head>
<body>
<header>
  <a href="/">NOC Manager</a>
  <span class="version">v{VERSION} • Build {BUILD_TIME}</span>
</header>
<main>
{body}
</main>
</body>
</html>"#
    )
}

const CSS: &str = r#"
*, *::before, *::after { box-sizing: border-box; }
body { margin: 0; background: #f4f5f7; color: #1f2328;
       font: 15px/1.55 -apple-system, Segoe UI, Roboto, Helvetica, Arial, sans-serif; }
header { background: #1f2328; padding: 14px 24px;
         display: flex; align-items: center; justify-content: space-between; }
header a { color: #fff; font-weight: 600; text-decoration: none; letter-spacing: .3px; }
header .version { color: #8b949e; font-size: 12px; font-family: monospace; }
main { max-width: 1240px; margin: 28px auto; padding: 0 16px; }
h1 { font-size: 21px; margin: 0; font-weight: 650; }
.head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 18px; }

/* -------------------------------------------------------------- table --- */
/* Wide content (many actions per row) scrolls in its own box; the page
   itself never scrolls horizontally. */
.table-wrap { overflow-x: auto; border-radius: 8px; border: 1px solid #d8dbe0;
              box-shadow: 0 1px 2px rgba(31, 35, 40, .04); }
table { width: 100%; min-width: 900px; border-collapse: collapse; background: #fff; }
th, td { text-align: left; padding: 10px 14px; border-bottom: 1px solid #eceef1; vertical-align: middle; }
th { background: #fafbfc; font-size: 12px; text-transform: uppercase; letter-spacing: .4px; color: #57606a; }
th.group { background: #eef0f3; color: #6e7781; font-size: 11px; text-align: center;
           letter-spacing: .6px; padding: 6px 14px; border-bottom: 1px solid #e1e4e8; }
thead tr:first-child th { border-bottom: 1px solid #e1e4e8; }
tr > *:nth-child(6) { border-left: 1px solid #e9ebef; }
tr > *:nth-child(9) { border-left: 1px solid #e9ebef; }
tbody tr:hover { background: #fafbfc; }
tr:last-child td { border-bottom: 0; }
td.url { max-width: 300px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
td.actions { white-space: nowrap; text-align: right; }
td.actions form { display: inline; }
code { background: #eceef1; padding: 1px 5px; border-radius: 4px; font-size: 13px; }
.muted { color: #8c959f; }
.empty { text-align: center; color: #8c959f; padding: 28px; }

/* -------------------------------------------------------------- badges --- */
.badge { font-size: 12px; padding: 2px 8px; border-radius: 10px; white-space: nowrap; }
.badge.on { background: #dafbe1; color: #116329; }
.badge.off { background: #ffebe9; color: #82071e; }
.badge.cmd { background: #fff4d6; color: #7a4b00; }
.flash { background: #dafbe1; border: 1px solid #aceebb; color: #116329;
         padding: 9px 14px; border-radius: 8px; margin: 0 0 18px; }

/* ---------------------------------------------------------------- btn --- */
.btn { display: inline-block; padding: 6px 12px; border: 1px solid #d8dbe0; border-radius: 6px;
       background: #fff; color: #1f2328; text-decoration: none; font-size: 14px; cursor: pointer; }
.btn:hover { background: #f4f5f7; }
.btn.primary { background: #1f6feb; border-color: #1f6feb; color: #fff; }
.btn.primary:hover { background: #1a5fd0; }
.btn.danger { color: #82071e; }
.btn.danger:hover { background: #ffebe9; }

/* --------------------------------------------------------- card / form --- */
.card { background: #fff; border: 1px solid #d8dbe0; border-radius: 8px; padding: 8px 24px 24px;
        max-width: 600px; box-shadow: 0 1px 2px rgba(31, 35, 40, .04); }
.card fieldset { border: 0; border-top: 1px solid #eceef1; margin: 0; padding: 18px 0; }
.card fieldset:first-of-type { border-top: 0; }
.card legend { padding: 0; font-size: 12px; font-weight: 650; text-transform: uppercase;
               letter-spacing: .5px; color: #57606a; margin-bottom: 12px; }
.card label { display: block; margin-bottom: 14px; font-size: 13px; color: #57606a; }
.card label:last-child { margin-bottom: 0; }
.card label.check { display: flex; align-items: center; gap: 8px; color: #1f2328; font-size: 15px; }
.card input[type=text], .card input[type=password] {
       display: block; width: 100%; margin-top: 4px; padding: 7px 10px;
       border: 1px solid #d8dbe0; border-radius: 6px; font-size: 15px; color: #1f2328; }
.card .actions { display: flex; gap: 8px; margin-top: 20px; }
.help { background: #f6f8fa; border: 1px solid #eceef1; border-radius: 6px;
        padding: 10px 12px; margin-top: 4px; font-size: 13px; color: #57606a; }
.error { background: #ffebe9; border: 1px solid #ffc1bc; color: #82071e;
         padding: 10px 12px; border-radius: 6px; max-width: 600px; }
"#;

/// Minimal HTML escaping for user-provided values.
fn esc(input: &str) -> String {
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
