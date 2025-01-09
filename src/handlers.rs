use axum::{
    extract::{Path, State},
    response::Html,
    Form,
};
use askama::Template;
use sqlx::PgPool;
use serde::Deserialize;
use crate::models::{Retrospective, RetroItem, ItemCategory};

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
        r#"SELECT * FROM retro_items WHERE retro_id = $1 AND category = 'GOOD'"#,
        retro_id
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let bad_items = sqlx::query_as!(
        RetroItem,
        r#"SELECT * FROM retro_items WHERE retro_id = $1 AND category = 'BAD'"#,
        retro_id
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let watch_items = sqlx::query_as!(
        RetroItem,
        r#"SELECT * FROM retro_items WHERE retro_id = $1 AND category = 'WATCH'"#,
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
        r#"INSERT INTO retro_items (retro_id, text, category) VALUES ($1, $2, $3::item_category) RETURNING *"#,
        retro_id,
        form.text,
        category.to_string()
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    Html(format!("<div class=\"card\">{}</div>", item.text))
}
