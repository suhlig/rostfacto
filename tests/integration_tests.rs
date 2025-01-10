use fantoccini::{Client, ClientBuilder};
use tokio;

async fn setup() -> Client {
    let caps = serde_json::json!({
        "browserName": "firefox",
        "moz:firefoxOptions": {
            "args": ["--headless"]
        }
    });
    
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
    
    // Test 2: Add items to different columns
    client.find(fantoccini::Locator::Css(".retro-column.good-column input")).await.unwrap()
        .send_keys("Good item").await.unwrap();
    client.find(fantoccini::Locator::Css(".retro-column.bad-column input")).await.unwrap()
        .send_keys("Bad item").await.unwrap();
    client.find(fantoccini::Locator::Css(".retro-column.watch-column input")).await.unwrap()
        .send_keys("Watch item").await.unwrap();
    
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
