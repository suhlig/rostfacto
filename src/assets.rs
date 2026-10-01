//! Embedded static assets.
//!
//! The `static/` tree is compiled into the binary by `rust-embed`, so a
//! non-debug build is a single self-contained artifact with no asset directory
//! to ship beside it. Debug builds read the files from disk instead (from the
//! `static/` path recorded at compile time), which keeps the "edit the CSS,
//! reload the page" workflow during development; enable this crate's
//! `debug-embed` feature to embed in debug builds too.

use std::borrow::Cow;
use std::fmt::Write as _;

use axum::{
    body::{Body, Bytes},
    extract::Path,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;

/// The `static/` directory, embedded at compile time.
#[derive(Embed)]
#[folder = "static/"]
struct Assets;

/// Serve a file from the embedded `static/` tree, or 404 if it does not exist.
///
/// `path` is everything below `/static/`, without a leading slash (axum strips
/// it when capturing the `{*path}` wildcard). Lookups are exact map hits, so a
/// traversal attempt such as `../src/main.rs` simply misses.
pub async fn serve(Path(path): Path<String>, headers: HeaderMap) -> Response {
    let Some(file) = Assets::get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let etag = etag_header(file.metadata.sha256_hash());

    if is_not_modified(&headers, &etag) {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        response.headers_mut().insert(header::ETAG, etag);
        return response;
    }

    let mut response = Response::new(Body::from(body_bytes(file.data)));
    let response_headers = response.headers_mut();
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&path)),
    );
    response_headers.insert(header::ETAG, etag);
    // Filenames are not content-hashed, so revalidate on every navigation; the
    // ETag above makes that a cheap 304 when the file did not change.
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

/// The response body as zero-copy bytes: embedded files are `'static`, so a
/// non-debug build borrows them straight out of the binary.
fn body_bytes(data: Cow<'static, [u8]>) -> Bytes {
    match data {
        Cow::Borrowed(bytes) => Bytes::from_static(bytes),
        Cow::Owned(bytes) => Bytes::from(bytes),
    }
}

/// Whether the request's `If-None-Match` matches `etag` (or is the wildcard `*`).
fn is_not_modified(headers: &HeaderMap, etag: &HeaderValue) -> bool {
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|candidate| candidate == "*" || candidate.as_bytes() == etag.as_bytes())
}

/// Build the `ETag` header from an embedded file's content hash.
fn etag_header(hash: [u8; 32]) -> HeaderValue {
    let mut value = String::with_capacity(66);
    value.push('"');
    for byte in hash {
        write!(value, "{byte:02x}").expect("writing to a String cannot fail");
    }
    value.push('"');
    HeaderValue::from_str(&value).expect("a hex digest is a valid header value")
}

/// The `Content-Type` for a static file, keyed off its extension.
///
/// The app sets `X-Content-Type-Options: nosniff`, so this must be correct: a
/// script or stylesheet served under the wrong type is rejected by browsers.
fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("css") => "text/css",
        Some("js") => "text/javascript",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("json") => "application/json",
        Some("woff2") => "font/woff2",
        Some("txt") => "text/plain; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, http::Request, routing::get, Router};
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new().route("/static/{*path}", get(serve))
    }

    async fn request(uri: &str, if_none_match: Option<&str>) -> Response {
        let mut builder = Request::builder().uri(uri);
        if let Some(etag) = if_none_match {
            builder = builder.header(header::IF_NONE_MATCH, etag);
        }
        app()
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    /// A nested path proves the `{*path}` wildcard captures everything below
    /// `/static/` without a leading slash — otherwise `Assets::get` would miss.
    #[tokio::test]
    async fn serves_a_nested_module_with_its_content_type() {
        let response = request("/static/js/site.js", None).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE].to_str().unwrap(),
            "text/javascript"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(!body.is_empty());
    }

    #[tokio::test]
    async fn serves_a_stylesheet() {
        let response = request("/static/custom.css", None).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE].to_str().unwrap(),
            "text/css"
        );
    }

    #[tokio::test]
    async fn unknown_asset_is_not_found() {
        let response = request("/static/nope.css", None).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn revalidates_a_known_asset_with_its_etag() {
        let etag = request("/static/custom.css", None).await.headers()[header::ETAG].clone();

        let response = request("/static/custom.css", Some(etag.to_str().unwrap())).await;

        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(response.headers().get(header::ETAG), Some(&etag));
    }

    #[test]
    fn maps_extensions_to_content_types() {
        assert_eq!(content_type("custom.css"), "text/css");
        assert_eq!(content_type("js/retro.js"), "text/javascript");
        assert_eq!(content_type("happy.svg"), "image/svg+xml");
        assert_eq!(content_type("favicon.ico"), "image/x-icon");
        assert_eq!(content_type("screenshots/board.png"), "image/png");
        assert_eq!(content_type("mystery"), "application/octet-stream");
    }
}
