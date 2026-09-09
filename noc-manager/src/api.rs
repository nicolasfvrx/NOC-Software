use std::sync::Arc;

use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::commands::{self, CommandError, KioskCommand};
use crate::health::{self, HeartbeatBody};
use crate::AppState;

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/kiosk/:username", get(kiosk))
        .route("/api/kiosk/:username/rdp", get(kiosk_rdp))
        .route(
            "/api/kiosk/:username/command",
            get(get_command).post(post_command),
        )
        .route(
            "/api/kiosk/:username/command/:id/ack",
            post(ack_command),
        )
        .route("/api/heartbeat/:app/:username", post(heartbeat))
        .route("/metrics", get(metrics))
        .layer(axum::middleware::from_fn_with_state(state, auth))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn kiosk(State(state): State<Arc<AppState>>, Path(username): Path<String>) -> Response {
    match state.storage.get(&username) {
        // enabled=false is returned as-is: the kiosk script decides what to do.
        Some(kiosk) => Json(kiosk).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "kiosk_not_found" })),
        )
            .into_response(),
    }
}

/// `GET /api/kiosk/{username}/rdp` - polled by NOC Display, keyed by the
/// Windows account's own username (must match `Kiosk.username`). A
/// deliberately separate endpoint from `GET /api/kiosk/{username}`: that
/// one is fetched and potentially logged by NOC Agent, and should never
/// carry an RDP password.
async fn kiosk_rdp(State(state): State<Arc<AppState>>, Path(username): Path<String>) -> Response {
    match state.storage.get(&username) {
        Some(kiosk) => Json(json!({
            "enabled": kiosk.rdp.enabled,
            "server": kiosk.rdp.server,
            "port": kiosk.rdp.port,
            "username": kiosk.username,
            "password": kiosk.rdp.password,
            "ignore_certificate_errors": kiosk.rdp.ignore_certificate_errors,
        }))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "kiosk_not_found" })),
        )
            .into_response(),
    }
}

// -------------------------------------------------------------- commands ---

#[derive(Debug, Deserialize)]
struct CommandRequest {
    #[serde(default)]
    action: String,
}

/// Body shape returned to the agent: only id and action.
fn command_view(command: &KioskCommand) -> serde_json::Value {
    json!({ "id": command.id, "action": command.action })
}

fn command_error(error: CommandError) -> Response {
    let status = match error {
        CommandError::KioskNotFound | CommandError::CommandNotFound => StatusCode::NOT_FOUND,
        CommandError::InvalidAction => StatusCode::BAD_REQUEST,
        CommandError::IdMismatch => StatusCode::CONFLICT,
        CommandError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({ "error": error.code() }))).into_response()
}

/// `GET /api/kiosk/{username}/command` - polled by the agent.
async fn get_command(State(state): State<Arc<AppState>>, Path(username): Path<String>) -> Response {
    if state.storage.get(&username).is_none() {
        return command_error(CommandError::KioskNotFound);
    }

    match state.commands.pending(&username) {
        Some(command) => {
            println!("COMMAND fetched username={username} id={}", command.id);
            Json(json!({ "command": command_view(&command) })).into_response()
        }
        None => Json(json!({ "command": null })).into_response(),
    }
}

/// `POST /api/kiosk/{username}/command` - admin helper, body: {"action": "..."}.
async fn post_command(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    body: Result<Json<CommandRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    // A malformed body can only ever mean an unusable action.
    let action = match body {
        Ok(Json(request)) => request.action,
        Err(_) => return command_error(CommandError::InvalidAction),
    };

    match commands::queue_command(&state.storage, &state.commands, &username, &action) {
        Ok(command) => (StatusCode::CREATED, Json(json!({ "command": command_view(&command) })))
            .into_response(),
        Err(error) => command_error(error),
    }
}

/// `POST /api/kiosk/{username}/command/{id}/ack` - agent confirms execution.
async fn ack_command(
    State(state): State<Arc<AppState>>,
    Path((username, id)): Path<(String, u64)>,
) -> Response {
    if state.storage.get(&username).is_none() {
        return command_error(CommandError::KioskNotFound);
    }

    match state.commands.ack(&username, id) {
        Ok(()) => Json(json!({ "status": "acknowledged" })).into_response(),
        Err(error) => command_error(error),
    }
}

// --------------------------------------------------------------- heartbeats ---

/// `POST /api/heartbeat/{app}/{username}` - polled periodically by the agent
/// and the display client. Does not require the kiosk to exist in
/// kiosks.json: health monitoring should work even for a not-yet-registered
/// or orphaned instance.
async fn heartbeat(
    State(state): State<Arc<AppState>>,
    Path((app, username)): Path<(String, String)>,
    body: Result<Json<HeartbeatBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if !health::is_allowed_app(&app) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_app", "allowed": health::APPS })),
        )
            .into_response();
    }
    let body = match body {
        Ok(Json(body)) => body,
        Err(_) => HeartbeatBody::default(),
    };
    state.health.record(&app, &username, body);
    Json(json!({ "status": "ok" })).into_response()
}

/// `GET /metrics` - Prometheus text exposition format, for a Prometheus
/// scrape job (from there, graph in Grafana).
async fn metrics(State(state): State<Arc<AppState>>) -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        health::render_prometheus(&state.health, state.health_stale_after_seconds),
    )
        .into_response()
}

/// Optional bearer-token check. Disabled when `api_token` is empty in config.toml.
async fn auth(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    if state.api_token.is_empty() {
        return next.run(request).await;
    }

    let provided = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .trim();

    if provided == state.api_token {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response()
    }
}
