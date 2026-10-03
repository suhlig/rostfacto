//! The participant readiness toggle (ephemeral presence state).

use super::access::require_retro_access;
use super::error::{bad_request, not_found_response};
use super::{database_error_response, log_database_error, HandlerError};
use crate::auth::AuthUser;
use crate::presence::ParticipantKey;
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Form,
};
use serde::Deserialize;
use uuid::Uuid;

/// Form for the participant readiness toggle.
#[derive(Debug, Deserialize)]
pub struct ReadyForm {
    /// The desired state, sent explicitly ("true"/"false").
    pub ready: Option<String>,
    /// Demo-mode participant id (ignored when authenticated).
    pub participant: Option<String>,
}

/// `POST /retro/{slug}/ready` — mark the current participant as done writing
/// cards (or writing again). This is ephemeral presence state, so it emits no
/// event; the presence poll loop broadcasts the updated roster over SSE.
pub async fn set_participant_ready(
    State(state): State<AppState>,
    user: AuthUser,
    Path(slug): Path<String>,
    Form(form): Form<ReadyForm>,
) -> Result<Response, HandlerError> {
    let retro = match require_retro_access(&state, &user, &slug).await? {
        Some(retro) => retro,
        None => return Ok(not_found_response(&state, &slug)),
    };

    let key = if state.config.demo_mode() {
        match form
            .participant
            .as_deref()
            .and_then(|value| Uuid::parse_str(value).ok())
        {
            Some(id) => ParticipantKey::Guest(id),
            None => return Ok(bad_request(&state, "Missing participant id")),
        }
    } else {
        ParticipantKey::User(user.user_id)
    };

    let ready = matches!(form.ready.as_deref(), Some("true") | Some("on") | Some("1"));

    if let Err(error) = state.presence.set_ready(retro.id, &key, ready).await {
        log_database_error("set_participant_ready", &error);
        return Ok(database_error_response());
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}
