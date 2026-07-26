use std::env;

#[derive(Clone)]
pub struct Config {
    pub bind_address: String,
    pub database_url: String,
    pub public_url: String,
    pub github_client_id: String,
    pub github_client_secret: String,
    pub github_enterprise_url: Option<String>,
    pub github_admin_org: Option<String>,
    pub github_admin_team_slug: Option<String>,
    pub github_user_org: Option<String>,
    pub session_secret: String,
}

impl Config {
    pub fn from_env(bind_address: String) -> Self {
        let database_url =
            env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");
        let github_client_id = env::var("GITHUB_CLIENT_ID").unwrap_or_default();
        let github_client_secret = env::var("GITHUB_CLIENT_SECRET").unwrap_or_default();
        let github_enterprise_url = env::var("GITHUB_ENTERPRISE_URL").ok();
        let github_admin_org = env::var("GITHUB_ADMIN_ORG").ok();
        let github_admin_team_slug = env::var("GITHUB_ADMIN_TEAM_SLUG").ok();
        let github_user_org = env::var("GITHUB_USER_ORG").ok();
        let public_url = env::var("PUBLIC_URL").unwrap_or_else(|_| {
            tracing::warn!("PUBLIC_URL not set; using http://localhost:3000 for OAuth redirects");
            "http://localhost:3000".to_string()
        });
        let session_secret = env::var("SESSION_SECRET").unwrap_or_else(|_| {
            tracing::warn!("SESSION_SECRET not set; using a default secret");
            "dev-secret".to_string()
        });

        Self {
            bind_address,
            database_url,
            public_url,
            github_client_id,
            github_client_secret,
            github_enterprise_url,
            github_admin_org,
            github_admin_team_slug,
            github_user_org,
            session_secret,
        }
    }

    /// When GITHUB_ADMIN_ORG is not configured, the app runs in demo mode:
    /// authentication is bypassed and all local users are treated as admins.
    pub fn demo_mode(&self) -> bool {
        self.github_admin_org.is_none()
    }

    pub fn auth_enabled(&self) -> bool {
        !self.demo_mode()
            && !self.github_client_id.is_empty()
            && !self.github_client_secret.is_empty()
    }

    pub fn admin_team(&self) -> Option<(&str, &str)> {
        match (&self.github_admin_org, &self.github_admin_team_slug) {
            (Some(org), Some(team)) => Some((org, team)),
            _ => None,
        }
    }
}
