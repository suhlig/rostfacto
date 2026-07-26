use axum::{
    routing::{delete, get, post},
    Router, ServiceExt,
};
use clap::Parser;
use sqlx::PgPool;
use tower::Layer;
use tower_http::{
    normalize_path::{NormalizePath, NormalizePathLayer},
    services::ServeDir,
};

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

    let app: Router = Router::new()
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
        .fallback(handlers::not_found)
        .with_state(pool);

    // Normalize trailing slashes before Axum route matching.
    let app = NormalizePathLayer::trim_trailing_slash().layer(app);

    let listener = match tokio::net::TcpListener::bind(&args.bind_address).await {
        Ok(listener) => listener,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            eprintln!("Error: Address already in use");
            std::process::exit(1);
        }
        Err(e) => return Err(e.into()),
    };
    axum::serve(
        listener,
        <NormalizePath<Router> as ServiceExt<axum::http::Request<axum::body::Body>>>::into_make_service(
            app,
        ),
    )
    .await?;
    Ok(())
}
