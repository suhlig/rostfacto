use crate::common::*;
use crate::test_helpers::*;
use thirtyfour::error::WebDriverResult;

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
    // count down from it. The deadline is sticky (it survives the elapse), so
    // the waits must accept only a deadline newer than the original one; the
    // equality check is below. Each "M:SS" countdown value lasts one second,
    // so use the tolerant at-most check for the running countdown.
    retro_a.click_extend(item_id).await?;
    let end_a = retro_a.wait_for_timer_end_after(item_id, end_a).await?;
    let end_b = retro_b.wait_for_timer_end_after(item_id, end_b).await?;
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
async fn test_extend_restarts_a_long_overdue_timer() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;

    let retros_page = browser.retros_page().await?;
    let retro = retros_page.create_retro("Overdue Timer Extend").await?;
    let item_id = retro.add_card("Good", "overdue card").await?;

    // Short auto-start duration so the badge reaches 0:00 quickly.
    retro
        .driver
        .execute("document.body.dataset.timerDefaultSeconds = '2'", vec![])
        .await?;
    retro.click_card(item_id).await?;
    retro.wait_for_timer_text(item_id, "0:00").await?;
    retro.wait_for_extend_button_visible(item_id).await?;

    // Make the timer overdue by more than the two minutes that +2 adds: with
    // the old `duration + 120` extend the deadline stayed in the past, so the
    // sweep instantly expired it again and the button looked broken.
    let pool = sqlx::PgPool::connect(&db.database_url)
        .await
        .expect("Failed to connect to test DB");
    sqlx::query("UPDATE items SET timer_started_at = NOW() - INTERVAL '10 minutes' WHERE id = $1")
        .bind(item_id)
        .execute(&pool)
        .await
        .expect("Failed to backdate the timer");
    drop(pool);

    // Pressing +2 must give a fresh, running countdown from the press rather
    // than leaving the badge stuck at 0:00.
    retro.click_extend(item_id).await?;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
    loop {
        let text = retro.timer_text(item_id).await?;
        let remaining = parse_timer(&text);
        if remaining > 0 {
            assert!(
                remaining <= 120,
                "extending an overdue timer should restart it at ~2 minutes, got {}",
                text
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the +2 min button did not restart the overdue timer (stuck at {})",
            text
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    browser.close().await?;
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

    // An action item is archived along with the cards, so it must clear too.
    retro_a.add_action_item("Action to archive").await?;
    retro_b
        .wait_for_action_item_with_text("Action to archive")
        .await?;

    // A completes the last card: B must see the all-done archive modal too.
    retro_a.click_card(item_id).await?;
    retro_b
        .verify_card_state(item_id, "card highlighted")
        .await?;
    retro_a.complete_card().await?;
    retro_b.verify_card_state(item_id, "card completed").await?;
    retro_a.wait_for_archive_modal().await?;
    retro_b.wait_for_archive_modal().await?;

    // A archives from the modal: B's board empties (cards and action items).
    retro_a.archive().await?;
    retro_b.wait_for_card_count("Good", 0).await?;
    retro_b.wait_for_action_item_count(0).await?;

    browser_a.close().await?;
    browser_b.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_sse_syncs_action_items_between_clients() -> WebDriverResult<()> {
    let _two_browsers = two_browser_permit().await;
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser_a = BrowserSession::new(&server.base_url()).await?;
    let browser_b = BrowserSession::new(&server.base_url()).await?;

    let retros_page = browser_a.retros_page().await?;
    let retro_a = retros_page.create_retro("SSE Sync Action Items").await?;
    let slug = retro_a.slug.clone();
    let retro_b = RetroPage::new(&browser_b.driver, &server.base_url(), &slug).await?;

    // A adds an action item: B sees it via SSE, and A shows exactly one (dedup).
    retro_a.add_action_item("Action from A").await?;
    retro_b
        .wait_for_action_item_with_text("Action from A")
        .await?;
    retro_a.wait_for_action_item_count(1).await?;
    retro_b.wait_for_action_item_count(1).await?;

    // B adds an action item: A sees it via SSE, and B shows exactly one (dedup).
    retro_b.add_action_item("Action from B").await?;
    retro_a
        .wait_for_action_item_with_text("Action from B")
        .await?;
    retro_a.wait_for_action_item_count(2).await?;
    retro_b.wait_for_action_item_count(2).await?;

    // A edits the first action item: B's copy updates in place.
    retro_a
        .edit_action_item("Action from A", "Edited action")
        .await?;
    retro_b
        .wait_for_action_item_with_text("Edited action")
        .await?;

    // B completes it: A sees the completed state.
    retro_b.complete_action_item("Edited action").await?;
    retro_a
        .wait_for_action_item_completed("Edited action")
        .await?;

    // A deletes it: B's copy disappears.
    retro_a.delete_action_item("Edited action").await?;
    retro_b.wait_for_action_item_count(1).await?;
    retro_b
        .wait_for_action_item_with_text("Action from B")
        .await?;

    browser_a.close().await?;
    browser_b.close().await?;
    Ok(())
}
