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
