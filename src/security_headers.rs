use axum::{
    http::{header, HeaderValue, Request},
    middleware::Next,
    response::Response,
};

/// Security headers applied to every response.
///
/// The CSP is strict: all JavaScript is served from `/static` (or the pinned,
/// SRI-protected htmx CDN bundle), so inline scripts and `eval` are banned.
/// All inline `onclick`/`hx-on` handlers were removed from the templates to
/// make this possible.
pub async fn apply(request: Request<axum::body::Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; \
             script-src 'self' https://cdn.jsdelivr.net; \
             style-src 'self' https://fonts.googleapis.com; \
             font-src 'self' https://fonts.gstatic.com; \
             img-src 'self' data:; \
             connect-src 'self'; \
             base-uri 'self'; \
             form-action 'self'; \
             frame-ancestors 'self'; \
             object-src 'none'",
        ),
    );
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
