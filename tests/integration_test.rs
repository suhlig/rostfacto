use thirtyfour::prelude::*;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_home_page() -> WebDriverResult<()> {
    let mut caps = DesiredCapabilities::firefox();
    caps.set_headless()?;

    let driver = WebDriver::new("http://localhost:4444", caps).await?;

    // Navigate to the homepage
    driver.goto("http://localhost:3000").await?;

    // Find the h1 element and verify its text
    let h1 = driver.find(By::ClassName("retro-title")).await?;
    assert_eq!(h1.text().await?, "RETROSPECTIVES");

    // Always close the browser
    driver.quit().await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_create_retro() -> WebDriverResult<()> {
    let mut caps = DesiredCapabilities::firefox();
    caps.set_headless()?;

    let driver = WebDriver::new("http://localhost:4444", caps).await?;

    // Navigate to the homepage
    driver.goto("http://localhost:3000").await?;

    // Click the "New Retrospective" button
    driver.find(By::Css("a[href='/retros/new']")).await?.click().await?;

    // Fill in the title
    let test_title = format!("Test Retro {}", chrono::Utc::now().timestamp());
    let title_input = driver.find(By::Css("input[name='title']")).await?;
    title_input.send_keys(&test_title).await?;

    // Click the submit button
    driver.find(By::Css("input[type='submit']")).await?.click().await?;

    // Navigate back to homepage
    driver.goto("http://localhost:3000").await?;

    // Find all cards and look for our test retro
    let cards = driver.find_all(By::ClassName("card")).await?;
    let mut found_card = None;
    for card in cards {
        let links = card.find_all(By::Tag("a")).await?;
        for link in links {
            if link.text().await? == test_title {
                found_card = Some(card);
                break;
            }
        }
        if found_card.is_some() {
            break;
        }
    }

    let our_card = found_card.expect("Newly created retro not found in list");

    // Extract the retro ID from the card's link href
    let link = our_card.find(By::Tag("a")).await?;
    let href = link.attr("href").await?.unwrap();
    let retro_id = href.split('/').last().unwrap();

    // Execute JavaScript to override the confirm dialog
    driver.execute("window.confirm = () => true", vec![]).await?;

    // Find and click the delete button
    our_card.find(By::Css(&format!("button[hx-delete='/retro/{}/delete']", retro_id))).await?.click().await?;

    // Wait a moment for the deletion to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Verify the retro is deleted
    let cards = driver.find_all(By::ClassName("card")).await?;
    for card in cards {
        let links = card.find_all(By::Tag("a")).await?;
        for link in links {
            assert_ne!(link.text().await?, test_title, "Retro was not deleted!");
        }
    }

    // Always close the browser
    driver.quit().await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_nonexistent_retro() -> WebDriverResult<()> {
    let mut caps = DesiredCapabilities::firefox();
    caps.set_headless()?;

    let driver = WebDriver::new("http://localhost:4444", caps).await?;

    // Navigate to a non-existent retro
    driver.goto("http://localhost:3000/retro/99999").await?;

    // Find the body text and verify it contains "not found"
    let body = driver.find(By::Tag("body")).await?;
    let body_text = body.text().await?.to_lowercase();
    assert!(body_text.contains("not found"));

    // Always close the browser
    driver.quit().await?;

    Ok(())
}
