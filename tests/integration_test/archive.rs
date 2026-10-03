use crate::test_helpers::*;
use thirtyfour::error::WebDriverResult;
use thirtyfour::By;

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
