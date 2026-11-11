//! A connection to the DAEMON: the socket, the version handshake, and the
//! task that reads its events.

use anyhow::{bail, Result};
use nebula_core::codec::{read_frame, write_frame};
use nebula_core::{ClientRequest, ServerEvent, PROTOCOL_VERSION};
use tokio::net::UnixStream;

/// The daemon hung up mid-request - the message every one-shot client shows.
pub const CLOSED_BEFORE_REPLY: &str = "daemon closed the connection before replying";

pub struct Connection {
    pub stream: UnixStream,
    pub daemon_pid: u32,
    pub protocol_version: u32,
}

pub async fn try_connect(sock: &std::path::Path) -> Result<UnixStream> {
    Ok(UnixStream::connect(sock).await?)
}

pub async fn handshake(mut stream: UnixStream) -> Result<Connection> {
    // Asked of the kernel before the daemon gets a word in: a daemon on
    // another protocol hangs up right after its answer.
    let listener_pid = peer_pid(&stream);
    write_frame(
        &mut stream,
        &ClientRequest::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .await?;
    match read_frame::<ServerEvent, _>(&mut stream).await? {
        Some(ServerEvent::HelloOk {
            protocol_version,
            daemon_pid,
        }) => {
            if nebula_core::protocol_compatible_with(protocol_version) {
                Ok(Connection {
                    stream,
                    daemon_pid,
                    protocol_version,
                })
            } else {
                bail!(version_skew_message(protocol_version, listener_pid))
            }
        }
        Some(ServerEvent::Incompatible {
            daemon_protocol_version,
        }) => bail!(version_skew_message(daemon_protocol_version, listener_pid)),
        other => bail!("unexpected handshake reply: {other:?}"),
    }
}

/// The pid of the process listening on the far end of `stream`, from the
/// kernel's peer credentials (`SO_PEERCRED`, `LOCAL_PEEREPID`) - which need
/// neither a protocol the daemon speaks nor a pidfile that is still there.
pub fn peer_pid(stream: &UnixStream) -> Option<i32> {
    let pid = stream.peer_cred().ok()?.pid()?;
    (pid > 1 && pid.unsigned_abs() != std::process::id()).then_some(pid)
}

/// Explain a failed version handshake in terms of the fix.
///
/// Which side is stale decides the remedy, and getting it backwards costs a
/// debugging session: when the *daemon* is ahead, the `nebula` that just ran
/// is an older build than the one the daemon was launched from, and `nebula
/// kill` cannot help - the live instance immediately respawns its daemon
/// from its own binary (the TUI's `spawn_daemon` uses `current_exe`), so the
/// skew survives every restart. That is the common shape in a checkout,
/// where `make dev` runs `target/debug` while PATH still finds an older
/// `nebula` from the last `make install`.
///
/// `daemon_pid` comes off the socket, so the message can name the process
/// even when its pidfile is gone - and offer the plain SIGTERM that stops it
/// cleanly if `nebula kill` somehow can't, before anyone reaches for `-9`.
fn version_skew_message(daemon_protocol_version: u32, daemon_pid: Option<i32>) -> String {
    let client = nebula_core::paths::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "unknown".into());
    let daemon = match daemon_pid {
        Some(pid) => format!(
            "{} (pid {pid})",
            daemon_exe_path(pid).unwrap_or_else(|| "unknown".into())
        ),
        None => "unknown".into(),
    };
    let header = format!(
        "protocol mismatch: the daemon speaks v{daemon_protocol_version}, this client \
         v{PROTOCOL_VERSION}\n  this client: {client}\n  the daemon:  {daemon}\n"
    );
    if daemon_protocol_version > PROTOCOL_VERSION {
        format!(
            "{header}This client is the older build, so `nebula kill` will not fix it — the \
             running instance respawns its daemon from its own binary. Install the daemon's \
             build over this one instead (`make install` from that checkout)."
        )
    } else {
        let by_hand = daemon_pid
            .map(|pid| {
                format!(
                    " Should that fail, `kill {pid}` asks the daemon for the same clean \
                     shutdown — never `kill -9`, which skips flushing its database."
                )
            })
            .unwrap_or_default();
        format!(
            "{header}The daemon is the older build — run `nebula kill` and relaunch. That \
             stops every live session.{by_hand}"
        )
    }
}

