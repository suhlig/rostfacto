use fantoccini::{Client, ClientBuilder};
use std::{process::Command, time::Duration};
use tokio::time::sleep;

async fn setup() -> Result<Client, fantoccini::error::NewSessionError> {
    // Kill any existing Firefox processes
    Command::new("pkill")
        .arg("-f")
        .arg("firefox")
        .output()
        .ok();
    
    // Give processes time to clean up
    sleep(Duration::from_secs(1)).await;
    
    let mut caps = serde_json::Map::new();
    caps.insert("browserName".to_string(), serde_json::json!("firefox"));
    caps.insert(
        "moz:firefoxOptions".to_string(),
        serde_json::json!({
            "args": ["--headless"],
            "binary": "/Applications/Firefox.app/Contents/MacOS/Firefox"
        }),
    );
    
    // Restart geckodriver
    Command::new("pkill")
        .arg("-f")
        .arg("geckodriver")
        .output()
        .ok();
    
    // Give geckodriver time to shut down
    sleep(Duration::from_secs(1)).await;
    
    // Start new geckodriver instance
    Command::new("geckodriver")
        .arg("--port")
        .arg("4444")
        .spawn()
        .ok();
    
    // Wait for geckodriver to be ready
    sleep(Duration::from_secs(2)).await;
    
    // Try to connect with retries
    for i in 0..3 {
        match ClientBuilder::native()
            .capabilities(caps.clone())
            .connect("http://localhost:4444")
            .await
        {
            Ok(client) => return Ok(client),
            Err(e) => {
                eprintln!("Attempt {} failed to connect to WebDriver: {}", i + 1, e);
                if i < 2 {
                    sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }
    
    // Final attempt
    ClientBuilder::native()
        .capabilities(caps)
        .connect("http://localhost:4444")
        .await
}

#[tokio::test]
async fn test_retro_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let client = setup().await?;
    
    // Test 1: Create a new retro
    client.goto("http://localhost:3000").await?;
    
    // Wait for the page to load
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    
    let input = client.find(fantoccini::Locator::Css("input[name='title']")).await?;
    input.send_keys("Test Retro").await?;
    client.find(fantoccini::Locator::Css("button[type='submit']")).await?.click().await?;
    
    // Wait for the page to update
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    
    // Click the newly created retro link
    client.find(fantoccini::Locator::Css("a")).await?.click().await?;
    
    // Wait for navigation
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    
    // Test 2: Add items to different columns
    let good_input = client.find(fantoccini::Locator::Css("form[hx-post*='/items/Good/'] input")).await?;
    good_input.send_keys("Good item").await?;
    good_input.send_keys("\n").await?;
    
    // Wait for HTMX to update the DOM
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    
    let bad_input = client.find(fantoccini::Locator::Css("form[hx-post*='/items/Bad/'] input")).await?;
    bad_input.send_keys("Bad item").await?;
    bad_input.send_keys("\n").await?;
    
    // Wait for HTMX to update the DOM
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    
    let watch_input = client.find(fantoccini::Locator::Css("form[hx-post*='/items/Watch/'] input")).await?;
    watch_input.send_keys("Watch item").await?;
    watch_input.send_keys("\n").await?;
    
    // Wait for HTMX to update the DOM
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    
    // Test 3: Toggle item status (Default -> Highlighted -> Completed)
    let good_item = client.find(fantoccini::Locator::Css("#good-items .card")).await.unwrap();
    good_item.click().await.unwrap();
    assert!(client.find(fantoccini::Locator::Css(".card.highlighted")).await.is_ok());
    good_item.click().await.unwrap();
    assert!(client.find(fantoccini::Locator::Css(".card.completed")).await.is_ok());
    
    // Test 4: Archive functionality
    // First complete all items
    for selector in &["#good-items .card", "#bad-items .card", "#watch-items .card"] {
        let item = client.find(fantoccini::Locator::Css(selector)).await.unwrap();
        item.click().await.unwrap(); // to highlighted
        item.click().await.unwrap(); // to completed
    }
    
    // Click archive button
    client.find(fantoccini::Locator::Css(".archive-btn")).await.unwrap()
        .click().await.unwrap();
    
    // Verify items are gone
    for selector in &["#good-items .card", "#bad-items .card", "#watch-items .card"] {
        assert!(client.find(fantoccini::Locator::Css(selector)).await.is_err());
    }
    
    client.close().await?;
    Ok(())
}
