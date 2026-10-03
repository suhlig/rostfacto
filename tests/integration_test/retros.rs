use crate::common::*;
use crate::test_helpers::*;
use thirtyfour::error::WebDriverResult;
use thirtyfour::By;

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
    // The clash is reported in the form (with the submitted values preserved),
    // not on a standalone 500 error page.
    assert_eq!(status, 409);
    assert!(text.contains("Slug is already in use"));
    assert!(text.contains("new-retro-form"));
    assert!(text.contains("value=\"Another\""));

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn test_slug_check_reports_taken_slugs() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    let retros_page = browser.retros_page().await?;
    let retro_page = retros_page
        .create_retro_with_slug("Slug Check", "slug-check-taken")
        .await?;

    let taken = fetch_text(
        &browser.driver,
        &server.base_url(),
        &format!("/retros/slug-check?slug={}", retro_page.slug),
    )
    .await?;
    assert!(taken.contains("Slug is already in use"));

    let free = fetch_text(
        &browser.driver,
        &server.base_url(),
        "/retros/slug-check?slug=slug-check-free",
    )
    .await?;
    assert!(free.is_empty());

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
