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

    // Get the retro data
    let retro = sqlx::query_as!(
        Retrospective,
        "SELECT * FROM retrospectives WHERE id = $1",
        retro_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Return just the inner content HTML - note the r###" syntax for more #s
    Html(format!(
        r###"<div class="container-fluid">
            <h1 class="retro-title">{}</h1>
            <div class="retro-grid">
                <div class="retro-column good-column">
                    <h2 style="text-align: center;">Good</h2>
                    <form hx-post="/items/Good/{}"
                          hx-target="#good-items"
                          hx-swap="beforeend"
                          hx-trigger="submit"
                          hx-on::after-request="this.reset()">
                        <input type="text"
                               name="text"
                               required
                               placeholder="Add a good item..."
                               style="width: 100%;">
                    </form>
                    <div id="good-items"></div>
                </div>
                <div class="retro-column watch-column">
                    <h2 style="text-align: center;">Watch</h2>
                    <form hx-post="/items/Watch/{}"
                          hx-target="#watch-items"
                          hx-swap="beforeend"
                          hx-trigger="submit"
                          hx-on::after-request="this.reset()">
                        <input type="text"
                               name="text"
                               required
                               placeholder="Add a watch item..."
                               style="width: 100%;">
                    </form>
                    <div id="watch-items"></div>
                </div>
                <div class="retro-column bad-column">
                    <h2 style="text-align: center;">Bad</h2>
                    <form hx-post="/items/Bad/{}"
                          hx-target="#bad-items"
                          hx-swap="beforeend"
                          hx-trigger="submit"
                          hx-on::after-request="this.reset()">
                        <input type="text"
                               name="text"
                               required
                               placeholder="Add a bad item..."
                               style="width: 100%;">
                    </form>
                    <div id="bad-items"></div>
                </div>
            </div>
        </div>"###,
        htmlescape::encode_minimal(&retro.title),
        retro.id,
        retro.id,
        retro.id
    ))
}
use askama::Template;
use sqlx::PgPool;
use serde::Deserialize;
use crate::models::{Retrospective, Item, Category, Status};

#[derive(Template)]
#[template(path = "new_retro.html")]
struct NewRetroTemplate {}

pub async fn new_retro() -> Html<String> {
    let template = NewRetroTemplate {};
    Html(template.render().unwrap())
}

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
        Item,
        r#"
        WITH item_info AS (
            SELECT retro_id, status as current_status
            FROM items
            WHERE id = $1
        ),
        highlighted_check AS (
            SELECT EXISTS (
                SELECT 1 FROM items
                WHERE retro_id = (SELECT retro_id FROM item_info)
                AND status = 'HIGHLIGHTED'::status
                AND id != $1
            ) as has_highlighted
        ),
        reset_highlighted AS (
            UPDATE items
            SET status = 'CREATED'::status
            WHERE retro_id = (SELECT retro_id FROM item_info)
            AND status = 'HIGHLIGHTED'::status
            AND id != $1
            AND NOT EXISTS (
                SELECT 1 FROM item_info
                WHERE current_status = 'CREATED'::status
            )
        )
        UPDATE items
        SET status = CASE
            WHEN status = 'COMPLETED'::status THEN 'COMPLETED'::status
            WHEN status = 'CREATED'::status AND NOT EXISTS (
                SELECT 1 FROM highlighted_check WHERE has_highlighted
            ) THEN 'HIGHLIGHTED'::status
            WHEN status = 'HIGHLIGHTED'::status THEN 'COMPLETED'::status
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
        Status::Highlighted => "highlighted",
        Status::Completed => "completed",
        Status::Created => "",
        Status::Archived => "archived", // Archived items will use the same style as completed
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
        format!(
            r##"<div class="card {status_class}">
            {text}
            <dialog id="archive-modal" open>
                <article>
                    <h3>Archive All Cards?</h3>
                    <footer>
                        <button class="secondary"
                                onclick="this.closest('dialog').close()">
                            No
                        </button>
                        <button class="primary"
                                hx-post="/retro/{retro_id}/archive"
                                hx-target=".container-fluid"
                                hx-swap="innerHTML">
                            Yes
                        </button>
                    </footer>
                </article>
            </dialog>
           </div>"##,
            status_class = status_class,
            text = htmlescape::encode_minimal(&item.text),
            retro_id = item.retro_id
        )
    } else {
        format!(
            r##"<div class="card {status_class}" hx-post="/items/{id}/toggle-status" hx-swap="outerHTML">{text}</div>"##,
            status_class = status_class,
            id = item.id,
            text = htmlescape::encode_minimal(&item.text)
        )
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

    let template = format!(
        r#"<div class="card" hx-post="/items/{}/toggle-status" hx-swap="outerHTML">{}</div>"#,
        item.id,
        htmlescape::encode_minimal(&item.text)
    );
    Html(template)
}
