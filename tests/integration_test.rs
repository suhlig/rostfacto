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

    // Find the h1 element with retro-title class and verify its text
    let h1 = driver.find(By::Css("h1.retro-title")).await?;
    assert_eq!(h1.text().await?, "Retrospectives");

    // Always close the browser
    driver.quit().await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_create_cards() -> WebDriverResult<()> {
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


    // Go to the retro page
    driver.goto(&format!("http://localhost:3000{}", href)).await?;

    // Add a card to each column
    for (target, text) in [
        ("#good-items", "Good point test"),
        ("#bad-items", "Bad point test"),
        ("#watch-items", "Watch point test")
    ] {
        let form = driver.find(By::Css(format!("form[hx-target='{}']", target).as_str())).await?;
        let input = form.find(By::Tag("input")).await?;
        input.send_keys(text).await?;
        input.send_keys("\u{E007}").await?;

        // Wait a moment for the card to appear
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Verify each card exists and is in default state (no special classes)
    for text in ["Good point test", "Bad point test", "Watch point test"] {
        let cards = driver.find_all(By::ClassName("card")).await?;
        let mut found = false;
        for card in cards {
            if card.text().await? == text {
                // Card should only have "card" class in default state
                let class_attr = card.attr("class").await?.unwrap();
                assert_eq!(class_attr.trim(), "card",
                    "Card '{}' should be in default state", text);
                found = true;
                break;
            }
        }
        assert!(found, "Card with text '{}' not found", text);
    }

    // Clean up - delete the retro
    driver.goto("http://localhost:3000").await?;
    let cards = driver.find_all(By::ClassName("card")).await?;
    for card in cards {
        let links = card.find_all(By::Tag("a")).await?;
        for link in links {
            if link.text().await? == test_title {
                // Execute JavaScript to override the confirm dialog
                driver.execute("window.confirm = () => true", vec![]).await?;

                let delete_button = card.find(By::Tag("button")).await?;
                delete_button.click().await?;
                break;
            }
        }
    }

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

    // Wait for redirect and verify we're on the retro page
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    let current_url = driver.current_url().await?;
    assert!(current_url.as_str().contains("/retro/"), "Should be redirected to retro page");

    // Verify the retro title is shown
    let title = driver.find(By::Css("h1.retro-title")).await?;
    assert_eq!(title.text().await?, test_title);

    // Extract retro ID from URL for cleanup
    let retro_id = current_url.path_segments().unwrap().last().unwrap();

    // Navigate to the homepage
    driver.goto("http://localhost:3000").await?;

    // Execute JavaScript to override the confirm dialog
    driver.execute("window.confirm = () => true", vec![]).await?;

    // Find and click the delete button
    driver.find(By::Css(&format!("button[hx-delete='/retro/{}/delete']", retro_id))).await?.click().await?;

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
