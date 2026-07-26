use axum::{
    routing::{delete, get, post},
    Router, ServiceExt,
};
use clap::Parser;
use config::Config;
use sqlx::PgPool;
use tower::Layer;
use tower_http::{
    normalize_path::{NormalizePath, NormalizePathLayer},
    services::ServeDir,
    trace::{DefaultOnResponse, TraceLayer},
};

/// Command line arguments
#[derive(Parser)]
struct Args {
    /// Bind address in format IP:PORT
    #[clap(long, default_value = "0.0.0.0:3000")]
    bind_address: String,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
    pub demo_user_id: Option<i32>,
}

mod auth;
mod config;
mod github;
mod handlers;
mod models;
pub mod templates;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rostfacto=info,tower_http=info".into()),
        )
        .init();

    let args = Args::parse();
    let config = Config::from_env(args.bind_address);

    if config.demo_mode() {
        tracing::warn!("GITHUB_ADMIN_ORG is not set: running in unsecured demo mode");
    } else {
        tracing::info!("GitHub authentication is enabled");
    }

    let pool = match PgPool::connect(&config.database_url).await {
        Ok(pool) => pool,
        Err(error) => {
            tracing::error!(
                error_type = "database_connection",
                "failed to connect to database"
            );
            tracing::debug!(error = %error, "database connection failure details");
            return Err(error.into());
        }
    };

    let demo_user_id = if config.demo_mode() {
        Some(auth::ensure_demo_user(&pool).await?)
    } else {
        None
    };

    let state = AppState {
        pool,
        config,
        demo_user_id,
    };

    let app: Router = Router::new()
        .route("/", get(handlers::home))
        .route("/retros", get(handlers::list_retros))
        .route("/retros/new", get(handlers::new_retro))
        .route("/retros", post(handlers::create_retro))
        .route("/retro/{slug}", get(handlers::show_retro))
        .route("/items/{category}/{retro_id}", post(handlers::add_item))
        .route(
            "/items/{id}",
            get(handlers::show_item).post(handlers::update_item),
        )
        .route("/items/{id}/edit", get(handlers::edit_item))
        .route("/items/{id}/status", post(handlers::change_item_status))
        .route("/retro/{retro_id}/archive", post(handlers::archive_retro))
        .route("/retro/{slug}/delete", delete(handlers::delete_retro))
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", get(auth::logout))
        .nest_service("/static", ServeDir::new("static"))
        .fallback(handlers::not_found)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<axum::body::Body>| {
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        path = request.uri().path()
                    )
                })
                .on_response(DefaultOnResponse::new().level(tracing::Level::INFO)),
        )
        .with_state(state.clone());

    // Normalize trailing slashes before Axum route matching.
    let app = NormalizePathLayer::trim_trailing_slash().layer(app);

    let listener = match tokio::net::TcpListener::bind(&state.config.bind_address).await {
        Ok(listener) => listener,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            tracing::error!(
                error_type = "address_in_use",
                "failed to bind HTTP listener"
            );
            std::process::exit(1);
        }
        Err(e) => {
            tracing::error!(error_type = ?e.kind(), "failed to bind HTTP listener");
            return Err(e.into());
        }
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
