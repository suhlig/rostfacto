use axum::{
    routing::{get, post, delete},
    Router,
};
use sqlx::PgPool;
use tower_http::services::ServeDir;

mod handlers;
mod models;
pub mod templates;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");
    let bind_address = "0.0.0.0:3000";

    let pool = PgPool::connect(&database_url)
        .await
        .unwrap();

    let app = Router::new()
        .route("/", get(handlers::home))
        .route("/retros", get(handlers::list_retros))
        .route("/retros/new", get(handlers::new_retro))
        .route("/retros", post(handlers::create_retro))
        .route("/retro/:slug", get(handlers::show_retro))
        .route("/items/:category/:retro_id", post(handlers::add_item))
        .route("/items/:id/status", post(handlers::change_item_status))
        .route("/retro/:retro_id/archive", post(handlers::archive_retro))
        .route("/retro/:slug/delete", delete(handlers::delete_retro))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(pool);

    let listener = tokio::net::TcpListener::bind(bind_address).await.unwrap();
    axum::serve(listener, app).await?;
    Ok(())
}
