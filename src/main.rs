use axum::{
    routing::{delete, get, post},
    Router,
};
use clap::Parser;
use sqlx::PgPool;
use tower_http::services::ServeDir;

/// Command line arguments
#[derive(Parser)]
struct Args {
    /// Bind address in format IP:PORT
    #[clap(long, default_value = "0.0.0.0:3000")]
    bind_address: String,
}

mod handlers;
mod models;
pub mod templates;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");

    let pool = PgPool::connect(&database_url).await.unwrap();

    let app = Router::new()
        .route("/", get(handlers::home))
        .route("/retros", get(handlers::list_retros))
        .route("/retros/new", get(handlers::new_retro))
        .route("/retros", post(handlers::create_retro))
        .route("/retro/{slug}", get(handlers::show_retro))
        .route("/items/{category}/{retro_id}", post(handlers::add_item))
        .route("/items/{id}/status", post(handlers::change_item_status))
        .route("/retro/{retro_id}/archive", post(handlers::archive_retro))
        .route("/retro/{slug}/delete", delete(handlers::delete_retro))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(pool);

    let listener = match tokio::net::TcpListener::bind(&args.bind_address).await {
        Ok(listener) => listener,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            eprintln!("Error: Address already in use");
            std::process::exit(1);
        }
        Err(e) => return Err(e.into()),
    };
    axum::serve(listener, app).await?;
    Ok(())
}
