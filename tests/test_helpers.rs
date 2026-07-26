use portpicker::pick_unused_port;
use rand::Rng;
use std::process::{Child, Command};
use thirtyfour::{common::capabilities::firefox::FirefoxPreferences, prelude::*};

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
}

impl BrowserSession {
    pub async fn home_page(&self) -> WebDriverResult<HomePage<'_>> {
        HomePage::new(&self.driver).await
    }

    pub async fn new() -> WebDriverResult<Self> {
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

        std::thread::sleep(std::time::Duration::from_secs(1));

        let mut caps = DesiredCapabilities::firefox();
        if !std::env::var("SHOW_BROWSER").is_ok() {
            caps.set_headless()?;
        }

        let mut prefs = FirefoxPreferences::new();
        let _ = prefs.set("webdriver.log.level", "error");
        caps.set_preferences(prefs)?;

        let driver = WebDriver::new(&format!("http://localhost:{}", port), caps).await?;
        let process = guard.0.take().expect("geckodriver process is present");
        Ok(Self { driver, process })
    }

    pub async fn retros_page(&self) -> WebDriverResult<RetrosPage<'_>> {
        RetrosPage::new(&self.driver).await
    }
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

pub struct HomePage<'a> {
    pub driver: &'a WebDriver,
}

impl<'a> HomePage<'a> {
    pub async fn new(driver: &'a WebDriver) -> WebDriverResult<Self> {
        driver.goto("http://localhost:3000").await?;
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
        driver.goto("http://localhost:3000/retros").await?;
        Ok(Self { driver })
    }

    pub async fn submit_new_retro(&self, title: &str, slug: &str) -> WebDriverResult<()> {
        self.driver.goto("http://localhost:3000/retros/new").await?;
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
        let test_title = format!("{} {}", title_prefix, rand::thread_rng().gen::<u32>());
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
            .goto(format!("http://localhost:3000/retro/{}", slug).as_str())
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

        let input = form.find(By::Tag("input")).await?;
        input.send_keys(text).await?;
        input.send_keys("\u{E007}").await?;

        // Wait for HTMX to finish processing the newly added card before
        // interacting with it.
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Get the newly created card's ID
        let card = self
            .driver
            .find(By::XPath(&format!(
                "//article[contains(@class, 'card') and contains(., '{}')]",
                text
            )))
            .await?;
        let id_str = card.attr("data-item-id").await?.unwrap();
        let id = id_str.parse::<i32>().unwrap();

        Ok(id)
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
            .find(By::Css(&format!("article[data-item-id='{}']", id)))
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
            .find_all(By::Css(&format!("{} article.card", target)))
            .await
    }

    pub async fn click_card(&self, id: i32) -> WebDriverResult<()> {
        let card = self.get_card(id).await?;
        card.click().await?;
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
        self.driver.goto("http://localhost:3000/retros").await?;
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
