use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse},
    http::StatusCode,
    Form,
};

pub async fn delete_retro(
    State(pool): State<PgPool>,
    Path(retro_id): Path<i32>,
) -> impl IntoResponse {
    // Delete the retro (items will be deleted automatically due to ON DELETE CASCADE)
    sqlx::query!(
        "DELETE FROM retrospectives WHERE id = $1",
        retro_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Return empty response with 200 status
    StatusCode::OK
}

pub async fn archive_retro(
    State(pool): State<PgPool>,
    Path(retro_id): Path<i32>,
) -> impl IntoResponse {
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

    (StatusCode::SEE_OTHER, [("Location", format!("/retro/{}", retro_id))]).into_response()
}

use askama::Template;
use sqlx::PgPool;
use serde::Deserialize;
use crate::models::{Retrospective, Item, Category, Status};

#[derive(Template)]
#[template(path = "item_card.html")]
struct ItemCardTemplate {
    item: Item,
}

#[derive(Template)]
#[template(path = "archive_modal.html")]
struct ArchiveModalTemplate {
    text: String,
    retro_id: i32,
}

#[derive(Template)]
#[template(path = "new_retro.html")]
struct NewRetroTemplate {}

pub async fn new_retro() -> Html<String> {
    let template = NewRetroTemplate {};
    Html(template.render().unwrap())
}

#[derive(Template)]
#[template(path = "home.html")]
struct HomeTemplate;

#[derive(Template)]
#[template(path = "retros.html")]
struct RetrosTemplate {
    retros: Vec<Retrospective>,
}

use axum::extract::Query;
use std::collections::HashMap;

pub async fn change_item_status(
    State(pool): State<PgPool>,
    Path(item_id): Path<i32>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    let action = params.get("action").map(|s| s.as_str());
    let item = sqlx::query_as!(
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
                  category as "category: _", created_at as "created_at!",
                  status as "status: _"
        "#,
        item_id,
        action
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        if e.as_database_error()
            .and_then(|de| de.constraint())
            .map_or(false, |c| c.contains("single_highlighted_item_per_retro"))
        {
            // Return the original item with a message
            return Html(format!(
                r##"<div class="card">
                    {}
                    <div class="error-message" style="color: red; margin-top: 0.5rem;">
                        Only one item can be highlighted at a time
                    </div>
                </div>"##,
                htmlescape::encode_minimal(&e.to_string())
            ));
        }
        panic!("Database error: {}", e);
    })
    .unwrap();

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
        let archive_modal = ArchiveModalTemplate {
            text: htmlescape::encode_minimal(&item.text),
            retro_id: item.retro_id,
        };
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
    let retro = sqlx::query_as!(
        Retrospective,
        "INSERT INTO retrospectives (title) VALUES ($1) RETURNING *",
        form.title
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Redirect to the new retro's page
    (StatusCode::SEE_OTHER, [("Location", format!("/retro/{}", retro.id))]).into_response()
}

pub async fn home() -> Html<String> {
    let template = HomeTemplate {};
    Html(template.render().unwrap())
}

pub async fn index(
    State(pool): State<PgPool>,
) -> Html<String> {
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

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate {
    code: &'static str,
    message: String,
}

#[derive(Template)]
#[template(path = "retro.html")]
struct RetroTemplate {
    retro: Retrospective,
    good_items: Vec<Item>,
    bad_items: Vec<Item>,
    watch_items: Vec<Item>,
}

pub async fn show_retro(
    State(pool): State<PgPool>,
    Path(retro_id): Path<i32>,
) -> impl IntoResponse {
    let retro = match sqlx::query_as!(
        Retrospective,
        "SELECT * FROM retrospectives WHERE id = $1",
        retro_id
    )
    .fetch_one(&pool)
    .await {
        Ok(retro) => retro,
        Err(_) => {
            let template = ErrorTemplate {
                code: "404",
                message: format!("Retrospective #{} not found", retro_id),
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
        retro_id
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
        retro_id
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
        retro_id
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let template = RetroTemplate {
        retro,
        good_items,
        bad_items,
        watch_items,
    };

    Html(template.render().unwrap()).into_response()
}

#[derive(Deserialize)]
pub struct NewRetro {
    title: String,
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
