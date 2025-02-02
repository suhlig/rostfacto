use thirtyfour::{prelude::*, common::capabilities::firefox::FirefoxPreferences};

fn should_show_browser() -> bool {
    std::env::var("SHOW_BROWSER").is_ok()
}
use portpicker::pick_unused_port;
use std::process::{Command, Child};
use rand::Rng;

struct GeckoDriver {
    process: Child,
    port: u16,
}

impl Drop for GeckoDriver {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn start_geckodriver() -> GeckoDriver {
    let port = pick_unused_port().expect("No ports available");

    let process = Command::new("geckodriver")
        .arg("--port")
        .arg(port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to start geckodriver");

    // Give geckodriver a moment to start
    std::thread::sleep(std::time::Duration::from_secs(1));

    GeckoDriver { process, port }
}

#[tokio::test]
async fn test_home_page() -> WebDriverResult<()> {
    let gecko = start_geckodriver();

    let mut caps = DesiredCapabilities::firefox();
    if !should_show_browser() {
        caps.set_headless()?;
    }
    caps.add_firefox_arg("--log-level=3")?; // Only show fatal errors

    // Create Firefox preferences and set them
    let mut prefs = FirefoxPreferences::new();
    let _ = prefs.set("webdriver.log.level", "error");
    caps.set_preferences(prefs)?;

    let driver = WebDriver::new(&format!("http://localhost:{}", gecko.port), caps).await?;

    // Navigate to the homepage
    driver.goto("http://localhost:3000").await?;

    // Find the h1 element with retro-title class and verify its text
    let h1 = driver.find(By::Css("h1.retro-title")).await?;
    assert_eq!(h1.text().await?, "Retrospectives");

    // Always close the browser
    driver.quit().await?;

    Ok(())
}

#[tokio::test]
async fn test_archive_retro() -> WebDriverResult<()> {
    let gecko = start_geckodriver();

    let mut caps = DesiredCapabilities::firefox();
    if !should_show_browser() {
        caps.set_headless()?;
    }

    // Create Firefox preferences and set them
    let mut prefs = FirefoxPreferences::new();
    let _ = prefs.set("webdriver.log.level", "error");
    caps.set_preferences(prefs)?;

    let driver = WebDriver::new(&format!("http://localhost:{}", gecko.port), caps).await?;

    // Create a new retro
    driver.goto("http://localhost:3000").await?;
    driver.find(By::Css("a[href='/retros/new']")).await?.click().await?;

    let test_title = format!("Archive Test Retro {}", rand::thread_rng().gen::<u32>());
    let title_input = driver.find(By::Css("input[name='title']")).await?;
    title_input.send_keys(&test_title).await?;
    driver.find(By::Css("input[type='submit']")).await?.click().await?;

    // Add a single card to Good column
    let good_form = driver.find(By::Css("form[hx-target='#good-items']")).await?;
    let good_input = good_form.find(By::Tag("input")).await?;
    good_input.send_keys("Card to archive").await?;
    good_input.send_keys("\u{E007}").await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Click the card to highlight it
    let card = driver.find(By::Css("#good-items .card")).await?;
    card.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Click again to complete it
    let highlighted_card = driver.find(By::Css("#good-items .card.highlighted")).await?;
    highlighted_card.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Verify the archive dialog appears
    let archive_dialog = driver.find(By::Id("archive-modal")).await?;
    assert!(archive_dialog.is_displayed().await?, "Archive dialog should be visible");

    // Click "Yes" on the archive dialog
    driver.execute("window.confirm = () => true", vec![]).await?;
    let archive_button = archive_dialog.find(By::Css("#archive-modal .primary")).await?;
    archive_button.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Verify all cards are gone
    let remaining_cards = driver.find_all(By::ClassName("card")).await?;
    assert_eq!(remaining_cards.len(), 0, "All cards should be archived");

    // Clean up - delete the retro
    driver.goto("http://localhost:3000").await?;
    let cards = driver.find_all(By::ClassName("card")).await?;
    for card in cards {
        let links = card.find_all(By::Tag("a")).await?;
        for link in links {
            if link.text().await? == test_title {
                driver.execute("window.confirm = () => true", vec![]).await?;
                let delete_button = card.find(By::Tag("button")).await?;
                delete_button.click().await?;
                break;
            }
        }
    }

    driver.quit().await?;

    Ok(())
}

#[tokio::test]
async fn test_create_cards() -> WebDriverResult<()> {
    let gecko = start_geckodriver();

    let mut caps = DesiredCapabilities::firefox();
    if !should_show_browser() {
        caps.set_headless()?;
    }

    // Create Firefox preferences and set them
    let mut prefs = FirefoxPreferences::new();
    let _ = prefs.set("webdriver.log.level", "error");
    caps.set_preferences(prefs)?;

    let driver = WebDriver::new(&format!("http://localhost:{}", gecko.port), caps).await?;

    // Navigate to the homepage
    driver.goto("http://localhost:3000").await?;

    // Click the "New Retrospective" button
    driver.find(By::Css("a[href='/retros/new']")).await?.click().await?;

    // Fill in the title
    let test_title = format!("Test Retro {}", rand::thread_rng().gen::<u32>());
    let title_input = driver.find(By::Css("input[name='title']")).await?;
    title_input.send_keys(&test_title).await?;

    // Click the submit button
    driver.find(By::Css("input[type='submit']")).await?.click().await?;

    // Navigate back to homepage
    driver.goto("http://localhost:3000").await?;

    // Find all cards and look for our test retro
    let cards = driver.find_all(By::ClassName("card")).await?;
    let mut found_card = None;
    for card in cards {
        let links = card.find_all(By::Tag("a")).await?;
        for link in links {
            if link.text().await? == test_title {
                found_card = Some(card);
                break;
            }
        }
        if found_card.is_some() {
            break;
        }
    }

    let our_card = found_card.expect("Newly created retro not found in list");

    // Extract the retro ID from the card's link href
    let link = our_card.find(By::Tag("a")).await?;
    let href = link.attr("href").await?.unwrap();


    // Go to the retro page
    driver.goto(&format!("http://localhost:3000{}", href)).await?;

    // Add a card to each column
    for (target, text) in [
        ("#good-items", "Good point test"),
        ("#bad-items", "Bad point test"),
        ("#watch-items", "Watch point test")
    ] {
        let form = driver.find(By::Css(format!("form[hx-target='{}']", target).as_str())).await?;
        let input = form.find(By::Tag("input")).await?;
        input.send_keys(text).await?;
        input.send_keys("\u{E007}").await?;

        // Wait a moment for the card to appear
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Verify each card exists and is in default state (no special classes)
    for text in ["Good point test", "Bad point test", "Watch point test"] {
        let cards = driver.find_all(By::ClassName("card")).await?;
        let mut found = false;
        for card in cards {
            if card.text().await? == text {
                // Card should only have "card" class in default state
                let class_attr = card.attr("class").await?.unwrap();
                assert_eq!(class_attr.trim(), "card",
                    "Card '{}' should be in default state", text);
                found = true;
                break;
            }
        }
        assert!(found, "Card with text '{}' not found", text);
    }

    // Clean up - delete the retro
    driver.goto("http://localhost:3000").await?;
    let cards = driver.find_all(By::ClassName("card")).await?;
    for card in cards {
        let links = card.find_all(By::Tag("a")).await?;
        for link in links {
            if link.text().await? == test_title {
                // Execute JavaScript to override the confirm dialog
                driver.execute("window.confirm = () => true", vec![]).await?;

                let delete_button = card.find(By::Tag("button")).await?;
                delete_button.click().await?;
                break;
            }
        }
    }

    // Always close the browser
    driver.quit().await?;

    Ok(())
}

#[tokio::test]
async fn test_create_retro() -> WebDriverResult<()> {
    let gecko = start_geckodriver();

    let mut caps = DesiredCapabilities::firefox();
    if !should_show_browser() {
        caps.set_headless()?;
    }

    // Create Firefox preferences and set them
    let mut prefs = FirefoxPreferences::new();
    let _ = prefs.set("webdriver.log.level", "error");
    caps.set_preferences(prefs)?;

    let driver = WebDriver::new(&format!("http://localhost:{}", gecko.port), caps).await?;

    // Navigate to the homepage
    driver.goto("http://localhost:3000").await?;

    // Click the "New Retrospective" button
    driver.find(By::Css("a[href='/retros/new']")).await?.click().await?;

    // Fill in the title
    let test_title = format!("Test Retro {}", rand::thread_rng().gen::<u32>());
    let title_input = driver.find(By::Css("input[name='title']")).await?;
    title_input.send_keys(&test_title).await?;

    // Click the submit button
    driver.find(By::Css("input[type='submit']")).await?.click().await?;

    // Wait for redirect and verify we're on the retro page
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    let current_url = driver.current_url().await?;
    assert!(current_url.as_str().contains("/retro/"), "Should be redirected to retro page");

    // Verify the retro title is shown
    let title = driver.find(By::Css("h1.retro-title")).await?;
    assert_eq!(title.text().await?, test_title);

    // Extract retro ID from URL for cleanup
    let retro_id = current_url.path_segments().unwrap().last().unwrap();

    // Navigate to the homepage
    driver.goto("http://localhost:3000").await?;

    // Execute JavaScript to override the confirm dialog
    driver.execute("window.confirm = () => true", vec![]).await?;

    // Find and click the delete button
    driver.find(By::Css(&format!("button[hx-delete='/retro/{}/delete']", retro_id))).await?.click().await?;

    // Wait a moment for the deletion to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Verify the retro is deleted
    let cards = driver.find_all(By::ClassName("card")).await?;
    for card in cards {
        let links = card.find_all(By::Tag("a")).await?;
        for link in links {
            assert_ne!(link.text().await?, test_title, "Retro was not deleted!");
        }
    }

    // Always close the browser
    driver.quit().await?;

    Ok(())
}

#[tokio::test]
async fn test_card_state_transitions() -> WebDriverResult<()> {
    let gecko = start_geckodriver();

    let mut caps = DesiredCapabilities::firefox();
    if !should_show_browser() {
        caps.set_headless()?;
    }

    // Create Firefox preferences and set them
    let mut prefs = FirefoxPreferences::new();
    let _ = prefs.set("webdriver.log.level", "error");
    caps.set_preferences(prefs)?;

    let driver = WebDriver::new(&format!("http://localhost:{}", gecko.port), caps).await?;

    // Create a new retro
    driver.goto("http://localhost:3000").await?;
    driver.find(By::Css("a[href='/retros/new']")).await?.click().await?;

    let test_title = format!("State Test Retro {}", rand::thread_rng().gen::<u32>());
    let title_input = driver.find(By::Css("input[name='title']")).await?;
    title_input.send_keys(&test_title).await?;
    driver.find(By::Css("input[type='submit']")).await?.click().await?;

    // Add first card to Good column
    let good_form = driver.find(By::Css("form[hx-target='#good-items']")).await?;
    let good_input = good_form.find(By::Tag("input")).await?;
    good_input.send_keys("First card").await?;
    good_input.send_keys("\u{E007}").await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Add second card to Bad column
    let bad_form = driver.find(By::Css("form[hx-target='#bad-items']")).await?;
    let bad_input = bad_form.find(By::Tag("input")).await?;
    bad_input.send_keys("Second card").await?;
    bad_input.send_keys("\u{E007}").await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Verify initial state of both cards
    let good_card = driver.find(By::Css("#good-items .card")).await?;
    let bad_card = driver.find(By::Css("#bad-items .card")).await?;

    let good_class = good_card.attr("class").await?.unwrap();
    let bad_class = bad_card.attr("class").await?.unwrap();
    assert_eq!(good_class.trim(), "card", "Good card should start in default state");
    assert_eq!(bad_class.trim(), "card", "Bad card should start in default state");

    // Click the first card (in Good column) and verify states
    good_card.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Re-fetch cards after the click
    let updated_good_card = driver.find(By::Css("#good-items .card")).await?;
    let updated_bad_card = driver.find(By::Css("#bad-items .card")).await?;

    // Verify first card is now highlighted
    let good_card_class = updated_good_card.attr("class").await?.unwrap();
    assert_eq!(good_card_class.trim(), "card highlighted", "Good card should be highlighted after click");

    // Verify bad card is still in default state
    let bad_card_class = updated_bad_card.attr("class").await?.unwrap();
    assert_eq!(bad_card_class.trim(), "card", "Bad card should remain in default state");

    // Try to click the second card (in Bad column) and verify states
    let fresh_bad_card = driver.find(By::Css("#bad-items .card")).await?;
    fresh_bad_card.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Re-fetch cards again after second click attempt
    let final_good_card = driver.find(By::Css("#good-items .card")).await?;
    let final_bad_card = driver.find(By::Css("#bad-items .card")).await?;

    // Verify good card is still highlighted and bad card is still in default state
    let final_good_class = final_good_card.attr("class").await?.unwrap();
    let final_bad_class = final_bad_card.attr("class").await?.unwrap();
    assert_eq!(final_good_class.trim(), "card highlighted", "Good card should remain highlighted");
    assert_eq!(final_bad_class.trim(), "card", "Bad card should still be in default state after attempted click");

    // Click the highlighted card and verify it transitions into Completed
    let final_good_card = driver.find(By::Css("#good-items .card")).await?;
    final_good_card.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Verify the card has transitioned to Completed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    let completed_card = driver.find(By::Css("#good-items .card")).await?;
    let completed_class = completed_card.attr("class").await?.unwrap();
    assert_eq!(completed_class.trim(), "card completed", "Good card should transition to Completed");

    // Ensure it's now possible to click another card
    let other_card = driver.find(By::Css(".card:not(.completed)")).await?;
    other_card.click().await?;

    // Clean up - delete the retro
    driver.goto("http://localhost:3000").await?;
    let cards = driver.find_all(By::ClassName("card")).await?;
    for card in cards {
        let links = card.find_all(By::Tag("a")).await?;
        for link in links {
            if link.text().await? == test_title {
                driver.execute("window.confirm = () => true", vec![]).await?;
                let delete_button = card.find(By::Tag("button")).await?;
                delete_button.click().await?;
                break;
            }
        }
    }

    driver.quit().await?;

    Ok(())
}

#[tokio::test]
async fn test_nonexistent_retro() -> WebDriverResult<()> {
    let gecko = start_geckodriver();

    let mut caps = DesiredCapabilities::firefox();
    if !should_show_browser() {
        caps.set_headless()?;
    }

    // Create Firefox preferences and set them
    let mut prefs = FirefoxPreferences::new();
    let _ = prefs.set("webdriver.log.level", "error");
    caps.set_preferences(prefs)?;

    let driver = WebDriver::new(&format!("http://localhost:{}", gecko.port), caps).await?;

    // Navigate to a non-existent retro
    driver.goto("http://localhost:3000/retro/99999").await?;

    // Find the body text and verify it contains "not found"
    let body = driver.find(By::Tag("body")).await?;
    let body_text = body.text().await?.to_lowercase();
    assert!(body_text.contains("not found"));

    // Always close the browser
    driver.quit().await?;

    Ok(())
}
