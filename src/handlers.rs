use crate::models::{Category, Item, Retrospective};
use crate::templates::{
    ArchiveModalTemplate, ErrorTemplate, HomeTemplate, ItemCardTemplate, NewRetroTemplate,
    RetroTemplate, RetrosTemplate,
};
use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    Form,
};
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;

pub async fn delete_retro(
    State(pool): State<PgPool>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    // Get retro by slug first
    let retro = match sqlx::query_as!(
        Retrospective,
        "SELECT * FROM retrospectives WHERE slug = $1",
        slug
    )
    .fetch_one(&pool)
    .await
    {
        Ok(retro) => retro,
        Err(_) => return not_found().await.into_response(),
    };

    // Delete the retro (items will be deleted automatically due to ON DELETE CASCADE)
    sqlx::query!("DELETE FROM retrospectives WHERE id = $1", retro.id)
        .execute(&pool)
        .await
        .unwrap();

    // Return empty response with 200 status
    StatusCode::OK.into_response()
}

pub async fn archive_retro(
    State(pool): State<PgPool>,
    Path(retro_id): Path<i32>,
) -> impl IntoResponse {
    // Get retro by id first
    let retro = match sqlx::query_as!(
        Retrospective,
        "SELECT * FROM retrospectives WHERE id = $1",
        retro_id
    )
    .fetch_one(&pool)
    .await
    {
        Ok(retro) => retro,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    // Archive all items
    sqlx::query!(
        r#"
        UPDATE items
        SET status = 'ARCHIVED'::status
        WHERE retro_id = $1
        "#,
        retro_id
    )
    .execute(&pool)
    .await
    .unwrap();

    (
        StatusCode::SEE_OTHER,
        [("Location", format!("/retro/{}", retro.slug))],
    )
        .into_response()
}

pub async fn new_retro() -> Html<String> {
    let template = NewRetroTemplate {};
    Html(template.render().unwrap())
}

pub async fn change_item_status(
    State(pool): State<PgPool>,
    Path(item_id): Path<i32>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    let action = params.get("action").map(|s| s.as_str());
    let item = match sqlx::query_as!(
        Item,
        r#"
        UPDATE items
        SET status = CASE
            WHEN status = 'COMPLETED'::status THEN 'COMPLETED'::status
            WHEN status = 'CREATED'::status AND $2 = 'highlight' THEN 'HIGHLIGHTED'::status
            WHEN status = 'HIGHLIGHTED'::status AND $2 = 'complete' THEN 'COMPLETED'::status
            WHEN status = 'HIGHLIGHTED'::status AND $2 = 'cancel' THEN 'CREATED'::status
            ELSE status
        END
        WHERE id = $1
        RETURNING id as "id!", retro_id as "retro_id!", text as "text!",
                  category as "category: _", created_at as "created_at!", status as "status: _"
        "#,
        item_id,
        action
    )
    .fetch_one(&pool)
    .await
    {
        Ok(item) => item,
        Err(e) => {
            if e.as_database_error()
                .and_then(|de| de.constraint())
                .map_or(false, |c| c.contains("single_highlighted_item_per_retro"))
            {
                return Html(format!(
                    r##"<div class="card">
                        <div class="error-message" style="color: #D25948; margin-top: 0.5rem; font-weight: 700;">
                            Only one item can be highlighted at a time
                        </div>
                    </div>"##
                ));
            }
            panic!("Database error: {}", e);
        }
    };

    // Check if all items in this retro are completed
    let all_completed = sqlx::query_scalar!(
        r#"
        SELECT NOT EXISTS (
            SELECT 1 FROM items
            WHERE retro_id = $1
            AND status != 'COMPLETED'::status
            AND status != 'ARCHIVED'::status
        )
        "#,
        item.retro_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = if all_completed.unwrap_or(false) {
        let archive_modal = ArchiveModalTemplate { item };
        archive_modal.render().unwrap()
    } else {
        let item_card = ItemCardTemplate { item };
        item_card.render().unwrap()
    };

    Html(template)
}

pub async fn create_retro(
    State(pool): State<PgPool>,
    Form(form): Form<NewRetro>,
) -> impl IntoResponse {
    // Validate slug
    if form.slug.is_empty() {
        let template = ErrorTemplate {
            code: "400",
            message: "Slug is required".to_string(),
        };
        return (StatusCode::BAD_REQUEST, Html(template.render().unwrap())).into_response();
    }
    if form.slug.len() > 255 {
        let template = ErrorTemplate {
            code: "400",
            message: "Slug must be 255 characters or less".to_string(),
        };
        return (StatusCode::BAD_REQUEST, Html(template.render().unwrap())).into_response();
    }
    if !form
        .slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        let template = ErrorTemplate {
            code: "400",
            message: "Slug can only contain lowercase letters, numbers, and dashes".to_string(),
        };
        return (StatusCode::BAD_REQUEST, Html(template.render().unwrap())).into_response();
    }

    let retro = match sqlx::query_as!(
        Retrospective,
        "INSERT INTO retrospectives (title, slug) VALUES ($1, $2) RETURNING *",
        form.title,
        form.slug
    )
    .fetch_one(&pool)
    .await
    {
        Ok(retro) => retro,
        Err(e) => {
            // Handle database errors (e.g., duplicate slug)
            let message = if let Some(db_err) = e.as_database_error() {
                if db_err.constraint() == Some("retrospectives_slug_key") {
                    "Slug is already in use".to_string()
                } else {
                    format!("Database error: {}", db_err)
                }
            } else {
                format!("Error creating retrospective: {}", e)
            };
            let template = ErrorTemplate {
                code: "500",
                message,
            };
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(template.render().unwrap()),
            )
                .into_response();
        }
    };

    // Redirect to the new retro's page using slug
    (
        StatusCode::SEE_OTHER,
        [("Location", format!("/retro/{}", retro.slug))],
    )
        .into_response()
}

