//! Who a request comes from: this machine itself, at `127.0.0.1` or
//! `localhost` through no proxy, or a device on the machine's TAILNET
//! through `tailscale serve`, which proxies `https://<the machine's tailnet
//! name>/` to this server on 127.0.0.1 and says which Tailscale user asks
//! in `Tailscale-User-Login`, a header it takes off any request that brings
//! its own. Anything else is refused, so a page another site serves cannot
//! reach the server by a name it resolves to this machine (DNS rebinding).
//!
//! The tailnet name is read from `tailscale status --json` when the server
//! starts, and again when a request through `tailscale serve` names a host
//! the server does not know, at most once every [`READ_AGAIN`], so a name
//! that changed, or a Tailscale started after the server, needs no restart
//! and no setting.

use axum::http::{header, HeaderMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The header `tailscale serve` names the Tailscale user who asks by.
pub const LOGIN: &str = "tailscale-user-login";

/// Headers a request that came through a proxy carries: `tailscale serve`
/// sets the `X-Forwarded-*` ones on every request, and a Tailscale one.
const PROXIED: [&str; 6] = [
    "x-forwarded-for",
    "x-forwarded-host",
    "forwarded",
    LOGIN,
    "tailscale-user-name",
    "tailscale-funnel-request",
];

/// How long `tailscale status --json` may take.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// How soon an unknown host may have the name read again.
const READ_AGAIN: Duration = Duration::from_secs(10);
/// Where the macOS app keeps its CLI, for a Mac whose PATH has none.
const MAC_APP_CLI: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";

/// Who a request comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visitor {
    /// This machine.
    Local,
    /// A device on the tailnet, through `tailscale serve`: the host it
    /// named, the tailnet name with any port, and its Tailscale user.
    Tailnet { host: String, login: String },
}

impl Visitor {
    /// Whether the request's `Origin` is the page this visitor was served:
    /// `http://` and a loopback name at `port` here, `https://` and the
    /// host it named on the tailnet.
    pub fn own_origin(&self, headers: &HeaderMap, port: u16) -> bool {
        let Some(origin) = headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) else {
            return false;
        };
        match self {
            Visitor::Local => origin
                .strip_prefix("http://")
                .is_some_and(|authority| loopback(authority, port)),
            Visitor::Tailnet { host, .. } => origin
                .strip_prefix("https://")
                .is_some_and(|authority| authority.eq_ignore_ascii_case(host)),
        }
    }
}

