use fantoccini::{ClientBuilder, error::NewSessionError};

#[tokio::test]
async fn test_home_page() -> Result<(), NewSessionError> {
    let client = ClientBuilder::native()
        .connect("http://localhost:4444")
        .await?;

    // Navigate to the homepage
    client.goto("http://localhost:3000").await.unwrap();

    // Find the h1 element and verify its text
    let h1 = client.find(fantoccini::Locator::Css("h1")).await.unwrap();
    assert_eq!(h1.text().await.unwrap(), "RETROSPECTIVES");

    // Always close the browser
    client.close().await.unwrap();

    Ok(())
}

#[tokio::test]
async fn test_nonexistent_retro() -> Result<(), NewSessionError> {
    let client = ClientBuilder::native()
        .connect("http://localhost:4444")
        .await?;

    // Navigate to a non-existent retro
    let response = client.goto("http://localhost:3000/retro/99999").await.unwrap();
    
    // Verify 404 status code
    assert_eq!(response.status().unwrap(), 404);

    // Find the body text and verify it contains "not found"
    let body_text = client.find(fantoccini::Locator::Css("body"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
        .to_lowercase();
    assert!(body_text.contains("not found"));

    // Always close the browser
    client.close().await.unwrap();

    Ok(())
}
