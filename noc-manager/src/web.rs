use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;

use crate::commands;
use crate::models::Kiosk;
use crate::AppState;

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
}

impl From<&KioskForm> for Kiosk {
    fn from(form: &KioskForm) -> Self {
        let mut kiosk = Kiosk {
            username: form.username.clone(),
            name: form.name.clone(),
            url: form.url.clone(),
            enabled: form.enabled.is_some(),
            restart_cron: Some(form.restart_cron.clone()),
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
        r#"<tr><td colspan="7" class="empty">No kiosk yet.</td></tr>"#.to_string()
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
                format!(
                    r#"<tr>
      <td>{name}</td>
      <td><code>{user}</code></td>
      <td class="url"><a href="{url}" target="_blank" rel="noreferrer">{url}</a></td>
      <td>{enabled}</td>
      <td>{cron}</td>
      <td>{command}</td>
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
<table>
  <thead>
    <tr><th>Name</th><th>Username</th><th>URL</th><th>Enabled</th><th>Restart schedule</th><th>Commande</th><th></th></tr>
  </thead>
  <tbody>
{rows}
  </tbody>
</table>"#
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
    match state.storage.update(&username, kiosk.clone()) {
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

    let error_html = match error {
        Some(e) => format!(r#"<p class="error">{}</p>"#, esc(e)),
        None => String::new(),
    };

    format!(
        r#"<h1>{title}</h1>
{error_html}
<form method="post" action="{action}" class="card">
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
  <label>Restart cron (optional)
    <input type="text" name="restart_cron" value="{cron}" placeholder="0 4 * * *">
  </label>
  <div class="help">
    <code>0 4 * * *</code> = every day at 04:00<br>
    <code>30 3 * * 1</code> = every Monday at 03:30<br>
    <code>0 */6 * * *</code> = every 6 hours<br>
    Leave empty for no scheduled restart. Only Firefox is restarted.
  </div>
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
<header><a href="/">NOC Manager</a></header>
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
       font: 15px/1.5 -apple-system, Segoe UI, Roboto, Helvetica, Arial, sans-serif; }
header { background: #1f2328; padding: 12px 24px; }
header a { color: #fff; font-weight: 600; text-decoration: none; letter-spacing: .3px; }
main { max-width: 1000px; margin: 24px auto; padding: 0 16px; }
h1 { font-size: 20px; margin: 0; }
.head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 16px; }
table { width: 100%; border-collapse: collapse; background: #fff;
        border: 1px solid #d8dbe0; border-radius: 6px; overflow: hidden; }
th, td { text-align: left; padding: 10px 12px; border-bottom: 1px solid #eceef1; vertical-align: middle; }
th { background: #fafbfc; font-size: 12px; text-transform: uppercase; letter-spacing: .4px; color: #57606a; }
tr:last-child td { border-bottom: 0; }
td.url { max-width: 320px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
td.actions { white-space: nowrap; text-align: right; }
td.actions form { display: inline; }
code { background: #eceef1; padding: 1px 5px; border-radius: 4px; font-size: 13px; }
.muted { color: #8c959f; }
.empty { text-align: center; color: #8c959f; padding: 24px; }
.badge { font-size: 12px; padding: 2px 8px; border-radius: 10px; }
.badge.on { background: #dafbe1; color: #116329; }
.badge.off { background: #ffebe9; color: #82071e; }
.badge.cmd { background: #fff4d6; color: #7a4b00; }
.flash { background: #dafbe1; border: 1px solid #aceebb; color: #116329;
         padding: 8px 12px; border-radius: 6px; margin: 0 0 16px; }
.btn { display: inline-block; padding: 6px 12px; border: 1px solid #d8dbe0; border-radius: 6px;
       background: #fff; color: #1f2328; text-decoration: none; font-size: 14px; cursor: pointer; }
.btn:hover { background: #f4f5f7; }
.btn.primary { background: #1f6feb; border-color: #1f6feb; color: #fff; }
.btn.primary:hover { background: #1a5fd0; }
.btn.danger { color: #82071e; }
.btn.danger:hover { background: #ffebe9; }
.card { background: #fff; border: 1px solid #d8dbe0; border-radius: 6px; padding: 20px; max-width: 560px; }
.card label { display: block; margin-bottom: 14px; font-size: 13px; color: #57606a; }
.card label.check { display: flex; align-items: center; gap: 8px; color: #1f2328; font-size: 15px; }
.card input[type=text] { display: block; width: 100%; margin-top: 4px; padding: 7px 10px;
       border: 1px solid #d8dbe0; border-radius: 6px; font-size: 15px; color: #1f2328; }
.card .actions { display: flex; gap: 8px; margin-top: 4px; }
.help { background: #f6f8fa; border: 1px solid #eceef1; border-radius: 6px;
        padding: 10px 12px; margin-bottom: 16px; font-size: 13px; color: #57606a; }
.error { background: #ffebe9; border: 1px solid #ffc1bc; color: #82071e;
         padding: 10px 12px; border-radius: 6px; max-width: 560px; }
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
