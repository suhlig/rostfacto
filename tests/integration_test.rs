use thirtyfour::prelude::*;

#[cfg(test)]
#[tokio::test]
async fn test_home_page() -> WebDriverResult<()> {
    let caps = DesiredCapabilities::firefox();
    let driver = WebDriver::new("http://localhost:4444", caps).await?;

    // Navigate to the homepage
    driver.goto("http://localhost:3000").await?;

    // Find the h1 element and verify its text
    let h1 = driver.find(By::Tag("h1")).await?;
    assert_eq!(h1.text().await?, "Retrospectives");

    // Always close the browser
    driver.quit().await?;

    Ok(())
}
