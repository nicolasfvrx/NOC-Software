//! Read models intentionally exclude URLs with credentials and RDP passwords.
use crate::{
    health::{self, Heartbeat, HistoryQuery},
    models::Kiosk,
    AppState,
};
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct Client {
    app: String,
    username: String,
    heartbeat: Heartbeat,
    presence: &'static str,
    kiosk: Option<String>,
}

#[derive(Serialize)]
pub struct KioskView {
    username: String,
    display_username: String,
    name: String,
    enabled: bool,
    status: &'static str,
    agent: Option<Heartbeat>,
    display: Option<Heartbeat>,
    pending: Option<String>,
    restart_cron: Option<String>,
}

pub fn kiosk_status(
    kiosk: &Kiosk,
    agent: Option<&Heartbeat>,
    display: Option<&Heartbeat>,
    now: u64,
    stale: u64,
) -> &'static str {
    if !kiosk.enabled {
        return "disabled";
    }
    if matches!((agent,display), (Some(a),Some(d)) if health::is_up(a,now,stale) && health::is_up(d,now,stale) && a.state == "RUNNING" && d.state == "CONNECTED")
    {
        "ready"
    } else {
        "attention"
    }
}

pub fn presence(beat: &Heartbeat, now: u64, stale: u64) -> &'static str {
    if health::is_up(beat, now, stale) {
        "online"
    } else {
        "stale"
    }
}

pub async fn status(State(state): State<Arc<AppState>>) -> Response {
    let now = health::now_unix();
    let kiosks = state.storage.all();
    let pending = state.commands.all_pending();
    let latest = state.health.snapshot();
    let get = |app: &str, user: &str| {
        latest
            .iter()
            .find(|((a, u), _)| a == app && u == user)
            .map(|(_, h)| h.clone())
    };
    let mut views: Vec<_> = kiosks
        .iter()
        .map(|k| {
            let agent = get("agent", &k.username);
            let display = get("display", k.display_username());
            KioskView {
                status: kiosk_status(
                    k,
                    agent.as_ref(),
                    display.as_ref(),
                    now,
                    state.health_stale_after_seconds,
                ),
                username: k.username.clone(),
                display_username: k.display_username().into(),
                name: k.name.clone(),
                enabled: k.enabled,
                agent,
                display,
                restart_cron: k.restart_cron.clone(),
                pending: pending
                    .get(&k.username)
                    .map(|c| format!("{} · #{}", c.action, c.id)),
            }
        })
        .collect();
    views.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.username.cmp(&b.username))
    });
    let clients: Vec<_> = latest
        .into_iter()
        .map(|((app, username), heartbeat)| Client {
            presence: presence(&heartbeat, now, state.health_stale_after_seconds),
            kiosk: kiosks
                .iter()
                .find(|k| {
                    if app == "agent" {
                        k.username == username
                    } else {
                        k.display_username() == username
                    }
                })
                .map(|k| k.username.clone()),
            app,
            username,
            heartbeat,
        })
        .collect();
    let json = serde_json::json!({"now":now,"stale_after_seconds":state.health_stale_after_seconds,
        "retention_days":state.health.retention_days(),"warning":state.health.warning(),"kiosks":views,"clients":clients});
    ([("Cache-Control", "no-store")], Json(json)).into_response()
}

pub async fn history(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HistoryQuery>,
) -> Response {
    if (!query.app.is_empty() && !health::is_allowed_app(&query.app))
        || matches!((query.from, query.to), (Some(a), Some(b)) if a > b)
    {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":"Filtres invalides"})),
        )
            .into_response();
    }
    match tokio::task::spawn_blocking(move || state.health.history(&query)).await {
        Ok(Ok(page)) => ([("Cache-Control","no-store")], Json(page)).into_response(),
        _ => (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error":"Historique indisponible. Consultez les journaux du Manager."}))).into_response(),
    }
}

pub async fn home() -> Html<String> {
    view("Accueil", "home")
}
pub async fn kiosks() -> Html<String> {
    view("Kiosques", "kiosks")
}
pub async fn supervision() -> Html<String> {
    view("Supervision", "supervision")
}

fn view(title: &str, kind: &str) -> Html<String> {
    Html(crate::web::page(
        title,
        &format!(
            r#"<div id="dashboard" data-page="{kind}">
        <div class="page-heading"><div><p class="eyebrow">NORFAIR OPERATION CENTER</p><h1>{title}</h1></div>
        <a class="btn primary" href="/kiosk/new">+ Ajouter un kiosque</a></div>
        <div id="workspace"><p class="empty">Chargement de la supervision…</p></div></div>
        <noscript><p class="error">Activez JavaScript pour consulter les statuts et l’historique.</p></noscript>"#
        ),
    ))
}

pub async fn css() -> impl IntoResponse {
    (
        [("Content-Type", "text/css; charset=utf-8")],
        include_str!("assets/manager.css"),
    )
}
pub async fn js() -> impl IntoResponse {
    (
        [("Content-Type", "text/javascript; charset=utf-8")],
        include_str!("assets/manager.js"),
    )
}
