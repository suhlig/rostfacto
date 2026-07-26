use crate::config::Config;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GitHubUser {
    pub id: i64,
    pub login: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GitHubTeam {
    #[allow(dead_code)]
    pub id: i64,
    pub slug: String,
    pub name: String,
}

pub fn api_base_url(config: &Config) -> String {
    config
        .github_enterprise_url
        .as_deref()
        .map(|url| format!("{}/api/v3", url.trim_end_matches('/')))
        .unwrap_or_else(|| "https://api.github.com".to_string())
}

fn github_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("rostfacto")
        .build()
        .expect("failed to build HTTP client")
}

pub fn oauth_authorize_url(config: &Config) -> String {
    config
        .github_enterprise_url
        .as_deref()
        .map(|url| format!("{}/login/oauth/authorize", url.trim_end_matches('/')))
        .unwrap_or_else(|| "https://github.com/login/oauth/authorize".to_string())
}

pub fn oauth_token_url(config: &Config) -> String {
    config
        .github_enterprise_url
        .as_deref()
        .map(|url| format!("{}/login/oauth/access_token", url.trim_end_matches('/')))
        .unwrap_or_else(|| "https://github.com/login/oauth/access_token".to_string())
}

pub async fn get_user(access_token: &str, config: &Config) -> Result<GitHubUser, reqwest::Error> {
    github_client()
        .get(format!("{}/user", api_base_url(config)))
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?
        .error_for_status()?
        .json::<GitHubUser>()
        .await
}

pub async fn is_team_member(
    org: &str,
    team_slug: &str,
    username: &str,
    access_token: &str,
    config: &Config,
) -> Result<bool, reqwest::Error> {
    let url = format!(
        "{}/orgs/{}/teams/{}/memberships/{}",
        api_base_url(config),
        org,
        team_slug,
        username
    );
    let response = github_client()
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?;

    Ok(response.status().is_success())
}

pub async fn list_org_teams(
    org: &str,
    access_token: &str,
    config: &Config,
) -> Result<Vec<GitHubTeam>, reqwest::Error> {
    github_client()
        .get(format!("{}/orgs/{}/teams", api_base_url(config), org))
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<GitHubTeam>>()
        .await
}