/// Best-effort path of the binary the daemon at `pid` was launched from, so
/// the mismatch message can name it. Asked of the OS rather than the
/// handshake: `Incompatible` is what a *newer* daemon sends an older client,
/// so adding a field to it would only break decoding on the clients that
/// need this message most. The buildstamp beside the pidfile is no help
/// either - it is a content hash, not a path.
fn daemon_exe_path(pid: i32) -> Option<String> {
    if let Ok(path) = std::fs::read_link(format!("/proc/{pid}/exe")) {
        return Some(path.display().to_string());
    }
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

/// Read the daemon's events off the socket on a task of their own and hand
/// them over a channel, so a reader that gives up waiting (a timeout, a
/// `select!`) never drops half a frame the way a cancelled `read_frame`
/// would.
pub fn spawn_reader(
    read_half: tokio::net::unix::OwnedReadHalf,
) -> tokio::sync::mpsc::Receiver<ServerEvent> {
    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<ServerEvent>(1024);
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(read_half);
        while let Ok(Some(ev)) = read_frame::<ServerEvent, _>(&mut reader).await {
            if event_tx.send(ev).await.is_err() {
                break;
            }
        }
        // Dropping event_tx closes the channel, signalling disconnect.
    });
    event_rx
}

#[cfg(test)]
mod tests {
    use super::*;

    // The whole point of the message: `nebula kill` is the fix for exactly
    // one of the two skews, and recommending it for the other sends the user
    // in a circle (kill the daemon, the live TUI respawns the same one).
    #[test]
    fn skew_message_blames_the_older_side() {
        let daemon_ahead = version_skew_message(PROTOCOL_VERSION + 2, None);
        assert!(daemon_ahead.contains("This client is the older build"));
        assert!(
            !daemon_ahead.contains("run `nebula kill` and relaunch"),
            "must not send the user to kill a daemon that is not the stale side: {daemon_ahead}"
        );
        assert!(daemon_ahead.contains("make install"));

        let daemon_behind = version_skew_message(PROTOCOL_VERSION - 1, None);
        assert!(daemon_behind.contains("The daemon is the older build"));
        assert!(daemon_behind.contains("run `nebula kill` and relaunch"));
    }

    // #68: with the daemon's pid known, the stale-daemon message names it
    // and hands over the manual fallback - SIGTERM, with -9 ruled out.
    #[test]
    fn skew_message_offers_a_clean_manual_kill_by_pid() {
        let msg = version_skew_message(PROTOCOL_VERSION - 1, Some(4242));
        assert!(msg.contains("(pid 4242)"), "{msg}");
        assert!(msg.contains("`kill 4242`"), "{msg}");
        assert!(msg.contains("never `kill -9`"), "{msg}");

        let unknown = version_skew_message(PROTOCOL_VERSION - 1, None);
        assert!(unknown.contains("the daemon:  unknown"), "{unknown}");
        assert!(!unknown.contains("`kill "), "no pid, no command: {unknown}");
    }

    #[test]
    fn skew_message_names_both_binaries() {
        let msg = version_skew_message(PROTOCOL_VERSION + 1, None);
        assert!(msg.contains("this client: "), "{msg}");
        assert!(msg.contains("the daemon:  "), "{msg}");
        // current_exe resolves under a test binary, so this half is never
        // the "unknown" fallback.
        assert!(msg.contains(&format!("v{PROTOCOL_VERSION}")), "{msg}");
    }
}
