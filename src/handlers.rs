use axum::{
    extract::{Path, State},
    response::Html,
    Form,
};

pub async fn archive_retro(
    State(pool): State<PgPool>,
    Path(retro_id): Path<i32>,
) -> Html<String> {
    sqlx::query!(
        r#"
        UPDATE retro_items 
        SET status = 'ARCHIVED'::item_status
        WHERE retro_id = $1
        "#,
        retro_id
    )
    .execute(&pool)
    .await
    .unwrap();

    Html("".to_string())
}
use askama::Template;
use sqlx::PgPool;
use serde::Deserialize;
use crate::models::{Retrospective, RetroItem, ItemCategory, ItemStatus};

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    retros: Vec<Retrospective>,
}

pub async fn toggle_status(
    State(pool): State<PgPool>,
    Path(item_id): Path<i32>,
) -> Html<String> {
    let item = sqlx::query_as!(
        RetroItem,
        r#"
        WITH item_info AS (
            SELECT retro_id, status as current_status
            FROM retro_items
            WHERE id = $1
        ),
        highlighted_check AS (
            SELECT EXISTS (
                SELECT 1 FROM retro_items
                WHERE retro_id = (SELECT retro_id FROM item_info)
                AND status = 'HIGHLIGHTED'::item_status
                AND id != $1
            ) as has_highlighted
        ),
        reset_highlighted AS (
            UPDATE retro_items
            SET status = 'DEFAULT'::item_status
            WHERE retro_id = (SELECT retro_id FROM item_info)
            AND status = 'HIGHLIGHTED'::item_status
            AND id != $1
            AND NOT EXISTS (
                SELECT 1 FROM item_info
                WHERE current_status = 'DEFAULT'::item_status
            )
        )
        UPDATE retro_items 
        SET status = CASE 
            WHEN status = 'COMPLETED'::item_status THEN 'COMPLETED'::item_status
            WHEN status = 'DEFAULT'::item_status AND NOT EXISTS (
                SELECT 1 FROM highlighted_check WHERE has_highlighted
            ) THEN 'HIGHLIGHTED'::item_status
            WHEN status = 'HIGHLIGHTED'::item_status THEN 'COMPLETED'::item_status
            ELSE status
        END
        WHERE id = $1
        RETURNING id as "id!", retro_id as "retro_id!", text as "text!", 
                  category as "category: _", created_at as "created_at!",
                  status as "status: _"
        "#,
        item_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let status_class = match item.status {
        ItemStatus::Highlighted => "highlighted",
        ItemStatus::Completed => "completed",
        ItemStatus::Default => "",
    };

    // Check if all items in this retro are completed
    let all_completed = sqlx::query_scalar!(
        r#"
        SELECT NOT EXISTS (
            SELECT 1 FROM retro_items 
            WHERE retro_id = $1 
            AND status != 'COMPLETED'::item_status
            AND status != 'ARCHIVED'::item_status
        )
        "#,
        item.retro_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = if all_completed.unwrap_or(false) {
        format!(
            r#"<div class="card {}" hx-post="/items/{}/toggle-status" hx-swap="outerHTML">
                {}
                <div class="archive-prompt" style="margin-top: 10px;">
                    <button class="archive-btn" 
                            hx-post="/retro/{}/archive" 
                            hx-target='#good-items, #bad-items, #watch-items'
                            hx-swap="innerHTML">
                        Archive All Cards
                    </button>
                </div>
               </div>"#,
            status_class, item.id, htmlescape::encode_minimal(&item.text), item.retro_id
        )
    } else {
        format!(
            r#"<div class="card {}" hx-post="/items/{}/toggle-status" hx-swap="outerHTML">{}</div>"#,
            status_class, item.id, htmlescape::encode_minimal(&item.text)
        )
    };
    Html(template)
}

pub async fn create_retro(
    State(pool): State<PgPool>,
    Form(form): Form<NewRetro>,
) -> Html<String> {
    let _retro = sqlx::query_as!(
        Retrospective,
        "INSERT INTO retrospectives (title) VALUES ($1) RETURNING *",
        form.title
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Redirect to the index page by re-using our index handler
    index(State(pool)).await
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

    let template = IndexTemplate { retros };
    Html(template.render().unwrap())
}

#[derive(Template)]
#[template(path = "retro.html")]
struct RetroTemplate {
    retro: Retrospective,
    good_items: Vec<RetroItem>,
    bad_items: Vec<RetroItem>,
    watch_items: Vec<RetroItem>,
}

pub async fn show_retro(
    State(pool): State<PgPool>,
    Path(retro_id): Path<i32>,
) -> Html<String> {
    let retro = sqlx::query_as!(
        Retrospective,
        "SELECT * FROM retrospectives WHERE id = $1",
        retro_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let good_items = sqlx::query_as!(
        RetroItem,
        r#"SELECT id as "id!", retro_id as "retro_id!", text as "text!", 
                  category as "category: _", created_at as "created_at!", status as "status: _"
           FROM retro_items 
           WHERE retro_id = $1 
           AND category = 'GOOD'
           AND status != 'ARCHIVED'::item_status
           ORDER BY created_at ASC"#,
        retro_id
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let bad_items = sqlx::query_as!(
        RetroItem,
        r#"SELECT id as "id!", retro_id as "retro_id!", text as "text!", 
                  category as "category: _", created_at as "created_at!", status as "status: _"
           FROM retro_items 
           WHERE retro_id = $1 
           AND category = 'BAD'
           AND status != 'ARCHIVED'::item_status
           ORDER BY created_at ASC"#,
        retro_id
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let watch_items = sqlx::query_as!(
        RetroItem,
        r#"SELECT id as "id!", retro_id as "retro_id!", text as "text!", 
                  category as "category: _", created_at as "created_at!", status as "status: _"
           FROM retro_items 
           WHERE retro_id = $1 
           AND category = 'WATCH'
           AND status != 'ARCHIVED'::item_status
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

    Html(template.render().unwrap())
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
    Path((category, retro_id)): Path<(ItemCategory, i32)>,
    Form(form): Form<NewItem>,
) -> Html<String> {
    let item = sqlx::query_as!(
        RetroItem,
        r#"INSERT INTO retro_items (retro_id, text, category, status) 
           VALUES ($1, $2, $3, 'DEFAULT') 
           RETURNING id as "id!", retro_id as "retro_id!", text as "text!", 
                     category as "category: _", created_at as "created_at!", status as "status: _""#,
        retro_id,
        form.text,
        category as ItemCategory
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let template = format!(
        r#"<div class="card" hx-post="/items/{}/toggle-status" hx-swap="outerHTML">{}</div>"#,
        item.id,
        htmlescape::encode_minimal(&item.text)
    );
    Html(template)
}
