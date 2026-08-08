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
    pub github_user_orgs: Vec<String>,
    /// Display name or email of the person to contact when an org's teams
    /// cannot be listed (e.g. SAML SSO authorization missing).
    pub github_app_owner: Option<String>,
    /// Whether authentication is disabled on purpose. Must be opted into
    /// explicitly with `DEMO_MODE=1`; a deployment without GitHub auth
    /// configuration fails closed instead of silently running unsecured.
    pub demo_mode: bool,
}

impl Config {
    pub fn from_env(bind_address: String) -> Self {
        let database_url =
            env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");

        // Demo mode (no authentication, every request treated as admin) must
        // be requested explicitly. Without it, a missing GITHUB_ADMIN_ORG is
        // a configuration error and prevents startup. Explicit auth
        // configuration always wins over DEMO_MODE, so a lingering DEMO_MODE=1
        // can never silently unsecure a real deployment.
        let github_admin_org = env::var("GITHUB_ADMIN_ORG").ok();
        let demo_mode = if github_admin_org.is_some() {
            false
        } else {
            env::var("DEMO_MODE")
                .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
                .unwrap_or(false)
        };

        if !demo_mode && github_admin_org.is_none() {
            panic!(
                "GITHUB_ADMIN_ORG must be set to enable GitHub authentication, \
                 or set DEMO_MODE=1 to run an unsecured demo instance"
            );
        }

        let github_client_id = env::var("GITHUB_CLIENT_ID").unwrap_or_default();
        let github_client_secret = env::var("GITHUB_CLIENT_SECRET").unwrap_or_default();
        let github_admin_team_slug = env::var("GITHUB_ADMIN_TEAM_SLUG").ok();
        let github_enterprise_url = env::var("GITHUB_ENTERPRISE_URL").ok();
        // GITHUB_USER_ORG may list multiple organizations, separated by colons.
        let github_user_orgs = env::var("GITHUB_USER_ORG")
            .map(|value| {
                value
                    .split(':')
                    .map(str::trim)
                    .filter(|org| !org.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let github_app_owner = env::var("GITHUB_APP_OWNER").ok();
        let public_url = match env::var("PUBLIC_URL") {
            Ok(url) => url,
            Err(_) if demo_mode => {
                tracing::warn!(
                    "PUBLIC_URL not set; using http://localhost:3000 for OAuth redirects"
                );
                "http://localhost:3000".to_string()
            }
            Err(_) => panic!(
                "PUBLIC_URL must be set when authentication is enabled \
                 (it is the public base URL used for the OAuth callback)"
            ),
        };

        if !demo_mode {
            // Fail closed: any of these missing would silently disable
            // authentication (every user becomes admin) or brick the OAuth
            // login flow at runtime.
            if github_client_id.is_empty() {
                panic!("GITHUB_CLIENT_ID must be set when authentication is enabled");
            }
            if github_client_secret.is_empty() {
                panic!("GITHUB_CLIENT_SECRET must be set when authentication is enabled");
            }
            if github_admin_team_slug.is_none() {
                panic!("GITHUB_ADMIN_TEAM_SLUG must be set when authentication is enabled");
            }
        }

        Self {
            bind_address,
            database_url,
            public_url,
            github_client_id,
            github_client_secret,
            github_enterprise_url,
            github_admin_org,
            github_admin_team_slug,
            github_user_orgs,
            github_app_owner,
            demo_mode,
        }
    }

    /// When `DEMO_MODE=1`, the app runs without authentication: all requests
    /// are treated as a synthetic admin user and a red banner warns that the
    /// instance is unsecured.
    pub fn demo_mode(&self) -> bool {
        self.demo_mode
    }

    pub fn admin_team(&self) -> Option<(&str, &str)> {
        match (&self.github_admin_org, &self.github_admin_team_slug) {
            (Some(org), Some(team)) => Some((org, team)),
            _ => None,
        }
    }

    /// URL of the page where users can authorize OAuth apps for SAML SSO
    /// organizations; the GitHub Enterprise equivalent when configured.
    pub fn applications_url(&self) -> String {
        self.github_enterprise_url
            .as_deref()
            .map(|url| format!("{}/settings/applications", url.trim_end_matches('/')))
            .unwrap_or_else(|| "https://github.com/settings/applications".to_string())
    }

    /// Whether session cookies should carry the `Secure` flag: true when the
    /// public URL is served over HTTPS.
    pub fn cookies_secure(&self) -> bool {
        self.public_url.starts_with("https://")
    }
}
