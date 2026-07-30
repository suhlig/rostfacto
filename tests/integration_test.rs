mod test_helpers;

use test_helpers::*;
use thirtyfour::error::WebDriverResult;
use thirtyfour::prelude::*;
use thirtyfour::By;

fn parse_timer(text: &str) -> u32 {
    let parts: Vec<&str> = text.trim().split(':').collect();
    assert_eq!(parts.len(), 2, "Timer should be in m:ss format");
    let minutes: u32 = parts[0].parse().expect("Invalid timer minutes");
    let seconds: u32 = parts[1].parse().expect("Invalid timer seconds");
    minutes * 60 + seconds
}

/// Submit the new-retro form directly via HTTP to test server-side validation.
async fn post_retros_form(
    driver: &WebDriver,
    base_url: &str,
    title: &str,
    slug: &str,
) -> WebDriverResult<(u64, String)> {
    driver.goto(base_url).await?;
    let script = format!(
        r#"return fetch('/retros', {{
            method: 'POST',
            headers: {{'Content-Type': 'application/x-www-form-urlencoded'}},
            body: 'title={title}&slug={slug}'
        }}).then(async r => ({{status: r.status, text: await r.text()}}));"#
    );
    let result = driver.execute(&script, vec![]).await?;
    let result = result.json();
    let status = result["status"].as_u64().unwrap();
    let text = result["text"].as_str().unwrap().to_string();
    Ok((status, text))
}

