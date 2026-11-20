//! What a device on the tailnet sees before it is paired ([`crate::devices`]):
//! one plain page, served in place of every other, that says to pair it
//! from nebula on the Mac and takes the code the Mac shows; the same page
//! with the code in it at a pairing's link, `/pair/<code>`; and `POST
//! /pair`, which pairs it and sets its cookie. The page is the server's
//! own, with nothing of the app in it, so an unpaired device is served
//! nothing of this machine but that page, the files that install the app
//! (the manifest, its icons and the service worker) and the font the page
//! is set in.

use crate::devices::{self, Refused};
use crate::tailnet::Visitor;
use crate::App;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Form};

/// The files that install the app, served to any device on the tailnet:
/// they are the same for every nebula and say nothing of this machine. The
/// service worker shows a notification pushed to the device, and nothing
/// else.
const INSTALL_FILES: [&str; 6] = [
    "/manifest.webmanifest",
    "/deck.webmanifest",
    "/apple-touch-icon.png",
    "/icon-192.png",
    "/icon-512.png",
    "/sw.js",
];

/// Whether `path` is served to a device on the tailnet before it is paired:
/// the pairing, a pairing's link, the files that install the app, and
/// Inter, which the pairing page is set in.
pub fn open_to_all(path: &str) -> bool {
    open_with(path, crate::assets::inter())
}

/// [`open_to_all`], with Inter at `font` in the build, if it is there.
fn open_with(path: &str, font: Option<&str>) -> bool {
    path == "/pair"
        || path
            .strip_prefix("/pair/")
            .is_some_and(|code| !code.contains('/'))
        || INSTALL_FILES.contains(&path)
        || font.is_some_and(|f| path.strip_prefix('/') == Some(f))
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

fn page(
    status: StatusCode,
    code: Option<&str>,
    problem: Option<&str>,
    name: &str,
    login: &str,
) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        html(code, problem, name, login),
    )
        .into_response()
}

/// The page: with `code`, the one a link opens, which needs only a press;
/// without, the one that asks for the code. `problem` says what went
/// wrong, `name` fills the device's name, and `login` is the Tailscale user
/// asking. It is dark or light as the app's page is on this device.
fn html(code: Option<&str>, problem: Option<&str>, name: &str, login: &str) -> String {
    let lead = match code {
        Some(_) => "<p>This link pairs this device with nebula on your Mac, until you remove it there.</p>\n<p class=\"muted\">To keep the page on the Home Screen, pair it here first, then choose Add to Home Screen in Safari's Share menu.</p>",
        None => "<p>Pair this device from nebula on your Mac: open Phones at the foot of its rail and choose Pair a phone.</p>\n<p class=\"muted\">Then scan the code it shows with this device's camera, or type the code here.</p>",
    };
    let problem = problem.map_or(String::new(), |p| {
        format!("<p class=\"problem\" role=\"alert\">{}</p>\n", esc(p))
    });
    let code_field = match code {
        Some(code) => format!("<input type=\"hidden\" name=\"code\" value=\"{}\">", esc(code)),
        None => "<label>Code<input name=\"code\" class=\"code\" placeholder=\"ABCD-EFGH\" required maxlength=\"12\" autocomplete=\"one-time-code\" autocapitalize=\"characters\" autocorrect=\"off\" spellcheck=\"false\" autofocus></label>".to_string(),
    };
    // Set in Inter where the build has it, else in the system's own type.
    let font = crate::assets::inter().map_or(String::new(), |f| {
        format!("@font-face {{ font-family: \"Inter Variable\"; font-style: normal; font-display: swap; font-weight: 100 900; src: url(\"/{f}\") format(\"woff2-variations\"); }}\n")
    });
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<meta name="color-scheme" content="dark light">
<meta name="theme-color" content="#24273a" media="(prefers-color-scheme: dark)">
<meta name="theme-color" content="#eff1f5" media="(prefers-color-scheme: light)">
<meta name="referrer" content="same-origin">
<meta name="mobile-web-app-capable" content="yes">
<meta name="apple-mobile-web-app-capable" content="yes">
<meta name="apple-mobile-web-app-title" content="nebula">
<meta name="apple-mobile-web-app-status-bar-style" content="default">
<script>
// The theme this device chose on the app's own page, which is this same
// origin, as its index.html reads it before the first paint; else the system's.
try {{ var theme = localStorage.getItem("nebula.theme"); }} catch (e) {{}}
if (theme === "light" || theme === "dark") document.documentElement.dataset.theme = theme;
</script>
<link rel="manifest" href="/manifest.webmanifest">
<link rel="apple-touch-icon" href="/apple-touch-icon.png">
<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='2 2 20 20'%3E%3Ccircle cx='9.5' cy='10' r='6.2' fill='%23c6a0f6' opacity='0.9'/%3E%3Ccircle cx='15.2' cy='11' r='5.2' fill='%238aadf4' opacity='0.8'/%3E%3Ccircle cx='11.6' cy='15.6' r='4.6' fill='%23f5bde6' opacity='0.8'/%3E%3Ccircle cx='15.6' cy='8.2' r='1.1' fill='%23f4f6ff'/%3E%3C/svg%3E">
<title>Pair this device</title>
<style>{font}{STYLE}
@media (prefers-color-scheme: light) {{ :root:not([data-theme="dark"]) {{ {LIGHT} }} }}
:root[data-theme="light"] {{ {LIGHT} }}
</style>
</head>
<body>
<main>
<p class="mark"><svg aria-hidden="true" width="28" height="28" viewBox="2 2 20 20"><circle cx="9.5" cy="10" r="6.2" fill="#c6a0f6" opacity="0.9"/><circle cx="15.2" cy="11" r="5.2" fill="#8aadf4" opacity="0.8"/><circle cx="11.6" cy="15.6" r="4.6" fill="#f5bde6" opacity="0.8"/><circle cx="15.6" cy="8.2" r="1.1" fill="#f4f6ff"/></svg>nebula</p>
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
    )
}

