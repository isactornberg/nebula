//! The web app, as built into the binary (`build.rs`), served at every
//! path that is not the API's: a file of the build by its path, else its
//! `index.html`, so a deep link (`/s/<id>`) opens the app on it. A binary
//! built with no build of the app serves a page that says how to make one.

use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

/// What every path serves when the binary carries no build of the app.
const NOT_BUILT: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="dark light">
<title>nebula</title>
<style>
  body { font: 15px/1.5 ui-sans-serif, system-ui, sans-serif; max-width: 36rem; margin: 15vh auto; padding: 0 16px; }
  code { font: 13px ui-monospace, monospace; }
</style>
</head>
<body>
<h1>The web app is not built</h1>
<p>This <code>nebula</code> binary was built without the web app, so there is nothing to show here yet. The API under <code>/api</code> works as it is.</p>
<p>Build nebula again with the app in it: <code>make build</code> or <code>make install</code> from a checkout builds the app first, or run <code>pnpm install --frozen-lockfile &amp;&amp; pnpm build</code> in <code>crates/nebula-web/app</code> before <code>cargo build</code>.</p>
</body>
</html>
"#;

fn file(path: &str) -> Option<&'static [u8]> {
    FILES
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, bytes)| *bytes)
}

/// The page every path that names no file of the build serves: the app's
/// `index.html`, or the page saying it was not built.
pub fn index_html() -> &'static [u8] {
    file("index.html").unwrap_or(NOT_BUILT.as_bytes())
}

/// The file `uri` names in the build, else [`index_html`].
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let (body, path) = match file(path).filter(|_| !path.is_empty()) {
        Some(body) => (body, path),
        None => (index_html(), "index.html"),
    };
    // Vite names every file under `assets/` by a hash of its contents.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(mime(path))),
            (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
        ],
        body,
    )
        .into_response()
}

/// The media type of a file of the build, by its extension.
fn mime(path: &str) -> &'static str {
    match path.rsplit_once('.').map_or("", |(_, ext)| ext) {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
