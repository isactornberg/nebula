//! The DEV WATCH (`make dev-watch`): a dev TUI that relaunches itself onto
//! every new build, in the same window and on the same alternate screen.
//!
//! `scripts/dev-watch.sh` rebuilds on every saved source file, moves the dev
//! daemon onto the new binary with `nebula reload` when the change can reach
//! it, and then writes `ready` to the status file named by
//! [`env::DEV_WATCH`]. The TUI polls that file; `ready` ends the loop as a
//! quit would (the selection is saved for the next launch to restore) and
//! execs the binary over this process instead of exiting. `building` and
//! `failed` only flash, so a broken build leaves the running TUI alone, and
//! `idle` (a build that relinked nothing) takes the `building` flash down.

use nebula_core::env;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// How often the status file is looked at: the delay between a build
/// landing and the relaunch.
pub(super) const POLL: Duration = Duration::from_millis(250);

/// The flash a `building` report puts up, for `idle` to take down.
pub(super) const BUILDING_FLASH: &str = "dev watch: rebuilding…";

/// The status file and the version of it last acted on.
pub(super) struct DevWatch {
    path: PathBuf,
    seen: Option<SystemTime>,
}

/// What the watcher last reported.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Status {
    Building,
    /// Built, but the binary is the one already running.
    Idle,
    Ready,
    /// The build or the daemon reload failed; the watcher's one-line why.
    Failed(String),
}

impl DevWatch {
    /// The watch [`env::DEV_WATCH`] asks for. Whatever the file says when
    /// the TUI starts is old news: it is the build this TUI already is.
    pub(super) fn from_env() -> Option<Self> {
        let path = PathBuf::from(env::non_empty(env::DEV_WATCH)?);
        let seen = modified(&path);
        Some(Self { path, seen })
    }

    /// The watcher's new report, if it wrote one since the last poll.
    pub(super) fn poll(&mut self) -> Option<Status> {
        let now = modified(&self.path);
        if now == self.seen {
            return None;
        }
        self.seen = now;
        parse(&std::fs::read_to_string(&self.path).ok()?)
    }
}

fn modified(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn parse(text: &str) -> Option<Status> {
    let (word, rest) = text
        .trim()
        .split_once(char::is_whitespace)
        .unwrap_or((text.trim(), ""));
    match word {
        "building" => Some(Status::Building),
        "idle" => Some(Status::Idle),
        "ready" => Some(Status::Ready),
        "failed" => Some(Status::Failed(rest.trim().to_string())),
        _ => None,
    }
}

/// Replace this process with the freshly built binary, same arguments and
/// environment. Only returns on failure, with the terminal already handed
/// back the way `restore_terminal` would.
pub(super) fn relaunch() -> anyhow::Error {
    use std::os::unix::process::CommandExt;
    super::host_terminal::release_for_relaunch();
    let err = match std::env::current_exe() {
        Ok(exe) => anyhow::Error::new(
            std::process::Command::new(&exe)
                .args(std::env::args_os().skip(1))
                .env(env::DEV_RELAUNCHED, "1")
                .exec(),
        )
        .context(format!("relaunch {}", exe.display())),
        Err(e) => anyhow::Error::new(e).context("relaunch"),
    };
    super::host_terminal::restore_terminal();
    err
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_file_reads_as_the_watcher_wrote_it() {
        assert_eq!(parse("building\n"), Some(Status::Building));
        assert_eq!(parse("ready"), Some(Status::Ready));
        assert_eq!(parse("idle\n"), Some(Status::Idle));
        assert_eq!(
            parse("failed error[E0308]: mismatched types\n"),
            Some(Status::Failed("error[E0308]: mismatched types".into()))
        );
        assert_eq!(parse("failed"), Some(Status::Failed(String::new())));
        assert_eq!(parse(""), None);
        assert_eq!(parse("garbage"), None);
    }

    #[test]
    fn only_a_rewrite_since_the_last_poll_is_news() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status");
        std::fs::write(&path, "ready\n").unwrap();
        let mut watch = DevWatch {
            seen: modified(&path),
            path: path.clone(),
        };
        assert_eq!(watch.poll(), None, "the build this TUI already is");
        // Past the filesystem's timestamp granularity.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&path, "building\n").unwrap();
        assert_eq!(watch.poll(), Some(Status::Building));
        assert_eq!(watch.poll(), None, "read once");
    }
}
