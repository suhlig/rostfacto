mod test_helpers;

use test_helpers::*;
use thirtyfour::error::WebDriverResult;
use thirtyfour::By;

#[tokio::test]
async fn test_home_page() -> WebDriverResult<()> {
    let browser = BrowserSession::new().await?;
    let home_page = browser.home_page().await?;
    home_page.verify_title("Rostfacto").await?;
    Ok(())
}

#[tokio::test]
async fn test_invalid_url() -> WebDriverResult<()> {
    let browser = BrowserSession::new().await?;

    // Navigate to non-existent page
    browser
        .driver
        .goto("http://localhost:3000/non-existent-page")
        .await?;

    // Verify 404 page content
    let error_code = browser.driver.find(By::Css(".error-page h1")).await?;
    assert_eq!(error_code.text().await?, "404");

    let error_message = browser.driver.find(By::Css(".error-page p")).await?;
    assert_eq!(error_message.text().await?, "Page not found");

    // Test home link works
    let home_link = browser.driver.find(By::Css(".error-page a")).await?;
    home_link.click().await?;

    let current_url = browser.driver.current_url().await?;
    assert!(current_url.to_string().ends_with("/"));

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
    retro_page
        .verify_card_state(card1_id, "card highlighted")
        .await?;
    retro_page.verify_card_state(card2_id, "card").await?;

    // Test failed highlight attempt
    retro_page.click_card(card2_id).await?;
    retro_page
        .verify_card_state(card1_id, "card highlighted")
        .await?;
    retro_page.verify_card_state(card2_id, "card").await?;

    // Complete card and verify transition
    retro_page.complete_card().await?;
    retro_page
        .verify_card_state(card1_id, "card completed")
        .await?;

    // Verify other card can now be clicked
    retro_page.click_card(card2_id).await?;
    retro_page
        .verify_card_state(card2_id, "card highlighted")
        .await?;

    retro_page.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn test_nonexistent_retro() -> WebDriverResult<()> {
    let browser = BrowserSession::new().await?;

    // Navigate to non-existent retro directly
    browser
        .driver
        .goto("http://localhost:3000/retro/99999")
        .await?;

    // Verify 404 page content
    let error_code = browser.driver.find(By::Css(".error-page h1")).await?;
    assert_eq!(error_code.text().await?, "404");

    let error_message = browser.driver.find(By::Css(".error-page p")).await?;
    assert_eq!(
        error_message.text().await?,
        "No retrospective with slug '99999' found"
    );

    let home_link = browser.driver.find(By::Css(".error-page a")).await?;
    assert_eq!(home_link.text().await?, "← Return to homepage");
    assert!(home_link.attr("href").await?.unwrap().contains("/"));

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
    retro_page
        .driver
        .find(By::Css("#archive-modal .secondary"))
        .await?
        .click()
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // Verify completed card remains visible
    retro_page
        .verify_card_state(card_id, "card completed")
        .await?;

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
    retro_page
        .driver
        .find(By::Css(".secondary"))
        .await?
        .click()
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Verify card state
    retro_page.verify_card_state(card_id, "card").await?;

    retro_page.cleanup().await?;
    Ok(())
}
