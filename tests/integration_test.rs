mod test_helpers;

use thirtyfour::{WebDriver, By, DesiredCapabilities};
use thirtyfour::error::WebDriverResult;
use thirtyfour::common::capabilities::firefox::FirefoxPreferences;
use test_helpers::*;

#[tokio::test]
async fn test_home_page() -> WebDriverResult<()> {
    let browser = BrowserSession::new().await?;
    let home_page = browser.home_page().await?;
    home_page.verify_title("Rostfacto").await?;
    Ok(())
}

#[tokio::test]
async fn test_archive_retro() -> WebDriverResult<()> {
    let browser = BrowserSession::new().await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Archive Test Retro").await?;

    // Add and process test card
    let card_id = retro_page.add_card("Good", "Card to archive").await?;
    retro_page.click_card(card_id).await?;
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

    // Add cards to all categories and get their IDs
    let good_id = retro_page.add_card("Good", "Good point test").await?;
    let bad_id = retro_page.add_card("Bad", "Bad point test").await?;
    let watch_id = retro_page.add_card("Watch", "Watch point test").await?;

    // Verify card states using IDs
    retro_page.verify_card_state(good_id, "card").await?;
    retro_page.verify_card_state(bad_id, "card").await?;
    retro_page.verify_card_state(watch_id, "card").await?;

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
    let browser = BrowserSession::new().await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("State Test Retro").await?;

    // Add test cards
    let card1_id = retro_page.add_card("Good", "First card").await?;
    let card2_id = retro_page.add_card("Bad", "Second card").await?;

    // Verify initial states
    retro_page.verify_card_state(card1_id, "card").await?;
    retro_page.verify_card_state(card2_id, "card").await?;

    // Test highlighting
    retro_page.click_card(card1_id).await?;
    retro_page.verify_card_state(card1_id, "card highlighted").await?;
    retro_page.verify_card_state(card2_id, "card").await?;

    // Test failed highlight attempt
    retro_page.click_card(card2_id).await?;
    retro_page.verify_card_state(card1_id, "card highlighted").await?;
    retro_page.verify_card_state(card2_id, "card").await?;

    // Complete card and verify transition
    retro_page.complete_card().await?;
    retro_page.verify_card_state(card1_id, "card completed").await?;

    // Verify other card can now be clicked
    retro_page.click_card(card2_id).await?;
    retro_page.verify_card_state(card2_id, "card highlighted").await?;

    retro_page.cleanup().await?;
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
    let browser = BrowserSession::new().await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Archive Display Test").await?;

    // Add and process test card
    let card_id = retro_page.add_card("Good", "Ephemeral test card").await?;
    retro_page.click_card(card_id).await?;
    retro_page.complete_card().await?;

    // Deny archiving the retro
    retro_page.driver.find(By::Css("#archive-modal .secondary")).await?.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // Verify completed card remains visible
    retro_page.verify_card_state(card_id, "card completed").await?;

    retro_page.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn test_cancel_highlighted_card() -> WebDriverResult<()> {
    let browser = BrowserSession::new().await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Cancel Test Retro").await?;

    // Add test card and get its ID
    let card_id = retro_page.add_card("Good", "Cancel test card").await?;

    // Click to highlight and cancel
    retro_page.click_card(card_id).await?;
    retro_page.driver.find(By::Css(".secondary")).await?.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Verify card state
    retro_page.verify_card_state(card_id, "card").await?;

    retro_page.cleanup().await?;
    Ok(())
}
