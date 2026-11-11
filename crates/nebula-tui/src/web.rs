//! The TUI's half of the link with the web app (`nebula serve`). The TUI
//! tells the DAEMON which session it has on screen, which the web app's
//! **Follow TUI** follows, and `⇧W` opens the web app in the browser on
//! that session. The web app's **Open in terminal** comes the other way,
//! as `ServerEvent::ShowSession` (`event_loop::server_events`).

use crate::app::App;
use crossterm::event::{Event, KeyEventKind, MouseEventKind};
use nebula_client::serve::serving_port;
use nebula_core::{paths, AgentId, ClientRequest, SessionRef, SHOW_PROTOCOL};
use std::path::Path;
use std::time::{Duration, Instant};

/// The live session on screen: the pane's, which on the grid reads the
/// card under the cursor and is gone once that is let go of (folded away,
/// Esc), and full screen is all there is, the ORCHESTRATOR VIEW among
/// them. None for a pane reading a pull request or an issue instead, for
/// a terminal or an archived session, and with no pane: the web app
/// shows none of those.
pub(crate) fn on_screen(app: &App) -> Option<AgentId> {
    let drawn = app.pane.collapsed || app.launcher_aimed();
    if !app.launcher_active() || !drawn || !app.pane_shows_terminal() {
        return None;
    }
    let SessionRef::Agent(id) = &app.pane.term.as_ref()?.sref else {
        return None;
    };
    app.tree
        .agents
        .iter()
        .any(|a| &a.id == id && !a.archived)
        .then(|| id.clone())
}

/// Is `event` the user turning to this TUI: a key, a click, a wheel, a
/// paste, or its window coming to the front? Hovering is not.
pub(crate) fn is_touch(event: &Event) -> bool {
    match event {
        Event::Key(key) => key.kind != KeyEventKind::Release,
        Event::Mouse(mouse) => matches!(
            mouse.kind,
            MouseEventKind::Down(_)
                | MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
                | MouseEventKind::ScrollLeft
                | MouseEventKind::ScrollRight
        ),
        Event::Paste(_) | Event::FocusGained => true,
        _ => false,
    }
}

/// Run by the event loop after every turn: tell the daemon what is on
/// screen, when that changed or the user touched this TUI. The first
/// report, sent on the first turn, is what makes this connection a TUI to
/// the daemon.
pub(crate) fn report(app: &mut App, out: &mut Vec<ClientRequest>) {
    let touched = std::mem::take(&mut app.requests.touched);
    if app.requests.daemon_protocol < SHOW_PROTOCOL {
        return;
    }
    let session = on_screen(app);
    if !touched && app.requests.reported.as_ref() == Some(&session) {
        return;
    }
    app.requests.reported = Some(session.clone());
    out.push(ClientRequest::ReportShowing { session, touched });
}

/// What `⇧W` says while it looks for the web app's server.
pub(crate) const OPENING: &str = "opening the web app…";
/// What a second `⇧W` says before the first has found it.
pub(crate) const STILL_OPENING: &str = "still opening the web app…";
/// What `⇧W` says over ssh, where the browser it could open is on another
/// machine than the server it would start.
pub(crate) const OVER_SSH: &str =
    "no browser to open over ssh: run nebula serve here and forward its port with ssh -L";

/// How long a `nebula serve` this TUI started has to write its port.
const START_WAIT: Duration = Duration::from_secs(10);
/// How often that wait looks at the port file.
const START_POLL: Duration = Duration::from_millis(100);

/// What a server-finding job answers: the page's URL, or what to say.
pub(crate) type Answer = Result<String, String>;

/// `⇧W`, and **Open in web app** in a card's menu: the web app in the
/// browser, on `session`, or on its board with none. Finding the server,
/// or starting one when none answers, is a BACKGROUND READ on a thread of
/// its own, landing in [`land`]; a loop with no channel (every unit test)
/// starts nothing.
pub(crate) fn open(app: &mut App, session: Option<AgentId>) {
    app.chrome.dirty = true;
    if app.chrome.is_remote {
        app.chrome.flash = Some(OVER_SSH.into());
        return;
    }
    // A second server started before the first wrote its port would
    // take another one, and the first would run on unseen.
    if app.jobs.web_opening.is_some() {
        app.chrome.flash = Some(STILL_OPENING.into());
        return;
    }
    let page = session.map_or_else(|| "/".to_string(), |id| format!("/s/{id}"));
    app.chrome.flash = Some(OPENING.into());
    if let Some(tx) = app.jobs.web_tx.clone() {
        let page = page.clone();
        std::thread::spawn(move || {
            let log = paths::log_dir().join("serve.log");
            let found = serve_url(&paths::serve_port_path(), || start_serve(&log), START_WAIT)
                .map_err(|why| format!("{why}: see {}", log.display()));
            let _ = tx.send(found.map(|base| base + &page));
        });
    }
    app.jobs.web_opening = Some(page);
}

