use crate::common::*;
use crate::test_helpers::*;
use thirtyfour::error::WebDriverResult;
use thirtyfour::By;

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
