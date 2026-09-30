use crate::{config::Config, AppState};
use axum::{
    extract::State,
    http::{header, HeaderValue, Request},
    middleware::Next,
    response::Response,
};
use url::Url;

/// GitHub's avatar host, where authenticated users' profile images are served.
const GITHUB_AVATARS_ORIGIN: &str = "https://avatars.githubusercontent.com";

/// Builds the Content-Security-Policy header value once at startup.
///
/// The policy is strict: all JavaScript is served from `/static` (or the
/// pinned, SRI-protected htmx CDN bundle), so inline scripts and `eval` are
/// banned. All inline `onclick`/`hx-on` handlers were removed from the
/// templates to make this possible.
///
/// `img-src` always allows GitHub's avatar host; when a GitHub Enterprise
/// Server is configured, its origin is allowed too (enterprise avatars are
/// served from the enterprise host). Because the enterprise host is
/// config-dependent, the value cannot be a `HeaderValue::from_static` and is
/// built here and shared via `AppState`.
pub fn content_security_policy(config: &Config) -> HeaderValue {
    let mut img_src = format!("'self' data: {GITHUB_AVATARS_ORIGIN}");
    if let Some(origin) = config.github_enterprise_url.as_deref().and_then(origin_of) {
        img_src.push(' ');
        img_src.push_str(&origin);
    }

    let policy = format!(
        "default-src 'self'; \
         script-src 'self' https://cdn.jsdelivr.net; \
         style-src 'self' https://fonts.googleapis.com; \
         font-src 'self' https://fonts.gstatic.com; \
         img-src {img_src}; \
         connect-src 'self'; \
         base-uri 'self'; \
         form-action 'self'; \
         frame-ancestors 'self'; \
         object-src 'none'"
    );

    HeaderValue::from_bytes(policy.as_bytes()).expect("CSP is a valid header value")
}

/// Returns the `scheme://host[:port]` origin of a URL, or `None` if it cannot
/// be parsed.
fn origin_of(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    let host = url.host_str()?;
    let mut origin = format!("{}://{}", url.scheme(), host);
    if let Some(port) = url.port() {
        origin.push_str(&format!(":{port}"));
    }
    Some(origin)
}

/// Security headers applied to every response.
pub async fn apply(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert(header::CONTENT_SECURITY_POLICY, state.csp.clone());
    // Older browsers that ignore frame-ancestors.
    headers.insert(
        header::X_FRAME_OPTIONS,
        HeaderValue::from_static("SAMEORIGIN"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        // Not exposed as a constant by the `http` crate (yet): literal name.
        "permissions-policy",
        HeaderValue::from_static("camera=(), geolocation=(), microphone=()"),
    );
    // Harmless over plain HTTP (browsers only honor it on HTTPS); required
    // once the app is served behind TLS.
    headers.insert(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(enterprise_url: Option<&str>) -> Config {
        Config {
            bind_address: "0.0.0.0:3000".to_string(),
            database_url: "postgres://unused".to_string(),
            public_url: "https://rostfacto.example.com".to_string(),
            github_client_id: String::new(),
            github_client_secret: String::new(),
            github_enterprise_url: enterprise_url.map(str::to_string),
            github_admin_org: Some("org".to_string()),
            github_admin_team_slug: Some("team".to_string()),
            github_user_orgs: Vec::new(),
            github_app_owner: None,
            demo_mode: false,
            presence_grace_seconds: crate::config::DEFAULT_PRESENCE_GRACE_SECONDS,
        }
    }

    fn csp(enterprise_url: Option<&str>) -> String {
        content_security_policy(&config(enterprise_url))
            .to_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn always_allows_github_avatars() {
        let policy = csp(None);
        assert!(policy.contains("img-src 'self' data: https://avatars.githubusercontent.com;"));
    }

    #[test]
    fn allows_enterprise_origin_when_configured() {
        let policy = csp(Some("https://github.example.com"));
        assert!(
            policy.contains("https://avatars.githubusercontent.com https://github.example.com;")
        );
    }

    #[test]
    fn enterprise_origin_keeps_non_default_port() {
        let policy = csp(Some("https://github.example.com:8443"));
        assert!(policy.contains("https://github.example.com:8443;"));
    }

    #[test]
    fn ignores_unparseable_enterprise_url() {
        let policy = csp(Some("not a url"));
        assert!(policy.contains("img-src 'self' data: https://avatars.githubusercontent.com;"));
    }
}
