use axum::{
    routing::{get, post},
    Router,
};
use sqlx::PgPool;
use tower_http::services::ServeDir;

mod handlers;
mod models;

#[tokio::main]
async fn main() {
    let pool = PgPool::connect("postgres://localhost/retro_db")
        .await
        .unwrap();

    let app = Router::new()
        .route("/retro/:id", get(handlers::show_retro))
        .route("/items/:category/:retro_id", post(handlers::add_item))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(pool);

    let addr = "0.0.0.0:3000".parse().unwrap();
    axum::serve(addr, app)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
