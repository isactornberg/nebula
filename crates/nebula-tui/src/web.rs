//! The TUI's half of the link with the web app (`nebula serve`). The TUI
//! tells the DAEMON which session it has on screen, which the web app's
//! **Follow TUI** follows, and `⇧W` opens the web app in the browser on
//! that session. The web app's **Open in terminal** comes the other way,
//! as `ServerEvent::ShowSession` (`event_loop::server_events`), and in the
//! TUI the user touched last it brings the terminal's window to the front
//! ([`raise`]).

use crate::app::App;
use crossterm::event::{Event, KeyEventKind, MouseEventKind};
use nebula_client::serve::serving_port;
use nebula_core::{paths, AgentId, ClientRequest, SessionRef, SHOW_PROTOCOL};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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

/// How this TUI's terminal window comes to the front, read off its
/// environment.
#[derive(Debug, PartialEq, Eq)]
enum Raise {
    /// WezTerm's `pane` through its CLI, once WezTerm says that pane is on
    /// this TUI's own terminal, and the app on macOS. A TUI in a nebula
    /// pane, or in tmux, inherited the variable from the terminal its
    /// daemon or server started in, and raises only the app.
    WezTerm { cli: PathBuf, pane: String },
    /// The macOS app `TERM_PROGRAM` names.
    App(&'static str),
}

/// None over ssh, where the window is on another machine, and in a
/// terminal nebula does not know.
fn how_to_raise(var: impl Fn(&str) -> Option<String>, remote: bool) -> Option<Raise> {
    let var = |name: &str| var(name).filter(|v| !v.is_empty());
    if remote {
        return None;
    }
    if let Some(pane) = var("WEZTERM_PANE") {
        // The CLI beside the app, which is on no PATH when the app came
        // from a disk image.
        let cli = var("WEZTERM_EXECUTABLE_DIR")
            .map_or_else(|| "wezterm".into(), |dir| Path::new(&dir).join("wezterm"));
        return Some(Raise::WezTerm { cli, pane });
    }
    let app = match var("TERM_PROGRAM")?.as_str() {
        "Apple_Terminal" => "Terminal",
        "iTerm.app" => "iTerm",
        "ghostty" => "Ghostty",
        _ => return None,
    };
    Some(Raise::App(app))
}

/// Runs a command: its stdout when it ran and succeeded.
type Exec<'a> = &'a mut dyn FnMut(&mut Command) -> Option<String>;

/// The web app's **Open in terminal** put a session up in this TUI, the
/// one the user touched last: its window comes to the front. Every way to
/// do that is a process to run, so it runs on a thread of its own, and
/// nothing it does can fail the show.
pub(crate) fn raise(remote: bool) {
    let Some(how) = how_to_raise(|name| std::env::var(name).ok(), remote) else {
        return;
    };
    if cfg!(test) {
        return;
    }
    std::thread::spawn(move || raise_with(&how, &mut quiet_output));
}

fn raise_with(how: &Raise, exec: Exec) {
    let app = match how {
        Raise::WezTerm { cli, pane } => {
            if wezterm_pane_is_ours(cli, pane, exec) {
                exec(Command::new(cli).args([
                    "cli",
                    "--no-auto-start",
                    "activate-pane",
                    "--pane-id",
                    pane,
                ]));
            } else {
                tracing::debug!(
                    pane,
                    "WezTerm's pane is not this TUI's: raising the app alone"
                );
            }
            "WezTerm"
        }
        Raise::App(app) => app,
    };
    if cfg!(target_os = "macos") {
        exec(Command::new("open").args(["-a", app]));
    }
}

/// Is WezTerm's `pane` on this process's terminal? `--no-auto-start`, since
/// with the socket of a WezTerm long gone the CLI would start a mux server
/// of its own.
fn wezterm_pane_is_ours(cli: &Path, pane: &str, exec: Exec) -> bool {
    #[derive(serde::Deserialize)]
    struct Pane {
        pane_id: u64,
        tty_name: Option<String>,
    }
    let pid = std::process::id().to_string();
    let Some(tty) = exec(Command::new("ps").args(["-o", "tty=", "-p", &pid])) else {
        return false;
    };
    let tty = format!("/dev/{}", tty.trim());
    exec(Command::new(cli).args(["cli", "--no-auto-start", "list", "--format", "json"]))
        .and_then(|list| serde_json::from_str::<Vec<Pane>>(&list).ok())
        .is_some_and(|panes| {
            panes
                .iter()
                .any(|p| p.pane_id.to_string() == pane && p.tty_name.as_deref() == Some(&tty))
        })
}

/// [`Exec`] for real: nothing on stdin, which is the TUI's terminal, and
/// stderr dropped, which would draw over the TUI.
fn quiet_output(cmd: &mut Command) -> Option<String> {
    let out = cmd.stdin(Stdio::null()).stderr(Stdio::null()).output();
    match out {
        Ok(out) if out.status.success() => Some(String::from_utf8_lossy(&out.stdout).into_owned()),
        Ok(out) => {
            tracing::debug!(?cmd, status = %out.status, "raising the window");
            None
        }
        Err(e) => {
            tracing::debug!(?cmd, error = %e, "raising the window");
            None
        }
    }
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

    fn env<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<String> + 'a {
        |name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        }
    }

    /// The window comes up the way the terminal the TUI runs in takes it:
    /// WezTerm by its pane, a macOS terminal by its app, and not at all
    /// over ssh or in a terminal nebula does not know.
    #[test]
    fn the_window_is_raised_the_way_its_terminal_takes() {
        let wezterm = [
            ("TERM_PROGRAM", "WezTerm"),
            ("WEZTERM_PANE", "7"),
            (
                "WEZTERM_EXECUTABLE_DIR",
                "/Applications/WezTerm.app/Contents/MacOS",
            ),
        ];
        assert_eq!(
            how_to_raise(env(&wezterm), false),
            Some(Raise::WezTerm {
                cli: "/Applications/WezTerm.app/Contents/MacOS/wezterm".into(),
                pane: "7".into()
            })
        );
        assert_eq!(
            how_to_raise(env(&[("WEZTERM_PANE", "7")]), false),
            Some(Raise::WezTerm {
                cli: "wezterm".into(),
                pane: "7".into()
            })
        );
        for (program, app) in [
            ("Apple_Terminal", "Terminal"),
            ("iTerm.app", "iTerm"),
            ("ghostty", "Ghostty"),
        ] {
            let vars = [("TERM_PROGRAM", program), ("WEZTERM_PANE", "")];
            assert_eq!(how_to_raise(env(&vars), false), Some(Raise::App(app)));
        }
        for unknown in [
            &[][..],
            &[("TERM_PROGRAM", "tmux")],
            &[("TERM_PROGRAM", "vscode")],
        ] {
            assert_eq!(how_to_raise(env(unknown), false), None, "{unknown:?}");
        }
        assert_eq!(how_to_raise(env(&wezterm), true), None, "over ssh");
        let ghostty = [("TERM_PROGRAM", "ghostty")];
        assert_eq!(how_to_raise(env(&ghostty), true), None, "over ssh");
    }

    /// The commands raising runs, each answered by `answer` instead of run.
    fn raised(how: &Raise, answer: impl Fn(&[String]) -> Option<String>) -> Vec<String> {
        let mut ran = Vec::new();
        raise_with(how, &mut |cmd: &mut Command| {
            let argv: Vec<String> = std::iter::once(cmd.get_program())
                .chain(cmd.get_args())
                .map(|s| s.to_string_lossy().into_owned())
                .collect();
            let answered = answer(&argv);
            ran.push(argv.join(" "));
            answered
        });
        ran
    }

    /// What `open -a` runs for `app`: nothing off macOS.
    fn open(app: &str) -> Vec<String> {
        cfg!(target_os = "macos")
            .then(|| format!("open -a {app}"))
            .into_iter()
            .collect()
    }

    /// WezTerm's pane comes up only when WezTerm says it is on this TUI's
    /// own terminal; a pane the TUI inherited, as one in a nebula pane
    /// does, and a WezTerm the CLI cannot reach leave only the app.
    #[test]
    fn wezterm_raises_the_pane_only_on_this_tuis_own_terminal() {
        let how = Raise::WezTerm {
            cli: "wezterm".into(),
            pane: "7".into(),
        };
        let panes = r#"[
            {"window_id": 0, "tab_id": 0, "pane_id": 3, "tty_name": "/dev/ttys004"},
            {"window_id": 1, "tab_id": 2, "pane_id": 7, "tty_name": "/dev/ttys009"}
        ]"#;
        let on = |tty: &'static str, wezterm_up: bool| {
            move |argv: &[String]| match argv[0].as_str() {
                "ps" => Some(format!("{tty}\n")),
                "wezterm" if argv[3] == "list" => wezterm_up.then(|| panes.to_string()),
                _ => Some(String::new()),
            }
        };
        let looked = [
            format!("ps -o tty= -p {}", std::process::id()),
            "wezterm cli --no-auto-start list --format json".to_string(),
        ];
        let activate = "wezterm cli --no-auto-start activate-pane --pane-id 7".to_string();
        assert_eq!(
            raised(&how, on("ttys009", true)),
            [&looked[..], &[activate], &open("WezTerm")[..]].concat()
        );
        for (tty, up) in [
            ("ttys004", true),
            ("ttys012", true),
            ("??", true),
            ("ttys009", false),
        ] {
            assert_eq!(
                raised(&how, on(tty, up)),
                [&looked[..], &open("WezTerm")[..]].concat(),
                "on {tty}, WezTerm up {up}"
            );
        }
    }

    #[test]
    fn a_macos_terminal_raises_its_app() {
        assert_eq!(
            raised(&Raise::App("Ghostty"), |_| Some(String::new())),
            open("Ghostty")
        );
    }
}
