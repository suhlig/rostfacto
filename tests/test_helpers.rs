#![allow(dead_code)] // items are shared across test crates that use different subsets

use portpicker::pick_unused_port;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use thirtyfour::{
    common::capabilities::firefox::FirefoxPreferences, error::WebDriverErrorInner, prelude::*,
};
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
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
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

pub struct HomePage<'a> {
    pub driver: &'a WebDriver,
}

impl<'a> HomePage<'a> {
    pub async fn new(driver: &'a WebDriver, base_url: &str) -> WebDriverResult<Self> {
        driver.goto(base_url).await?;
        Ok(Self { driver })
    }

    pub async fn verify_title(&self, expected: &str) -> WebDriverResult<()> {
        let h1 = self.driver.find(By::Css("h1")).await?;
        assert_eq!(h1.text().await?, expected);
        Ok(())
    }
}

pub struct RetrosPage<'a> {
    pub driver: &'a WebDriver,
    base_url: String,
}

impl<'a> RetrosPage<'a> {
    pub async fn new(driver: &'a WebDriver, base_url: &str) -> WebDriverResult<Self> {
        driver.goto(format!("{}/retros", base_url).as_str()).await?;
        Ok(Self {
            driver,
            base_url: base_url.to_string(),
        })
    }

    pub async fn submit_new_retro(&self, title: &str, slug: &str) -> WebDriverResult<()> {
        self.driver
            .goto(format!("{}/retros/new", self.base_url).as_str())
            .await?;
        let title_input = self.driver.find(By::Css("input[name='title']")).await?;
        title_input.send_keys(title).await?;
        let slug_input = self.driver.find(By::Css("input[name='slug']")).await?;
        slug_input.send_keys(slug).await?;
        self.driver
            .find(By::Css(".new-retro-form button[type='submit']"))
            .await?
            .click()
            .await?;
        Ok(())
    }

    pub async fn create_retro(&self, title_prefix: &str) -> WebDriverResult<RetroPage<'_>> {
        let test_title = format!("{} {}", title_prefix, rand::random::<u32>());
        let slug = test_title
            .to_lowercase()
            .replace(' ', "-")
            .chars()
            .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
            .collect::<String>()
            .chars()
            .take(255)
            .collect::<String>();
        self.submit_new_retro(&test_title, &slug).await?;
        RetroPage::new(self.driver, &self.base_url, &slug).await
    }

    pub async fn create_retro_with_slug(
        &self,
        title: &str,
        slug: &str,
    ) -> WebDriverResult<RetroPage<'_>> {
        self.submit_new_retro(title, slug).await?;
        RetroPage::new(self.driver, &self.base_url, slug).await
    }
}

pub struct RetroPage<'a> {
    pub driver: &'a WebDriver,
    pub title: String,
    pub slug: String,
    base_url: String,
}

impl<'a> RetroPage<'a> {
    pub async fn new(driver: &'a WebDriver, base_url: &str, slug: &str) -> WebDriverResult<Self> {
        driver
            .goto(format!("{}/retro/{}", base_url, slug).as_str())
            .await?;
        // Get the actual title from the page
        let title_element = driver.find(By::Css("h1")).await?;
        let title = title_element.text().await?;
        Ok(Self {
            driver,
            title,
            slug: slug.to_string(),
            base_url: base_url.to_string(),
        })
    }

    pub async fn retro_id(&self) -> WebDriverResult<i32> {
        let header = self.driver.find(By::Css(".retro-header")).await?;
        let id_str = header.attr("data-retro-id").await?.unwrap();
        Ok(id_str.parse::<i32>().unwrap())
    }

