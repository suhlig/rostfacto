use axum::{
    routing::{get, post, delete},
    Router,
};
use sqlx::PgPool;
use tower_http::services::ServeDir;

mod handlers;
mod models;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = PgPool::connect("postgres://localhost/rostfacto-dev") // TODO Read from env var
        .await
        .unwrap();

    let app = Router::new()
        .route("/", get(handlers::index))
        .route("/retros/new", get(handlers::new_retro))
        .route("/retros", post(handlers::create_retro))
        .route("/retro/:id", get(handlers::show_retro))
        .route("/items/:category/:retro_id", post(handlers::add_item))
        .route("/items/:id/toggle-status", post(handlers::toggle_status))
        .route("/retro/:id/archive", post(handlers::archive_retro))
        .route("/retro/:id/delete", delete(handlers::delete_retro))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(pool);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await?;
    Ok(())
}