/// The job's answer: the page opens in the browser, or the footer says
/// why it did not.
pub(crate) fn land(app: &mut App, answer: Answer) {
    app.jobs.web_opening = None;
    app.chrome.dirty = true;
    app.chrome.flash = Some(match answer {
        Ok(url) if crate::event_loop::open_url(&url) => {
            format!("opened {}", crate::app::pretty_url(&url))
        }
        Ok(url) => format!("couldn't open {url}"),
        Err(why) => why,
    });
}

/// The web app's base URL: the `nebula serve` whose port `port_file`
/// names when it answers there, else the one `start` starts, once it has
/// written its port within `wait`.
fn serve_url(
    port_file: &Path,
    start: impl FnOnce() -> std::io::Result<()>,
    wait: Duration,
) -> Result<String, String> {
    let base = |port| format!("http://127.0.0.1:{port}");
    if let Some(port) = serving_port(port_file) {
        return Ok(base(port));
    }
    start().map_err(|e| format!("couldn't start nebula serve: {e}"))?;
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        std::thread::sleep(START_POLL);
        if let Some(port) = serving_port(port_file) {
            return Ok(base(port));
        }
    }
    Err("nebula serve did not come up".into())
}

/// `nebula serve --no-open`, detached the way the TUI starts its daemon:
/// this binary, in a session of its own, so it outlives this TUI and holds
/// no terminal. What it prints goes to `log`.
fn start_serve(log: &Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)?;
    let mut cmd = std::process::Command::new(paths::current_exe()?);
    cmd.args(["serve", "--no-open"])
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    unsafe {
        cmd.pre_exec(crate::ipc::own_session);
    }
    let mut child = cmd.spawn()?;
    // Reaped if it ends while this TUI runs, rather than left a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener};

    /// A stand-in for `nebula serve` on a free port, answering every
    /// connection as it does, and its port.
    fn listener() -> (TcpListener, u16) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let serving = listener.try_clone().unwrap();
        std::thread::spawn(move || {
            for mut stream in serving.incoming().flatten() {
                let mut asked = Vec::new();
                let mut byte = [0];
                while !asked.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1)
                {
                    asked.push(byte[0]);
                }
                let head = format!(
                    "HTTP/1.0 200 OK\r\nServer: {}\r\n\r\n",
                    nebula_client::serve::SERVER
                );
                let _ = stream.write_all(head.as_bytes());
            }
        });
        (listener, port)
    }

    /// A port nothing listens on: one a listener had, and let go.
    fn dead_port() -> u16 {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    /// A server the port file names, and that answers, is the one used:
    /// nothing is started.
    #[test]
    fn a_live_server_is_used_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("serve.port");
        let (_listener, port) = listener();
        std::fs::write(&file, format!("{port}\n")).unwrap();
        let url = serve_url(&file, || panic!("started a second server"), START_WAIT);
        assert_eq!(url, Ok(format!("http://127.0.0.1:{port}")));
    }

    /// With no port file, or one naming a port nobody answers on, a server
    /// is started, and its URL is the port it then writes.
    #[test]
    fn with_no_server_one_is_started_and_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("serve.port");
        for stale in [None, Some(dead_port())] {
            if let Some(port) = stale {
                std::fs::write(&file, format!("{port}\n")).unwrap();
            }
            let (listener, port) = listener();
            let started = std::cell::Cell::new(false);
            let url = serve_url(
                &file,
                || {
                    started.set(true);
                    std::fs::write(&file, format!("{port}\n"))
                },
                START_WAIT,
            );
            assert!(started.get(), "stale {stale:?}: nothing was started");
            assert_eq!(url, Ok(format!("http://127.0.0.1:{port}")));
            drop(listener);
            std::fs::remove_file(&file).unwrap();
        }
    }

    /// A server that never writes its port, or cannot be started at all,
    /// is said so, not waited on for ever.
    #[test]
    fn a_server_that_never_comes_up_is_said_so() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("serve.port");
        let quiet = serve_url(&file, || Ok(()), Duration::from_millis(300));
        assert_eq!(quiet, Err("nebula serve did not come up".into()));
        let failed = serve_url(
            &file,
            || Err(std::io::Error::other("no such binary")),
            START_WAIT,
        );
        assert_eq!(
            failed,
            Err("couldn't start nebula serve: no such binary".into())
        );
    }
}
