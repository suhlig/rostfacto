use crate::config::Config;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GitHubUser {
    pub id: i64,
    pub login: String,
    pub name: Option<String>,
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

/// Error listing an org's teams: either a network-level failure or an HTTP
/// error from GitHub. The HTTP variant keeps GitHub's response body (which
/// names the cause, e.g. SAML SSO enforcement) and the `X-GitHub-SSO`
/// authorization URL, if the org sent one.
#[derive(Debug)]
pub enum ListTeamsError {
    Transport(reqwest::Error),
    Http {
        status: reqwest::StatusCode,
        body: String,
        sso_url: Option<String>,
    },
}

impl std::fmt::Display for ListTeamsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ListTeamsError::Transport(e) => write!(f, "transport error: {e}"),
            ListTeamsError::Http {
                status,
                body,
                sso_url,
            } => {
                write!(f, "GitHub responded {status}: {body}")?;
                if let Some(url) = sso_url {
                    write!(f, " (SAML authorization required: {url})")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ListTeamsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ListTeamsError::Transport(e) => Some(e),
            ListTeamsError::Http { .. } => None,
        }
    }
}

/// Lists all teams of the org that are visible to the authenticated user.
/// GitHub's "List teams" endpoint only returns teams the authenticated user
/// can see: org owners see all teams, org members see closed teams and teams
/// they belong to. The response is paginated (max 100 entries per page), so
/// follow the `Link` header until the last page.
pub async fn list_org_teams(
    org: &str,
    access_token: &str,
    config: &Config,
) -> Result<Vec<GitHubTeam>, ListTeamsError> {
    let mut teams = Vec::new();
    let mut url = format!("{}/orgs/{}/teams?per_page=100", api_base_url(config), org);

    loop {
        let response = github_client()
            .get(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(ListTeamsError::Transport)?;

        if !response.status().is_success() {
            let status = response.status();
            let sso_url = response
                .headers()
                .get("x-github-sso")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let body = response.text().await.unwrap_or_default();
            return Err(ListTeamsError::Http {
                status,
                body,
                sso_url,
            });
        }

        let next_url = response.headers().get("link").and_then(next_page_url);
        teams.extend(
            response
                .json::<Vec<GitHubTeam>>()
                .await
                .map_err(ListTeamsError::Transport)?,
        );

        match next_url {
            Some(next) => url = next,
            None => break,
        }
    }

    Ok(teams)
}

/// Extracts the `rel="next"` URL from a GitHub `Link` header, if present.
fn next_page_url(header: &reqwest::header::HeaderValue) -> Option<String> {
    let link = header.to_str().ok()?;
    let next = link.split(',').find(|part| part.contains("rel=\"next\""))?;
    let start = next.find('<')? + 1;
    let end = next.find('>')?;
    (end > start).then(|| next[start..end].to_string())
}
