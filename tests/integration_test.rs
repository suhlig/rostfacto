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
    // The landing page shows the screenshot carousel, which must start on
    // the first slide, and the Postfacto comparison.
    home_page
        .driver
        .find(By::Css(".landing-carousel .carousel-slide.is-active"))
        .await?;
    home_page
        .driver
        .find(By::Css(".landing-differences"))
        .await?;
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
async fn test_delete_action_item_with_custom_confirmation_dialog() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Action Item Delete Test").await?;

    let retro_id = retro_page.retro_id().await?;
    let add_script = format!(
        "return fetch('/retro/{retro_id}/action-items', {{ method: 'POST', headers: {{ 'Content-Type': 'application/x-www-form-urlencoded' }}, body: 'text=Delete%20me' }}).then(r => r.status);"
    );
    let add_status = retro_page.driver.execute(&add_script, vec![]).await?;
    assert_eq!(add_status.json().as_u64(), Some(200));
    retro_page.driver.refresh().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    // Clicking delete must open a custom dialog instead of the native browser
    // confirmation (hx-confirm). The delete button is hidden until the item is
    // hovered, so move the pointer over the item first.
    let item = retro_page
        .driver
        .find(By::Css(".action-column .action-item"))
        .await?;
    item.scroll_into_view().await?;
    retro_page
        .driver
        .action_chain()
        .move_to_element_center(&item)
        .perform()
        .await?;
    item.find(By::Css(".action-item-delete"))
        .await?
        .click()
        .await?;
    let dialog = retro_page
        .driver
        .find(By::Css(".action-item-delete-dialog[open]"))
        .await?;
    assert_eq!(
        dialog.find(By::Css("h3")).await?.text().await?,
        "Delete this action item?"
    );

    // Cancelling keeps the action item on the page.
    dialog.find(By::Css(".btn-cancel")).await?.click().await?;
    assert_eq!(
        retro_page
            .driver
            .find_all(By::Css(".action-column .action-item"))
            .await?
            .len(),
        1,
        "Action item should remain on the page after cancelling"
    );

    // Confirming removes the action item from the page. Hover again because
    // the pointer moved to the (centered) dialog while cancelling.
    let item = retro_page
        .driver
        .find(By::Css(".action-column .action-item"))
        .await?;
    item.scroll_into_view().await?;
    retro_page
        .driver
        .action_chain()
        .move_to_element_center(&item)
        .perform()
        .await?;
    retro_page
        .driver
        .find(By::Css(".action-item-delete"))
        .await?
        .click()
        .await?;
    retro_page
        .driver
        .find(By::Css(".action-item-delete-dialog[open] .btn-primary"))
        .await?
        .click()
        .await?;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    loop {
        let remaining = retro_page
            .driver
            .find_all(By::Css(".action-column .action-item"))
            .await?;
        if remaining.is_empty() {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("Action item was not removed from the page after confirming deletion");
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_action_item_survives_edit_and_completion() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page
        .create_retro("Action Item Lifecycle Test")
        .await?;

    let retro_id = retro_page.retro_id().await?;
    let add_script = format!(
        "return fetch('/retro/{retro_id}/action-items', {{ method: 'POST', headers: {{ 'Content-Type': 'application/x-www-form-urlencoded' }}, body: 'text=Original%20action%20item' }}).then(r => r.status);"
    );
    let add_status = retro_page.driver.execute(&add_script, vec![]).await?;
    assert_eq!(add_status.json().as_u64(), Some(200));
    retro_page.driver.refresh().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let item = retro_page
        .driver
        .find(By::Css(".action-column .action-item"))
        .await?;

    assert_eq!(
        item.find(By::Css(".action-item-text"))
            .await?
            .text()
            .await?,
        "Original action item"
    );

    item.find(By::Css(".action-item-edit"))
        .await?
        .click()
        .await?;

    let edit_form = retro_page
        .driver
        .find(By::Css(".action-item.editing form"))
        .await?;
    let edit_input = edit_form.find(By::Css("input[name='text']")).await?;
    edit_input.clear().await?;
    edit_input.send_keys("Edited action item").await?;

    edit_form
        .find(By::Css("button[type='submit']"))
        .await?
        .click()
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let edited_item = retro_page
        .driver
        .find(By::Css(".action-column .action-item"))
        .await?;

    assert_eq!(
        edited_item
            .find(By::Css(".action-item-text"))
            .await?
            .text()
            .await?,
        "Edited action item"
    );

    edited_item
        .find(By::Css(".action-item-checkbox"))
        .await?
        .click()
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let completed_item = retro_page
        .driver
        .find(By::Css(".action-column .action-item.completed"))
        .await?;
    assert_eq!(
        completed_item
            .find(By::Css(".action-item-text"))
            .await?
            .text()
            .await?,
        "Edited action item"
    );

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_command_enter_submits_new_card() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Keyboard Shortcut Test").await?;

    let form = retro_page
        .driver
        .find(By::Css("form[hx-target='#good-items']"))
        .await?;
    let input = form.find(By::Tag("textarea")).await?;
    input.send_keys("Cmd+Enter card").await?;
    input.send_keys(Key::Control + Key::Enter).await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let cards = retro_page.get_cards_in_category("Good").await?;
    assert_eq!(
        cards.len(),
        1,
        "Card should be submitted via keyboard shortcut"
    );
    let card_text = cards[0].find(By::Css(".card-text")).await?.text().await?;
    assert_eq!(card_text, "Cmd+Enter card");

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_keyboard_shortcuts() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Keyboard Shortcuts Test").await?;

    let card_id = retro_page
        .add_card("Good", "Keyboard shortcut card")
        .await?;

    // Esc cancels inline editing.
    retro_page
        .driver
        .find(By::Css(format!(
            "article[data-item-id='{}'] .card-text-edit",
            card_id
        )))
        .await?
        .click()
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    let edit_input = retro_page
        .driver
        .find(By::Css(format!(
            "article[data-item-id='{}'] textarea[name='text']",
            card_id
        )))
        .await?;
    edit_input.send_keys(Key::Escape).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    retro_page.verify_card_state(card_id, "card").await?;

    // Enter highlights a focused card.
    let card = retro_page.get_card(card_id).await?;
    let script = r#"
        arguments[0].focus();
        arguments[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
        return true;
    "#;
    retro_page
        .driver
        .execute(script, vec![card.to_json()?])
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    retro_page
        .verify_card_state(card_id, "card highlighted")
        .await?;

    // Esc cancels the highlight.
    let highlighted_card = retro_page.get_card(card_id).await?;
    let script = r#"
        arguments[0].focus();
        arguments[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
        return true;
    "#;
    retro_page
        .driver
        .execute(script, vec![highlighted_card.to_json()?])
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    retro_page.verify_card_state(card_id, "card").await?;

    // L likes a focused card.
    let card = retro_page.get_card(card_id).await?;
    let script = r#"
        arguments[0].focus();
        arguments[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'l', bubbles: true, cancelable: true }));
        return true;
    "#;
    retro_page
        .driver
        .execute(script, vec![card.to_json()?])
        .await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    let like_count = retro_page
        .driver
        .find(By::Css(format!(
            "article[data-item-id='{}'] .like-count",
            card_id
        )))
        .await?
        .text()
        .await?;
    assert_eq!(like_count, "1", "L should like the focused card");

    // N focuses the add-card input.
    let active_class = retro_page
        .driver
        .execute(
            r#"
                document.body.focus();
                document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', bubbles: true, cancelable: true }));
                return document.activeElement ? document.activeElement.className : '';
            "#,
            vec![],
        )
        .await?
        .json()
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        active_class.contains("add-card-input"),
        "N should focus the add-card input, got classes: {}",
        active_class
    );

    // ? opens the keyboard shortcuts help dialog.
    let dialog_open = retro_page
        .driver
        .execute(
            r#"
                document.body.focus();
                document.body.dispatchEvent(new KeyboardEvent('keydown', { key: '?', bubbles: true, cancelable: true }));
                return document.getElementById('keyboard-help').hasAttribute('open');
            "#,
            vec![],
        )
        .await?
        .json()
        .as_bool()
        .unwrap();
    assert!(
        dialog_open,
        "? should open the keyboard shortcuts help dialog"
    );
    retro_page
        .driver
        .execute("document.getElementById('keyboard-help').close();", vec![])
        .await?;

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
        .verify_card_state(card_id, "card highlighted")
        .await?;
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

    let open_delete_dialogs = browser
        .driver
        .find_all(By::Css(".delete-confirm-dialog[open]"))
        .await?;
    assert!(
        open_delete_dialogs.is_empty(),
        "Delete confirmation dialog should close after deleting the retro"
    );

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

    // Wait for the highlight to land and the timer to count down from 5:00.
    // Polling (instead of a fixed sleep) keeps this reliable on slow machines,
    // where the highlight swap can lag.
    retro_page.wait_for_timer_text_at_most(card_id, 299).await?;
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

#[tokio::test]
async fn test_archive_snapshot_is_created_and_viewable() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Snapshot Archive Test").await?;

    let card_id = retro_page.add_card("Good", "Card in snapshot").await?;
    retro_page.click_card(card_id).await?;
    retro_page.complete_card().await?;
    retro_page.archive().await?;

    retro_page.navigate_to_archives().await?;
    let archive_rows = retro_page
        .driver
        .find_all(By::Css(".retro-table tbody tr"))
        .await?;
    assert_eq!(
        archive_rows.len(),
        1,
        "One archive snapshot should be listed"
    );

    let view_link = retro_page
        .driver
        .find(By::Css(".retro-table tbody td:last-child a"))
        .await?;
    view_link.click().await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let archived_cards = retro_page
        .driver
        .find_all(By::Css("#good-items .card"))
        .await?;
    assert_eq!(
        archived_cards.len(),
        1,
        "Archived card should be visible in snapshot"
    );

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_archive_link_disabled_when_empty() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Empty Retro Archive Link").await?;

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
    let class_attr = archive_link.attr("class").await?.unwrap_or_default();
    assert!(
        class_attr.contains("disabled"),
        "Archive link should be disabled when there are no cards"
    );

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_archive_empty_retro_creates_no_snapshot() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page
        .create_retro("Empty Retro Archive Server")
        .await?;

    // Submit the archive form directly via fetch, even though the UI disables the link
    let script = format!(
        r#"return fetch('/retro/{}/archive', {{
            method: 'POST',
            headers: {{'Content-Type': 'application/x-www-form-urlencoded'}}
        }}).then(r => r.status);"#,
        retro_page.retro_id().await?
    );
    let result = browser.driver.execute(&script, vec![]).await?;
    let status = result.json().as_u64().unwrap();
    assert_eq!(
        status, 200,
        "Archive request should redirect and be followed"
    );

    retro_page.navigate_to_archives().await?;
    let archive_rows = retro_page
        .driver
        .find_all(By::Css(".retro-table tbody tr"))
        .await?;
    assert_eq!(
        archive_rows.len(),
        0,
        "No archive snapshot should be created for an empty retro"
    );

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_sse_syncs_cards_between_clients() -> WebDriverResult<()> {
    let _two_browsers = two_browser_permit().await;
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser_a = BrowserSession::new(&server.base_url()).await?;
    let browser_b = BrowserSession::new(&server.base_url()).await?;

    let retros_page = browser_a.retros_page().await?;
    let retro_a = retros_page.create_retro("SSE Sync Cards").await?;
    let slug = retro_a.slug.clone();
    let retro_b = RetroPage::new(&browser_b.driver, &server.base_url(), &slug).await?;

    // A adds a card: B sees it via SSE, and A shows exactly one (dedup).
    retro_a.add_card("Good", "Card from A").await?;
    retro_b
        .wait_for_card_with_text("Good", "Card from A")
        .await?;
    retro_a.wait_for_card_count("Good", 1).await?;
    retro_b.wait_for_card_count("Good", 1).await?;

    // B adds a card: A sees it via SSE, and B shows exactly one (dedup).
    retro_b.add_card("Watch", "Card from B").await?;
    retro_a
        .wait_for_card_with_text("Watch", "Card from B")
        .await?;
    retro_a.wait_for_card_count("Watch", 1).await?;
    retro_b.wait_for_card_count("Watch", 1).await?;

    browser_a.close().await?;
    browser_b.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_sse_syncs_likes_text_and_status_between_clients() -> WebDriverResult<()> {
    let _two_browsers = two_browser_permit().await;
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser_a = BrowserSession::new(&server.base_url()).await?;
    let browser_b = BrowserSession::new(&server.base_url()).await?;

    let retros_page = browser_a.retros_page().await?;
    let retro_a = retros_page.create_retro("SSE Sync Mutations").await?;
    let slug = retro_a.slug.clone();
    let retro_b = RetroPage::new(&browser_b.driver, &server.base_url(), &slug).await?;

    let item_id = retro_a.add_card("Good", "Shared card").await?;
    retro_b
        .wait_for_card_with_text("Good", "Shared card")
        .await?;

    // B likes: both clients' counts update (B via HTMX, A via SSE).
    retro_b.like_card(item_id).await?;
    retro_b.wait_for_like_count(item_id, "1").await?;
    retro_a.wait_for_like_count(item_id, "1").await?;

    // A edits the text: B's card updates in place.
    retro_a.edit_card(item_id, "Edited text").await?;
    retro_b.wait_for_card_text(item_id, "Edited text").await?;

    // B highlights: A's card becomes highlighted.
    retro_b.click_card(item_id).await?;
    retro_a
        .verify_card_state(item_id, "card highlighted")
        .await?;

    // A completes: B's card becomes completed.
    retro_a.complete_card().await?;
    retro_b.verify_card_state(item_id, "card completed").await?;

    browser_a.close().await?;
    browser_b.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_sse_syncs_timers_between_clients() -> WebDriverResult<()> {
    let _two_browsers = two_browser_permit().await;
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser_a = BrowserSession::new(&server.base_url()).await?;
    let browser_b = BrowserSession::new(&server.base_url()).await?;

    let retros_page = browser_a.retros_page().await?;
    let retro_a = retros_page.create_retro("SSE Sync Timers").await?;
    let slug = retro_a.slug.clone();
    let retro_b = RetroPage::new(&browser_b.driver, &server.base_url(), &slug).await?;

    let item_id = retro_a.add_card("Good", "Timed card").await?;
    retro_b
        .wait_for_card_with_text("Good", "Timed card")
        .await?;

    // Use a short auto-start duration so the timer elapses quickly.
    let script = "document.body.dataset.timerDefaultSeconds = '2'";
    retro_a.driver.execute(script, vec![]).await?;

    // A highlights the card: A's client starts the timer, and B must show the
    // identical server-rendered deadline.
    retro_a.click_card(item_id).await?;
    retro_b
        .verify_card_state(item_id, "card highlighted")
        .await?;
    let end_a = retro_a.wait_for_timer_end_at(item_id).await?;
    let end_b = retro_b.wait_for_timer_end_at(item_id).await?;
    assert_eq!(
        end_a, end_b,
        "both clients should show the same timer deadline"
    );

    // Both count down to 0:00 and reveal the +2 min button.
    retro_a.wait_for_timer_text(item_id, "0:00").await?;
    retro_b.wait_for_timer_text(item_id, "0:00").await?;
    retro_a.wait_for_extend_button_visible(item_id).await?;
    retro_b.wait_for_extend_button_visible(item_id).await?;

    // A extends the timer: both clients show the same new deadline, and both
    // count down from it. The deadline equality is checked above; here we only
    // need to see the countdown running (each "M:SS" value lasts one second,
    // so use the tolerant at-most check).
    retro_a.click_extend(item_id).await?;
    let end_a = retro_a.wait_for_timer_end_at(item_id).await?;
    let end_b = retro_b.wait_for_timer_end_at(item_id).await?;
    assert_eq!(
        end_a, end_b,
        "both clients should show the extended deadline"
    );
    retro_a.wait_for_timer_text_at_most(item_id, 125).await?;
    retro_b.wait_for_timer_text_at_most(item_id, 125).await?;

    // A cancels the highlight: both clients lose the timer badge.
    retro_a.cancel_card().await?;
    retro_a.verify_card_state(item_id, "card").await?;
    retro_b.verify_card_state(item_id, "card").await?;

    browser_a.close().await?;
    browser_b.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_sse_syncs_archive_and_all_done_modal_between_clients() -> WebDriverResult<()> {
    let _two_browsers = two_browser_permit().await;
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser_a = BrowserSession::new(&server.base_url()).await?;
    let browser_b = BrowserSession::new(&server.base_url()).await?;

    let retros_page = browser_a.retros_page().await?;
    let retro_a = retros_page.create_retro("SSE Sync Archive").await?;
    let slug = retro_a.slug.clone();
    let retro_b = RetroPage::new(&browser_b.driver, &server.base_url(), &slug).await?;

    let item_id = retro_a.add_card("Good", "Last card").await?;
    retro_b.wait_for_card_with_text("Good", "Last card").await?;

    // A completes the last card: B must see the all-done archive modal too.
    retro_a.click_card(item_id).await?;
    retro_b
        .verify_card_state(item_id, "card highlighted")
        .await?;
    retro_a.complete_card().await?;
    retro_b.verify_card_state(item_id, "card completed").await?;
    retro_a.wait_for_archive_modal().await?;
    retro_b.wait_for_archive_modal().await?;

    // A archives from the modal: B's board empties.
    retro_a.archive().await?;
    retro_b.wait_for_card_count("Good", 0).await?;

    browser_a.close().await?;
    browser_b.close().await?;
    Ok(())
}
