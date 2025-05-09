use portpicker::pick_unused_port;
use rand::Rng;
use std::process::{Child, Command};
use thirtyfour::{common::capabilities::firefox::FirefoxPreferences, prelude::*};

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
        let process = Command::new("geckodriver")
            .arg("--port")
            .arg(port.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("Failed to start geckodriver");

        std::thread::sleep(std::time::Duration::from_secs(1));

        let mut caps = DesiredCapabilities::firefox();
        if !std::env::var("SHOW_BROWSER").is_ok() {
            caps.set_headless()?;
        }

        let mut prefs = FirefoxPreferences::new();
        let _ = prefs.set("webdriver.log.level", "error");
        caps.set_preferences(prefs)?;

        let driver = WebDriver::new(&format!("http://localhost:{}", port), caps).await?;
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

    pub async fn create_retro(&self, title_prefix: &str) -> WebDriverResult<RetroPage<'_>> {
        self.driver.goto("http://localhost:3000/retros/new").await?;
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

        let title_input = self.driver.find(By::Css("input[name='title']")).await?;
        title_input.send_keys(&test_title).await?;
        let slug_input = self.driver.find(By::Css("input[name='slug']")).await?;
        slug_input.send_keys(&slug).await?;

        self.driver
            .find(By::Css("input[type='submit']"))
            .await?
            .click()
            .await?;

        RetroPage::new(self.driver, &slug).await
    }
}

pub struct RetroPage<'a> {
    pub driver: &'a WebDriver,
    pub title: String,
}

impl<'a> RetroPage<'a> {
    pub async fn new(driver: &'a WebDriver, slug: &str) -> WebDriverResult<Self> {
        driver
            .goto(format!("http://localhost:3000/retro/{}", slug).as_str())
            .await?;
        // Get the actual title from the page
        let title_element = driver.find(By::Css("h1")).await?;
        let title = title_element.text().await?;
        Ok(Self { driver, title })
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

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

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
        let card = self
            .driver
            .find(By::Css(&format!("article[data-item-id='{}']", id)))
            .await?;

        let class_attr = card.attr("class").await?.unwrap();
        assert_eq!(
            class_attr.trim(),
            expected_class,
            "Card {} should be in {} state",
            id,
            expected_class
        );
        Ok(())
    }

    pub async fn click_card(&self, id: i32) -> WebDriverResult<()> {
        let card = self
            .driver
            .find(By::Css(&format!("article[data-item-id='{}']", id)))
            .await?;
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

    pub async fn cleanup(self) -> WebDriverResult<()> {
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
}
