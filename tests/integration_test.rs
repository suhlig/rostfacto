mod test_helpers;

use thirtyfour::{WebDriver, By, DesiredCapabilities};
use thirtyfour::error::WebDriverResult;
use thirtyfour::common::capabilities::firefox::FirefoxPreferences;
use test_helpers::*;

#[tokio::test]
async fn test_home_page() -> WebDriverResult<()> {
    let gecko = test_helpers::start_geckodriver();

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

    // Find the h1 element and verify its text
    let h1 = driver.find(By::Css("h1")).await?;
    assert_eq!(h1.text().await?, "Rostfacto");

    // Close the browser
    driver.quit().await?;

    Ok(())
}

#[tokio::test]
async fn test_archive_retro() -> WebDriverResult<()> {
    let browser = BrowserSession::new().await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Archive Test Retro").await?;

    // Add and process test card
    retro_page.add_card("Good", "Card to archive").await?;
    retro_page.click_card("Card to archive").await?;
    retro_page.complete_card().await?;

    // Handle archive flow
    retro_page.archive().await?;

    // Verify all cards are archived
    let remaining_cards = retro_page.driver.find_all(By::ClassName("card")).await?;
    assert_eq!(remaining_cards.len(), 0, "All cards should be archived");

    retro_page.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn test_create_cards() -> WebDriverResult<()> {
    let browser = BrowserSession::new().await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Test Retro").await?;

    // Add cards to all categories
    retro_page.add_card("Good", "Good point test").await?;
    retro_page.add_card("Bad", "Bad point test").await?;
    retro_page.add_card("Watch", "Watch point test").await?;

    // Verify card states
    retro_page.verify_card_state("Good point test", "card").await?;
    retro_page.verify_card_state("Bad point test", "card").await?;
    retro_page.verify_card_state("Watch point test", "card").await?;

    retro_page.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn test_create_retro() -> WebDriverResult<()> {
    let browser = BrowserSession::new().await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Test Retro").await?;

    // Verify the retro title is shown
    let title = retro_page.driver.find(By::Css("h1")).await?;
    assert_eq!(title.text().await?, retro_page.title);

    retro_page.cleanup().await?;
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
    let test_title = create_test_retro(&driver, "State Test Retro").await?;

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

    // Re-fetch cards after the click to avoid stale element references
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

    // Complete the card - get fresh reference to avoid stale element
    driver.find(By::Css(".primary")).await?.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Verify the card has transitioned to Completed
    let completed_card = driver.find(By::Css("#good-items .card.completed")).await?;
    let completed_class = completed_card.attr("class").await?.unwrap();
    assert_eq!(completed_class.trim(), "card completed", "Good card should transition to Completed");

    // Ensure it's now possible to click another card
    let other_card = driver.find(By::Css(".card:not(.completed)")).await?;
    other_card.click().await?;

    // Clean up - delete the retro
    cleanup_retro(&driver, &test_title).await?;

    // Always close the browser
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

#[tokio::test]
async fn test_archived_card_display() -> WebDriverResult<()> {
    let gecko = start_geckodriver();
    let mut caps = DesiredCapabilities::firefox();
    if !should_show_browser() {
        caps.set_headless()?;
    }

    let mut prefs = FirefoxPreferences::new();
    prefs.set("webdriver.log.level", "error")?;
    caps.set_preferences(prefs)?;

    let driver = WebDriver::new(&format!("http://localhost:{}", gecko.port), caps).await?;

    // Create test retro
    let test_title = create_test_retro(&driver, "Archive Display Test").await?;

    // Add and archive test card
    let card_text = "Ephemeral test card";
    let form = driver.find(By::Css("form[hx-target='#good-items']")).await?;
    let input = form.find(By::Tag("input")).await?;
    input.send_keys(card_text).await?;
    input.send_keys("\u{E007}").await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Highlight the card
    driver.find(By::Css(".card")).await?.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Complete the card - get fresh reference to avoid stale element
    driver.find(By::Css(".primary")).await?.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Deny archiving the retro
    driver.find(By::Css("#archive-modal .secondary")).await?.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // TODO verify only complete cards are displayed

    // Clean up - delete the retro
    cleanup_retro(&driver, &test_title).await?;

    // Always close the browser
    driver.quit().await?;

    Ok(())
}

#[tokio::test]
async fn test_cancel_highlighted_card() -> WebDriverResult<()> {
    let gecko = start_geckodriver();
    let mut caps = DesiredCapabilities::firefox();
    if !should_show_browser() {
        caps.set_headless()?;
    }

    let mut prefs = FirefoxPreferences::new();
    prefs.set("webdriver.log.level", "error")?;
    caps.set_preferences(prefs)?;

    let driver = WebDriver::new(&format!("http://localhost:{}", gecko.port), caps).await?;

    // Create test retro
    let test_title = create_test_retro(&driver, "Cancel Test Retro").await?;

    // Add test card
    let form = driver.find(By::Css("form[hx-target='#good-items']")).await?;
    let input = form.find(By::Tag("input")).await?;
    input.send_keys("Cancel test card").await?;
    input.send_keys("\u{E007}").await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Click to highlight
    let card = driver.find(By::Css(".card")).await?;
    card.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Click cancel button
    driver.find(By::Css(".secondary")).await?.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Verify card state
    let updated_card = driver.find(By::Css(".card")).await?;
    let class_attr = updated_card.attr("class").await?.unwrap();
    assert!(!class_attr.contains("highlighted"), "Card should not be highlighted after cancel");
    assert!(!class_attr.contains("completed"), "Card should not be completed after cancel");
    assert_eq!(class_attr.trim(), "card", "Card should be in default state");

    // Clean up - delete the retro
    cleanup_retro(&driver, &test_title).await?;

    // Always close the browser
    driver.quit().await?;

    Ok(())
}
