use crate::common::*;
use crate::test_helpers::*;
use thirtyfour::error::WebDriverResult;
use thirtyfour::prelude::*;
use thirtyfour::By;

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
async fn test_inline_validation_error_on_card_add() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page.create_retro("Inline Error Test").await?;

    // Whitespace-only text passes the textarea's `required` but fails the
    // server's trim check. The 400 is swapped into the form's error slot by
    // `hx-status:400`; the global noSwap would otherwise drop it silently.
    let form = retro_page
        .driver
        .find(By::Css("form[hx-target='#good-items']"))
        .await?;
    form.find(By::Tag("textarea"))
        .await?
        .send_keys("   ")
        .await?;
    form.find(By::Css("button[type='submit']"))
        .await?
        .click()
        .await?;

    retro_page
        .wait_for_inline_error("good-items-error", "Card text is required")
        .await?;
    // The rejected submission added no card.
    retro_page.wait_for_card_count("Good", 0).await?;

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
    retro_page.cancel_card().await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Verify card state
    retro_page.verify_card_state(card_id, "card").await?;

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

    // Attempting to highlight a second card should show an error on that card.
    // The error is rendered by the HTMX response to the highlight request,
    // which can lag the click under load; wait instead of finding it
    // immediately.
    retro_page.click_card(second_id).await?;
    retro_page
        .wait_for_card_error_message(second_id, "Only one item can be highlighted at a time")
        .await?;

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

    retro_page.wait_for_like_count(card_id, "0").await?;

    retro_page.like_card(card_id).await?;
    retro_page.wait_for_like_count(card_id, "1").await?;

    retro_page.like_card(card_id).await?;
    retro_page.wait_for_like_count(card_id, "0").await?;

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