    pub async fn add_card(&self, category: &str, text: &str) -> WebDriverResult<i32> {
        let target = match category {
            "Good" => "#good-items",
            "Bad" => "#bad-items",
            "Watch" => "#watch-items",
            _ => panic!("Invalid category"),
        };

        let form = self
            .driver
            .find(By::Css(format!("form[hx-target='{}']", target).as_str()))
            .await?;

        let input = form.find(By::Tag("textarea")).await?;
        input.send_keys(text).await?;
        form.find(By::Css("button[type='submit']"))
            .await?
            .click()
            .await?;

        // Wait for HTMX to swap the new card in. Polling instead of a fixed
        // sleep keeps the helper reliable on slow machines, where the swap can
        // take longer than a single sleep.
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        let cards = loop {
            let cards = self
                .driver
                .find_all(By::Css(format!("{} article.card", target)))
                .await?;
            if !cards.is_empty() {
                break cards;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("Timed out waiting for a new card in {}", target);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        };

        // New cards are prepended to the category list.
        let id_str = cards[0].attr("data-item-id").await?.unwrap();
        let id = id_str.parse::<i32>().unwrap();

        Ok(id)
    }

    /// Click an element matching `selector`, re-finding it when a render
    /// replaces it between the find and the click (SSE re-fetches swap cards
    /// and buttons under load). `what` names the element in the timeout panic.
    async fn click_with_retry(&self, selector: &str, what: &str) -> WebDriverResult<()> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            match self.driver.find(By::Css(selector)).await {
                Ok(element) => match element.click().await {
                    Ok(()) => return Ok(()),
                    Err(error)
                        if matches!(*error, WebDriverErrorInner::StaleElementReference(..)) => {}
                    Err(error) => return Err(error),
                },
                Err(error) if matches!(*error, WebDriverErrorInner::NoSuchElement(..)) => {}
                Err(error) => return Err(error),
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("Timed out clicking {}", what);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    pub async fn edit_card(&self, id: i32, text: &str) -> WebDriverResult<()> {
        self.click_with_retry(
            &format!("article[data-item-id='{}'] .card-text-edit", id),
            "the edit button",
        )
        .await?;

        // The edit button swap renders the textarea; poll for it instead of
        // finding it immediately (the swap can lag under load).
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        let input = loop {
            if let Ok(input) = self
                .driver
                .find(By::Css(format!(
                    "article[data-item-id='{}'] textarea[name='text']",
                    id
                )))
                .await
            {
                break input;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("Timed out waiting for the edit textarea on card {}", id);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        };
        input.clear().await?;
        input.send_keys(text).await?;
        self.click_with_retry(
            &format!("article[data-item-id='{}'] .btn-save-edit", id),
            "the save button",
        )
        .await?;

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        Ok(())
    }

    pub async fn verify_card_state(&self, id: i32, expected_class: &str) -> WebDriverResult<()> {
        let selector = format!("article[data-item-id='{}']", id);
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);

        loop {
            let card = self.driver.find(By::Css(&selector)).await?;
            let class_attr = card.attr("class").await?.unwrap_or_default();
            if class_attr.trim() == expected_class {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                assert_eq!(
                    class_attr.trim(),
                    expected_class,
                    "Card {} should be in {} state",
                    id,
                    expected_class
                );
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    pub async fn get_card(&self, id: i32) -> WebDriverResult<WebElement> {
        self.driver
            .find(By::Css(format!("article[data-item-id='{}']", id)))
            .await
    }

    pub async fn get_cards_in_category(&self, category: &str) -> WebDriverResult<Vec<WebElement>> {
        let target = match category {
            "Good" => "#good-items",
            "Bad" => "#bad-items",
            "Watch" => "#watch-items",
            _ => panic!("Invalid category"),
        };
        self.driver
            .find_all(By::Css(format!("{} article.card", target)))
            .await
    }

    /// Wait until the category contains a card with the given text (SSE
    /// delivery is asynchronous).
    pub async fn wait_for_card_with_text(&self, category: &str, text: &str) -> WebDriverResult<()> {
        let target = match category {
            "Good" => "#good-items",
            "Bad" => "#bad-items",
            "Watch" => "#watch-items",
            _ => panic!("Invalid category"),
        };
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            let cards = self
                .driver
                .find_all(By::Css(format!("{} article.card", target)))
                .await?;
            let mut found = false;
            for card in cards {
                if let Ok(text_span) = card.find(By::Css(".card-text")).await {
                    if text_span.text().await? == text {
                        found = true;
                        break;
                    }
                }
            }
            if found {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("Timed out waiting for card '{}' in {}", text, category);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    /// Wait until the category contains exactly `expected` cards.
    pub async fn wait_for_card_count(
        &self,
        category: &str,
        expected: usize,
    ) -> WebDriverResult<()> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            let cards = self.get_cards_in_category(category).await?;
            if cards.len() == expected {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "Timed out waiting for {} cards in {}, got {}",
                    expected,
                    category,
                    cards.len()
                );
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    /// Wait until the card's text matches (SSE delivery is asynchronous).
    pub async fn wait_for_card_text(&self, id: i32, expected: &str) -> WebDriverResult<()> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            let card = self.get_card(id).await?;
            let text = card.find(By::Css(".card-text")).await?.text().await?;
            if text == expected {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "Timed out waiting for card {} text '{}', got '{}'",
                    id, expected, text
                );
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    /// Wait until the card's like count matches (SSE delivery is asynchronous).
    pub async fn wait_for_like_count(&self, id: i32, expected: &str) -> WebDriverResult<()> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            let card = self.get_card(id).await?;
            let count = card.find(By::Css(".like-count")).await?.text().await?;
            if count == expected {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "Timed out waiting for card {} like count '{}', got '{}'",
                    id, expected, count
                );
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    /// Wait until the all-done archive modal is open (SSE delivery is
    /// asynchronous).
    pub async fn wait_for_archive_modal(&self) -> WebDriverResult<()> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            if self
                .driver
                .find(By::Css("#archive-modal[open]"))
                .await
                .is_ok()
            {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("Timed out waiting for the all-done archive modal");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    pub async fn click_card(&self, id: i32) -> WebDriverResult<()> {
        let card = self.get_card(id).await?;
        // Use a JavaScript click on the article element so that the click event
        // target is the article itself, not the nested text-edit button.
        self.driver
            .execute("arguments[0].click()", vec![card.to_json()?])
            .await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        Ok(())
    }

    pub async fn like_card(&self, id: i32) -> WebDriverResult<()> {
        self.click_with_retry(
            &format!("article[data-item-id='{}'] .like-button", id),
            "the like button",
        )
        .await?;
        // Wait for HTMX to swap the card with the updated like count.
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        Ok(())
    }

    pub async fn timer_text(&self, id: i32) -> WebDriverResult<String> {
        let badge = self
            .driver
            .find(By::Css(format!(
                "article[data-item-id='{}'] .timer-badge",
                id
            )))
            .await?;
        badge.text().await
    }

    /// Like `timer_text`, but returns `None` while the badge does not exist
    /// yet (the highlighted card, and with it the badge, is swapped in
    /// asynchronously after the highlight request).
    pub async fn try_timer_text(&self, id: i32) -> WebDriverResult<Option<String>> {
        let badges = self
            .driver
            .find_all(By::Css(format!(
                "article[data-item-id='{}'] .timer-badge",
                id
            )))
            .await?;
        match badges.first() {
            Some(badge) => Ok(Some(badge.text().await?)),
            None => Ok(None),
        }
    }

    /// Wait until the badge carries a server-rendered deadline, returning it
    /// as epoch milliseconds (identical on every client).
    pub async fn wait_for_timer_end_at(&self, id: i32) -> WebDriverResult<i64> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            let badge = self
                .driver
                .find(By::Css(format!(
                    "article[data-item-id='{}'] .timer-badge",
                    id
                )))
                .await?;
            if let Some(end_at) = badge.attr("data-end-at").await? {
                if let Ok(end_at) = end_at.parse::<i64>() {
                    return Ok(end_at);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                // Snapshot the badge state for diagnosis: the flake under load
                // is a badge that exists but carries no deadline.
                let mut state = String::new();
                for attr in [
                    "data-end-at",
                    "data-elapsed",
                    "data-initial-seconds",
                    "class",
                ] {
                    if let Ok(Some(value)) = badge.attr(attr).await {
                        state.push_str(&format!(" {}={:?}", attr, value));
                    }
                }
                let text = badge.text().await.unwrap_or_default();
                // The badge sits inside .timer-wrap inside article.card.
                let card_class = match badge.find(By::XPath("../..")).await {
                    Ok(card) => match card.attr("class").await {
                        Ok(Some(class)) => class,
                        _ => String::new(),
                    },
                    Err(_) => String::new(),
                };
                panic!(
                    "Timed out waiting for a server-rendered timer deadline on card {}; badge text {:?} card class {:?}{}",
                    id, text, card_class, state
                );
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    /// Wait until the badge's server-rendered deadline is strictly newer than
    /// `old_end_at`. The previous deadline stays on the badge until the
    /// extend response (or the SSE event) replaces it, so a plain presence
    /// wait could return the stale value.
    pub async fn wait_for_timer_end_after(&self, id: i32, old_end_at: i64) -> WebDriverResult<i64> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            let badge = self
                .driver
                .find(By::Css(format!(
                    "article[data-item-id='{}'] .timer-badge",
                    id
                )))
                .await?;
            if let Some(end_at) = badge.attr("data-end-at").await? {
                if let Ok(end_at) = end_at.parse::<i64>() {
                    if end_at > old_end_at {
                        return Ok(end_at);
                    }
                }
            }
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "Timed out waiting for an extended timer deadline on card {}",
                    id
                );
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    /// Wait until the timer badge shows the given text (SSE delivery is
    /// asynchronous). Only use this for values that persist (e.g. "0:00");
    /// for a running countdown use `wait_for_timer_text_at_most`.
    pub async fn wait_for_timer_text(&self, id: i32, expected: &str) -> WebDriverResult<()> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            if let Some(text) = self.try_timer_text(id).await? {
                if text == expected {
                    return Ok(());
                }
                if tokio::time::Instant::now() >= deadline {
                    panic!(
                        "Timed out waiting for card {} timer text '{}', got '{}'",
                        id, expected, text
                    );
                }
            } else if tokio::time::Instant::now() >= deadline {
                panic!("Timed out waiting for the timer badge on card {}", id);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    /// Wait until the running countdown on the card shows at most
    /// `max_seconds` remaining. Each "M:SS" value is only displayed for one
    /// second, so an exact-text wait can miss it on a slow machine; this
    /// tolerant check is the reliable way to assert a countdown is running.
    /// Also tolerates the badge not existing yet (highlight still landing).
    pub async fn wait_for_timer_text_at_most(
        &self,
        id: i32,
        max_seconds: i64,
    ) -> WebDriverResult<()> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            if let Some(text) = self.try_timer_text(id).await? {
                if let Some(remaining) = parse_countdown(&text) {
                    if remaining <= max_seconds {
                        return Ok(());
                    }
                }
                if tokio::time::Instant::now() >= deadline {
                    panic!(
                        "Timed out waiting for card {} timer to reach {}s, got '{}'",
                        id, max_seconds, text
                    );
                }
            } else if tokio::time::Instant::now() >= deadline {
                panic!("Timed out waiting for the timer badge on card {}", id);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    /// Parses a "M:SS" countdown string into seconds.
    fn parse_countdown(text: &str) -> Option<i64> {
        let (minutes, seconds) = text.split_once(':')?;
        Some(minutes.parse::<i64>().ok()? * 60 + seconds.parse::<i64>().ok()?)
    }

    /// Wait until the +2 min button is visible (timer running or elapsed).
    pub async fn wait_for_extend_button_visible(&self, id: i32) -> WebDriverResult<()> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        loop {
            let button = self
                .driver
                .find(By::Css(format!(
                    "article[data-item-id='{}'] .timer-extend",
                    id
                )))
                .await?;
            if button.is_displayed().await? {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("Timed out waiting for the +2 min button on card {}", id);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    /// Click the +2 min timer button on a card.
    pub async fn click_extend(&self, id: i32) -> WebDriverResult<()> {
        self.click_with_retry(
            &format!("article[data-item-id='{}'] .timer-extend", id),
            "the +2 min button",
        )
        .await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        Ok(())
    }

    /// Click the Cancel button on the highlighted card.
    pub async fn cancel_card(&self) -> WebDriverResult<()> {
        self.click_with_retry(".card-actions .btn-secondary", "the Cancel button")
            .await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        Ok(())
    }

    pub async fn complete_card(&self) -> WebDriverResult<()> {
        // The highlight response may still be settling; the retry loop re-finds
        // the button until it is present and stays clickable.
        self.click_with_retry(".card-actions .btn-primary", "the Done button")
            .await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        Ok(())
    }

    pub async fn archive(&self) -> WebDriverResult<()> {
        // The all-done modal is opened asynchronously (client-side after the
        // last card is completed), so poll for its button instead of finding
        // it immediately.
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        let archive_button = loop {
            if let Ok(button) = self.driver.find(By::Css("#archive-modal .primary")).await {
                if button.is_displayed().await? {
                    break button;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("Timed out waiting for the archive dialog");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        };
        archive_button.click().await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        Ok(())
    }

    pub async fn archive_from_menu(&self) -> WebDriverResult<()> {
        self.driver
            .find(By::Css(".account-menu button"))
            .await?
            .click()
            .await?;
        let archive_link = self.driver.find(By::Css(".archive-menu-link")).await?;
        assert!(
            archive_link.is_displayed().await?,
            "Archive menu item should be visible"
        );
        archive_link.click().await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

        // If there are unaddressed cards, a confirmation dialog is shown.
        if let Ok(confirm_button) = self
            .driver
            .find(By::Css(".archive-confirm-dialog[open] .btn-archive"))
            .await
        {
            confirm_button.click().await?;
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        Ok(())
    }

    pub async fn navigate_to_archives(&self) -> WebDriverResult<()> {
        self.driver
            .goto(format!("{}/retro/{}/archives", self.base_url, self.slug).as_str())
            .await?;
        Ok(())
    }

    pub async fn delete(&self) -> WebDriverResult<()> {
        self.driver
            .goto(format!("{}/retros", self.base_url).as_str())
            .await?;
        let rows = self.driver.find_all(By::Css("table tr")).await?;
        let mut clicked = false;
        for row in rows {
            if let Ok(link) = row.find(By::Tag("a")).await {
                if link.text().await? == self.title {
                    let delete_button = row.find(By::Css(".delete-btn")).await?;
                    delete_button.click().await?;
                    let confirm_button = self
                        .driver
                        .find(By::Css(".delete-confirm-dialog[open] .btn-primary"))
                        .await?;
                    confirm_button.click().await?;
                    clicked = true;
                    break;
                }
            }
        }

        if !clicked {
            return Ok(());
        }

        // Wait for the HTMX delete request to finish and the row to be removed.
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        loop {
            let xpath = format!("//table//tr[contains(., '{}')]", self.title);
            let still_present = !self.driver.find_all(By::XPath(&xpath)).await?.is_empty();
            if !still_present {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("Timed out waiting for retro '{}' to be deleted", self.title);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }
    }
}

/// Parses a "M:SS" countdown string into seconds.
fn parse_countdown(text: &str) -> Option<i64> {
    let (minutes, seconds) = text.split_once(':')?;
    Some(minutes.parse::<i64>().ok()? * 60 + seconds.parse::<i64>().ok()?)
}
