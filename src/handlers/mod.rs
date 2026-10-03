//! HTTP handlers, grouped by resource.
//!
//! The handlers are split across submodules by route group (retros, items,
//! timers, action items, presence) plus shared helpers (errors, access checks,
//! event emission). This module owns the pieces every group needs and
//! re-exports the public handlers so `main.rs` can keep referring to them as
//! `handlers::foo`.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// Upper bound for user-supplied card and action item text. Mirrored by the
/// `*_text_length_check` constraints in migration 024; both must agree.
pub(crate) const MAX_ITEM_TEXT_LENGTH: usize = 5_000;
/// Upper bound for the retro title. Mirrored by `retrospectives_title_length_check`.
pub(crate) const MAX_RETRO_TITLE_LENGTH: usize = 200;

/// Error type returned by handlers. `axum::http::Response` is larger than the
/// 128-byte threshold that trips `clippy::result_large_err`, so it is boxed;
/// `IntoResponse` unwraps it again at the HTTP boundary. `From<Response>` lets
/// `?` on helper results convert a plain `Response` into this type.
pub(crate) struct HandlerError(Box<Response>);

impl From<Response> for HandlerError {
    fn from(response: Response) -> Self {
        Self(Box::new(response))
    }
}

impl IntoResponse for HandlerError {
    fn into_response(self) -> Response {
        *self.0
    }
}

pub(crate) fn log_database_error(operation: &'static str, error: &sqlx::Error) {
    if let Some(database_error) = error.as_database_error() {
        tracing::error!(
            operation,
            database_code = database_error.code().as_deref(),
            constraint = database_error.constraint(),
            "database operation failed"
        );
    } else {
        tracing::error!(operation, error_type = "sqlx", "database operation failed");
    }
    tracing::debug!(operation, error = %error, "database operation failure details");
}

pub(crate) fn database_error_response() -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
}

/// Build a compile-time-checked `SELECT` of the full item projection.
///
/// The projection (the column list, the `users` join, and the derived
/// `likes_count`/`author_initials` columns) is identical at every call site;
/// only the `WHERE` clause differs. sqlx requires the query to be a string
/// literal, so the shared part lives here and callers append their own filter
/// via the `+` literal concatenation sqlx supports. `$where` must be a literal.
macro_rules! item_query_as {
    ($where:literal $(, $arg:expr)* $(,)?) => {
        sqlx::query_as!(
            $crate::models::Item,
            r#"SELECT i.id as "id!", i.retro_id as "retro_id!", i.text as "text!",
                      i.category as "category: _", i.created_at as "created_at!", i.status as "status: _",
                      i.created_by as "author_id!", u.display_name as "author_name!",
                      u.avatar_url as "author_avatar_url?",
                      ''::text as "author_initials!",
                      (SELECT COUNT(*) FROM likes WHERE item_id = i.id) as "likes_count!",
                      i.archive_id as "archive_id: _", i.archived_at as "archived_at: _",
                      i.timer_started_at as "timer_started_at: _", i.timer_duration_seconds as "timer_duration_seconds: _",
                      i.timer_ends_at as "timer_ends_at: _", i.timer_elapsed_at as "timer_elapsed_at: _",
                      i.updated_at as "updated_at!"
               FROM items i
               JOIN users u ON u.id = i.created_by
               WHERE "# + $where
            $(, $arg)*
        )
    };
}

/// Build a compile-time-checked query over the shared action-item projection.
///
/// The column list is identical at every call site, but it sits in the middle
/// of a `SELECT` and at the end of an `INSERT ... RETURNING`, so callers pass
/// the SQL before the projection (`$prefix`) and after it (`$suffix`); the
/// projection itself lives here once. Both must be literals. The `INSERT` form
/// passes an empty `$suffix`.
macro_rules! action_item_query_as {
    ($prefix:literal, $suffix:literal $(, $arg:expr)* $(,)?) => {
        sqlx::query_as!(
            $crate::models::ActionItem,
            $prefix + r#"id as "id!", retro_id as "retro_id!", text as "text!", created_at as "created_at!",
                        completed_at as "completed_at: _", archive_id as "archive_id: _", archived_at as "archived_at: _""# + $suffix
            $(, $arg)*
        )
    };
}

mod access;
mod action_items;
mod emit;
mod error;
mod items;
mod presence;
mod retros;
mod timers;

pub(crate) use access::require_retro_access;
pub use action_items::{
    add_action_item, complete_action_item, delete_action_item, edit_action_item, show_action_item,
    update_action_item,
};
pub use error::not_found;
pub(crate) use error::not_found_response;
pub use items::{add_item, change_item_status, edit_item, like_item, show_item, update_item};
pub use presence::set_participant_ready;
pub use retros::{
    archive_retro, create_retro, delete_retro, home, list_archives, list_retros, new_retro,
    show_archive, show_retro, slug_check,
};
pub use timers::{extend_item_timer, start_item_timer, timer_sweep_loop};
