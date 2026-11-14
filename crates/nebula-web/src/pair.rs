//! What a device on the tailnet sees before it is paired ([`crate::devices`]):
//! one plain page, served in place of every other, that says to pair it
//! from nebula on the Mac and takes the code the Mac shows; the same page
//! with the code in it at a pairing's link, `/pair/<code>`; and `POST
//! /pair`, which pairs it and sets its cookie. The page is the server's
//! own, with nothing of the app in it, so an unpaired device is served
//! nothing of this machine but that page and the files that install the
//! app (the manifest and its icons).

use crate::devices::{self, Refused};
use crate::tailnet::Visitor;
use crate::App;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Form};

/// The files that install the app, served to any device on the tailnet:
/// they are the same for every nebula and say nothing of this machine.
const INSTALL_FILES: [&str; 4] = [
    "/manifest.webmanifest",
    "/apple-touch-icon.png",
    "/icon-192.png",
    "/icon-512.png",
];

/// Whether `path` is served to a device on the tailnet before it is paired:
/// the pairing, a pairing's link, and the files that install the app.
pub fn open_to_all(path: &str) -> bool {
    path == "/pair"
        || path
            .strip_prefix("/pair/")
            .is_some_and(|code| !code.contains('/'))
        || INSTALL_FILES.contains(&path)
}

/// What a device that is not let in gets at `path`: the page for a page,
/// and for the API an error saying the same.
pub fn refused(path: &str, why: Refused, login: &str, headers: &HeaderMap) -> Response {
    let status = match why {
        Refused::OtherUser => StatusCode::FORBIDDEN,
        _ => StatusCode::UNAUTHORIZED,
    };
    if path.starts_with("/api/") {
        return crate::error(status, why.message());
    }
    let problem = (why == Refused::OtherUser).then(|| why.message());
    page(status, None, problem, &guess(headers), login)
}

/// `GET /pair/<code>`: the page a pairing's link opens, to pair with a
/// press, or saying the link no longer pairs.
pub async fn link(
    State(app): State<App>,
    Extension(visitor): Extension<Visitor>,
    Path(code): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Visitor::Tailnet { login, .. } = visitor else {
        return on_the_mac();
    };
    match app.devices.pairs(&code) {
        Ok(()) => page(StatusCode::OK, Some(&code), None, &guess(&headers), &login),
        Err(_) => page(
            StatusCode::GONE,
            None,
            Some("This link has expired or was used already: make a new one with Pair a phone on your Mac, or type the code it shows here."),
            &guess(&headers),
            &login,
        ),
    }
}

#[derive(serde::Deserialize)]
pub struct PairForm {
    #[serde(default)]
    code: String,
    #[serde(default)]
    name: String,
}

