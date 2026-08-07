//! Regenerates the screenshots shown on the home page carousel.
//!
//! This test is ignored by default; run it explicitly with:
//!
//! ```command
//! cargo test --test screenshot_capture -- --ignored --nocapture
//! ```
//!
//! It starts its own app instance on a fresh database (like the integration
//! tests), drives Firefox through the individual actions, and saves a PNG of
//! the board after each step into `static/screenshots/`.

mod test_helpers;

use std::path::Path;

use test_helpers::*;
use thirtyfour::prelude::*;
use thirtyfour::By;

const SCREENSHOT_DIR: &str = "static/screenshots";

async fn save_board_screenshot(retro: &RetroPage<'_>, name: &str) -> WebDriverResult<()> {
    std::fs::create_dir_all(SCREENSHOT_DIR).expect("failed to create screenshots directory");
    let board = retro.driver.find(By::Css(".board")).await?;
    board
        .screenshot(Path::new(SCREENSHOT_DIR).join(name).as_path())
        .await?;
    println!("saved {}/{}", SCREENSHOT_DIR, name);
    Ok(())
}

/// Wait for an element to appear (htmx/SSE swaps are asynchronous).
async fn wait_for(
    driver: &WebDriver,
    selector: &str,
    description: &str,
) -> WebDriverResult<WebElement> {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
    loop {
        if let Ok(element) = driver.find(By::Css(selector)).await {
            return Ok(element);
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("Timed out waiting for {}", description);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

#[tokio::test]
#[ignore = "manual helper: regenerate the home page screenshots"]
async fn capture_home_page_screenshots() -> WebDriverResult<()> {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let browser = BrowserSession::new(&server.base_url()).await?;
    browser.driver.set_window_rect(0, 0, 1280, 900).await?;

    let retros = browser.retros_page().await?;
    let retro = retros
        .create_retro_with_slug("Retro: Launch of the new checkout", "landing-demo")
        .await?;

    // Seed the board with cards in every column.
    let _good_b = retro
        .add_card("Good", "Deployment was smooth thanks to the runbook")
        .await?;
    let good_a = retro
        .add_card("Good", "The new checkout handles edge cases really well")
        .await?;
    retro
        .add_card("Bad", "The migration script took way too long")
        .await?;
    retro
        .add_card(
            "Watch",
            "Will the payment provider rate limits hold up at peak?",
        )
        .await?;
    let watch_b = retro
        .add_card("Watch", "Need to monitor the error rate after launch")
        .await?;

    // 1. The board at a glance.
    save_board_screenshot(&retro, "board.png").await?;

    // 2. Adding a card: type into the input, then submit.
    let add_form = retro
        .driver
        .find(By::Css("form[hx-target='#good-items']"))
        .await?;
    let input = add_form.find(By::Tag("textarea")).await?;
    input
        .send_keys("The team nailed the launch communication")
        .await?;
    save_board_screenshot(&retro, "add-card.png").await?;
    add_form
        .find(By::Css("button[type='submit']"))
        .await?
        .click()
        .await?;
    let new_card = wait_for(
        &browser.driver,
        "#good-items article.card",
        "the newly added card",
    )
    .await?;
    let new_card_id = new_card
        .attr("data-item-id")
        .await?
        .unwrap()
        .parse::<i32>()
        .unwrap();

    // 3. Highlighting a card starts the discussion timer.
    retro.click_card(new_card_id).await?;
    retro
        .verify_card_state(new_card_id, "card highlighted")
        .await?;
    // Let the countdown render before capturing.
    tokio::time::sleep(tokio::time::Duration::from_millis(700)).await;
    save_board_screenshot(&retro, "highlight.png").await?;

    // 4. Completing the highlighted card.
    retro.complete_card().await?;
    retro
        .verify_card_state(new_card_id, "card completed")
        .await?;
    save_board_screenshot(&retro, "complete.png").await?;

    // 5. Liking a card.
    retro.like_card(good_a).await?;
    retro.wait_for_like_count(good_a, "1").await?;
    save_board_screenshot(&retro, "like.png").await?;

    // 6. Editing a card inline.
    let edit_button = wait_for(
        &browser.driver,
        &format!("article[data-item-id='{}'] .card-text-edit", watch_b),
        "the edit button",
    )
    .await?;
    edit_button.click().await?;
    wait_for(
        &browser.driver,
        &format!("article[data-item-id='{}'].editing", watch_b),
        "the inline edit form",
    )
    .await?;
    save_board_screenshot(&retro, "edit.png").await?;

    browser.close().await?;
    Ok(())
}
