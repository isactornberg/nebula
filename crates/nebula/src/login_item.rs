//! `nebula login-item on | off | status`: the LOGIN ITEM, a macOS
//! LaunchAgent that starts the daemon at login, so the SCHEDULES run after
//! a reboot with no TUI opened first.
//!
//! It runs the daemon of the installed nebula `on` ran from, through the
//! user's login shell so it has the PATH a terminal has, with no `NEBULA_*`
//! variable, so it is the installed instance's daemon on its default
//! runtime and data dirs. Only an installed nebula writes it: a cargo build,
//! and an instance on a data or runtime dir of its own (`make dev`,
//! dev-watch), are refused, so a dev build never claims it. It only ever
//! writes or removes the file, never `launchctl`: launchd loads it at the
//! next login, and the daemon running now is left alone, as a `bootout`
//! would stop it and every session with it.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use nebula_core::paths::LOGIN_ITEM_LABEL as LABEL;

pub(crate) enum Op {
    On,
    Off,
    Status,
}

pub(crate) fn run(op: Op) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!(
            "the login item is a macOS LaunchAgent; elsewhere, have your init system run \
             `nebula daemon` at login (a systemd user unit, say)"
        );
    }
    let file = plist_path()?;
    match op {
        Op::On => {
            let exe = installed_exe()?;
            let shell = nebula_core::shell::user_shell();
            let log = nebula_core::paths::log_dir().join("login-item.log");
            let was = named_binary(&file);
            write(&file, &plist(&exe, &shell, &log))?;
            let instead = match was {
                Some(old) if old != exe => format!(", in place of the one for {}", old.display()),
                _ => String::new(),
            };
            println!(
                "nebula's daemon starts at your next login, from {}{instead}: {}.\n\
                 The daemon running now is left as it is.",
                exe.display(),
                file.display()
            );
        }
        Op::Off => match std::fs::remove_file(&file) {
            Ok(()) => println!(
                "Removed {}: nebula's daemon no longer starts at login. The daemon running now \
                 keeps running.",
                file.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("nebula's daemon does not start at login: there is no login item.")
            }
            Err(e) => return Err(e).with_context(|| format!("remove {}", file.display())),
        },
        Op::Status => match named_binary(&file) {
            None if file.exists() => println!(
                "{} is there but is not one nebula wrote: `nebula login-item on` writes it again.",
                file.display()
            ),
            None => println!("off: nebula's daemon does not start at login."),
            Some(exe) => {
                let this = nebula_core::paths::current_exe()
                    .ok()
                    .and_then(|e| std::fs::canonicalize(e).ok());
                let whose = if !exe.exists() {
                    " That binary is gone: `nebula login-item on` from the installed nebula \
                     points it at that one."
                } else if this.as_deref() == Some(exe.as_path()) {
                    " That is this nebula."
                } else {
                    " That is another nebula than this one."
                };
                println!(
                    "on: the daemon of {} starts at login ({}).{whose}",
                    exe.display(),
                    file.display()
                );
            }
        },
    }
    Ok(())
}

fn plist_path() -> Result<PathBuf> {
    nebula_core::paths::login_item_plist().context("no home directory")
}

/// The nebula this runs from, once it is an installed one.
fn installed_exe() -> Result<PathBuf> {
    use nebula_core::env;
    let own = [env::DATA_DIR, env::RUNTIME_DIR, env::DEV_WATCH]
        .into_iter()
        .find(|var| env::non_empty(var).is_some());
    if let Some(var) = own {
        bail!(
            "this nebula runs on {var}, an instance of its own such as a dev build's: the login \
             item starts the installed nebula's daemon, so run `nebula login-item on` from a \
             shell outside nebula's sessions"
        );
    }
    let exe = std::fs::canonicalize(nebula_core::paths::current_exe()?)?;
    if is_cargo_build(&exe) {
        bail!(
            "{} is a cargo build: the login item starts an installed nebula, so `make install` \
             it and run that nebula's `login-item on`",
            exe.display()
        );
    }
    Ok(exe)
}

/// A binary in a cargo target dir.
fn is_cargo_build(exe: &Path) -> bool {
    let dirs: Vec<_> = exe.components().map(|c| c.as_os_str()).collect();
    dirs.windows(2)
        .any(|w| w[0] == "target" && ["debug", "release"].iter().any(|profile| w[1] == *profile))
}

