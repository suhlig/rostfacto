//! Error and validation responses shared by the handlers.

use crate::auth::MaybeAuthUser;
use crate::templates::{ErrorTemplate, InlineErrorTemplate};
use crate::AppState;
use askama::Template;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};

pub(super) fn forbidden(state: &AppState, message: &str) -> Response {
    let template = ErrorTemplate {
        code: "403",
        message: message.to_string(),
        demo_mode: state.config.demo_mode(),
    };
    (StatusCode::FORBIDDEN, Html(template.render().unwrap())).into_response()
}

pub(crate) fn not_found_response(state: &AppState, slug: &str) -> Response {
    let template = ErrorTemplate {
        code: "404",
        message: format!("No retrospective with slug '{}' found", slug),
        demo_mode: state.config.demo_mode(),
    };
    (StatusCode::NOT_FOUND, Html(template.render().unwrap())).into_response()
}

pub(super) fn not_found_page(state: &AppState) -> Response {
    let template = ErrorTemplate {
        code: "404",
        message: "Page not found".to_string(),
        demo_mode: state.config.demo_mode(),
    };
    (StatusCode::NOT_FOUND, Html(template.render().unwrap())).into_response()
}

pub(super) fn bad_request(state: &AppState, message: &str) -> Response {
    let template = ErrorTemplate {
        code: "400",
        message: message.to_string(),
        demo_mode: state.config.demo_mode(),
    };
    (StatusCode::BAD_REQUEST, Html(template.render().unwrap())).into_response()
}

/// Whether the request came from htmx, which sends `HX-Request: true`.
pub(super) fn is_htmx_request(headers: &HeaderMap) -> bool {
    headers
        .get("hx-request")
        .and_then(|value| value.to_str().ok())
        == Some("true")
}

/// A validation error response. An htmx request gets a small inline fragment
/// that the form's `hx-status:400` swaps into its error slot; any other request
/// gets the full error page.
pub(super) fn validation_error(state: &AppState, headers: &HeaderMap, message: &str) -> Response {
    if is_htmx_request(headers) {
        let template = InlineErrorTemplate {
            message: message.to_string(),
        };
        (StatusCode::BAD_REQUEST, Html(template.render().unwrap())).into_response()
    } else {
        bad_request(state, message)
    }
}

/// Fallback route: render the 404 page for any unmatched path.
pub async fn not_found(State(state): State<AppState>, _user: MaybeAuthUser) -> impl IntoResponse {
    not_found_page(&state)
}
