use fantoccini::{Client, ClientBuilder};
async fn setup() -> Client {
    let mut caps = serde_json::Map::new();
    caps.insert("browserName".to_string(), serde_json::json!("firefox"));
    caps.insert(
        "moz:firefoxOptions".to_string(),
        serde_json::json!({
            "args": ["--headless"],
            "binary": "/Applications/Firefox.app/Contents/MacOS/Firefox"
        }),
    );
    
    ClientBuilder::native()
        .capabilities(caps)
        .connect("http://localhost:4444")
        .await
        .expect("Failed to connect to WebDriver")
}

#[tokio::test]
async fn test_retro_workflow() {
    let client = setup().await;
    
    // Test 1: Create a new retro
    client.goto("http://localhost:3000").await.unwrap();
    let input = client.find(fantoccini::Locator::Css("input[name='title']")).await.unwrap();
    input.send_keys("Test Retro").await.unwrap();
    client.find(fantoccini::Locator::Css("button[type='submit']")).await.unwrap().click().await.unwrap();
    
    // Click the newly created retro link
    client.find(fantoccini::Locator::Css("a")).await.unwrap().click().await.unwrap();
    
    // Wait a moment for navigation
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // Test 2: Add items to different columns
    let good_input = client.find(fantoccini::Locator::Css("form[hx-post*='/items/Good/'] input")).await.unwrap();
    good_input.send_keys("Good item").await.unwrap();
    good_input.send_keys("\n").await.unwrap();
    
    let bad_input = client.find(fantoccini::Locator::Css("form[hx-post*='/items/Bad/'] input")).await.unwrap();
    bad_input.send_keys("Bad item").await.unwrap();
    bad_input.send_keys("\n").await.unwrap();
    
    let watch_input = client.find(fantoccini::Locator::Css("form[hx-post*='/items/Watch/'] input")).await.unwrap();
    watch_input.send_keys("Watch item").await.unwrap();
    watch_input.send_keys("\n").await.unwrap();
    
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
    
    client.close().await.unwrap();
}