/// The LaunchAgent that starts `exe`'s daemon at login, through `shell` as
/// a login shell, its own errors in `log`.
fn plist(exe: &Path, shell: &str, log: &Path) -> String {
    let start = format!(
        "exec {} daemon",
        nebula_core::shell::single_quote(&exe.to_string_lossy())
    );
    let string = |s: &str| format!("<string>{}</string>", escape(s));
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \t<key>Label</key>\n\t{label}\n\
         \t<key>ProgramArguments</key>\n\t<array>\n\
         \t\t{shell}\n\t\t<string>-l</string>\n\t\t<string>-c</string>\n\t\t{start}\n\
         \t</array>\n\
         \t<key>EnvironmentVariables</key>\n\t<dict>\n\
         \t\t<key>SHELL</key>\n\t\t{shell}\n\
         \t</dict>\n\
         \t<key>RunAtLoad</key>\n\t<true/>\n\
         \t<key>ProcessType</key>\n\t<string>Interactive</string>\n\
         \t<key>StandardErrorPath</key>\n\t{log}\n\
         </dict>\n\
         </plist>\n",
        label = string(LABEL),
        shell = string(shell),
        start = string(&start),
        log = string(&log.to_string_lossy()),
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The nebula whose daemon the LaunchAgent at `file` starts, when it is one
/// [`plist`] wrote.
fn named_binary(file: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(file).ok()?;
    let start = text.split("<string>exec '").nth(1)?;
    let quoted = start.split("' daemon</string>").next()?;
    let unescaped = quoted
        .replace("'\\''", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    Some(PathBuf::from(unescaped))
}

/// Write `text` to `file`, renamed into place, its folder made first.
fn write(file: &Path, text: &str) -> Result<()> {
    let dir = file.parent().context("the LaunchAgents folder")?;
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let tmp = file.with_extension(format!("plist.{}", std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, file).with_context(|| format!("write {}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `on` writes is a property list launchd reads, naming the
    /// binary, which `status` reads back, whatever its path holds.
    #[test]
    fn the_login_item_is_a_launch_agent_for_the_installed_binary() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new("/Users/me/My Tools & <bin>/it's/nebula");
        let file = dir
            .path()
            .join("LaunchAgents")
            .join("dev.nebula.daemon.plist");
        let text = plist(exe, "/bin/zsh", Path::new("/Users/me/state/login-item.log"));
        write(&file, &text).unwrap();
        assert_eq!(named_binary(&file).as_deref(), Some(exe));
        assert!(text.contains("<string>exec '/Users/me/My Tools &amp; &lt;bin&gt;/it'\\''s/nebula' daemon</string>"), "{text}");
        assert!(text.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(!text.contains("KeepAlive"), "a `nebula kill` stays a kill");
        assert!(
            !text.contains("NEBULA_"),
            "the installed instance's own dirs"
        );
        if Path::new("/usr/bin/plutil").exists() {
            let lint = std::process::Command::new("/usr/bin/plutil")
                .arg("-lint")
                .arg(&file)
                .output()
                .unwrap();
            assert!(lint.status.success(), "{lint:?}");
            let json = std::process::Command::new("/usr/bin/plutil")
                .args(["-convert", "json", "-o", "-"])
                .arg(&file)
                .output()
                .unwrap();
            let read: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
            assert_eq!(read["Label"], "dev.nebula.daemon");
            assert_eq!(
                read["ProgramArguments"],
                serde_json::json!([
                    "/bin/zsh",
                    "-l",
                    "-c",
                    "exec '/Users/me/My Tools & <bin>/it'\\''s/nebula' daemon"
                ])
            );
            assert_eq!(read["EnvironmentVariables"]["SHELL"], "/bin/zsh");
        }
        std::fs::write(&file, "<plist/>").unwrap();
        assert_eq!(named_binary(&file), None, "not one nebula wrote");
    }

    #[test]
    fn a_cargo_build_is_no_installed_nebula() {
        assert!(is_cargo_build(Path::new(
            "/code/nebula/target/debug/nebula"
        )));
        assert!(is_cargo_build(Path::new(
            "/code/nebula/target/release/nebula"
        )));
        assert!(!is_cargo_build(Path::new("/Users/me/.local/bin/nebula")));
        assert!(!is_cargo_build(Path::new("/opt/target/nebula")));
    }
}