/// The names this server answers to on this machine: loopback at the
/// bound port.
fn loopback(authority: &str, port: u16) -> bool {
    ["127.0.0.1", "localhost"]
        .iter()
        .any(|host| authority.eq_ignore_ascii_case(&format!("{host}:{port}")))
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// Who the request with `headers` comes from, to a server bound at `port`
/// on a machine whose tailnet name is `name`, or why it is refused.
pub fn visitor(headers: &HeaderMap, port: u16, name: Option<&str>) -> Result<Visitor, String> {
    let refused = || format!("this server answers only to http://127.0.0.1:{port}");
    let host = header(headers, header::HOST.as_str()).ok_or_else(refused)?;
    let proxied = PROXIED.iter().any(|h| headers.contains_key(*h));
    if loopback(host, port) {
        return if proxied {
            Err(
                "a request through a proxy must name this machine's tailnet name as its host"
                    .into(),
            )
        } else {
            Ok(Visitor::Local)
        };
    }
    let bare = host.rsplit_once(':').map_or(host, |(bare, _)| bare);
    if !name.is_some_and(|name| bare.eq_ignore_ascii_case(name)) {
        return Err(refused());
    }
    let login = header(headers, LOGIN).map(str::trim).unwrap_or_default();
    if login.is_empty() {
        return Err(
            "tailscale serve names no Tailscale user for this device: only a person's device on the tailnet may reach this page"
                .into(),
        );
    }
    if header(headers, "x-forwarded-proto") != Some("https") {
        return Err(format!(
            "reach this page at https://{bare}/, through tailscale serve over HTTPS: a device is kept paired by a cookie only HTTPS carries"
        ));
    }
    Ok(Visitor::Tailnet {
        host: host.to_string(),
        login: login.to_string(),
    })
}

/// This machine's tailnet name, as `tailscale status --json` last said it.
#[derive(Clone)]
pub struct Tailnet(Arc<Inner>);

struct Inner {
    name: Mutex<Option<String>>,
    /// When the name was last read; never set for a name that is fixed.
    read_at: Mutex<Option<Instant>>,
    reads: bool,
}

impl Tailnet {
    /// The name as `tailscale status --json` says it, read on a task of its
    /// own, so the server never waits on the CLI to start.
    pub fn spawn() -> Tailnet {
        let tailnet = Tailnet(Arc::new(Inner {
            name: Mutex::new(None),
            read_at: Mutex::new(None),
            reads: true,
        }));
        let reading = tailnet.clone();
        tokio::spawn(async move { reading.read_again(true).await });
        tailnet
    }

    /// A name that is never read: what tests stand in.
    #[cfg(test)]
    pub fn fixed(name: Option<&str>) -> Tailnet {
        Tailnet(Arc::new(Inner {
            name: Mutex::new(name.map(str::to_string)),
            read_at: Mutex::new(None),
            reads: false,
        }))
    }

    pub fn name(&self) -> Option<String> {
        self.0
            .name
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Read the name again: at once when `now`, else only when it was not
    /// read in the last [`READ_AGAIN`].
    pub async fn read_again(&self, now: bool) {
        if !self.0.reads {
            return;
        }
        {
            let mut at = self.0.read_at.lock().unwrap_or_else(|e| e.into_inner());
            if !now && at.is_some_and(|at| at.elapsed() < READ_AGAIN) {
                return;
            }
            *at = Some(Instant::now());
        }
        let name = status().await;
        *self.0.name.lock().unwrap_or_else(|e| e.into_inner()) = name;
    }

    /// Who the request with `headers` comes from ([`visitor`]); a request
    /// through `tailscale serve` for a host it does not know has the name
    /// read again first, in case it changed.
    pub async fn visitor(&self, headers: &HeaderMap, port: u16) -> Result<Visitor, String> {
        let got = visitor(headers, port, self.name().as_deref());
        if got.is_ok() || !headers.contains_key(LOGIN) {
            return got;
        }
        let before = self.name();
        self.read_again(false).await;
        if self.name() == before {
            return got;
        }
        visitor(headers, port, self.name().as_deref())
    }
}

/// `tailscale status --json`'s name for this machine, from the `tailscale`
/// on PATH or the macOS app's.
async fn status() -> Option<String> {
    for cli in ["tailscale", MAC_APP_CLI] {
        let run = tokio::process::Command::new(cli)
            .args(["status", "--json"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .output();
        match tokio::time::timeout(READ_TIMEOUT, run).await {
            Ok(Ok(out)) => return dns_name(&out.stdout),
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::NotFound => continue,
            _ => return None,
        }
    }
    None
}

/// The machine's own name in `tailscale status --json`, without the dot
/// that ends it.
fn dns_name(json: &[u8]) -> Option<String> {
    let status: serde_json::Value = serde_json::from_slice(json).ok()?;
    let name = status["Self"]["DNSName"].as_str()?.trim_end_matches('.');
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAME: &str = "my-mac.tailnet-name.ts.net";
    const PORT: u16 = 7682;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.append(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        map
    }

    /// What `tailscale serve` sends for a request from a person's device.
    fn served(host: &str) -> Vec<(&str, &str)> {
        vec![
            ("host", host),
            ("x-forwarded-for", "100.101.102.103"),
            ("x-forwarded-host", host),
            ("x-forwarded-proto", "https"),
            ("tailscale-user-login", "ada@example.com"),
            ("tailscale-user-name", "Ada"),
        ]
    }

    #[test]
    fn this_machine_is_local_only_at_its_own_port_through_no_proxy() {
        for host in ["127.0.0.1:7682", "localhost:7682", "LOCALHOST:7682"] {
            let got = visitor(&headers(&[("host", host)]), PORT, Some(NAME));
            assert_eq!(got, Ok(Visitor::Local), "{host}");
        }
        for host in [
            "127.0.0.1",
            "127.0.0.1:7683",
            "evil.example:7682",
            "192.168.1.10:7682",
        ] {
            assert!(
                visitor(&headers(&[("host", host)]), PORT, Some(NAME)).is_err(),
                "{host}"
            );
        }
        assert!(visitor(&headers(&[]), PORT, Some(NAME)).is_err(), "no Host");
        // tailscale serve forwards the Host a device sends, so a device can
        // name this machine's loopback: its proxy headers give it away.
        for proxy in [
            "x-forwarded-for",
            "x-forwarded-host",
            "forwarded",
            "tailscale-user-login",
        ] {
            let got = visitor(
                &headers(&[("host", "127.0.0.1:7682"), (proxy, "x")]),
                PORT,
                Some(NAME),
            );
            assert!(got.is_err(), "{proxy}");
        }
    }

    #[test]
    fn a_device_on_the_tailnet_names_the_tailnet_name_as_a_user_over_https() {
        for host in [
            NAME,
            "MY-MAC.tailnet-name.ts.net",
            "my-mac.tailnet-name.ts.net:8443",
        ] {
            assert_eq!(
                visitor(&headers(&served(host)), PORT, Some(NAME)),
                Ok(Visitor::Tailnet {
                    host: host.into(),
                    login: "ada@example.com".into()
                }),
                "{host}"
            );
        }
        // Not this machine's name, or none known.
        assert!(visitor(
            &headers(&served("other.tailnet-name.ts.net")),
            PORT,
            Some(NAME)
        )
        .is_err());
        assert!(visitor(&headers(&served(NAME)), PORT, None).is_err());
        // A tagged device or a Funnel request: no user.
        let mut tagged = served(NAME);
        tagged.retain(|(k, _)| !k.starts_with("tailscale-user"));
        assert!(visitor(&headers(&tagged), PORT, Some(NAME)).is_err());
        let mut blank = served(NAME);
        blank.retain(|(k, _)| *k != LOGIN);
        blank.push((LOGIN, " "));
        assert!(visitor(&headers(&blank), PORT, Some(NAME)).is_err());
        // Plain HTTP, where no Secure cookie keeps a device paired.
        let mut plain = served(NAME);
        plain.retain(|(k, _)| *k != "x-forwarded-proto");
        let why = visitor(&headers(&plain), PORT, Some(NAME)).unwrap_err();
        assert!(why.contains(&format!("https://{NAME}/")), "{why}");
    }

    #[test]
    fn an_act_must_come_from_the_page_the_visitor_was_served() {
        let local = Visitor::Local;
        let tailnet = Visitor::Tailnet {
            host: NAME.into(),
            login: "ada@example.com".into(),
        };
        let origin = |o: &str| headers(&[("origin", o)]);
        assert!(local.own_origin(&origin("http://127.0.0.1:7682"), PORT));
        assert!(local.own_origin(&origin("http://localhost:7682"), PORT));
        assert!(tailnet.own_origin(&origin(&format!("https://{NAME}")), PORT));
        for o in [
            format!("http://{NAME}"),
            format!("https://{NAME}:8443"),
            "https://evil.example".into(),
            "null".into(),
            "http://127.0.0.1:7682".into(),
        ] {
            assert!(!tailnet.own_origin(&origin(&o), PORT), "{o}");
        }
        assert!(!local.own_origin(&origin(&format!("https://{NAME}")), PORT));
        assert!(!tailnet.own_origin(&headers(&[]), PORT), "no Origin");
    }

    #[test]
    fn the_name_is_the_machine_s_own_without_its_final_dot() {
        let status = br#"{"BackendState":"Running","Self":{"DNSName":"My-Mac.tailnet-name.ts.net.","HostName":"My Mac"},"CertDomains":["my-mac.tailnet-name.ts.net"]}"#;
        assert_eq!(dns_name(status).as_deref(), Some(NAME));
        assert_eq!(dns_name(br#"{"Self":{"DNSName":""}}"#), None);
        assert_eq!(dns_name(br#"{"BackendState":"NoState"}"#), None);
        assert_eq!(dns_name(b"not json"), None);
    }
}
