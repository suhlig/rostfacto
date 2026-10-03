//! Helpers shared by the integration test modules.

use thirtyfour::error::WebDriverResult;
use thirtyfour::prelude::*;

pub fn parse_timer(text: &str) -> u32 {
    let parts: Vec<&str> = text.trim().split(':').collect();
    assert_eq!(parts.len(), 2, "Timer should be in m:ss format");
    let minutes: u32 = parts[0].parse().expect("Invalid timer minutes");
    let seconds: u32 = parts[1].parse().expect("Invalid timer seconds");
    minutes * 60 + seconds
}

/// Submit the new-retro form directly via HTTP to test server-side validation.
pub async fn post_retros_form(
    driver: &WebDriver,
    base_url: &str,
    title: &str,
    slug: &str,
) -> WebDriverResult<(u64, String)> {
    driver.goto(base_url).await?;
    let script = format!(
        r#"return fetch('/retros', {{
            method: 'POST',
            headers: {{'Content-Type': 'application/x-www-form-urlencoded'}},
            body: 'title={title}&slug={slug}'
        }}).then(async r => ({{status: r.status, text: await r.text()}}));"#
    );
    let result = driver.execute(&script, vec![]).await?;
    let result = result.json();
    let status = result["status"].as_u64().unwrap();
    let text = result["text"].as_str().unwrap().to_string();
    Ok((status, text))
}

/// Fetch a path and return the HTTP status code.
pub async fn fetch_status(driver: &WebDriver, base_url: &str, path: &str) -> WebDriverResult<u64> {
    driver.goto(base_url).await?;
    let script = format!(r#"return fetch('{}').then(r => r.status);"#, path);
    let result = driver.execute(&script, vec![]).await?;
    Ok(result.json().as_u64().unwrap())
}

/// Fetch a path and return the response body as text.
pub async fn fetch_text(driver: &WebDriver, base_url: &str, path: &str) -> WebDriverResult<String> {
    driver.goto(base_url).await?;
    let script = format!(r#"return fetch('{}').then(r => r.text());"#, path);
    let result = driver.execute(&script, vec![]).await?;
    Ok(result.json().as_str().unwrap().to_string())
}
