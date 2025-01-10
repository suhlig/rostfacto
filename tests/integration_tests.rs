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
    async fn add_item(client: &Client, category: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Find and fill the input field
        let input_selector = format!("form[hx-post*='/items/{}/'] input", category);
        
        // Wait for the form to be ready with exponential backoff
        let mut delay = 100;
        let max_attempts = 15;
        let mut input = None;
        
        for attempt in 0..max_attempts {
            match client.find(fantoccini::Locator::Css(&input_selector)).await {
                Ok(element) => {
                    input = Some(element);
                    break;
                },
                Err(_) => {
                    if attempt == max_attempts - 1 {
                        return Err(format!("Form input for '{}' not found after {} attempts", category, max_attempts).into());
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    delay = std::cmp::min(delay * 2, 2000);
                }
            }
        }
        
        let input = input.ok_or("Input field not found")?;
        input.send_keys(text).await?;
        
        // Submit the form and wait for response
        input.send_keys("\n").await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        // Wait for the item to appear with exponential backoff
        let item_selector = format!("#{}-items .card", category.to_lowercase());
        let mut delay = 200; // Start with longer initial delay
        let max_attempts = 15;
        
        for attempt in 0..max_attempts {
            match client.find(fantoccini::Locator::Css(&item_selector)).await {
                Ok(element) => {
                    // Wait a bit for text content to be populated
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    
                    // Verify the text content
                    if let Ok(content) = element.text().await {
                        if content.contains(text) {
                            return Ok(());
                        }
                    }
                },
                Err(_) => {}
            }
            
            if attempt == max_attempts - 1 {
                return Err(format!("Item '{}' did not appear after {} attempts", text, max_attempts).into());
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            delay = std::cmp::min(delay * 2, 2000);
        }
        
        Err("Failed to verify item content".into())
    }

    add_item(&client, "Good", "Good item").await?;
    add_item(&client, "Bad", "Bad item").await?;
    add_item(&client, "Watch", "Watch item").await?;
    
    // Test 3: Toggle item status (Default -> Highlighted -> Completed)
    async fn toggle_item(client: &Client, selector: &str) -> Result<(), Box<dyn std::error::Error>> {
        let item = client.find(fantoccini::Locator::Css(selector)).await?;
        
        // First click - to highlighted
        item.click().await?;
        for _ in 0..5 {
            if client.find(fantoccini::Locator::Css(".card.highlighted")).await.is_ok() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
        
        // Second click - to completed
        item.click().await?;
        for _ in 0..5 {
            if client.find(fantoccini::Locator::Css(".card.completed")).await.is_ok() {
                return Ok(());
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
        Err("Item status did not change as expected".into())
    }

    toggle_item(&client, "#good-items .card").await?;
    
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
