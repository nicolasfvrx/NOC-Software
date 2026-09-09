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
use crate::AppState;

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/kiosk/:username", get(kiosk))
        .route(
            "/api/kiosk/:username/command",
            get(get_command).post(post_command),
        )
        .route(
            "/api/kiosk/:username/command/:id/ack",
            post(ack_command),
        )
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
