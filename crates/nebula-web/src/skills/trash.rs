//! The user's Trash: a file or folder moved there, a link as the link, and
//! where it landed, so it can be moved back. On macOS it is the Trash Finder
//! shows, through `NSFileManager`; elsewhere it is the freedesktop.org Trash
//! in the user's data dir, its `.trashinfo` beside it.

use std::ffi::CString;
use std::fs;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// Move `path` to the Trash: `dir` laid out as the freedesktop.org spec has
/// it, or the system's for `None`. Says where in the Trash it is.
pub(super) fn trash(path: &Path, dir: Option<&Path>) -> io::Result<PathBuf> {
    match dir {
        Some(dir) => freedesktop(dir, path),
        None => system(path),
    }
}

#[cfg(target_os = "macos")]
fn system(path: &Path) -> io::Result<PathBuf> {
    use objc2_foundation::{NSFileManager, NSURL};
    let url =
        NSURL::from_file_path(path).ok_or_else(|| io::Error::other("macOS takes no such path"))?;
    let mut landed = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut landed))
        .map_err(|e| io::Error::other(e.localizedDescription().to_string()))?;
    landed
        .and_then(|url| url.to_file_path())
        .ok_or_else(|| io::Error::other("macOS did not say where in the Trash it went"))
}

#[cfg(not(target_os = "macos"))]
fn system(_: &Path) -> io::Result<PathBuf> {
    Err(io::Error::other("there is no Trash here"))
}

/// The freedesktop.org Trash at `dir`: `path` renamed into `files/`, under
/// its own name or the first `name.N` free, after its `info/name.trashinfo`
/// is made, which holds the name for it.
fn freedesktop(dir: &Path, path: &Path) -> io::Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let (files, info) = (dir.join("files"), dir.join("info"));
    for d in [&files, &info] {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(d)?;
    }
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("it has no name"))?
        .to_string_lossy()
        .into_owned();
    let note = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        encoded(path),
        now()
    );
    for n in 1.. {
        let base = if n == 1 {
            name.clone()
        } else {
            format!("{name}.{n}")
        };
        let info = info.join(format!("{base}.trashinfo"));
        let mut out = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&info)
        {
            Ok(out) => out,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        let to = files.join(&base);
        let moved = out
            .write_all(note.as_bytes())
            .and_then(|()| rename_new(path, &to));
        match moved {
            Ok(()) => return Ok(to),
            Err(e) => {
                let _ = fs::remove_file(&info);
                // A file left in `files/` with no info of its own.
                if e.kind() != io::ErrorKind::AlreadyExists {
                    return Err(e);
                }
            }
        }
    }
    unreachable!("a name is always free")
}

/// Move `trashed` back to `to`, only while nothing is there, and take its
/// `.trashinfo` with it when it has one.
pub(super) fn put_back(trashed: &Path, to: &Path) -> io::Result<()> {
    rename_new(trashed, to)?;
    if let (Some(files), Some(name)) = (trashed.parent(), trashed.file_name()) {
        if files.file_name().is_some_and(|f| f == "files") {
            let mut info = name.to_os_string();
            info.push(".trashinfo");
            let _ = fs::remove_file(files.with_file_name("info").join(info));
        }
    }
    Ok(())
}

/// `rename(from, to)` that fails with `AlreadyExists` when something is at
/// `to`, rather than replacing it.
fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
    let c = |p: &Path| CString::new(p.as_os_str().as_bytes()).map_err(io::Error::other);
    let (from, to) = (c(from)?, c(to)?);
    // SAFETY: both are NUL-terminated paths that live through the call.
    #[cfg(target_os = "macos")]
    let done = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    let done = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if done == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// `path` as a `.trashinfo` `Path=` holds it: absolute, its bytes
/// percent-encoded but for the unreserved ones and `/`.
fn encoded(path: &Path) -> String {
    let mut out = String::new();
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Now in local time, as `YYYY-MM-DDThh:mm:ss`.
fn now() -> String {
    // SAFETY: localtime_r fills `tm` from `t`, both owned here.
    let tm = unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}
