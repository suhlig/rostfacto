#![allow(dead_code)] // items are shared across test crates that use different subsets

use portpicker::pick_unused_port;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use thirtyfour::{common::capabilities::firefox::FirefoxPreferences, prelude::*};
use tokio::sync::{Mutex, Semaphore, SemaphorePermit};
use url::Url;

// Firefox becomes unstable when multiple browser instances run concurrently.
// Limit the suite to one Firefox at a time, matching thirtyfour's own test
// harness.
static FIREFOX_LOCK: Semaphore = Semaphore::const_new(2);

// Tests that open two BrowserSessions must be serialized: with two concurrent
// two-browser tests, each would hold one of the two FIREFOX_LOCK permits and
// then wait forever for its second session. Acquire this permit (plus the
// browser permits) for the whole test body.
static TWO_BROWSER_LOCK: Semaphore = Semaphore::const_new(1);

/// Permit that serializes tests opening two `BrowserSession`s.
pub async fn two_browser_permit() -> SemaphorePermit<'static> {
    TWO_BROWSER_LOCK.acquire().await.unwrap()
}

const TEMPLATE_DB_NAME: &str = "rostfacto_test_template";
static TEMPLATE_READY: AtomicBool = AtomicBool::new(false);
static TEMPLATE_LOCK: Mutex<()> = Mutex::const_new(());

/// Drop leftover test databases from previous, interrupted runs. Kept apart
/// from `TestDb::new` so it runs exactly once per test process, under the
/// template lock.
async fn clean_up_stale_test_databases(admin_pool: &sqlx::PgPool) {
    let stale = sqlx::query_scalar!(
        r#"
        SELECT datname
        FROM pg_database
        WHERE datname LIKE 'rostfacto_test\_%'
          AND datname <> 'rostfacto_test_template'
          AND NOT EXISTS (
              SELECT 1 FROM pg_stat_activity
              WHERE pg_stat_activity.datname = pg_database.datname
          )
        "#
    )
    .fetch_all(admin_pool)
    .await;

    let Ok(stale) = stale else { return };
    let mut dropped = 0;
    for name in &stale {
        let result = sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
            name
        )))
        .execute(admin_pool)
        .await;
        match result {
            Ok(_) => dropped += 1,
            Err(error) => eprintln!("failed to drop stale test database {}: {}", name, error),
        }
    }
    if dropped > 0 {
        eprintln!("dropped {} stale test database(s)", dropped);
    }
}

/// A fresh PostgreSQL database created from a migrated template.
pub struct TestDb {
    pub database_url: String,
    db_name: String,
    admin_url: String,
}

impl TestDb {
    pub async fn new() -> Self {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");
        let base_url = Url::parse(&database_url).expect("DATABASE_URL must be a valid URL");
        let admin_url = Self::admin_url(&base_url);

        {
            let _guard = TEMPLATE_LOCK.lock().await;
            if !TEMPLATE_READY.load(Ordering::SeqCst) {
                Self::ensure_template_db(&admin_url).await;
                TEMPLATE_READY.store(true, Ordering::SeqCst);
            }
        }

        let db_name = format!("rostfacto_test_{:016x}", rand::random::<u64>());
        let pool = sqlx::PgPool::connect(&admin_url)
            .await
            .expect("Failed to connect to Postgres for test DB setup");

        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE DATABASE \"{}\" TEMPLATE \"{}\"",
            db_name, TEMPLATE_DB_NAME
        )))
        .execute(&pool)
        .await
        .expect("Failed to create test database");

        let database_url = Self::replace_db_name(&base_url, &db_name);

        Self {
            database_url,
            db_name,
            admin_url,
        }
    }

    async fn ensure_template_db(admin_url: &str) {
        let pool = sqlx::PgPool::connect(admin_url)
            .await
            .expect("Failed to connect to Postgres for template setup");

        // Reclaim test databases left behind by interrupted runs (killed test
        // processes never run their Drop cleanup). Only databases without
        // active connections are dropped, so a concurrently running test
        // process is never disturbed.
        clean_up_stale_test_databases(&pool).await;

        let create_result = sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE DATABASE \"{}\"",
            TEMPLATE_DB_NAME
        )))
        .execute(&pool)
        .await;

        if let Err(e) = create_result {
            let message = e.to_string();
            if !message.contains("already exists") {
                panic!("Failed to create template database: {}", e);
            }
        }

        let template_url = Self::replace_db_name(
            &Url::parse(admin_url).expect("admin URL should be valid"),
            TEMPLATE_DB_NAME,
        );
        let template_pool = sqlx::PgPool::connect(&template_url)
            .await
            .expect("Failed to connect to template database");

        sqlx::migrate!("./migrations")
            .run(&template_pool)
            .await
            .expect("Failed to run migrations on template database");
    }

    fn admin_url(base_url: &Url) -> String {
        let mut url = base_url.clone();
        url.set_path("/postgres");
        url.to_string()
    }

    fn replace_db_name(base_url: &Url, db_name: &str) -> String {
        let mut url = base_url.clone();
        url.set_path(&format!("/{}", db_name));
        url.to_string()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let admin_url = self.admin_url.clone();
        let db_name = self.db_name.clone();

        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
            runtime.block_on(async {
                let pool = match sqlx::PgPool::connect(&admin_url).await {
                    Ok(p) => p,
                    Err(_) => return,
                };

                let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                    "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
                    db_name
                )))
                .execute(&pool)
                .await;
            });
        })
        .join()
        .ok();
    }
}

