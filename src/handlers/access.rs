//! Loading resources and enforcing retro access.

use super::error::forbidden;
use super::{database_error_response, log_database_error, HandlerError};
use crate::auth::AuthUser;
use crate::models::{apply_author_initials, ActionItem, Item, Retrospective};
use crate::AppState;
use sqlx::PgPool;

pub(super) async fn load_item_with_initials(
    conn: &mut sqlx::PgConnection,
    item_id: i32,
) -> Result<Item, sqlx::Error> {
    let mut items = item_query_as!(
        r#"i.retro_id = (SELECT retro_id FROM items WHERE id = $1)"#,
        item_id
    )
    .fetch_all(&mut *conn)
    .await?;

    apply_author_initials(&mut [&mut items]);
    items
        .into_iter()
        .find(|item| item.id == item_id)
        .ok_or(sqlx::Error::RowNotFound)
}

pub(super) async fn load_action_item(
    pool: &PgPool,
    action_item_id: i32,
) -> Result<ActionItem, sqlx::Error> {
    action_item_query_as!(
        "SELECT ",
        " FROM action_items WHERE id = $1",
        action_item_id
    )
    .fetch_one(pool)
    .await
}

pub(super) async fn load_retro(
    pool: &PgPool,
    slug: &str,
) -> Result<Option<Retrospective>, sqlx::Error> {
    sqlx::query_as!(
        Retrospective,
        "SELECT * FROM retrospectives WHERE slug = $1",
        slug
    )
    .fetch_optional(pool)
    .await
}

pub(crate) async fn require_retro_access(
    state: &AppState,
    user: &AuthUser,
    slug: &str,
) -> Result<Option<Retrospective>, HandlerError> {
    let retro = match load_retro(&state.pool, slug).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(None),
        Err(error) => {
            log_database_error("load_retro_by_slug", &error);
            return Err(database_error_response().into());
        }
    };

    if user.is_admin || user.is_member_of_team(&retro.team_slug) {
        Ok(Some(retro))
    } else {
        Err(forbidden(state, "You do not have access to this retrospective").into())
    }
}

pub(super) async fn require_retro_access_by_id(
    state: &AppState,
    user: &AuthUser,
    retro_id: i32,
) -> Result<Option<Retrospective>, HandlerError> {
    let retro = match sqlx::query_as!(
        Retrospective,
        "SELECT * FROM retrospectives WHERE id = $1",
        retro_id
    )
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(None),
        Err(error) => {
            log_database_error("load_retro_by_id", &error);
            return Err(database_error_response().into());
        }
    };

    if user.is_admin || user.is_member_of_team(&retro.team_slug) {
        Ok(Some(retro))
    } else {
        Err(forbidden(state, "You do not have access to this retrospective").into())
    }
}
