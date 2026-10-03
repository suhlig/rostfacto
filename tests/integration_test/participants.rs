use crate::test_helpers::*;
use thirtyfour::error::WebDriverResult;

#[tokio::test]
async fn test_participants_panel_shows_single_browser_once() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;

    let retros_page = browser.retros_page().await?;
    let retro = retros_page.create_retro("Participants Single").await?;

    retro.wait_for_participant_count(1).await?;
    assert_eq!(retro.participant_names().await?, vec!["Guest 1"]);
    assert_eq!(retro.participant_counter_text().await?, "1");

    // The entry must be stable: no duplicate appears after the initial snapshot.
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    assert_eq!(retro.participant_count().await?, 1);

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_participants_panel_lists_two_browsers_in_join_order() -> WebDriverResult<()> {
    let _two_browsers = two_browser_permit().await;
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser_a = BrowserSession::new(&server.base_url()).await?;
    let browser_b = BrowserSession::new(&server.base_url()).await?;

    let retros_page = browser_a.retros_page().await?;
    let retro_a = retros_page.create_retro("Participants Two").await?;
    // A must be registered before B joins so the join order is deterministic.
    retro_a.wait_for_participant_count(1).await?;

    let retro_b = RetroPage::new(&browser_b.driver, &server.base_url(), &retro_a.slug).await?;

    retro_a.wait_for_participant_count(2).await?;
    retro_b.wait_for_participant_count(2).await?;

    let expected = vec!["Guest 1".to_string(), "Guest 2".to_string()];
    assert_eq!(retro_a.participant_names().await?, expected);
    assert_eq!(retro_b.participant_names().await?, expected);
    assert_eq!(retro_a.participant_counter_text().await?, "2");
    assert_eq!(retro_b.participant_counter_text().await?, "2");

    browser_a.close().await?;
    browser_b.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_participants_panel_dedups_tabs_of_same_browser() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;

    let retros_page = browser.retros_page().await?;
    let retro = retros_page.create_retro("Participants Tabs").await?;
    retro.wait_for_participant_count(1).await?;

    let first_tab = browser.driver.window().await?;
    let second_tab = browser.driver.new_tab().await?;
    browser.driver.switch_to_window(second_tab).await?;
    let retro_second_tab = RetroPage::new(&browser.driver, &server.base_url(), &retro.slug).await?;

    // The second tab shares the localStorage id, so it is the same participant.
    retro_second_tab.wait_for_participant_count(1).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    assert_eq!(retro_second_tab.participant_count().await?, 1);
    assert_eq!(retro_second_tab.participant_names().await?, vec!["Guest 1"]);

    browser.driver.switch_to_window(first_tab).await?;
    assert_eq!(retro.participant_count().await?, 1);

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_participants_panel_removes_participant_after_disconnect() -> WebDriverResult<()> {
    let _two_browsers = two_browser_permit().await;
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser_a = BrowserSession::new(&server.base_url()).await?;
    let browser_b = BrowserSession::new(&server.base_url()).await?;

    let retros_page = browser_a.retros_page().await?;
    let retro_a = retros_page.create_retro("Participants Leave").await?;
    retro_a.wait_for_participant_count(1).await?;

    let retro_b = RetroPage::new(&browser_b.driver, &server.base_url(), &retro_a.slug).await?;
    retro_a.wait_for_participant_count(2).await?;
    retro_b.wait_for_participant_count(2).await?;

    // B leaves the board: its SSE connection drops and, after the (short,
    // test-configured) grace period, A's roster shrinks back to one.
    browser_b.driver.goto(&server.base_url()).await?;
    retro_a.wait_for_participant_count(1).await?;
    assert_eq!(retro_a.participant_names().await?, vec!["Guest 1"]);
    assert_eq!(retro_a.participant_counter_text().await?, "1");

    browser_a.close().await?;
    browser_b.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_participants_panel_is_shared_across_app_instances() -> WebDriverResult<()> {
    let _two_browsers = two_browser_permit().await;
    let db = TestDb::new().await;
    // Two app processes behind one database, like a load-balanced deployment.
    let (server_1, server_2) = TestServer::start_pair(&db.database_url).await;
    let browser_a = BrowserSession::new(&server_1.base_url()).await?;
    let browser_b = BrowserSession::new(&server_2.base_url()).await?;

    let retros_page = browser_a.retros_page().await?;
    let retro_a = retros_page
        .create_retro("Participants Multi Process")
        .await?;
    retro_a.wait_for_participant_count(1).await?;

    let retro_b = RetroPage::new(&browser_b.driver, &server_2.base_url(), &retro_a.slug).await?;

    // The roster lives in the shared database and is propagated by each
    // process's poll loop, so both browsers see both participants.
    retro_a.wait_for_participant_count(2).await?;
    retro_b.wait_for_participant_count(2).await?;

    // Guest numbers come from a global sequence, so they stay unique across
    // processes and the order is the join order.
    let expected = vec!["Guest 1".to_string(), "Guest 2".to_string()];
    assert_eq!(retro_a.participant_names().await?, expected);
    assert_eq!(retro_b.participant_names().await?, expected);
    assert_eq!(retro_a.participant_counter_text().await?, "2");
    assert_eq!(retro_b.participant_counter_text().await?, "2");

    browser_a.close().await?;
    browser_b.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_participants_panel_removes_participant_leaving_other_app_instance(
) -> WebDriverResult<()> {
    let _two_browsers = two_browser_permit().await;
    let db = TestDb::new().await;
    let (server_1, server_2) = TestServer::start_pair(&db.database_url).await;
    let browser_a = BrowserSession::new(&server_1.base_url()).await?;
    let browser_b = BrowserSession::new(&server_2.base_url()).await?;

    let retros_page = browser_a.retros_page().await?;
    let retro_a = retros_page.create_retro("Participants Multi Leave").await?;
    retro_a.wait_for_participant_count(1).await?;

    let retro_b = RetroPage::new(&browser_b.driver, &server_2.base_url(), &retro_a.slug).await?;
    retro_a.wait_for_participant_count(2).await?;
    retro_b.wait_for_participant_count(2).await?;

    // B leaves the board on instance 2. Instance 2 stops heartbeating B's row;
    // instance 1's poll loop expires it after the grace period and pushes the
    // shrunken roster to A.
    browser_b.driver.goto(&server_2.base_url()).await?;
    retro_a.wait_for_participant_count(1).await?;
    assert_eq!(retro_a.participant_names().await?, vec!["Guest 1"]);
    assert_eq!(retro_a.participant_counter_text().await?, "1");

    browser_a.close().await?;
    browser_b.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_participant_ready_state_syncs_between_browsers() -> WebDriverResult<()> {
    let _two_browsers = two_browser_permit().await;
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser_a = BrowserSession::new(&server.base_url()).await?;
    let browser_b = BrowserSession::new(&server.base_url()).await?;

    let retros_page = browser_a.retros_page().await?;
    let retro_a = retros_page.create_retro("Ready Sync").await?;
    retro_a.wait_for_participant_count(1).await?;

    let retro_b = RetroPage::new(&browser_b.driver, &server.base_url(), &retro_a.slug).await?;
    retro_a.wait_for_participant_count(2).await?;
    retro_b.wait_for_participant_count(2).await?;

    // Nobody has indicated they are done writing yet.
    assert_eq!(
        retro_a.participant_ready_states().await?,
        vec![false, false]
    );
    assert!(!retro_a.self_ready().await?);

    // A marks themselves done writing.
    retro_a.toggle_ready().await?;
    retro_a.wait_for_self_ready(true).await?;

    // Both browsers see A (the first joiner) as ready, B as still writing.
    retro_a.wait_for_participant_ready(0, true).await?;
    retro_b.wait_for_participant_ready(0, true).await?;
    assert_eq!(retro_b.participant_ready_states().await?, vec![true, false]);
    assert!(!retro_b.self_ready().await?);

    // A resumes writing: the indicator clears everywhere.
    retro_a.toggle_ready().await?;
    retro_a.wait_for_self_ready(false).await?;
    retro_b.wait_for_participant_ready(0, false).await?;

    browser_a.close().await?;
    browser_b.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_participant_ready_state_is_shared_across_app_instances() -> WebDriverResult<()> {
    let _two_browsers = two_browser_permit().await;
    let db = TestDb::new().await;
    let (server_1, server_2) = TestServer::start_pair(&db.database_url).await;
    let browser_a = BrowserSession::new(&server_1.base_url()).await?;
    let browser_b = BrowserSession::new(&server_2.base_url()).await?;

    let retros_page = browser_a.retros_page().await?;
    let retro_a = retros_page.create_retro("Ready Multi Process").await?;
    retro_a.wait_for_participant_count(1).await?;

    let retro_b = RetroPage::new(&browser_b.driver, &server_2.base_url(), &retro_a.slug).await?;
    retro_a.wait_for_participant_count(2).await?;
    retro_b.wait_for_participant_count(2).await?;

    // B (on instance 2) marks ready; A (on instance 1) sees it through the
    // shared presence table, propagated by instance 1's poll loop.
    retro_b.toggle_ready().await?;
    retro_b.wait_for_self_ready(true).await?;
    retro_a.wait_for_participant_ready(1, true).await?;

    browser_a.close().await?;
    browser_b.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_participants_panel_state_is_reflected_in_url() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    browser.driver.set_window_rect(0, 0, 1280, 900).await?;

    let retros_page = browser.retros_page().await?;
    let retro = retros_page.create_retro("Participants Url State").await?;

    // Desktop default: open, and the URL carries no explicit preference yet.
    assert!(retro.participants_panel_open().await?);
    assert!(!browser
        .driver
        .current_url()
        .await?
        .as_str()
        .contains("participants="));

    // Closing records the state in the URL.
    retro.close_participants_panel().await?;
    assert!(!retro.participants_panel_open().await?);
    assert!(browser
        .driver
        .current_url()
        .await?
        .as_str()
        .contains("participants=closed"));

    // Reopening records the open state.
    retro.open_participants_panel().await?;
    assert!(retro.participants_panel_open().await?);
    assert!(browser
        .driver
        .current_url()
        .await?
        .as_str()
        .contains("participants=open"));

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_participants_panel_state_deep_link() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    browser.driver.set_window_rect(0, 0, 1280, 900).await?;

    let retros_page = browser.retros_page().await?;
    let retro = retros_page.create_retro("Participants Deep Link").await?;

    // A link with ?participants=closed opens the board with the panel collapsed,
    // even on a desktop viewport where the default is open.
    browser
        .driver
        .goto(
            format!(
                "{}/retro/{}?participants=closed",
                server.base_url(),
                retro.slug
            )
            .as_str(),
        )
        .await?;
    retro.wait_for_participants_panel(false).await?;

    browser
        .driver
        .goto(
            format!(
                "{}/retro/{}?participants=open",
                server.base_url(),
                retro.slug
            )
            .as_str(),
        )
        .await?;
    retro.wait_for_participants_panel(true).await?;

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_participants_panel_state_back_forward() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    browser.driver.set_window_rect(0, 0, 1280, 900).await?;

    let retros_page = browser.retros_page().await?;
    let retro = retros_page.create_retro("Participants History").await?;

    assert!(retro.participants_panel_open().await?);

    retro.close_participants_panel().await?;
    assert!(!retro.participants_panel_open().await?);

    // Back returns to the pre-toggle state (the responsive default: open).
    browser.driver.back().await?;
    retro.wait_for_participants_panel(true).await?;

    // Forward re-applies the closed state.
    browser.driver.forward().await?;
    retro.wait_for_participants_panel(false).await?;

    browser.close().await?;
    Ok(())
}
