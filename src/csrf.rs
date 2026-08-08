use crate::AppState;
use axum::{
    extract::State,
    http::{header, Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use url::Url;

/// Cross-site request forgery defense-in-depth for cookie-authenticated
/// mutations.
///
/// The primary defense is the session cookie's `SameSite=Lax` attribute:
/// browsers refuse to send it on cross-site POST/PUT/PATCH/DELETE requests.
/// This middleware adds a second line of defense for the cases SameSite does
/// not cover (e.g. an attacker controlling a sibling subdomain, which counts
/// as same-site): when a state-changing request carries an `Origin` (or, for
/// legacy clients, a `Referer`) header, it must match the request's `Host` or
/// the configured public URL.
///
/// Requests without an `Origin`/`Referer` header are allowed: Firefox, for
/// one, omits both on same-origin form POSTs, while browsers always send
/// `Origin` on cross-origin requests — so a missing header here cannot be a
/// cross-site request from a modern browser, and SameSite=Lax already
/// neutralizes such requests from legacy browsers.
pub async fn check(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if is_unsafe_method(request.method()) && !origin_is_acceptable(&request, &state.config) {
        tracing::warn!(
            origin = request
                .headers()
                .get(header::ORIGIN)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("<none>"),
            referer = request
                .headers()
                .get(header::REFERER)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("<none>"),
            host = request
                .headers()
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("<none>"),
            "rejected state-changing request from a foreign origin"
        );
        return (
            StatusCode::FORBIDDEN,
            "Cross-site request rejected (Origin/Referer does not match this site)",
        )
            .into_response();
    }
    next.run(request).await
}

fn is_unsafe_method(method: &Method) -> bool {
    matches!(
        method,
        &Method::POST | &Method::PUT | &Method::PATCH | &Method::DELETE
    )
}

fn origin_is_acceptable(
    request: &Request<axum::body::Body>,
    config: &crate::config::Config,
) -> bool {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "null")
        .and_then(normalize_host);

    // Legacy clients may omit Origin but send a Referer.
    let origin = origin.or_else(|| {
        request
            .headers()
            .get(header::REFERER)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(normalize_host)
    });

    // No Origin/Referer at all: allowed (see module docs).
    let Some((origin_host, origin_port)) = origin else {
        return true;
    };

    // The origin must match either the request's Host header (as sent by the
    // browser) or the configured public URL (which covers deployments where a
    // proxy rewrites the Host header).
    let request_host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(normalize_host);

    [request_host, normalize_host(&config.public_url)]
        .into_iter()
        .flatten()
        .any(|(host, port)| {
            host == origin_host && (origin_port.is_none() || port.is_none() || port == origin_port)
        })
}

/// Normalizes an origin-like value (`https://example.com:8443`) or bare
/// host/port (`example.com:8443`) to `(hostname, port)`. Returns `None` for
/// values that are not valid origins/hosts.
fn normalize_host(value: &str) -> Option<(String, Option<u16>)> {
    let value = value.trim().to_ascii_lowercase();
    if value.contains("://") {
        let url = Url::parse(&value).ok()?;
        return Some((url.host_str()?.to_string(), url.port()));
    }
    let (host, port) = match value.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, Some(port.parse().ok()?))
        }
        _ => (value.as_str(), None),
    };
    if host.is_empty() {
        return None;
    }
    Some((host.to_string(), port))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req_with(
        host: &str,
        origin: Option<&str>,
        referer: Option<&str>,
    ) -> Request<axum::body::Body> {
        let mut builder = Request::builder().method(Method::POST).uri("/items/1");
        builder = builder.header(header::HOST, host);
        if let Some(origin) = origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        if let Some(referer) = referer {
            builder = builder.header(header::REFERER, referer);
        }
        builder.body(axum::body::Body::empty()).unwrap()
    }

    fn config(public_url: &str) -> crate::config::Config {
        crate::config::Config {
            bind_address: "0.0.0.0:3000".to_string(),
            database_url: "postgres://unused".to_string(),
            public_url: public_url.to_string(),
            github_client_id: String::new(),
            github_client_secret: String::new(),
            github_enterprise_url: None,
            github_admin_org: Some("org".to_string()),
            github_admin_team_slug: Some("team".to_string()),
            github_user_orgs: Vec::new(),
            github_app_owner: None,
            demo_mode: false,
        }
    }

    #[test]
    fn allows_missing_origin_and_referer() {
        // Firefox omits both on same-origin form POSTs; SameSite=Lax is the
        // primary defense for the cross-site case.
        let request = req_with("example.com", None, None);
        assert!(origin_is_acceptable(
            &request,
            &config("https://example.com")
        ));
    }

    #[test]
    fn allows_origin_null() {
        let request = req_with("example.com", Some("null"), None);
        assert!(origin_is_acceptable(
            &request,
            &config("https://example.com")
        ));
    }

    #[test]
    fn rejects_cross_site_origin() {
        let request = req_with("example.com", Some("https://evil.example"), None);
        assert!(!origin_is_acceptable(
            &request,
            &config("https://example.com")
        ));
    }

    #[test]
    fn accepts_same_origin() {
        let request = req_with("example.com", Some("https://example.com"), None);
        assert!(origin_is_acceptable(
            &request,
            &config("https://example.com")
        ));
    }

    #[test]
    fn accepts_origin_with_port_matching_host() {
        let request = req_with("localhost:8080", Some("http://localhost:8080"), None);
        assert!(origin_is_acceptable(
            &request,
            &config("http://localhost:3000")
        ));
    }

    #[test]
    fn accepts_origin_matching_public_url_when_host_differs() {
        let request = req_with("internal-host", Some("https://rostfacto.example.com"), None);
        assert!(origin_is_acceptable(
            &request,
            &config("https://rostfacto.example.com")
        ));
    }

    #[test]
    fn falls_back_to_referer() {
        let request = req_with("example.com", None, Some("https://example.com/retros"));
        assert!(origin_is_acceptable(
            &request,
            &config("https://example.com")
        ));
    }

    #[test]
    fn rejects_foreign_referer() {
        let request = req_with("example.com", None, Some("https://evil.example/x"));
        assert!(!origin_is_acceptable(
            &request,
            &config("https://example.com")
        ));
    }

    #[test]
    fn unsafe_methods_are_detected() {
        assert!(is_unsafe_method(&Method::POST));
        assert!(is_unsafe_method(&Method::DELETE));
        assert!(!is_unsafe_method(&Method::GET));
        assert!(!is_unsafe_method(&Method::HEAD));
        assert!(!is_unsafe_method(&Method::OPTIONS));
    }
}
