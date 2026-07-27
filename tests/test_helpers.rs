use portpicker::pick_unused_port;
use std::process::{Child, Command};
use std::sync::OnceLock;
use thirtyfour::{common::capabilities::firefox::FirefoxPreferences, prelude::*};
use tokio::sync::{Semaphore, SemaphorePermit};

static TEST_SERVER: OnceLock<(u16, Child)> = OnceLock::new();

// Firefox becomes unstable when multiple browser instances run concurrently.
// Limit the suite to one Firefox at a time, matching thirtyfour's own test
// harness.
static FIREFOX_LOCK: Semaphore = Semaphore::const_new(2);

/// Return the base URL of the test server, starting it on a random port if necessary.
pub fn base_url() -> String {
    let port = test_server_port();
    format!("http://127.0.0.1:{}", port)
}

fn test_server_port() -> u16 {
    let (port, _) = TEST_SERVER.get_or_init(|| {
        let port = pick_unused_port().expect("No ports available");
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");

        let mut child = Command::new("cargo")
            .args([
                "run",
                "--quiet",
                "--",
                "--bind-address",
                &format!("127.0.0.1:{}", port),
            ])
            .env("DATABASE_URL", database_url)
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

        // Wait for the server to accept connections.
        let addr = format!("127.0.0.1:{}", port);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                panic!("Test server failed to start on {}", addr);
            }
            if std::net::TcpStream::connect(&addr).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        (port, child)
    });
    *port
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
        HomePage::new(&self.driver).await
    }

    pub async fn new() -> WebDriverResult<Self> {
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
        })
    }

    pub async fn retros_page(&self) -> WebDriverResult<RetrosPage<'_>> {
        RetrosPage::new(&self.driver).await
    }
}

pub struct HomePage<'a> {
    pub driver: &'a WebDriver,
}

impl<'a> HomePage<'a> {
    pub async fn new(driver: &'a WebDriver) -> WebDriverResult<Self> {
        driver.goto(&base_url()).await?;
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
}

impl<'a> RetrosPage<'a> {
    pub async fn new(driver: &'a WebDriver) -> WebDriverResult<Self> {
        driver
            .goto(format!("{}/retros", base_url()).as_str())
            .await?;
        Ok(Self { driver })
    }

    pub async fn submit_new_retro(&self, title: &str, slug: &str) -> WebDriverResult<()> {
        self.driver
            .goto(format!("{}/retros/new", base_url()).as_str())
            .await?;
        let title_input = self.driver.find(By::Css("input[name='title']")).await?;
        title_input.send_keys(title).await?;
        let slug_input = self.driver.find(By::Css("input[name='slug']")).await?;
        slug_input.send_keys(slug).await?;
        self.driver
            .find(By::Css("input[type='submit']"))
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
        RetroPage::new(self.driver, &slug).await
    }

    pub async fn create_retro_with_slug(
        &self,
        title: &str,
        slug: &str,
    ) -> WebDriverResult<RetroPage<'_>> {
        self.submit_new_retro(title, slug).await?;
        RetroPage::new(self.driver, slug).await
    }
}

pub struct RetroPage<'a> {
    pub driver: &'a WebDriver,
    pub title: String,
    pub slug: String,
}

impl<'a> RetroPage<'a> {
    pub async fn new(driver: &'a WebDriver, slug: &str) -> WebDriverResult<Self> {
        driver
            .goto(format!("{}/retro/{}", base_url(), slug).as_str())
            .await?;
        // Get the actual title from the page
        let title_element = driver.find(By::Css("h1")).await?;
        let title = title_element.text().await?;
        Ok(Self {
            driver,
            title,
            slug: slug.to_string(),
        })
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

        // Wait for HTMX to finish processing the newly added card before
        // interacting with it.
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // New cards are prepended to the category list.
        let card = self
            .driver
            .find(By::Css(format!("{} article.card", target)))
            .await?;
        let id_str = card.attr("data-item-id").await?.unwrap();
        let id = id_str.parse::<i32>().unwrap();

        Ok(id)
    }

    pub async fn edit_card(&self, id: i32, text: &str) -> WebDriverResult<()> {
        self.driver
            .find(By::Css(format!(
                "article[data-item-id='{}'] .card-text-edit",
                id
            )))
            .await?
            .click()
            .await?;

        let input = self
            .driver
            .find(By::Css(format!(
                "article[data-item-id='{}'] textarea[name='text']",
                id
            )))
            .await?;
        input.clear().await?;
        input.send_keys(text).await?;
        self.driver
            .find(By::Css(format!(
                "article[data-item-id='{}'] .btn-save-edit",
                id
            )))
            .await?
            .click()
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

    pub async fn complete_card(&self) -> WebDriverResult<()> {
        self.driver.find(By::Css(".primary")).await?.click().await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        Ok(())
    }

    pub async fn archive(&self) -> WebDriverResult<()> {
        let archive_button = self.driver.find(By::Css("#archive-modal .primary")).await?;
        assert!(
            archive_button.is_displayed().await?,
            "Archive dialog should be visible"
        );
        archive_button.click().await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        Ok(())
    }

    pub async fn delete(&self) -> WebDriverResult<()> {
        self.driver
            .goto(format!("{}/retros", base_url()).as_str())
            .await?;
        let rows = self.driver.find_all(By::Css("table tr")).await?;
        for row in rows {
            if let Ok(link) = row.find(By::Tag("a")).await {
                if link.text().await? == self.title {
                    self.driver
                        .execute("window.confirm = () => true", vec![])
                        .await?;
                    let delete_button = row.find(By::Tag("button")).await?;
                    delete_button.click().await?;
                    break;
                }
            }
        }
        Ok(())
    }

    pub async fn cleanup(self) -> WebDriverResult<()> {
        self.delete().await?;
        Ok(())
    }
}