/// `POST /pair`: pair the device that sends the open pairing's code, and
/// keep it paired with a cookie.
pub async fn pair(
    State(app): State<App>,
    Extension(visitor): Extension<Visitor>,
    headers: HeaderMap,
    Form(form): Form<PairForm>,
) -> Response {
    let Visitor::Tailnet { login, .. } = visitor else {
        return on_the_mac();
    };
    match app
        .devices
        .pair(&form.code, &form.name, &login, devices::token_in(&headers))
    {
        Ok(token) => (
            StatusCode::SEE_OTHER,
            [
                (header::LOCATION, "/".to_string()),
                (header::SET_COOKIE, devices::set_cookie(&token)),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
        )
            .into_response(),
        Err(why) => page(
            StatusCode::FORBIDDEN,
            None,
            Some(why.message()),
            &form.name,
            &login,
        ),
    }
}

fn on_the_mac() -> Response {
    crate::error(
        StatusCode::NOT_FOUND,
        "Pairing is for a device that reaches this page over the tailnet.",
    )
}

fn guess(headers: &HeaderMap) -> String {
    let agent = headers
        .get(header::USER_AGENT)
        .and_then(|a| a.to_str().ok())
        .unwrap_or_default();
    devices::name_from(agent).to_string()
}

/// The page: with `code`, the one a link opens, which needs only a press;
/// without, the one that asks for the code. `problem` says what went
/// wrong, `name` fills the device's name, and `login` is the Tailscale user
/// asking.
fn page(
    status: StatusCode,
    code: Option<&str>,
    problem: Option<&str>,
    name: &str,
    login: &str,
) -> Response {
    let lead = match code {
        Some(_) => "<p>This link pairs this device with nebula on your Mac, until you remove it there.</p>\n<p class=\"muted\">To keep the page on the Home Screen, pair it here first, then choose Add to Home Screen in Safari's Share menu.</p>",
        None => "<p>Pair this device from nebula on your Mac: open Phones in the top bar of its page and choose Pair a phone.</p>\n<p class=\"muted\">Then scan the code it shows with this device's camera, or type the code here.</p>",
    };
    let problem = problem.map_or(String::new(), |p| {
        format!("<p class=\"problem\" role=\"alert\">{}</p>\n", esc(p))
    });
    let code_field = match code {
        Some(code) => format!("<input type=\"hidden\" name=\"code\" value=\"{}\">", esc(code)),
        None => "<label>Code<input name=\"code\" class=\"code\" placeholder=\"ABCD-EFGH\" required maxlength=\"12\" autocomplete=\"one-time-code\" autocapitalize=\"characters\" autocorrect=\"off\" spellcheck=\"false\" autofocus></label>".to_string(),
    };
    let body = format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<meta name="color-scheme" content="dark">
<meta name="theme-color" content="#1e2030">
<meta name="referrer" content="same-origin">
<meta name="mobile-web-app-capable" content="yes">
<meta name="apple-mobile-web-app-capable" content="yes">
<meta name="apple-mobile-web-app-title" content="nebula">
<meta name="apple-mobile-web-app-status-bar-style" content="black-translucent">
<link rel="manifest" href="/manifest.webmanifest">
<link rel="apple-touch-icon" href="/apple-touch-icon.png">
<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23c6a0f6' stroke-width='2.5'%3E%3Ccircle cx='12' cy='12' r='3'/%3E%3Ccircle cx='12' cy='12' r='9' stroke-dasharray='3 3'/%3E%3C/svg%3E">
<title>Pair this device</title>
<style>{STYLE}</style>
</head>
<body>
<main>
<p class="mark"><svg aria-hidden="true" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke-width="2"><circle cx="12" cy="12" r="3"/><circle cx="12" cy="12" r="9" stroke-dasharray="3 3"/></svg>nebula</p>
<h1>Pair this device</h1>
{problem}{lead}
<form method="post" action="/pair">
{code_field}
<label>Name<input name="name" value="{name}" maxlength="40" autocomplete="off" autocorrect="off"></label>
<button type="submit">Pair this device</button>
</form>
<p class="quiet">Signed in to Tailscale as {login}.</p>
</main>
</body>
</html>
"##,
        name = esc(name),
        login = esc(login),
    );
    (
        status,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

const STYLE: &str = "
:root { color-scheme: dark; }
* { box-sizing: border-box; }
html, body { margin: 0; background: #24273a; }
body { min-height: 100vh; min-height: 100dvh; color: #cad3f5; font: 14px/1.55 ui-monospace, \"JetBrains Mono\", SFMono-Regular, Menlo, monospace; -webkit-text-size-adjust: 100%; }
main { display: flex; flex-direction: column; gap: 12px; max-width: 30rem; margin: 0 auto; padding: calc(28px + env(safe-area-inset-top)) max(20px, env(safe-area-inset-right)) calc(28px + env(safe-area-inset-bottom)) max(20px, env(safe-area-inset-left)); }
p, h1 { margin: 0; }
.mark { display: flex; align-items: center; gap: 8px; margin-bottom: 20px; font-weight: 700; }
.mark svg { stroke: #c6a0f6; }
h1 { font-size: 20px; line-height: 1.3; }
.muted { color: #a5adcb; }
.quiet { color: #939ab7; font-size: 12px; overflow-wrap: anywhere; }
.problem { padding: 10px 12px; border: 1px solid rgb(237 135 150 / 0.4); border-radius: 8px; background: rgb(237 135 150 / 0.08); color: #ed8796; }
form { display: flex; flex-direction: column; gap: 16px; margin: 12px 0 8px; }
label { display: flex; flex-direction: column; gap: 6px; color: #a5adcb; font-size: 12px; font-weight: 700; letter-spacing: 0.08em; text-transform: uppercase; }
input { min-height: 44px; padding: 10px 12px; border: 1px solid #494d64; border-radius: 8px; background: #1e2030; color: #cad3f5; font: 16px/1.4 ui-monospace, \"JetBrains Mono\", SFMono-Regular, Menlo, monospace; letter-spacing: normal; text-transform: none; }
input.code { letter-spacing: 0.14em; text-transform: uppercase; }
input::placeholder { color: #939ab7; opacity: 1; }
input:focus-visible, button:focus-visible { outline: 2px solid #b7bdf8; outline-offset: 2px; }
button { min-height: 44px; border: 0; border-radius: 8px; background: #c6a0f6; color: #24273a; font: 700 15px/1 ui-monospace, \"JetBrains Mono\", SFMono-Regular, Menlo, monospace; cursor: pointer; }
button:active { background: #b7bdf8; }
";

/// `text` safe in HTML text and in a quoted attribute.
fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_pairing_and_the_install_files_are_open_before_pairing() {
        for path in [
            "/pair",
            "/pair/K7QXM2PD",
            "/manifest.webmanifest",
            "/icon-192.png",
        ] {
            assert!(open_to_all(path), "{path}");
        }
        for path in [
            "/",
            "/pair/x/y",
            "/pair/../api/state",
            "/pairs",
            "/api/devices",
            "/index.html",
            "/assets/index.js",
        ] {
            assert!(!open_to_all(path), "{path}");
        }
    }

    #[test]
    fn what_the_page_shows_is_escaped() {
        assert_eq!(
            esc(r#"<a href="x">'&'</a>"#),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }
}
