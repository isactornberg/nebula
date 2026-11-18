//! Files handed to an agent by their path in a prompt: a drop or a `^V`
//! image in the TUI's boxes, and an image sent from the web app. They are
//! kept in the DATA DIR's `attachments/`, outside every checkout, and go
//! once they are [`KEEP`] old.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Directory under the DATA DIR the files go to.
pub const DIR: &str = "attachments";

/// How long a file outlives the prompt that named it. A prompt is sent
/// within minutes; a week covers a session resumed days later that looks
/// at it again.
pub const KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// `attachments/` in the DATA DIR.
pub fn dir() -> PathBuf {
    crate::paths::data_dir().join(DIR)
}

/// Delete the files in `dir` older than [`KEEP`]. Best effort: a file that
/// can't be read or removed is left for the next time.
pub fn prune(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .ok()
            .filter(|m| m.is_file())
            .and_then(|m| m.modified().ok())
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > KEEP);
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}
