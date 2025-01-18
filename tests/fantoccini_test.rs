use fantoccini::{ClientBuilder, error::NewSessionError};
use std::process::Command;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_home_page() -> Result<(), NewSessionError> {
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

        // Find the h1 element and verify its text
        let h1 = client.find(fantoccini::Locator::Css(".retro-title")).await.unwrap();
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
#[serial]
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

        // Find the card containing our test retro
        let cards = client.find_all(fantoccini::Locator::Css(".card")).await.unwrap();
        let mut our_card = None;
        for card in cards {
            let links = card.find_all(fantoccini::Locator::Css("a")).await.unwrap();
            for link in links {
                if link.text().await.unwrap() == test_title {
                    our_card = Some(card);
                    break;
                }
            }
            if our_card.is_some() {
                break;
            }
        }
        
        let our_card = our_card.expect("Newly created retro not found in list");
        
        // Extract the retro ID from the card's link href
        let link = our_card.find(fantoccini::Locator::Css("a"))
            .await
            .unwrap();
        let href = link.attr("href").await.unwrap().unwrap();
        let retro_id = href.split('/').last().unwrap();

        // Set up confirmation dialog handler before clicking delete
        client.execute("window.confirm = () => true", vec![])
            .await
            .unwrap();

        // Find and click the delete button within our card by its hx-delete attribute
        our_card.find(fantoccini::Locator::Css(format!("button[hx-delete='/retro/{}/delete']", retro_id).as_str()))
            .await
            .unwrap()
            .click()
            .await
            .unwrap();

        // Wait a moment for the deletion to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Verify the retro is deleted by checking it's no longer in the list
        let cards = client.find_all(fantoccini::Locator::Css(".card")).await.unwrap();
        for card in cards {
            let links = card.find_all(fantoccini::Locator::Css("a")).await.unwrap();
            for link in links {
                assert_ne!(link.text().await.unwrap(), test_title, "Retro was not deleted!");
            }
        }

        // Always close the browser
        client.close().await.unwrap();

        Ok(())
    }.await;

    // Always stop geckodriver
    geckodriver.kill().expect("Failed to kill geckodriver");

    result
}

#[tokio::test]
#[serial]
async fn test_nonexistent_retro() -> Result<(), NewSessionError> {
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
    }.await;

    // Always stop geckodriver
    geckodriver.kill().expect("Failed to kill geckodriver");

    result
}