pub async fn not_found() -> impl IntoResponse {
    let template = ErrorTemplate {
        code: "404",
        message: "Page not found".to_string(),
    };
    (StatusCode::NOT_FOUND, Html(template.render().unwrap()))
}

pub async fn home() -> Html<String> {
    let template = HomeTemplate {};
    Html(template.render().unwrap())
}

pub async fn list_retros(State(pool): State<PgPool>) -> Html<String> {
    let retros = sqlx::query_as!(
        Retrospective,
        "SELECT * FROM retrospectives ORDER BY created_at DESC"
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let template = RetrosTemplate { retros };
    Html(template.render().unwrap())
}

pub async fn show_retro(State(pool): State<PgPool>, Path(slug): Path<String>) -> impl IntoResponse {
    let retro = match sqlx::query_as!(
        Retrospective,
        "SELECT * FROM retrospectives WHERE slug = $1",
        slug
    )
    .fetch_one(&pool)
    .await
    {
        Ok(retro) => retro,
        Err(_) => {
            let template = ErrorTemplate {
                code: "404",
                message: format!("No retrospective with slug '{}' found", slug),
            };
            return (StatusCode::NOT_FOUND, Html(template.render().unwrap())).into_response();
        }
    };

    let good_items = sqlx::query_as!(
        Item,
        r#"SELECT id as "id!", retro_id as "retro_id!", text as "text!",
                  category as "category: _", created_at as "created_at!", status as "status: _"
           FROM items
           WHERE retro_id = $1
           AND category = 'GOOD'
           AND status != 'ARCHIVED'::status
           ORDER BY created_at ASC"#,
        retro.id
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let bad_items = sqlx::query_as!(
        Item,
        r#"SELECT id as "id!", retro_id as "retro_id!", text as "text!",
                  category as "category: _", created_at as "created_at!", status as "status: _"
           FROM items
           WHERE retro_id = $1
           AND category = 'BAD'
           AND status != 'ARCHIVED'::status
           ORDER BY created_at ASC"#,
        retro.id
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let watch_items = sqlx::query_as!(
        Item,
        r#"SELECT id as "id!", retro_id as "retro_id!", text as "text!",
                  category as "category: _", created_at as "created_at!", status as "status: _"
           FROM items
           WHERE retro_id = $1
           AND category = 'WATCH'
           AND status != 'ARCHIVED'::status
           ORDER BY created_at ASC"#,
        retro.id
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let all_completed = sqlx::query_scalar!(
        r#"
        SELECT NOT EXISTS (
            SELECT 1 FROM items
            WHERE retro_id = $1
            AND status != 'COMPLETED'::status
            AND status != 'ARCHIVED'::status
        )
        "#,
        retro.id
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .unwrap_or(false);

    let template = RetroTemplate {
        retro,
        good_items,
        bad_items,
        watch_items,
        show_archive_modal: all_completed,
    };

    Html(template.render().unwrap()).into_response()
}

#[derive(Deserialize)]
pub struct NewRetro {
    title: String,
    slug: String,
}

#[derive(Deserialize)]
pub struct NewItem {
    text: String,
}

pub async fn add_item(
    State(pool): State<PgPool>,
    Path((category, retro_id)): Path<(Category, i32)>,
    Form(form): Form<NewItem>,
) -> Html<String> {
    let item = sqlx::query_as!(
        Item,
        r#"INSERT INTO items (retro_id, text, category, status)
           VALUES ($1, $2, $3, 'CREATED'::status)
           RETURNING id as "id!", retro_id as "retro_id!", text as "text!",
                     category as "category: _", created_at as "created_at!", status as "status: _""#,
        retro_id,
        form.text,
        category as Category
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = ItemCardTemplate { item };
    Html(template.render().unwrap())
}