/// The app's tokens (`app/src/tokens.css`): dark is Catppuccin Macchiato,
/// and [`LIGHT`] Catppuccin Latte, its quiet text darkened to 4.5:1.
const STYLE: &str = r#"
:root { --bg: #24273a; --panel: #1e2030; --sunk: #181926; --raise: #363a4f; --text: #cad3f5; --sub: #b8c0e0; --muted: #a5adcb; --faint: #939ab7; --accent: #c6a0f6; --on-accent: #181926; --red: #ed8796; --problem: #ed8796; color-scheme: dark; }
* { box-sizing: border-box; }
html, body { margin: 0; background: var(--bg); }
body { min-height: 100vh; min-height: 100dvh; color: var(--text); font: 15px/1.5 "Inter Variable", system-ui, -apple-system, "Segoe UI", sans-serif; -webkit-font-smoothing: antialiased; -webkit-text-size-adjust: 100%; }
main { display: flex; flex-direction: column; gap: 12px; max-width: 27rem; margin: 0 auto; padding: calc(24px + env(safe-area-inset-top)) max(20px, env(safe-area-inset-right)) calc(28px + env(safe-area-inset-bottom)) max(20px, env(safe-area-inset-left)); }
p, h1 { margin: 0; }
.mark { display: flex; align-items: center; gap: 9px; margin-bottom: 22px; font-size: 19px; font-weight: 600; letter-spacing: -0.015em; }
h1 { font-size: 24px; font-weight: 600; letter-spacing: -0.015em; line-height: 1.25; }
.muted { color: var(--muted); }
.quiet { color: var(--faint); font-size: 13px; overflow-wrap: anywhere; }
.problem { padding: 11px 14px; border: 1px solid color-mix(in srgb, var(--red) 35%, transparent); border-radius: 12px; background: color-mix(in srgb, var(--red) 9%, transparent); color: var(--problem); }
form { display: flex; flex-direction: column; gap: 16px; margin: 14px 0 6px; }
label { display: flex; flex-direction: column; gap: 6px; color: var(--sub); font-size: 13px; font-weight: 600; }
input { min-height: 48px; padding: 11px 14px; border: 1px solid var(--raise); border-radius: 12px; outline: none; background: var(--sunk); color: var(--text); font: inherit; font-size: 17px; font-weight: 400; line-height: 1.3; }
input:focus { border-color: color-mix(in srgb, var(--accent) 60%, transparent); }
input.code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-variant-ligatures: none; letter-spacing: 0.14em; text-transform: uppercase; }
input::placeholder { color: var(--faint); opacity: 1; }
button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
button { min-height: 48px; border: 0; border-radius: 12px; background: var(--accent); color: var(--on-accent); font: inherit; font-size: 16px; font-weight: 600; line-height: 1; cursor: pointer; }
button:active { background: color-mix(in srgb, var(--accent) 88%, var(--text)); }
"#;

/// Latte, in place of Macchiato's values, where the page is light.
const LIGHT: &str = "--bg: #eff1f5; --panel: #e6e9ef; --sunk: #dce0e8; --raise: #ccd0da; --text: #4c4f69; --sub: #545770; --muted: #5c5f77; --faint: #65687e; --accent: #8839ef; --on-accent: #eff1f5; --red: #d20f39; --problem: #930b28; color-scheme: light;";

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
            "/deck.webmanifest",
            "/icon-192.png",
            "/sw.js",
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
    fn the_font_the_page_is_set_in_is_open_and_nothing_beside_it() {
        let font = Some("assets/inter-latin-wght-normal-Dx4kXJAl.woff2");
        assert!(open_with(
            "/assets/inter-latin-wght-normal-Dx4kXJAl.woff2",
            font
        ));
        for path in [
            "/assets/inter-latin-wght-normal-Dx4kXJAl.woff2/x",
            "/assets/jetbrains-mono-latin-wght-normal-B9CIFXIH.woff2",
            "/assets/index-CeMc-Ie8.js",
            "/assets/",
        ] {
            assert!(!open_with(path, font), "{path}");
        }
        assert!(!open_with(
            "/assets/inter-latin-wght-normal-Dx4kXJAl.woff2",
            None
        ));
    }

    #[test]
    fn the_page_is_light_as_the_app_is_on_this_device() {
        let page = html(
            None,
            Some("That code is not right."),
            "iPhone",
            "ada@example.com",
        );
        // The device's own choice, under the key the app's index.html reads.
        assert!(page.contains(r#"localStorage.getItem("nebula.theme")"#));
        assert!(page.contains(r#":root[data-theme="light"] { --bg: #eff1f5;"#));
        // Else the system's.
        assert!(page.contains(r#"@media (prefers-color-scheme: light) { :root:not([data-theme="dark"]) { --bg: #eff1f5;"#));
        assert!(page.contains(r#"<p class="problem" role="alert">That code is not right.</p>"#));
        if let Some(out) = std::env::var_os("NEBULA_PAIR_PAGE") {
            std::fs::write(out, &page).unwrap();
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