/// A test server process started for a single test.
pub struct TestServer {
    process: Child,
    port: u16,
}

impl TestServer {
    /// Start two app processes (distinct ports, distinct presence instance
    /// ids) sharing one database, to exercise the multi-process setup.
    pub async fn start_pair(database_url: &str) -> (Self, Self) {
        let first = Self::start(database_url).await;
        let second = Self::start(database_url).await;
        (first, second)
    }

    pub async fn start(database_url: &str) -> Self {
        let port = pick_unused_port().expect("No ports available");

        // Run the binary cargo already built for this test run instead of
        // spawning `cargo run`: the latter rebuilds the app (and the
        // dependencies it shares with the test build, e.g. reqwest/rustls/sqlx)
        // from inside every test, serializing the suite on the build lock and
        // loading the machine with concurrent compiles while the browsers are
        // starting.
        let mut child = Command::new(env!("CARGO_BIN_EXE_rostfacto"))
            .args(["--bind-address", &format!("127.0.0.1:{}", port)])
            .env("DATABASE_URL", database_url)
            .env("DEMO_MODE", "1")
            .env("PUBLIC_URL", format!("http://127.0.0.1:{}", port))
            // Short presence grace period so disconnect tests stay fast.
            .env("PRESENCE_GRACE_SECONDS", "2")
            .env_remove("GITHUB_ADMIN_ORG")
            .env_remove("GITHUB_ADMIN_TEAM_SLUG")
            .env_remove("GITHUB_USER_ORG")
            .env_remove("GITHUB_CLIENT_ID")
            .env_remove("GITHUB_CLIENT_SECRET")
            .env_remove("GITHUB_ENTERPRISE_URL")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("Failed to start test server");

        let base_url = format!("http://127.0.0.1:{}", port);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let readiness_url = format!("{}/retros", base_url);
        loop {
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                panic!("Test server failed to start on {}", base_url);
            }
            match reqwest::get(&readiness_url).await {
                Ok(response) if response.status().is_success() => break,
                _ => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            }
        }

        Self {
            process: child,
            port,
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// Guard that kills the geckodriver process if driver setup fails before we take ownership.
struct GeckodriverGuard(Option<Child>);

impl Drop for GeckodriverGuard {
    fn drop(&mut self) {
        if let Some(mut process) = self.0.take() {
            let _ = process.kill();
            let _ = process.wait();
        }
    }
}

pub struct BrowserSession {
    pub driver: WebDriver,
    process: Child,
    _firefox_permit: SemaphorePermit<'static>,
    base_url: String,
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        // Only tear down the geckodriver process.  The WebDriver session must
        // be quit explicitly with `BrowserSession::close()` before the session
        // is dropped.  Calling `quit()` synchronously from Drop inside a tokio
        // runtime blocks the executor and serializes/hangs the test suite.
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

impl BrowserSession {
    /// Quit the browser and stop geckodriver.  Call this explicitly at the end
    /// of every test instead of relying on Drop.
    pub async fn close(self) -> WebDriverResult<()> {
        // Clone the handle so we can quit the session while still letting the
        // owned `WebDriver` drop normally afterwards.
        self.driver.clone().quit().await?;
        Ok(())
    }

    pub async fn home_page(&self) -> WebDriverResult<HomePage<'_>> {
        HomePage::new(&self.driver, &self.base_url).await
    }

    pub async fn retros_page(&self) -> WebDriverResult<RetrosPage<'_>> {
        RetrosPage::new(&self.driver, &self.base_url).await
    }

    pub async fn new(base_url: &str) -> WebDriverResult<Self> {
        let permit = FIREFOX_LOCK.acquire().await.unwrap();
        let port = pick_unused_port().expect("No ports available");
        let mut guard = GeckodriverGuard(Some(
            Command::new("geckodriver")
                .arg("--port")
                .arg(port.to_string())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("Failed to start geckodriver"),
        ));

        let mut caps = DesiredCapabilities::firefox();
        if !std::env::var("SHOW_BROWSER").is_ok() {
            caps.set_headless()?;
        }

        let mut prefs = FirefoxPreferences::new();
        let _ = prefs.set("webdriver.log.level", "error");
        caps.set_preferences(prefs)?;

        // Retry connecting to geckodriver instead of relying on a fixed sleep.
        let url = format!("http://localhost:{}", port);
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);
        let driver = loop {
            match WebDriver::new(&url, caps.clone()).await {
                Ok(driver) => break driver,
                Err(e) if tokio::time::Instant::now() >= deadline => return Err(e),
                Err(_) => tokio::time::sleep(tokio::time::Duration::from_millis(200)).await,
            }
        };

        let process = guard.0.take().expect("geckodriver process is present");
        Ok(Self {
            driver,
            process,
            _firefox_permit: permit,
            base_url: base_url.to_string(),
        })
    }
}

mod pages;
pub use pages::*;