/// Fetch a path and return the HTTP status code.
async fn fetch_status(driver: &WebDriver, base_url: &str, path: &str) -> WebDriverResult<u64> {
    driver.goto(base_url).await?;
    let script = format!(r#"return fetch('{}').then(r => r.status);"#, path);
    let result = driver.execute(&script, vec![]).await?;
    Ok(result.json().as_u64().unwrap())
}

#[tokio::test]
async fn test_home_page() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let home_page = browser.home_page().await?;
    home_page.verify_title("Rostfacto").await?;
    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_invalid_url() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;

    // Navigate to non-existent page
    browser
        .driver
        .goto(format!("{}/non-existent-page", server.base_url()).as_str())
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

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_archive_retro() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
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

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_archive_retro_from_menu() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Menu Archive Test Retro").await?;

    // Add, highlight, and complete the only card
    let card_id = retro_page.add_card("Good", "Card to archive").await?;
    retro_page.click_card(card_id).await?;
    retro_page.complete_card().await?;

    // Dismiss the automatic archive modal
    let cancel_button = retro_page
        .driver
        .find(By::Css("#archive-modal .btn-cancel"))
        .await?;
    cancel_button.click().await?;

    // Refresh so the server-rendered menu includes the archive button.
    // The archive modal is shown again on load, so dismiss it first.
    retro_page.driver.refresh().await?;
    retro_page
        .driver
        .find(By::Css("#archive-modal .btn-cancel"))
        .await?
        .click()
        .await?;

    // Archive all cards from the account menu
    retro_page.archive_from_menu().await?;

    // Verify all cards are archived
    let remaining_cards = retro_page.driver.find_all(By::ClassName("card")).await?;
    assert_eq!(remaining_cards.len(), 0, "All cards should be archived");

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_archive_menu_shows_confirmation_dialog_for_unaddressed_cards() -> WebDriverResult<()>
{
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page
        .create_retro("Menu Archive Dialog Test Retro")
        .await?;

    let _card_id = retro_page.add_card("Good", "Unaddressed card").await?;

    retro_page
        .driver
        .find(By::Css(".account-menu button"))
        .await?
        .click()
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let archive_link = retro_page
        .driver
        .find(By::Css(".archive-menu-link"))
        .await?;
    assert!(archive_link.is_displayed().await?);
    archive_link.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let dialog = retro_page
        .driver
        .find(By::Css(".archive-confirm-dialog[open]"))
        .await?;
    assert!(
        dialog.is_displayed().await?,
        "Confirmation dialog should be shown for unaddressed cards"
    );

    // Cancel the dialog
    dialog.find(By::Css(".btn-cancel")).await?.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    // Card should still be present
    let remaining_cards = retro_page.driver.find_all(By::ClassName("card")).await?;
    assert_eq!(
        remaining_cards.len(),
        1,
        "Card should remain after canceling the archive dialog"
    );

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_archive_retro_from_menu_with_unaddressed_cards() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page
        .create_retro("Menu Archive Unaddressed Test Retro")
        .await?;

    // Add a card but leave it unaddressed
    retro_page.add_card("Good", "Unaddressed card").await?;

    // Archive all cards from the account menu; this should show a confirmation dialog
    retro_page.archive_from_menu().await?;

    // Verify all cards are archived
    let remaining_cards = retro_page.driver.find_all(By::ClassName("card")).await?;
    assert_eq!(remaining_cards.len(), 0, "All cards should be archived");

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_create_cards() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
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

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_edit_card() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Edit Card Multiline Test").await?;

    let new_card_input = retro_page
        .driver
        .find(By::Css("form[hx-target='#good-items'] textarea"))
        .await?;
    let initial_height = retro_page
        .driver
        .execute(
            "return arguments[0].getBoundingClientRect().height",
            vec![new_card_input.to_json()?],
        )
        .await?
        .json()
        .as_f64()
        .unwrap();
    new_card_input
        .send_keys("First line\nSecond line\nThird line")
        .await?;
    let expanded_height = retro_page
        .driver
        .execute(
            "return arguments[0].getBoundingClientRect().height",
            vec![new_card_input.to_json()?],
        )
        .await?
        .json()
        .as_f64()
        .unwrap();
    assert!(expanded_height > initial_height);
    new_card_input.clear().await?;

    let card_id = retro_page
        .add_card("Good", "Original first line\nOriginal second line")
        .await?;
    retro_page
        .edit_card(card_id, "Updated first line\nUpdated second line")
        .await?;

    let card = retro_page.get_card(card_id).await?;
    let card_text = card.find(By::Css(".card-text")).await?;
    assert_eq!(
        card_text.text().await?,
        "Updated first line\nUpdated second line"
    );

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_create_retro() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Test Retro").await?;

    // Verify the retro title is shown
    let title = retro_page.driver.find(By::Css("h1")).await?;
    assert_eq!(title.text().await?, retro_page.title);

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_card_state_transitions() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
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

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_nonexistent_retro() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;

    // Navigate to non-existent retro directly
    browser
        .driver
        .goto(format!("{}/retro/99999", server.base_url()).as_str())
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

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_archived_card_display() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
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

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_cancel_highlighted_card() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Cancel Test Retro").await?;

    // Add test card and get its ID
    let card_id = retro_page.add_card("Good", "Cancel test card").await?;

    // Click to highlight and cancel
    retro_page.click_card(card_id).await?;
    retro_page
        .driver
        .find(By::Css(".card-actions .secondary"))
        .await?
        .click()
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Verify card state
    retro_page.verify_card_state(card_id, "card").await?;

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_retros_trailing_slash() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let status = fetch_status(&browser.driver, &server.base_url(), "/retros/").await?;
    assert_eq!(status, 200, "/retros/ should be normalized to /retros");
    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_retros_list_page() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("List Test Retro").await?;

    // Return to the list page and verify the retro appears
    let retros_page = browser.retros_page().await?;
    let _ = retros_page;
    let row = browser
        .driver
        .find(By::XPath(format!(
            "//table//tr[contains(., '{}')]",
            retro_page.title
        )))
        .await?;
    let link = row.find(By::Tag("a")).await?;
    assert_eq!(link.text().await?, retro_page.title);
    assert!(link.attr("href").await?.unwrap().contains("/retro/"));

    let delete_button = row.find(By::Tag("button")).await?;
    assert_eq!(delete_button.text().await?, "Delete");

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_create_retro_validation_empty_slug() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let (status, text) = post_retros_form(&browser.driver, &server.base_url(), "Test", "").await?;
    assert_eq!(status, 400);
    assert!(text.contains("Slug is required"));
    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_create_retro_validation_invalid_slug() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let (status, text) =
        post_retros_form(&browser.driver, &server.base_url(), "Test", "BadSlug").await?;
    assert_eq!(status, 400);
    assert!(text.contains("Slug can only contain lowercase letters, numbers, and dashes"));
    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_create_retro_validation_long_slug() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let slug = "a".repeat(256);
    let (status, text) =
        post_retros_form(&browser.driver, &server.base_url(), "Test", &slug).await?;
    assert_eq!(status, 400);
    assert!(text.contains("Slug must be 255 characters or less"));
    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_create_retro_validation_duplicate_slug() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page
        .create_retro_with_slug("Duplicate Test", "duplicate-test-slug")
        .await?;

    let (status, text) = post_retros_form(
        &browser.driver,
        &server.base_url(),
        "Another",
        &retro_page.slug,
    )
    .await?;
    assert_eq!(status, 500);
    assert!(text.contains("Slug is already in use"));

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_delete_retro() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Delete Test Retro").await?;

    retro_page.delete().await?;

    // Verify the retro is gone
    browser
        .driver
        .goto(format!("{}/retro/{}", server.base_url(), retro_page.slug).as_str())
        .await?;
    let error_code = browser.driver.find(By::Css(".error-page h1")).await?;
    assert_eq!(error_code.text().await?, "404");

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_item_ordering() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Ordering Test Retro").await?;

    let first_id = retro_page.add_card("Good", "first card").await?;
    let second_id = retro_page.add_card("Good", "second card").await?;

    let cards = retro_page.get_cards_in_category("Good").await?;
    assert_eq!(cards.len(), 2);

    // HTMX prepends new cards, so the newest card appears first.
    let first_card_id = cards[0]
        .attr("data-item-id")
        .await?
        .unwrap()
        .parse::<i32>()
        .unwrap();
    let second_card_id = cards[1]
        .attr("data-item-id")
        .await?
        .unwrap()
        .parse::<i32>()
        .unwrap();
    assert_eq!(first_card_id, second_id);
    assert_eq!(second_card_id, first_id);

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_completed_card_cannot_be_highlighted() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Completed Lock Test").await?;

    let card_id = retro_page.add_card("Good", "completed card").await?;
    // Leave another card active so completing the first does not trigger the archive modal
    let other_id = retro_page.add_card("Bad", "active card").await?;

    retro_page.click_card(card_id).await?;
    retro_page.complete_card().await?;
    retro_page
        .verify_card_state(card_id, "card completed")
        .await?;
    retro_page.verify_card_state(other_id, "card").await?;

    // Try to click the completed card again
    retro_page.click_card(card_id).await?;
    retro_page
        .verify_card_state(card_id, "card completed")
        .await?;

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_single_highlight_error_message() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Highlight Error Test").await?;

    let first_id = retro_page.add_card("Good", "first card").await?;
    let second_id = retro_page.add_card("Good", "second card").await?;

    retro_page.click_card(first_id).await?;
    retro_page
        .verify_card_state(first_id, "card highlighted")
        .await?;

    // Attempting to highlight a second card should show an error on that card
    retro_page.click_card(second_id).await?;
    let second_card = retro_page.get_card(second_id).await?;
    let error = second_card.find(By::Css(".error-message")).await?;
    assert_eq!(
        error.text().await?,
        "Only one item can be highlighted at a time"
    );

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_like_card() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Like Test Retro").await?;

    let card_id = retro_page.add_card("Good", "card to like").await?;

    let card = retro_page.get_card(card_id).await?;
    let count = card.find(By::Css(".like-count")).await?.text().await?;
    assert_eq!(count, "0", "New cards should start with zero likes");

    retro_page.like_card(card_id).await?;
    let card = retro_page.get_card(card_id).await?;
    let count = card.find(By::Css(".like-count")).await?.text().await?;
    assert_eq!(count, "1", "Liking a card should increment the count");

    retro_page.like_card(card_id).await?;
    let card = retro_page.get_card(card_id).await?;
    let count = card.find(By::Css(".like-count")).await?.text().await?;
    assert_eq!(count, "0", "Liking again should toggle the like off");

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_like_does_not_restart_highlighted_timer() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Timer Like Test Retro").await?;

    let card_id = retro_page.add_card("Good", "card to time").await?;
    retro_page.click_card(card_id).await?;

    // Wait long enough to be sure the timer has counted down from 5:00.
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    let before = retro_page.timer_text(card_id).await?;
    let before_seconds = parse_timer(&before);
    assert!(
        before_seconds < 300,
        "Timer should have counted down before the like (got {})",
        before
    );

    retro_page.like_card(card_id).await?;
    let after = retro_page.timer_text(card_id).await?;
    let after_seconds = parse_timer(&after);
    assert!(
        after_seconds < 300,
        "Timer should not restart after liking the card (got {})",
        after
    );
    assert!(
        after_seconds <= before_seconds,
        "Timer should not jump forward after liking (before: {}, after: {})",
        before,
        after
    );

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_static_asset_served() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    browser
        .driver
        .goto(format!("{}/static/happy.svg", server.base_url()).as_str())
        .await?;
    let svg = browser.driver.find(By::Tag("svg")).await?;
    assert!(svg.is_displayed().await?);
    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_missing_static_file_404() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let status = fetch_status(
        &browser.driver,
        &server.base_url(),
        "/static/nonexistent.css",
    )
    .await?;
    assert_eq!(status, 404);
    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_demo_banner_shown() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    browser.driver.goto(&server.base_url()).await?;
    let banner = browser.driver.find(By::Css(".demo-banner")).await?;
    let text = banner.text().await?;
    assert!(text.contains("unsecured demo instance"));
    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_auth_login_redirects_in_demo_mode() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    // In demo mode, /auth/login should redirect back to / (demo mode has no real OAuth).
    let status = fetch_status(&browser.driver, &server.base_url(), "/auth/login").await?;
    assert_eq!(status, 200, "/auth/login should be reachable in demo mode");
    browser.close().await?;
    Ok(())
}
