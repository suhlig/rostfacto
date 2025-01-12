use fantoccini::{ClientBuilder, error::NewSessionError};
use std::process::{Child, Command};

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
    }.await;

    // Always stop geckodriver
    geckodriver.kill().expect("Failed to kill geckodriver");

    result
}

#[tokio::test]
async fn test_create_retro() -> Result<(), NewSessionError> {
    // Start geckodriver
    let mut geckodriver = Command::new("geckodriver")
        .arg("--port")
        .arg("4444")
        .spawn()
        .expect("Failed to start geckodriver");

    // Give geckodriver time to start up
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let result = async {
        let client = ClientBuilder::native()
        .connect("http://localhost:4444")
        .await?;

    // Navigate to the homepage
    client.goto("http://localhost:3000").await.unwrap();

    // Click the "New Retrospective" button
    client.find(fantoccini::Locator::Css("a[href='/retros/new']"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();

    // Fill in the title
    let test_title = format!("Test Retro {}", chrono::Utc::now().timestamp());
    client.find(fantoccini::Locator::Css("input[name='title']"))
        .await
        .unwrap()
        .send_keys(&test_title)
        .await
        .unwrap();

    // Click the submit button
    client.find(fantoccini::Locator::Css("input[type='submit']"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();

    // Navigate back to homepage
    client.goto("http://localhost:3000").await.unwrap();

    // Verify the new retro appears in the list
    let retro_links = client.find_all(fantoccini::Locator::Css("ul.retros li a"))
        .await
        .unwrap();
    
    let mut found = false;
    for link in retro_links {
        if link.text().await.unwrap() == test_title {
            found = true;
            break;
        }
    }
    assert!(found, "Newly created retro not found in list");

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
    client.goto("http://localhost:3000/retro/99999").await.unwrap();
    
    // Find the body text and verify it contains "not found" and "404"
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
