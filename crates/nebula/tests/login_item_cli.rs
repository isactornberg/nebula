//! `nebula login-item` against a temporary HOME, never the real one: it
//! writes, reads and removes the LaunchAgent there, launchd reads what it
//! wrote, and a dev build or an instance of its own is refused. Nothing
//! here runs `launchctl`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The binary under test as an installed nebula: a hard link outside the
/// cargo target dir.
fn installed(home: &Path) -> PathBuf {
    let bin = home.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let nebula = bin.join("nebula");
    std::fs::hard_link(env!("CARGO_BIN_EXE_nebula"), &nebula)
        .or_else(|_| std::fs::copy(env!("CARGO_BIN_EXE_nebula"), &nebula).map(drop))
        .unwrap();
    nebula
}

fn run(nebula: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(nebula)
        .args(args)
        .env("HOME", home)
        .env("SHELL", "/bin/zsh")
        .env_remove("NEBULA_DATA_DIR")
        .env_remove("NEBULA_RUNTIME_DIR")
        .env_remove("NEBULA_DEV_WATCH")
        .env_remove("NEBULA_CONFIG_FILE")
        .output()
        .unwrap()
}

fn said(out: &Output) -> String {
    assert!(out.status.success(), "{out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
#[cfg(target_os = "macos")]
fn the_login_item_starts_the_installed_daemon_at_login() {
    let tmp = tempfile::tempdir().unwrap();
    let home = std::fs::canonicalize(tmp.path()).unwrap();
    let nebula = installed(&home);
    let plist = home.join("Library/LaunchAgents/dev.nebula.daemon.plist");

    assert!(said(&run(&nebula, &home, &["login-item", "status"])).starts_with("off:"));
    let on = said(&run(&nebula, &home, &["login-item", "on"]));
    assert!(on.contains("starts at your next login"), "{on}");
    assert!(
        on.contains("The daemon running now is left as it is."),
        "{on}"
    );
    let lint = Command::new("/usr/bin/plutil")
        .arg("-lint")
        .arg(&plist)
        .output()
        .unwrap();
    assert!(lint.status.success(), "{lint:?}");
    let json = Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(&plist)
        .output()
        .unwrap();
    let read: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(read["Label"], "dev.nebula.daemon");
    assert_eq!(read["RunAtLoad"], true);
    assert_eq!(
        read["ProgramArguments"],
        serde_json::json!([
            "/bin/zsh",
            "-l",
            "-c",
            format!("exec '{}' daemon", nebula.display())
        ])
    );
    assert_eq!(
        read["EnvironmentVariables"],
        serde_json::json!({ "SHELL": "/bin/zsh" })
    );
    let log = read["StandardErrorPath"].as_str().unwrap();
    assert!(
        log.starts_with(home.to_str().unwrap()) && log.ends_with("login-item.log"),
        "{log}"
    );
    let status = said(&run(&nebula, &home, &["login-item", "status"]));
    assert!(
        status.starts_with(&format!(
            "on: the daemon of {} starts at login",
            nebula.display()
        )),
        "{status}"
    );
    assert!(status.contains("That is this nebula."), "{status}");

    // Another install's `on` takes the item over, and says whose it was.
    let other = home.join("elsewhere/nebula");
    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    std::fs::hard_link(&nebula, &other).unwrap();
    let over = said(&run(&other, &home, &["login-item", "on"]));
    assert!(
        over.contains(&format!("in place of the one for {}", nebula.display())),
        "{over}"
    );
    let status = said(&run(&nebula, &home, &["login-item", "status"]));
    assert!(
        status.contains("That is another nebula than this one."),
        "{status}"
    );

    let off = said(&run(&nebula, &home, &["login-item", "off"]));
    assert!(off.contains("no longer starts at login"), "{off}");
    assert!(!plist.exists());
    assert!(said(&run(&nebula, &home, &["login-item", "off"])).contains("there is no login item"));

    // A dev build, and an instance on dirs of its own, never claim it.
    let dev = Command::new(env!("CARGO_BIN_EXE_nebula"))
        .args(["login-item", "on"])
        .env("HOME", &home)
        .env_remove("NEBULA_DATA_DIR")
        .env_remove("NEBULA_RUNTIME_DIR")
        .env_remove("NEBULA_DEV_WATCH")
        .output()
        .unwrap();
    assert!(!dev.status.success());
    assert!(
        String::from_utf8_lossy(&dev.stderr).contains("is a cargo build"),
        "{dev:?}"
    );
    let own = Command::new(&nebula)
        .args(["login-item", "on"])
        .env("HOME", &home)
        .env("NEBULA_DATA_DIR", home.join("dev-data"))
        .output()
        .unwrap();
    assert!(!own.status.success());
    assert!(
        String::from_utf8_lossy(&own.stderr).contains("this nebula runs on NEBULA_DATA_DIR"),
        "{own:?}"
    );
    assert!(!plist.exists(), "neither wrote one");
}
