use crate::test_helpers::*;
use thirtyfour::error::WebDriverResult;
use thirtyfour::By;

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
    let edit_input = edit_form.find(By::Css("textarea[name='text']")).await?;
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
