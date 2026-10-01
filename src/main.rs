use axum::{
    extract::DefaultBodyLimit,
    http::HeaderValue,
    middleware,
    routing::{delete, get, post},
    Router, ServiceExt,
};
use clap::Parser;
use config::Config;
use events::EventHub;
use presence::PresenceHub;
use sqlx::PgPool;
use tokio::sync::watch;
use tower::Layer;
use tower_http::{
    normalize_path::{NormalizePath, NormalizePathLayer},
    trace::{DefaultOnResponse, TraceLayer},
};

/// Reject request bodies larger than this up front. Form payloads are small
/// (the largest fields are capped at 5000 characters), so this bounds the
/// memory a single request can consume.
const MAX_BODY_BYTES: usize = 64 * 1024;

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
    pub events: EventHub,
    pub presence: PresenceHub,
    /// Fires `true` when the process is shutting down. Long-lived SSE streams
    /// select on it so they end promptly; otherwise axum's graceful shutdown
    /// would wait forever for them to close on their own.
    pub shutdown: watch::Receiver<bool>,
    /// Content-Security-Policy header value, built once at startup because it
    /// depends on the configured GitHub Enterprise host (avatar origin).
    pub csp: HeaderValue,
}

mod assets;
mod auth;
mod config;
mod csrf;
mod events;
mod github;
mod handlers;
mod models;
mod presence;
mod security_headers;
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

    let cleaned_sessions = auth::cleanup_expired_sessions(&pool).await?;
    if cleaned_sessions > 0 {
        tracing::info!(count = cleaned_sessions, "cleaned up expired sessions");
    }

    let demo_user_id = if config.demo_mode() {
        Some(auth::ensure_demo_user(&pool).await?)
    } else {
        None
    };

    // Listen for DB events and fan them out to SSE subscribers. Started
    // before the HTTP server so no mutation can race the subscription.
    let events = EventHub::new();
    tokio::spawn(events::notifier_loop(pool.clone(), events.clone()));
    // Mark elapsed highlight timers so every client sees them expire together.
    tokio::spawn(handlers::timer_sweep_loop(pool.clone()));

    // Presence lives in Postgres so every process shows the same roster:
    // the heartbeat keeps this process's rows fresh, the poll loop expires
    // stale rows and propagates roster changes to local SSE subscribers.
    let presence = PresenceHub::new(pool.clone(), config.grace_duration());
    tokio::spawn(presence::heartbeat_loop(presence.clone()));
    tokio::spawn(presence::poll_loop(presence.clone()));

    let csp = security_headers::content_security_policy(&config);

    // Notifies long-lived SSE streams to end when the process shuts down.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let state = AppState {
        pool,
        config,
        demo_user_id,
        events,
        presence,
        shutdown: shutdown_rx,
        csp,
    };

    let app: Router = Router::new()
        .route("/", get(handlers::home))
        .route("/retros", get(handlers::list_retros))
        .route("/retros/new", get(handlers::new_retro))
        .route("/retros", post(handlers::create_retro))
        .route("/retro/{slug}", get(handlers::show_retro))
        .route("/retro/{slug}/ready", post(handlers::set_participant_ready))
        .route("/retro/{slug}/events", get(events::retro_events))
        .route("/retro/{slug}/archives", get(handlers::list_archives))
        .route("/retro/{slug}/archives/{id}", get(handlers::show_archive))
        .route("/items/{category}/{retro_id}", post(handlers::add_item))
        .route(
            "/items/{id}",
            get(handlers::show_item).post(handlers::update_item),
        )
        .route("/items/{id}/edit", get(handlers::edit_item))
        .route("/items/{id}/status", post(handlers::change_item_status))
        .route("/items/{id}/like", post(handlers::like_item))
        .route("/items/{id}/timer/start", post(handlers::start_item_timer))
        .route(
            "/items/{id}/timer/extend",
            post(handlers::extend_item_timer),
        )
        .route(
            "/retro/{retro_id}/action-items",
            post(handlers::add_action_item),
        )
        .route(
            "/action-items/{id}",
            get(handlers::show_action_item)
                .post(handlers::update_action_item)
                .delete(handlers::delete_action_item),
        )
        .route("/action-items/{id}/edit", get(handlers::edit_action_item))
        .route(
            "/action-items/{id}/complete",
            post(handlers::complete_action_item),
        )
        .route("/retro/{retro_id}/archive", post(handlers::archive_retro))
        .route("/retro/{slug}/delete", delete(handlers::delete_retro))
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", post(auth::logout))
        // Static assets are embedded in the binary (see `assets`), so a release
        // build is self-contained; the wildcard captures the path below /static.
        .route("/static/{*path}", get(assets::serve))
        .fallback(handlers::not_found)
        // CSRF defense-in-depth for cookie-authenticated mutations: rejects
        // state-changing requests from foreign origins.
        .layer(middleware::from_fn_with_state(state.clone(), csrf::check))
        // Security headers (CSP, HSTS, frame protection, ...) on every response.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security_headers::apply,
        ))
        // Cap the request body so oversized form payloads cannot exhaust memory.
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
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
    .with_graceful_shutdown(shutdown_signal(shutdown_tx))
    .await?;

    // The server has stopped accepting connections and drained in-flight
    // requests. Delete this instance's presence rows so the roster clears
    // immediately on every process instead of waiting out the grace period.
    if let Err(error) = state.presence.shutdown().await {
        tracing::warn!(error = %error, "failed to clean up presence rows on shutdown");
    }

    Ok(())
}

/// Resolves when the process receives SIGTERM or SIGINT, so `axum::serve` can
/// stop accepting connections and let in-flight requests (and SSE streams)
/// finish before the process exits. This is what makes rolling restarts in a
/// multi-process deployment clean.
async fn shutdown_signal(shutdown_tx: watch::Sender<bool>) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }

    tracing::info!("shutdown signal received, draining connections");
    // End the SSE streams so the graceful shutdown can actually complete.
    let _ = shutdown_tx.send(true);
}
