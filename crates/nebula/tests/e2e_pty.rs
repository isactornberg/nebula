//! End-to-end daemon tests over the real IPC surface: entity CRUD, PTY
//! attach/detach with scrollback replay, git worktree ops, and persistence
//! across a daemon restart.

use nebula_core::codec::{read_frame, write_frame};
use nebula_core::env;
use nebula_core::{
    AgentKind, ClientRequest, Entity, EntityId, ServerEvent, SessionRef, PROTOCOL_VERSION,
};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::net::UnixStream;

/// How long a daemon reply or broadcast may take to arrive.
const EVENT_TIMEOUT: Duration = Duration::from_secs(5);
/// Same, for events that wait on a PTY child (spawn, exit, hook round-trip).
const SLOW_TIMEOUT: Duration = Duration::from_secs(10);
/// Same, for chains of several respawns.
const SPAWN_CHAIN_TIMEOUT: Duration = Duration::from_secs(20);
/// Sleep between polls of the filesystem or a counter.
const POLL_STEP: Duration = Duration::from_millis(50);

struct TestEnv {
    tmp: tempfile::TempDir,
    runtime_dir: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let runtime_dir = tmp.path().join("rt");
        Self { tmp, runtime_dir }
    }

    fn sock(&self) -> PathBuf {
        self.runtime_dir.join("daemon.sock")
    }

    /// The `nebula` binary under test, pointed at this env's runtime and
    /// data dirs — the base every daemon spawn and one-shot CLI run shares.
    fn cli(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_nebula"));
        cmd.env(env::RUNTIME_DIR, &self.runtime_dir)
            .env(env::DATA_DIR, self.tmp.path().join("data"))
            // A developer's own settings file, exported in their shell,
            // would otherwise be the one every test reads and launches by.
            .env_remove(env::CONFIG_FILE);
        cmd
    }

    fn spawn_daemon(&self) -> DaemonProc {
        self.spawn_daemon_with_agent_cmd("/bin/sh") // no real claude in tests
    }

    fn spawn_daemon_with_agent_cmd(&self, agent_cmd: &str) -> DaemonProc {
        self.spawn_daemon_with(agent_cmd, &[])
    }

    fn spawn_daemon_with(&self, agent_cmd: &str, envs: &[(&str, &str)]) -> DaemonProc {
        self.spawn_daemon_in(Path::new("/bin/sh"), Some(agent_cmd), envs)
    }

    /// Daemon with no `NEBULA_AGENT_CMD` override, so agent spawns take the
    /// real login-shell path, and `$SHELL` set to `shell`. Lets a test decide
    /// what the daemon can find on PATH.
    fn spawn_daemon_with_shell(&self, shell: &Path) -> DaemonProc {
        self.spawn_daemon_in(shell, None, &[])
    }

    fn spawn_daemon_in(
        &self,
        shell: &Path,
        agent_cmd: Option<&str>,
        envs: &[(&str, &str)],
    ) -> DaemonProc {
        let mut cmd = self.cli();
        cmd.args(["daemon", "--foreground"])
            .env("SHELL", shell)
            .env(env::WORKTREE_SYNC_MS, "100") // fast external-worktree pickup
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        match agent_cmd {
            Some(agent_cmd) => cmd.env(env::AGENT_CMD, agent_cmd),
            None => cmd.env_remove(env::AGENT_CMD),
        };
        for (k, v) in envs {
            cmd.env(k, v);
        }
        DaemonProc(cmd.spawn().unwrap())
    }

    /// A `$SHELL` that answers `-l -i -c` but sees no agent CLI on PATH.
    fn blind_shell(&self) -> PathBuf {
        let path = self.tmp.path().join("blind-shell.sh");
        std::fs::write(
            &path,
            "#!/bin/sh\nPATH=/usr/bin:/bin\nexport PATH\nexec /bin/sh -c \"$4\"\n",
        )
        .unwrap();
        make_executable(&path);
        path
    }

    /// Write the daemon's `config.json` (read from `NEBULA_DATA_DIR`)
    /// before boot.
    fn write_config(&self, json: &str) {
        let data = self.tmp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("config.json"), json).unwrap();
    }

    /// A committed git repo to act as the project.
    fn make_repo(&self) -> PathBuf {
        let repo = self.tmp.path().join("repo");
        make_repo_at(&repo);
        repo
    }
}

/// A daemon spawned for one test, killed when the test's scope ends.
///
/// Without this, a test that panics before its closing `Shutdown` — a failed
/// assertion, a timeout — leaks its `nebula daemon --foreground`. Nothing
/// reaps it: it detaches from the test binary and outlives the whole `cargo
/// test` run. They pile up across days of development, and dozens of them
/// holding watchers and fds starve *later* runs' daemons, which surfaces as
/// every test in the file failing with "daemon socket never appeared" —
/// an error that points nowhere near the actual cause.
struct DaemonProc(std::process::Child);

impl std::ops::Deref for DaemonProc {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for DaemonProc {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for DaemonProc {
    fn drop(&mut self) {
        // Already exited (the clean path: `Shutdown` + `wait_for_exit`).
        // Checking matters — `Child::kill` on a reaped child is an error
        // rather than a signal to whatever now owns that recycled pid, but
        // there is nothing to do here either way.
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        // SIGTERM, not SIGKILL: the daemon's handler runs the same clean
        // shutdown as `Shutdown`, taking its PTY children down with it.
        // SIGKILL would leave those orphaned instead.
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &self.0.id().to_string()])
            .stderr(std::process::Stdio::null())
            .status();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if matches!(self.0.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn connect(sock: &Path) -> UnixStream {
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        match UnixStream::connect(sock).await {
            Ok(s) => return s,
            Err(_) if tokio::time::Instant::now() < deadline => tokio::time::sleep(POLL_STEP).await,
            Err(e) => panic!("daemon socket never appeared: {e}"),
        }
    }
}

async fn handshake(stream: &mut UnixStream) {
    write_frame(
        stream,
        &ClientRequest::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .await
    .unwrap();
    match read_frame::<ServerEvent, _>(stream).await.unwrap() {
        Some(ServerEvent::HelloOk { .. }) => {}
        other => panic!("bad handshake reply: {other:?}"),
    }
}

/// Collect events until `pred` says done (returns all seen events).
async fn read_events_until(
    stream: &mut UnixStream,
    timeout: Duration,
    mut pred: impl FnMut(&[ServerEvent]) -> bool,
) -> Vec<ServerEvent> {
    let mut seen = Vec::new();
    let ok = tokio::time::timeout(timeout, async {
        loop {
            match read_frame::<ServerEvent, _>(stream).await.unwrap() {
                Some(ev) => {
                    seen.push(ev);
                    if pred(&seen) {
                        return;
                    }
                }
                None => panic!("daemon closed connection early"),
            }
        }
    })
    .await;
    assert!(ok.is_ok(), "timed out waiting for events; saw: {seen:#?}");
    seen
}

/// Everything the daemon sends within `window` — for holding still and
/// then asserting on what did (or did not) happen, where
/// `read_events_until` would treat the quiet as a failure.
async fn read_events_for(stream: &mut UnixStream, window: Duration) -> Vec<ServerEvent> {
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + window;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            return seen;
        }
        match tokio::time::timeout(left, read_frame::<ServerEvent, _>(stream)).await {
            Ok(Ok(Some(ev))) => seen.push(ev),
            Ok(Ok(None)) => panic!("daemon closed connection early"),
            Ok(Err(e)) => panic!("read failed: {e}"),
            Err(_) => return seen,
        }
    }
}

fn find_ack(events: &[ServerEvent], want_req: u64) -> Option<&ServerEvent> {
    events.iter().find(|e| {
        matches!(e, ServerEvent::Ack { req_id, .. } if *req_id == want_req)
            || matches!(e, ServerEvent::Error { req_id: Some(r), .. } if *r == want_req)
    })
}

fn find_tail(events: &[ServerEvent], want_req: u64) -> Option<&ServerEvent> {
    events
        .iter()
        .find(|e| matches!(e, ServerEvent::OutputTail { req_id, .. } if *req_id == want_req))
}

fn collected_output(events: &[ServerEvent]) -> Vec<u8> {
    let mut out = Vec::new();
    for e in events {
        match e {
            ServerEvent::Scrollback { data, .. } | ServerEvent::Output { data, .. } => {
                out.extend_from_slice(data)
            }
            _ => {}
        }
    }
    out
}

#[tokio::test]
async fn full_crud_attach_and_restart_persistence() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let events = subscribe(&mut c).await;
    match &events[0] {
        ServerEvent::Snapshot { projects, .. } => assert!(projects.is_empty()),
        other => panic!("expected snapshot first, got {other:?}"),
    }

    // ---- AddProject: creates project + main worktree row ----
    write_frame(
        &mut c,
        &ClientRequest::AddProject {
            req_id: 1,
            path: repo.clone(),
            name: None,
            create_missing: false,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        find_ack(evs, 1).is_some()
            && evs.iter().any(|e| {
                matches!(
                    e,
                    ServerEvent::EntityUpserted {
                        entity: Entity::Worktree(_)
                    }
                )
            })
    })
    .await;
    let ServerEvent::Ack {
        created: Some(EntityId::Project(project_id)),
        ..
    } = find_ack(&events, 1).unwrap()
    else {
        panic!("AddProject failed: {events:#?}");
    };
    let project_id = project_id.clone();
    let main_worktree = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Worktree(w),
            } if w.is_main => Some(w.clone()),
            _ => None,
        })
        .expect("main worktree upsert");
    assert_eq!(main_worktree.branch, "main");

    // ---- CreateTerminal + attach + echo through the PTY ----
    write_frame(
        &mut c,
        &ClientRequest::CreateTerminal {
            req_id: 2,
            worktree: main_worktree.id.clone(),
            name: None,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Terminal(term_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateTerminal failed: {events:#?}");
    };
    let term_id = term_id.clone();

    let sref = SessionRef::Terminal(term_id);
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: sref.clone(),
            from_seq: None,
            cols: 80,
            rows: 24,
        },
    )
    .await
    .unwrap();
    let marker = "nebula_e2e_marker_4519";
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: format!("pwd; echo {marker}\n").into_bytes(),
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        let text = String::from_utf8_lossy(&collected_output(evs)).into_owned();
        // The typed line is echoed, and echoed again when the shell's line
        // editor redraws what was typed before its prompt. Only a marker at
        // the start of a line is the command's own, printed after `pwd`'s.
        text.contains(&format!("\n{marker}"))
    })
    .await;
    // The shell runs in the worktree directory.
    let text = String::from_utf8_lossy(&collected_output(&events)).into_owned();
    assert!(
        text.contains("repo"),
        "terminal cwd should be the worktree: {text}"
    );

    // ---- TailOutput: the end of the ring, for the grid's terminal card ----
    write_frame(
        &mut c,
        &ClientRequest::TailOutput {
            req_id: 900,
            session: sref.clone(),
            max_bytes: 4096,
            after_seq: None,
        },
    )
    .await
    .unwrap();
    let events =
        read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_tail(evs, 900).is_some()).await;
    let Some(ServerEvent::OutputTail {
        tail: Some(tail), ..
    }) = find_tail(&events, 900)
    else {
        panic!("TailOutput answered with no tail: {events:#?}");
    };
    let text = String::from_utf8_lossy(&tail.data).into_owned();
    assert!(text.contains(marker), "the tail holds the marker: {text}");
    assert_eq!((tail.cols, tail.rows), (80, 24), "at the attached size");
    assert!(tail.end_seq as usize >= tail.data.len());
    // Asked again from where that left off, a ring that has not grown
    // answers with no bytes.
    write_frame(
        &mut c,
        &ClientRequest::TailOutput {
            req_id: 901,
            session: sref.clone(),
            max_bytes: 4096,
            after_seq: Some(tail.end_seq),
        },
    )
    .await
    .unwrap();
    let events =
        read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_tail(evs, 901).is_some()).await;
    let Some(ServerEvent::OutputTail {
        tail: Some(again), ..
    }) = find_tail(&events, 901)
    else {
        panic!("TailOutput answered with no tail: {events:#?}");
    };
    if again.end_seq == tail.end_seq {
        assert!(again.data.is_empty(), "nothing new, no bytes: {again:?}");
    }
    // A session with no PTY has no tail at all.
    write_frame(
        &mut c,
        &ClientRequest::TailOutput {
            req_id: 902,
            session: SessionRef::Terminal(nebula_core::TerminalId("no-such".into())),
            max_bytes: 4096,
            after_seq: None,
        },
    )
    .await
    .unwrap();
    let events =
        read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_tail(evs, 902).is_some()).await;
    assert!(
        matches!(
            find_tail(&events, 902),
            Some(ServerEvent::OutputTail { tail: None, .. })
        ),
        "{events:#?}"
    );

    // ---- CreateAgent (NEBULA_AGENT_CMD=/bin/sh stands in for claude) ----
    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 3,
            worktree: main_worktree.id.clone(),
            name: "agent-1".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 3).is_some()).await;
    assert!(
        matches!(
            find_ack(&events, 3),
            Some(ServerEvent::Ack {
                created: Some(EntityId::Agent(_)),
                ..
            })
        ),
        "CreateAgent failed: {events:#?}"
    );

    // ---- CreateWorktree: real `git worktree add` on disk ----
    write_frame(
        &mut c,
        &ClientRequest::CreateWorktree {
            req_id: 4,
            project: project_id.clone(),
            branch: "feature-x".into(),
            base: None,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        find_ack(evs, 4).is_some()
            && evs.iter().any(|e| {
                matches!(e, ServerEvent::EntityUpserted { entity: Entity::Worktree(w) } if w.branch == "feature-x")
            })
    })
    .await;
    let ServerEvent::Ack {
        created: Some(EntityId::Worktree(feature_wt_id)),
        ..
    } = find_ack(&events, 4).unwrap()
    else {
        panic!("CreateWorktree failed: {events:#?}");
    };
    let feature_wt_id = feature_wt_id.clone();
    let feature_wt_path = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Worktree(w),
            } if w.id == feature_wt_id => Some(w.path.clone()),
            _ => None,
        })
        .expect("worktree upsert carries its path");
    assert!(feature_wt_path.exists(), "worktree dir created on disk");

    // ---- CheckWorktree: the DELETE CHECK sees an uncommitted file ----
    std::fs::write(feature_wt_path.join("draft.txt"), "half done").unwrap();
    write_frame(
        &mut c,
        &ClientRequest::CheckWorktree {
            req_id: 50,
            id: feature_wt_id.clone(),
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::WorktreeChecked { req_id: 50, .. }))
    })
    .await;
    let check = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::WorktreeChecked {
                req_id: 50, check, ..
            } => Some(check.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("CheckWorktree unanswered: {events:#?}"));
    assert_eq!(check.changes, 1, "{check:?}");
    assert!(
        check.process_error.is_none() && check.git_error.is_none(),
        "{check:?}"
    );
    assert!(feature_wt_path.exists(), "a check touches nothing");

    // ---- DeleteWorktree removes it from disk ----
    write_frame(
        &mut c,
        &ClientRequest::DeleteWorktree {
            req_id: 5,
            id: feature_wt_id.clone(),
            force: true,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| find_ack(evs, 5).is_some()).await;
    assert!(
        matches!(find_ack(&events, 5), Some(ServerEvent::Ack { .. })),
        "DeleteWorktree failed: {events:#?}"
    );
    assert!(!feature_wt_path.exists(), "worktree dir removed from disk");

    // ---- restart: tree persists, boot sweep marks nothing (agents fresh) ----
    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);

    let mut daemon2 = env.spawn_daemon();
    let mut c2 = connect(&env.sock()).await;
    handshake(&mut c2).await;
    write_frame(&mut c2, &ClientRequest::Subscribe)
        .await
        .unwrap();
    let events = read_events_until(&mut c2, EVENT_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::Snapshot { .. }))
    })
    .await;
    let ServerEvent::Snapshot {
        projects,
        worktrees,
        agents,
        terminals,
        ..
    } = &events[0]
    else {
        panic!("expected snapshot");
    };
    assert_eq!(projects.len(), 1, "project persisted");
    assert_eq!(worktrees.len(), 1, "only main worktree remains");
    assert_eq!(agents.len(), 1, "agent persisted");
    assert_eq!(agents[0].name, "agent-1");
    assert_eq!(terminals.len(), 1, "terminal persisted");
    assert!(!agents[0].alive, "no PTY after restart until reattach");

    // Reattach the persisted terminal: lazy respawn, cwd still the worktree.
    let sref2 = SessionRef::Terminal(terminals[0].id.clone());
    write_frame(
        &mut c2,
        &ClientRequest::Attach {
            session: sref2.clone(),
            from_seq: None,
            cols: 80,
            rows: 24,
        },
    )
    .await
    .unwrap();
    let marker2 = "nebula_e2e_after_restart_8846";
    write_frame(
        &mut c2,
        &ClientRequest::Input {
            session: sref2,
            data: format!("echo {marker2}\n").into_bytes(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c2, EVENT_TIMEOUT, |evs| {
        String::from_utf8_lossy(&collected_output(evs))
            .matches(marker2)
            .count()
            >= 2
    })
    .await;

    write_frame(&mut c2, &ClientRequest::Shutdown)
        .await
        .unwrap();
    wait_for_exit(&mut daemon2);
}

/// The daemon must answer a child's kitty-keyboard support query (nothing
/// else ever would — the child talks to a virtual terminal), track pushed
/// flags, and tell attached clients so they switch key encodings. This is
/// what makes Cmd/Option combos reach Claude Code.
#[tokio::test]
async fn kitty_keyboard_negotiation_passthrough() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    write_frame(
        &mut c,
        &ClientRequest::AddProject {
            req_id: 1,
            path: repo.clone(),
            name: None,
            create_missing: false,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 1).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Project(_)),
        ..
    } = find_ack(&events, 1).unwrap()
    else {
        panic!("AddProject failed: {events:#?}");
    };
    // AddProject's worktree upsert goes to subscribers only; fetch it via the DB
    // snapshot path instead: create the terminal against the main worktree id
    // that Subscribe would report. Simplest: subscribe now.
    let events = subscribe(&mut c).await;
    let worktree_id = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::Snapshot { worktrees, .. } => worktrees.first().map(|w| w.id.clone()),
            _ => None,
        })
        .expect("main worktree in snapshot");

    write_frame(
        &mut c,
        &ClientRequest::CreateTerminal {
            req_id: 2,
            worktree: worktree_id,
            name: None,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Terminal(term_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateTerminal failed: {events:#?}");
    };
    let sref = SessionRef::Terminal(term_id.clone());
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: sref.clone(),
            from_seq: None,
            cols: 100,
            rows: 30,
        },
    )
    .await
    .unwrap();
    // Attach reports the child's current (legacy) flags right away.
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::KittyFlags { flags: 0, .. }))
    })
    .await;

    // The child queries support and reads the daemon's reply off its own
    // stdin — the same detection recipe Claude Code uses. `tr` makes the
    // reply greppable in plain text.
    let probe = "stty -icanon -echo min 0 time 20; printf '\\033[?u'; sleep 1; \
                 printf 'REPLY:'; dd bs=64 count=1 2>/dev/null | tr '\\033' 'E'; echo; stty sane\n";
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: probe.into(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        String::from_utf8_lossy(&collected_output(evs)).contains("REPLY:E[?0u")
    })
    .await;

    // Pushing flags reaches the attached client…
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: b"printf '\\033[>1u'\n".to_vec(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::KittyFlags { flags: 1, .. }))
    })
    .await;

    // …survives a re-attach (fresh client learns the current mode)…
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: sref.clone(),
            from_seq: None,
            cols: 100,
            rows: 30,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::KittyFlags { .. }))
    })
    .await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ServerEvent::KittyFlags { flags: 1, .. })),
        "re-attach must report the pushed flags: {events:#?}"
    );

    // …and popping restores legacy.
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: b"printf '\\033[<u'\n".to_vec(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::KittyFlags { flags: 0, .. }))
    })
    .await;

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A child asking where its cursor is (`CSI 6 n`) is answered from the
/// daemon's own screen for the session, at the size the PTY has now. That
/// is the query crossterm's `cursor::position()` makes, and it timed out
/// after two seconds inside a nebula terminal (#66).
#[tokio::test]
async fn cursor_position_query_is_answered_at_the_pty_size() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    write_frame(
        &mut c,
        &ClientRequest::CreateTerminal {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: None,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Terminal(term_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateTerminal failed: {events:#?}");
    };
    let sref = SessionRef::Terminal(term_id.clone());

    // Park the cursor past the bottom-right corner (it clamps there), ask,
    // and read the reply back off stdin; `tr` makes it greppable.
    let probe = "stty -icanon -echo min 0 time 20; printf '\\033[999;999H\\033[6n'; sleep 1; \
                 printf 'REPLY:'; dd bs=64 count=1 2>/dev/null | tr '\\033' 'E'; echo; stty sane\n";
    // The first query builds the screen from the shell's output so far; the
    // re-attach at a new size must reach it.
    for (cols, rows) in [(100, 30), (120, 40)] {
        write_frame(
            &mut c,
            &ClientRequest::Attach {
                session: sref.clone(),
                from_seq: None,
                cols,
                rows,
            },
        )
        .await
        .unwrap();
        // The daemon applies the attach's resize before it reads the next
        // frame, so the probe cannot outrun it.
        write_frame(
            &mut c,
            &ClientRequest::Input {
                session: sref.clone(),
                data: probe.into(),
            },
        )
        .await
        .unwrap();
        let want = format!("REPLY:E[{rows};{cols}R");
        read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
            String::from_utf8_lossy(&collected_output(evs)).contains(&want)
        })
        .await;
    }

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// True end-to-end status detection: the agent PTY (a /bin/sh stand-in for
/// claude) uses its *injected* NEBULA_* env to curl the daemon's hook
/// endpoint, exactly like the installed claude hooks would — and the
/// subscribed client sees StatusChanged.
#[tokio::test]
async fn hook_post_from_agent_pty_drives_status() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;

    write_frame(
        &mut c,
        &ClientRequest::AddProject {
            req_id: 1,
            path: repo.clone(),
            name: None,
            create_missing: false,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(
                e,
                ServerEvent::EntityUpserted {
                    entity: Entity::Worktree(_)
                }
            )
        })
    })
    .await;
    let worktree = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Worktree(w),
            } => Some(w.clone()),
            _ => None,
        })
        .unwrap();

    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: "hooked".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(agent_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let agent_id = agent_id.clone();

    // Hook install happened at spawn: managed hooks exist in the worktree.
    let settings_path = repo.join(".claude/settings.local.json");
    assert!(settings_path.exists(), "hooks installed into worktree");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    assert!(settings["hooks"]["Stop"][0]["_nebulaManaged"]
        .as_bool()
        .unwrap());

    // Drive the shell inside the agent PTY to POST hooks with its own env.
    let sref = SessionRef::Agent(agent_id.clone());
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: sref.clone(),
            from_seq: None,
            cols: 120,
            rows: 30,
        },
    )
    .await
    .unwrap();
    let curl = |event: &str, body: &str| {
        format!(
            "curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \
             -H 'Content-Type: application/json' -d '{body}' \
             \"$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent={event}\"\n"
        )
    };

    // UserPromptSubmit → running
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: curl(
                "UserPromptSubmit",
                r#"{"session_id":"sess-1","prompt":"fix the  login\nredirect"}"#,
            )
            .into_bytes(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::Running, .. }
                if *agent == agent_id)
        })
    })
    .await;
    // …and the prompt itself lands on the row, condensed to one line, as
    // the RECENT PROMPTS upsert that follows the status change.
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id
                    && a.recent_prompts.iter().any(|p| p.text == "fix the login redirect"))
        })
    })
    .await;

    // Notification(permission_prompt) → needs_feedback
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: curl(
                "Notification",
                r#"{"session_id":"sess-1","notification_type":"permission_prompt"}"#,
            )
            .into_bytes(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::NeedsFeedback, .. }
                if *agent == agent_id)
        })
    })
    .await;

    // A foreign session's Stop is ignored…
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: curl("Stop", r#"{"session_id":"someone-elses-claude"}"#).into_bytes(),
        },
    )
    .await
    .unwrap();
    // …while the owning session's Stop finishes the agent.
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: curl("Stop", r#"{"session_id":"sess-1"}"#).into_bytes(),
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::Finished, .. }
                if *agent == agent_id)
        })
    })
    .await;
    // The foreign Stop must not have produced its own StatusChanged→Finished
    // before the NeedsFeedback→Finished one (i.e. exactly one Finished).
    let finished_count = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                ServerEvent::StatusChanged {
                    status: nebula_core::AgentStatus::Finished,
                    ..
                }
            )
        })
        .count();
    assert_eq!(finished_count, 1, "foreign-session Stop must be ignored");

    // Session id was captured for --resume.
    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
    let mut daemon2 = env.spawn_daemon();
    let mut c2 = connect(&env.sock()).await;
    handshake(&mut c2).await;
    write_frame(&mut c2, &ClientRequest::Subscribe)
        .await
        .unwrap();
    let events = read_events_until(&mut c2, EVENT_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::Snapshot { .. }))
    })
    .await;
    let ServerEvent::Snapshot { agents, .. } = &events[0] else {
        panic!()
    };
    assert_eq!(
        agents[0].session_id.as_deref(),
        Some("sess-1"),
        "session id persisted"
    );
    write_frame(&mut c2, &ClientRequest::Shutdown)
        .await
        .unwrap();
    wait_for_exit(&mut daemon2);
}

/// cwd-based re-homing end to end: an agent created in the main checkout
/// posts a hook whose payload reports a cwd inside another worktree of the
/// same project (claude entered a worktree it created mid-conversation) —
/// the daemon re-homes the agent row there and broadcasts the upsert.
/// The cancel path. Claude Code fires NO hook when the user interrupts a
/// turn — `Stop` is documented not to run on a user interrupt, and the
/// `idle_prompt` notification that normally rescues a hookless turn end is
/// suppressed precisely because the user just pressed a key. What it does
/// still do is clear its OSC 9;4 progress bar, so nebula reads busy/idle
/// straight off the PTY. This drives the whole path — raw bytes out of the
/// child, through the pump's scanner, into the status machine — with no HTTP
/// hook involved at all.
#[tokio::test]
async fn pty_progress_sequence_drives_status_without_any_hook() {
    pty_busy_idle_sequences_drive_status("9;4;3;", "9;4;0;").await;
}

/// Codex 0.163.0 emits OSC 7501 instead of OSC 9;4. Its hookless turn
/// completion must take the same PTY -> STATUS MACHINE -> TUI event path.
#[tokio::test]
async fn pty_codex_lifecycle_drives_status_without_any_hook() {
    pty_busy_idle_sequences_drive_status(
        "7501;state=working:app=codex",
        "7501;state=idle:app=codex",
    )
    .await;
}

/// Esc on a Claude permission dialog fires no hook, and Claude 2.1.319
/// draws no progress bar outside Ghostty and iTerm2, so the row stayed red
/// until its next turn. What Claude does write is the cancel, into the
/// transcript every hook names: after a keypress mid-turn the daemon reads
/// it, and the session is finished.
#[tokio::test]
async fn esc_on_a_permission_dialog_finishes_the_turn_from_the_transcript() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent_id = create_agent_get_id(&mut c, &worktree.id, "dialog", 2).await;
    let sref = SessionRef::Agent(agent_id.clone());
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: sref.clone(),
            from_seq: None,
            cols: 120,
            rows: 30,
        },
    )
    .await
    .unwrap();

    // The dialog is up, its tool call the transcript's newest line.
    let transcript = env.tmp.path().join("sess-1.jsonl");
    std::fs::write(
        &transcript,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"touch esc-probe.txt"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"touch esc-probe.txt"}}]}}"#,
            "\n",
        ),
    )
    .unwrap();
    let body = format!(
        r#"{{"session_id":"sess-1","transcript_path":"{}"}}"#,
        transcript.display()
    );
    let curl = format!(
        "curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \
         -H 'Content-Type: application/json' -d '{body}' \
         \"$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent=PermissionRequest\"\n"
    );
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: curl.into_bytes(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::NeedsFeedback, .. }
                if *agent == agent_id)
        })
    })
    .await;

    // Esc, and what Claude 2.1.319 appends to the transcript for it.
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: b"\x1b".to_vec(),
        },
    )
    .await
    .unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap();
    std::io::Write::write_all(
        &mut file,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"The user doesn't want to proceed with this tool use.","is_error":true,"tool_use_id":"t1"}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user for tool use]"}]}}"#,
            "\n",
            r#"{"type":"system","subtype":"turn_duration","durationMs":1389,"isMeta":false}"#,
            "\n",
        )
        .as_bytes(),
    )
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::Finished, .. }
                if *agent == agent_id)
        })
    })
    .await;

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

async fn pty_busy_idle_sequences_drive_status(busy: &str, idle: &str) {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent_id = create_agent_get_id(&mut c, &worktree.id, "cancelled", 2).await;

    let sref = SessionRef::Agent(agent_id.clone());
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: sref.clone(),
            from_seq: None,
            cols: 120,
            rows: 30,
        },
    )
    .await
    .unwrap();

    // Have the shell in the agent PTY emit the CLI's lifecycle sequences.
    // The command text is echoed back by the tty, which is the point: only
    // the real escape bytes may move the status, never a mention of them.
    // `\ddd` octal, not `\e` — POSIX printf, so this works under dash too.
    let emit = |payload: &str| format!("printf '\\033]{payload}\\033\\134'\n").into_bytes();

    // Turn starts: progress goes indeterminate → running.
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: emit(busy),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::Running, .. }
                if *agent == agent_id)
        })
    })
    .await;

    // The turn finishes or is cancelled, and nothing else happens.
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: emit(idle),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::Finished, .. }
                if *agent == agent_id)
        })
    })
    .await;

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

#[tokio::test]
async fn hook_cwd_rehomes_agent_to_other_worktree() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;

    write_frame(
        &mut c,
        &ClientRequest::AddProject {
            req_id: 1,
            path: repo.clone(),
            name: None,
            create_missing: false,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(
                e,
                ServerEvent::EntityUpserted {
                    entity: Entity::Worktree(_)
                }
            )
        })
    })
    .await;
    let main_worktree = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Worktree(w),
            } => Some(w.clone()),
            _ => None,
        })
        .unwrap();

    // A second worktree — the one the agent will "enter".
    write_frame(
        &mut c,
        &ClientRequest::CreateWorktree {
            req_id: 2,
            project: main_worktree.project_id.clone(),
            branch: "feat".into(),
            base: None,
        },
    )
    .await
    .unwrap();
    // The upsert broadcast and the Ack ride different channels — wait for
    // the upsert itself.
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Worktree(w) }
                if w.branch == "feat")
        })
    })
    .await;
    let feat_worktree = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Worktree(w),
            } if w.branch == "feat" => Some(w.clone()),
            _ => None,
        })
        .expect("feat worktree upsert");

    // Agent lives in the main checkout.
    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 3,
            worktree: main_worktree.id.clone(),
            name: "mover".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 3).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(agent_id)),
        ..
    } = find_ack(&events, 3).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let agent_id = agent_id.clone();

    // POST a hook from inside the agent PTY whose payload reports the feat
    // worktree as cwd — exactly what claude sends after entering it.
    let sref = SessionRef::Agent(agent_id.clone());
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: sref.clone(),
            from_seq: None,
            cols: 120,
            rows: 30,
        },
    )
    .await
    .unwrap();
    let body = format!(
        r#"{{"session_id":"sess-1","cwd":"{}"}}"#,
        feat_worktree.path.display()
    );
    let curl = format!(
        "curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \
         -H 'Content-Type: application/json' -d '{body}' \
         \"$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent=UserPromptSubmit\"\n"
    );
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: curl.into_bytes(),
        },
    )
    .await
    .unwrap();

    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id && a.worktree_id == feat_worktree.id)
        })
    })
    .await;
    assert!(
        events.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id && a.worktree_id == feat_worktree.id)
        }),
        "agent re-homed to the worktree its hook cwd reported: {events:#?}"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// CLAUDE TITLE SYNC end to end, with a /bin/sh standing in for claude:
/// the row's name reaches the CLI as the `UserPromptSubmit` reply's
/// `sessionTitle`, and a `/rename` inside the CLI — which fires no hook,
/// only rewrites the window title and the `custom-title.json` beside the
/// transcript the hooks named — retitles the row.
#[tokio::test]
async fn claude_session_title_and_row_name_stay_tied() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;

    write_frame(
        &mut c,
        &ClientRequest::AddProject {
            req_id: 1,
            path: repo.clone(),
            name: None,
            create_missing: false,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(
                e,
                ServerEvent::EntityUpserted {
                    entity: Entity::Worktree(_)
                }
            )
        })
    })
    .await;
    let worktree = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Worktree(w),
            } => Some(w.clone()),
            _ => None,
        })
        .unwrap();

    // A name typed at creation: settled, so it is due a push into Claude.
    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: "Typed In Nebula".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(agent_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let agent_id = agent_id.clone();
    let sref = SessionRef::Agent(agent_id.clone());
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: sref.clone(),
            from_seq: None,
            cols: 120,
            rows: 30,
        },
    )
    .await
    .unwrap();

    // The transcript claude would report, in a dir this test controls.
    let transcripts = env.tmp.path().join("transcripts");
    std::fs::create_dir_all(&transcripts).unwrap();
    let transcript = transcripts.join("sess-1.jsonl");
    let body = format!(
        r#"{{"session_id":"sess-1","transcript_path":"{}"}}"#,
        transcript.display()
    );
    // The marker is split in the typed line (`D""ONE`) so the shell's echo
    // of the command can't satisfy the wait — only curl's finished reply.
    let curl = |marker: &str| {
        format!(
            "curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \
             -H 'Content-Type: application/json' -d '{body}' \
             \"$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent=UserPromptSubmit\"; \
             echo {marker}\n"
        )
    };
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: curl("REPLY-D\"\"ONE").into_bytes(),
        },
    )
    .await
    .unwrap();
    // nebula → Claude: the reply hands the CLI the row's name.
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        String::from_utf8_lossy(&collected_output(evs)).contains("REPLY-DONE")
    })
    .await;
    let output = String::from_utf8_lossy(&collected_output(&events)).to_string();
    assert!(
        output.contains(r#""sessionTitle":"Typed In Nebula""#),
        "reply carries the row name: {output}"
    );

    // Claude → nebula: `/rename` persists the title beside the transcript
    // and rewrites the window title; no hook fires. Only the OSC bytes
    // come from the PTY — the sidecar is claude's file, written here.
    let sidecar_dir = transcripts.join("sess-1");
    std::fs::create_dir_all(&sidecar_dir).unwrap();
    std::fs::write(
        sidecar_dir.join("custom-title.json"),
        r#"{"customTitle":"Renamed In Claude"}"#,
    )
    .unwrap();
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: b"printf '\\033]0;\xe2\x9c\xb3 Renamed In Claude\\007'\n".to_vec(),
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id && a.name == "Renamed In Claude")
        })
    })
    .await;
    assert!(
        events.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id && a.name == "Renamed In Claude")
        }),
        "row retitled from claude's /rename: {events:#?}"
    );

    // Now the two agree: the next prompt's reply pushes nothing.
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: curl("REPLY-T\"\"WO").into_bytes(),
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        String::from_utf8_lossy(&collected_output(evs)).contains("REPLY-TWO")
    })
    .await;
    let output = String::from_utf8_lossy(&collected_output(&events)).to_string();
    assert!(
        !output.contains("sessionTitle"),
        "no push once claude holds the name: {output}"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A kill-and-respawn behind an attached client — Restart here, and the
/// same path a `nebula worktree` relocation or a cloud re-entry takes —
/// rebinds that client to the new PTY: a fresh Scrollback and the new
/// process's output arrive on the attachment it already holds, with no
/// second Attach. Before this the forward task died with the old PTY and
/// the TUI's pane sat frozen until the user clicked away and back.
#[tokio::test]
async fn restart_rebinds_an_attached_client_to_the_new_pty() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    // Stand-in CLI: announce which boot this is, then park.
    let counter = env.tmp.path().join("boots");
    let script = env.tmp.path().join("agent.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nn=$(( $(cat '{c}' 2>/dev/null || echo 0) + 1 ))\necho $n > '{c}'\n\
             echo \"booted $n\"\nexec sleep 600\n",
            c = counter.display()
        ),
    )
    .unwrap();
    make_executable(&script);
    let mut daemon = env.spawn_daemon_with_agent_cmd(script.to_str().unwrap());

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;

    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: main_worktree.id.clone(),
            name: "agent-1".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(agent_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let agent_id = agent_id.clone();
    let sref = SessionRef::Agent(agent_id.clone());

    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: sref.clone(),
            from_seq: None,
            cols: 120,
            rows: 30,
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        String::from_utf8_lossy(&collected_output(evs)).contains("booted 1")
    })
    .await;

    // The daemon kills the PTY and spawns another under the same ref; the
    // attachment above is all this client ever sends.
    write_frame(
        &mut c,
        &ClientRequest::RestartAgent {
            req_id: 3,
            id: agent_id.clone(),
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        find_ack(evs, 3).is_some()
            && String::from_utf8_lossy(&collected_output(evs)).contains("booted 2")
    })
    .await;
    assert!(
        matches!(find_ack(&events, 3), Some(ServerEvent::Ack { .. })),
        "restart failed: {events:#?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ServerEvent::Scrollback { session, .. } if session == &sref)),
        "the rebind replays the new PTY's ring: {events:#?}"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// Codex mirror of the claude hook test: a codex-kind agent gets its hooks
/// installed into codex's home (not the worktree — one trust approval has
/// to cover every worktree), and posts to `/api/hooks/codex` drive the same
/// status machine (PermissionRequest is codex's native waiting signal).
#[tokio::test]
async fn codex_hooks_install_and_drive_status() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let codex_home = env.tmp.path().join("codex-home");
    // A worktree copy from an older nebula, alongside a foreign managed
    // group: the spawn must prune ours and leave theirs alone.
    let stale = repo.join(".codex");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(
        stale.join("hooks.json"),
        r#"{"hooks":{"Stop":[
            {"_nebulaManaged":true,"hooks":[{"type":"command",
              "command":"curl $NEBULA_API_URL/api/hooks/codex?agentId=$NEBULA_AGENT_ID"}]},
            {"_mcManaged":true,"hooks":[{"type":"command","command":"curl $MC_API_URL/x"}]}]}}"#,
    )
    .unwrap();
    let mut daemon =
        env.spawn_daemon_with("/bin/sh", &[("CODEX_HOME", codex_home.to_str().unwrap())]);

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;

    write_frame(
        &mut c,
        &ClientRequest::AddProject {
            req_id: 1,
            path: repo.clone(),
            name: None,
            create_missing: false,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(
                e,
                ServerEvent::EntityUpserted {
                    entity: Entity::Worktree(_)
                }
            )
        })
    })
    .await;
    let worktree = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Worktree(w),
            } => Some(w.clone()),
            _ => None,
        })
        .unwrap();

    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: "codexed".into(),
            kind: AgentKind::Codex,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(agent_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let agent_id = agent_id.clone();

    // Codex hooks were installed into codex's home — and only codex-shaped
    // ones (no claude-specific Notification/AskUserQuestion groups).
    let hooks_path = codex_home.join("hooks.json");
    assert!(hooks_path.exists(), "codex hooks installed into codex home");
    let hooks: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&hooks_path).unwrap()).unwrap();
    assert!(hooks["hooks"]["Stop"][0]["_nebulaManaged"]
        .as_bool()
        .unwrap());
    assert!(hooks["hooks"]["Stop"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .contains("/api/hooks/codex?"));
    assert!(hooks["hooks"].get("Notification").is_none());
    assert!(hooks["hooks"].get("PreToolUse").is_none());

    // The stale worktree copy lost our group and kept the foreign one.
    let stale_hooks: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(stale.join("hooks.json")).unwrap()).unwrap();
    let stop = stale_hooks["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 1, "only the foreign group survives: {stop:#?}");
    assert!(stop[0]["_mcManaged"].as_bool().unwrap());

    // Drive the shell inside the agent PTY to POST codex hooks with its env.
    let sref = SessionRef::Agent(agent_id.clone());
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: sref.clone(),
            from_seq: None,
            cols: 120,
            rows: 30,
        },
    )
    .await
    .unwrap();
    let curl = |event: &str, body: &str| {
        format!(
            "curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \
             -H 'Content-Type: application/json' -d '{body}' \
             \"$NEBULA_API_URL/api/hooks/codex?agentId=$NEBULA_AGENT_ID&hookEvent={event}\"\n"
        )
    };

    // UserPromptSubmit → running
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: curl("UserPromptSubmit", r#"{"session_id":"codex-sess-1"}"#).into_bytes(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::Running, .. }
                if *agent == agent_id)
        })
    })
    .await;

    // PermissionRequest (codex's native waiting signal) → needs_feedback
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: curl("PermissionRequest", r#"{"session_id":"codex-sess-1"}"#).into_bytes(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::NeedsFeedback, .. }
                if *agent == agent_id)
        })
    })
    .await;

    // Stop → finished
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: curl("Stop", r#"{"session_id":"codex-sess-1"}"#).into_bytes(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::Finished, .. }
                if *agent == agent_id)
        })
    })
    .await;

    // Kind and session id survive a daemon restart (feeds `codex resume`).
    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
    let mut daemon2 = env.spawn_daemon();
    let mut c2 = connect(&env.sock()).await;
    handshake(&mut c2).await;
    write_frame(&mut c2, &ClientRequest::Subscribe)
        .await
        .unwrap();
    let events = read_events_until(&mut c2, EVENT_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::Snapshot { .. }))
    })
    .await;
    let ServerEvent::Snapshot { agents, .. } = &events[0] else {
        panic!()
    };
    assert_eq!(agents[0].kind, AgentKind::Codex, "kind persisted");
    assert_eq!(
        agents[0].session_id.as_deref(),
        Some("codex-sess-1"),
        "codex session id persisted"
    );
    write_frame(&mut c2, &ClientRequest::Shutdown)
        .await
        .unwrap();
    wait_for_exit(&mut daemon2);
}

#[tokio::test]
async fn external_worktrees_are_adopted_and_dropped() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    write_frame(&mut c, &ClientRequest::Subscribe)
        .await
        .unwrap();
    write_frame(
        &mut c,
        &ClientRequest::AddProject {
            req_id: 1,
            path: repo.clone(),
            name: None,
            create_missing: false,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 1).is_some()).await;
    assert!(
        matches!(find_ack(&events, 1), Some(ServerEvent::Ack { .. })),
        "AddProject failed: {events:#?}"
    );

    // A worktree created behind nebula's back — exactly what an agent (or a
    // human in another shell) does.
    let wt_path = env.tmp.path().join("repo-worktrees").join("agent-branch");
    let git_worktree = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("worktree")
            .args(args)
            .arg(&wt_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    assert!(
        git_worktree(&["add", "-b", "agent-branch"]),
        "external git worktree add failed"
    );

    // The auto-sync adopts it without any client request.
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter().any(|e| matches!(
            e,
            ServerEvent::EntityUpserted { entity: Entity::Worktree(w) } if w.branch == "agent-branch"
        ))
    })
    .await;
    let adopted = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Worktree(w),
            } if w.branch == "agent-branch" => Some(w.clone()),
            _ => None,
        })
        .unwrap();
    assert!(!adopted.is_main, "adopted checkout is not the main row");
    assert!(
        adopted.path.exists(),
        "adopted row points at the real checkout"
    );

    // Removing it externally drops the row too (nothing lives there).
    assert!(
        git_worktree(&["remove", "--force"]),
        "external git worktree remove failed"
    );
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(
                e,
                ServerEvent::EntityRemoved { id: EntityId::Worktree(id) } if *id == adopted.id
            )
        })
    })
    .await;

    // Switching branches on the root checkout renames the main row in place
    // (the probe watches .git/HEAD, not just the worktrees registry).
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["checkout", "-b", "renamed-root"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success(),
        "git checkout -b renamed-root failed"
    );
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(
                e,
                ServerEvent::EntityUpserted { entity: Entity::Worktree(w) }
                    if w.is_main && w.branch == "renamed-root"
            )
        })
    })
    .await;
    assert!(
        events.iter().any(|e| matches!(
            e,
            ServerEvent::EntityUpserted { entity: Entity::Worktree(w) }
                if w.is_main && w.branch == "renamed-root"
        )),
        "main row should refresh to the new branch: {events:#?}"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `nebula upgrade` daemon handoff: with a live session the old daemon is
/// left running (restart is the user's call); once idle, the upgrade shuts
/// it down so the next launch spawns the new binary.
#[tokio::test]
async fn upgrade_shuts_down_idle_daemon_but_spares_live_sessions() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    write_frame(&mut c, &ClientRequest::Subscribe)
        .await
        .unwrap();
    write_frame(
        &mut c,
        &ClientRequest::AddProject {
            req_id: 1,
            path: repo.clone(),
            name: None,
            create_missing: false,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        find_ack(evs, 1).is_some()
            && evs.iter().any(|e| {
                matches!(
                    e,
                    ServerEvent::EntityUpserted {
                        entity: Entity::Worktree(_)
                    }
                )
            })
    })
    .await;
    let main_worktree = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Worktree(w),
            } if w.is_main => Some(w.clone()),
            _ => None,
        })
        .expect("main worktree upsert");

    write_frame(
        &mut c,
        &ClientRequest::CreateTerminal {
            req_id: 2,
            worktree: main_worktree.id.clone(),
            name: None,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Terminal(term_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateTerminal failed: {events:#?}");
    };
    let term_id = term_id.clone();

    // Stub installer: the upgrade command runs it, then handles the daemon.
    // No `nebula` on PATH, so there is no installed binary to restart onto
    // and the live daemon is left where it is.
    let script = env.tmp.path().join("stub-install.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    let run_upgrade = || {
        env.cli()
            .args(["upgrade", "--force"])
            .env(env::INSTALL_URL, format!("file://{}", script.display()))
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap()
    };

    // A live terminal PTY keeps the daemon alive through the upgrade.
    let out = run_upgrade();
    assert!(
        out.status.success(),
        "upgrade failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1 live session"),
        "expected live-session note, got: {stdout}"
    );
    assert!(
        daemon.try_wait().unwrap().is_none(),
        "daemon must keep running while sessions are live"
    );

    // Exit the shell; the daemon marks the terminal dead and is now idle.
    let sref = SessionRef::Terminal(term_id.clone());
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: sref.clone(),
            from_seq: None,
            cols: 80,
            rows: 24,
        },
    )
    .await
    .unwrap();
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref,
            data: b"exit\n".to_vec(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(
                e,
                ServerEvent::EntityUpserted { entity: Entity::Terminal(t) }
                    if t.id == term_id && !t.alive
            )
        })
    })
    .await;

    let out = run_upgrade();
    assert!(
        out.status.success(),
        "idle upgrade failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no live sessions"),
        "expected idle-shutdown note, got: {stdout}"
    );
    wait_for_exit(&mut daemon);
}

/// The number printed after the first `key` in `text` that has one — the
/// shell's answer, not the echo of the command that asked for it.
fn number_after(text: &str, key: &str) -> Option<i32> {
    text.match_indices(key).find_map(|(at, _)| {
        let digits: String = text[at + key.len()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    })
}

/// Close-out of a connection the daemon dropped: everything it still had
/// queued, then the end of the stream.
async fn read_until_closed(stream: &mut UnixStream) {
    let closed = tokio::time::timeout(SLOW_TIMEOUT, async {
        while let Ok(Some(_)) = read_frame::<ServerEvent, _>(stream).await {}
    })
    .await;
    assert!(
        closed.is_ok(),
        "the restart never closed the old connection"
    );
}

/// IN-PLACE RESTART, end to end: `nebula reload` moves the daemon onto a
/// binary while an agent and a terminal keep running — the same processes,
/// scrollback intact, status kept, hooks still reaching the daemon on the
/// port and token the agent was born with — and `nebula upgrade` does it
/// again onto the `nebula` it finds on PATH, from the image the first
/// restart left behind.
#[tokio::test]
async fn reload_and_upgrade_restart_the_daemon_without_stopping_sessions() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent_id = create_agent_get_id(&mut c, &worktree.id, "kept", 2).await;
    write_frame(
        &mut c,
        &ClientRequest::CreateTerminal {
            req_id: 3,
            worktree: worktree.id.clone(),
            name: None,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 3).is_some()).await;
    let Some(ServerEvent::Ack {
        created: Some(EntityId::Terminal(term_id)),
        ..
    }) = find_ack(&events, 3)
    else {
        panic!("CreateTerminal failed: {events:#?}");
    };
    let term_id = term_id.clone();
    let agent = SessionRef::Agent(agent_id.clone());
    let term = SessionRef::Terminal(term_id.clone());

    let attach = |sref: &SessionRef| ClientRequest::Attach {
        session: sref.clone(),
        from_seq: None,
        cols: 100,
        rows: 30,
    };
    let input = |sref: &SessionRef, line: String| ClientRequest::Input {
        session: sref.clone(),
        data: line.into_bytes(),
    };
    for (sref, label) in [(&agent, "agent"), (&term, "term")] {
        write_frame(&mut c, &attach(sref)).await.unwrap();
        write_frame(&mut c, &input(sref, format!("echo \"pid-{label}=$$\"\n")))
            .await
            .unwrap();
    }
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        let text = String::from_utf8_lossy(&collected_output(evs)).into_owned();
        number_after(&text, "pid-agent=").is_some() && number_after(&text, "pid-term=").is_some()
    })
    .await;
    let text = String::from_utf8_lossy(&collected_output(&events)).into_owned();
    let agent_pid = number_after(&text, "pid-agent=").unwrap();
    let term_pid = number_after(&text, "pid-term=").unwrap();

    // The agent is mid-turn as the restart happens.
    let curl = |event: &str| {
        format!(
            "curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \
             -H 'Content-Type: application/json' -d '{{\"session_id\":\"sess-1\"}}' \
             \"$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent={event}\"\n"
        )
    };
    write_frame(&mut c, &input(&agent, curl("UserPromptSubmit")))
        .await
        .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::Running, .. }
                if *agent == agent_id)
        })
    })
    .await;

    // ---- `nebula reload`: onto the binary the command runs from ----
    let out = env.cli().arg("reload").output().unwrap();
    assert!(
        out.status.success(),
        "reload failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("2 live sessions kept running"), "{stdout}");
    assert!(
        daemon.try_wait().unwrap().is_none(),
        "the daemon restarts in place — same process"
    );
    read_until_closed(&mut c).await;

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let events = subscribe(&mut c).await;
    let Some(ServerEvent::Snapshot {
        agents, terminals, ..
    }) = events.last()
    else {
        panic!("expected a snapshot: {events:#?}");
    };
    let row = agents.iter().find(|a| a.id == agent_id).unwrap();
    assert!(row.alive, "the agent is still live: {row:?}");
    assert_eq!(row.status, nebula_core::AgentStatus::Running, "status kept");
    assert!(terminals.iter().any(|t| t.id == term_id && t.alive));

    // The scrollback came along, and the same shell answers.
    write_frame(&mut c, &attach(&agent)).await.unwrap();
    write_frame(&mut c, &input(&agent, "echo \"after-agent=$$\"\n".into()))
        .await
        .unwrap();
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        let text = String::from_utf8_lossy(&collected_output(evs)).into_owned();
        number_after(&text, "after-agent=").is_some()
    })
    .await;
    let text = String::from_utf8_lossy(&collected_output(&events)).into_owned();
    assert_eq!(
        number_after(&text, "pid-agent="),
        Some(agent_pid),
        "replayed"
    );
    assert_eq!(number_after(&text, "after-agent="), Some(agent_pid));

    // Its hooks still land, on the port and token it was spawned with.
    write_frame(&mut c, &input(&agent, curl("Stop")))
        .await
        .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::Finished, .. }
                if *agent == agent_id)
        })
    })
    .await;

    // ---- `nebula upgrade`: onto the `nebula` on PATH, a second time ----
    let script = env.tmp.path().join("stub-install.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    let bin = env.tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_nebula"), bin.join("nebula")).unwrap();
    let out = env
        .cli()
        .args(["upgrade", "--force"])
        .env(env::INSTALL_URL, format!("file://{}", script.display()))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "upgrade failed: {stdout}");
    assert!(stdout.contains("2 live sessions kept running"), "{stdout}");
    read_until_closed(&mut c).await;

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;
    write_frame(&mut c, &attach(&term)).await.unwrap();
    write_frame(&mut c, &input(&term, "echo \"again-term=$$\"\n".into()))
        .await
        .unwrap();
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        let text = String::from_utf8_lossy(&collected_output(evs)).into_owned();
        number_after(&text, "again-term=").is_some()
    })
    .await;
    let text = String::from_utf8_lossy(&collected_output(&events)).into_owned();
    assert_eq!(number_after(&text, "again-term="), Some(term_pid));
    assert!(pid_alive(agent_pid) && pid_alive(term_pid));

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
    wait_pid_dead(agent_pid, SLOW_TIMEOUT, "agent shell").await;
    wait_pid_dead(term_pid, SLOW_TIMEOUT, "terminal shell").await;
}

/// IN-PLACE RESTART for every harness: one live agent of each kind goes
/// mid-turn, the daemon restarts, and each ends its turn afterward on the
/// same process — the hooked kinds through their own `/api/hooks/<kind>`
/// route, the hookless ones through the progress bar their CLI draws.
#[tokio::test]
async fn reload_keeps_an_agent_of_every_harness() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    env.write_config(r#"{"custom_harnesses": [{"id": "agy", "program": "agy"}]}"#);
    let homes = ["codex-home", "pi-home", "xdg-config"].map(|d| {
        let dir = env.tmp.path().join(d);
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_str().unwrap().to_string()
    });
    let mut daemon = env.spawn_daemon_with(
        "/bin/sh",
        &[
            ("CODEX_HOME", &homes[0]),
            ("PI_CODING_AGENT_DIR", &homes[1]),
            ("XDG_CONFIG_HOME", &homes[2]),
        ],
    );

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let mut agents = Vec::new();
    for (req_id, kind) in (10u64..).zip(AgentKind::ALL) {
        write_frame(
            &mut c,
            &ClientRequest::CreateAgent {
                req_id,
                worktree: worktree.id.clone(),
                name: kind.as_str().into(),
                kind,
                custom_harness: (kind == AgentKind::Custom).then(|| "agy".into()),
                model: None,
                effort: None,
                auto_title: false,
                cloud_prompt: None,
                starting_prompt: None,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
            },
        )
        .await
        .unwrap();
        let events = read_events_until(&mut c, SPAWN_CHAIN_TIMEOUT, |evs| {
            find_ack(evs, req_id).is_some()
        })
        .await;
        let Some(ServerEvent::Ack {
            created: Some(EntityId::Agent(id)),
            ..
        }) = find_ack(&events, req_id)
        else {
            panic!("CreateAgent {kind:?} failed: {events:#?}");
        };
        agents.push((kind, id.clone()));
    }

    let hooked = |kind: AgentKind| {
        matches!(
            kind,
            AgentKind::Claude
                | AgentKind::Codex
                | AgentKind::Cursor
                | AgentKind::Pi
                | AgentKind::OpenCode
        )
    };
    // A hooked kind posts the event to its own route; a hookless one draws
    // (`UserPromptSubmit`) or clears (`Stop`) its progress bar.
    let turn = |kind: AgentKind, event: &str| {
        let line = if hooked(kind) {
            format!(
                "curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \
                 -H 'Content-Type: application/json' -d '{{\"session_id\":\"sess-{kind}\"}}' \
                 \"$NEBULA_API_URL/api/hooks/{kind}?agentId=$NEBULA_AGENT_ID&hookEvent={event}\" \
                 >/dev/null\n",
                kind = kind.as_str()
            )
        } else {
            let state = if event == "Stop" { 0 } else { 3 };
            format!("printf '\\033]9;4;{state};\\007'\n")
        };
        ClientRequest::Input {
            session: SessionRef::Agent(agents.iter().find(|(k, _)| *k == kind).unwrap().1.clone()),
            data: line.into_bytes(),
        }
    };
    let attach = |id: &nebula_core::AgentId| ClientRequest::Attach {
        session: SessionRef::Agent(id.clone()),
        from_seq: None,
        cols: 100,
        rows: 30,
    };
    let echo_pid = |id: &nebula_core::AgentId, key: &str| ClientRequest::Input {
        session: SessionRef::Agent(id.clone()),
        data: format!("echo \"{key}-{}=$$\"\n", id.0).into_bytes(),
    };
    let all_reached = |evs: &[ServerEvent], want: nebula_core::AgentStatus| {
        agents.iter().all(|(_, id)| {
            evs.iter().any(|e| {
                matches!(e, ServerEvent::StatusChanged { agent, status, .. }
                    if agent == id && *status == want)
            })
        })
    };

    // Every agent answers with its shell's pid and starts a turn.
    for (kind, id) in &agents {
        write_frame(&mut c, &attach(id)).await.unwrap();
        write_frame(&mut c, &echo_pid(id, "pid")).await.unwrap();
        write_frame(&mut c, &turn(*kind, "UserPromptSubmit"))
            .await
            .unwrap();
    }
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        all_reached(evs, nebula_core::AgentStatus::Running)
    })
    .await;
    let mut pids = std::collections::HashMap::new();
    let mut text = String::from_utf8_lossy(&collected_output(&events)).into_owned();
    for (kind, id) in &agents {
        let key = format!("pid-{}=", id.0);
        if number_after(&text, &key).is_none() {
            let more = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
                number_after(&String::from_utf8_lossy(&collected_output(evs)), &key).is_some()
            })
            .await;
            text.push_str(&String::from_utf8_lossy(&collected_output(&more)));
        }
        pids.insert(*kind, number_after(&text, &key).unwrap());
    }

    let out = env.cli().arg("reload").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "reload failed: {stdout}");
    assert!(
        stdout.contains(&format!("{} live sessions kept running", agents.len())),
        "{stdout}"
    );
    read_until_closed(&mut c).await;

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let events = subscribe(&mut c).await;
    let Some(ServerEvent::Snapshot { agents: rows, .. }) = events.last() else {
        panic!("expected a snapshot: {events:#?}");
    };
    for (kind, id) in &agents {
        let row = rows.iter().find(|a| a.id == *id).unwrap();
        assert!(row.alive, "{kind:?} is still live: {row:?}");
        assert_eq!(
            row.status,
            nebula_core::AgentStatus::Running,
            "{kind:?} kept its status"
        );
    }

    // The same shell answers each, and each turn ends the way its harness
    // reports one.
    for (kind, id) in &agents {
        write_frame(&mut c, &attach(id)).await.unwrap();
        write_frame(&mut c, &echo_pid(id, "after")).await.unwrap();
        write_frame(&mut c, &turn(*kind, "Stop")).await.unwrap();
    }
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        all_reached(evs, nebula_core::AgentStatus::Finished)
    })
    .await;
    let mut text = String::from_utf8_lossy(&collected_output(&events)).into_owned();
    for (kind, id) in &agents {
        let key = format!("after-{}=", id.0);
        if number_after(&text, &key).is_none() {
            let more = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
                number_after(&String::from_utf8_lossy(&collected_output(evs)), &key).is_some()
            })
            .await;
            text.push_str(&String::from_utf8_lossy(&collected_output(&more)));
        }
        assert_eq!(
            number_after(&text, &key),
            Some(pids[kind]),
            "{kind:?} runs on the same process"
        );
    }

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
    for (kind, pid) in pids {
        wait_pid_dead(pid, SLOW_TIMEOUT, &format!("{kind:?} shell")).await;
    }
}

/// AddProject with `create_missing` makes the directory and `git init`s it.
/// An existing folder outside any repository is refused as it stands (not a
/// git repository) and `git init`ed with `create_missing` — the client's
/// confirm.
#[tokio::test]
async fn add_project_creates_missing_dir_and_inits() {
    let env = TestEnv::new();
    let mut daemon = env.spawn_daemon();

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;

    let new_dir = env.tmp.path().join("brand-new-project");
    assert!(!new_dir.exists());
    write_frame(
        &mut c,
        &ClientRequest::AddProject {
            req_id: 1,
            path: new_dir.clone(),
            name: None,
            create_missing: true,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 1).is_some()).await;
    assert!(
        matches!(
            find_ack(&events, 1),
            Some(ServerEvent::Ack {
                created: Some(EntityId::Project(_)),
                ..
            })
        ),
        "AddProject with create_missing failed: {events:#?}"
    );
    assert!(new_dir.join(".git").is_dir(), "git init ran in the new dir");

    // An existing folder in no repository: refused as it stands…
    let plain_dir = env.tmp.path().join("plain-folder");
    std::fs::create_dir_all(&plain_dir).unwrap();
    for (req_id, create_missing) in [(2, false), (3, true)] {
        write_frame(
            &mut c,
            &ClientRequest::AddProject {
                req_id,
                path: plain_dir.clone(),
                name: None,
                create_missing,
            },
        )
        .await
        .unwrap();
        let events =
            read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, req_id).is_some()).await;
        if create_missing {
            // …and `git init`ed once the user said yes.
            assert!(
                matches!(
                    find_ack(&events, req_id),
                    Some(ServerEvent::Ack {
                        created: Some(EntityId::Project(_)),
                        ..
                    })
                ),
                "AddProject with create_missing on a plain folder failed: {events:#?}"
            );
            assert!(
                plain_dir.join(".git").is_dir(),
                "git init ran in the folder"
            );
        } else {
            assert!(
                matches!(find_ack(&events, req_id), Some(ServerEvent::Error { .. })),
                "expected not-a-git-repo error: {events:#?}"
            );
            assert!(!plain_dir.join(".git").exists(), "nothing inited unasked");
        }
    }

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// Subscribe + AddProject boilerplate; returns the main worktree row.
async fn add_project_get_main_worktree(c: &mut UnixStream, repo: &Path) -> nebula_core::Worktree {
    write_frame(c, &ClientRequest::Subscribe).await.unwrap();
    read_events_until(c, EVENT_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::Snapshot { .. }))
    })
    .await;
    write_frame(
        c,
        &ClientRequest::AddProject {
            req_id: 1,
            path: repo.to_path_buf(),
            name: None,
            create_missing: false,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(c, EVENT_TIMEOUT, |evs| {
        find_ack(evs, 1).is_some()
            && evs.iter().any(|e| {
                matches!(
                    e,
                    ServerEvent::EntityUpserted {
                        entity: Entity::Worktree(w)
                    } if w.is_main
                )
            })
    })
    .await;
    events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Worktree(w),
            } if w.is_main => Some(w.clone()),
            _ => None,
        })
        .expect("main worktree upsert")
}

/// PrewarmAgent boots the CLI while the user is "typing the name"; the
/// following CreateAgent must adopt that already-running PTY (its slow boot
/// output is already in scrollback) and replay the hooks it fired before the
/// row existed (SessionStart → the session id the row's first turn saves).
#[tokio::test]
async fn prewarmed_session_is_adopted_by_create_agent() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    // Stand-in agent CLI with a deliberately slow boot: posts SessionStart
    // (like claude does), sleeps, prints a marker, then becomes a shell. If
    // adoption works, the marker is in scrollback the moment we attach; a
    // cold spawn at CreateAgent time couldn't print it for another 3s.
    let script = env.tmp.path().join("slow-agent.sh");
    let ready = env.tmp.path().join("prewarm-ready");
    std::fs::write(
        &script,
        format!(
            concat!(
            "#!/bin/sh\n",
            "curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \\\n",
            "  -H 'Content-Type: application/json' -d '{{\"session_id\":\"warm-sid-99\"}}' \\\n",
            "  \"$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent=SessionStart\" \\\n",
            "  >/dev/null 2>&1\n",
            "sleep 3\n",
            ": > '{}'\n",
            "echo PREWARM_READY\n",
            "exec /bin/sh\n",
            ),
            ready.display(),
        ),
    )
    .unwrap();
    make_executable(&script);
    let mut daemon = env.spawn_daemon_with_agent_cmd(script.to_str().unwrap());

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;

    // Kind picked — warm the CLI. No reply expected.
    write_frame(
        &mut c,
        &ClientRequest::PrewarmAgent {
            worktree: worktree.id.clone(),
            kind: AgentKind::Claude,
            model: None,
            effort: None,
        },
    )
    .await
    .unwrap();
    // "User types the name": wait until the warm boot has reached the shell.
    wait_for_file(&ready, "prewarm-ready marker").await;

    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: "warm-agent".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(agent_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let agent_id = agent_id.clone();

    // Attach: the boot marker must already be there (window far shorter
    // than the script's 3s boot, so a cold spawn cannot pass).
    let sref = SessionRef::Agent(agent_id.clone());
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: sref.clone(),
            from_seq: None,
            cols: 100,
            rows: 30,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, Duration::from_secs(2), |evs| {
        String::from_utf8_lossy(&collected_output(evs)).contains("PREWARM_READY")
    })
    .await;
    drop(events);

    // The adopted PTY is interactive.
    let marker = "adopted_marker_7731";
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: format!("echo {marker}\n").into_bytes(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        String::from_utf8_lossy(&collected_output(evs))
            .matches(marker)
            .count()
            >= 2
    })
    .await;

    // The SessionStart the warm CLI posted before the row existed was
    // buffered and replayed. A session id is saved only once a turn has
    // run, and a Stop saves only the id already adopted — so the first
    // turn's Stop persisting warm-sid-99 proves the replay happened.
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: sref.clone(),
            data: concat!(
                r#"curl -sS -m 3 -X POST -H "Authorization: Bearer $NEBULA_API_TOKEN" -H 'Content-Type: application/json' -d '{"session_id":"warm-sid-99"}' "$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent=Stop""#,
                "\n"
            )
            .as_bytes()
            .to_vec(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::Finished, .. }
                if *agent == agent_id)
        })
    })
    .await;
    let mut c2 = connect(&env.sock()).await;
    handshake(&mut c2).await;
    write_frame(&mut c2, &ClientRequest::Subscribe)
        .await
        .unwrap();
    let events = read_events_until(&mut c2, EVENT_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::Snapshot { .. }))
    })
    .await;
    let ServerEvent::Snapshot { agents, .. } = &events[0] else {
        panic!("expected snapshot");
    };
    let agent = agents.iter().find(|a| a.id == agent_id).expect("agent row");
    assert_eq!(agent.name, "warm-agent");
    assert!(agent.alive, "adopted session is live");
    assert_eq!(
        agent.session_id.as_deref(),
        Some("warm-sid-99"),
        "buffered SessionStart replayed at adoption, saved at the first Stop"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A prewarmed CLI that dies immediately (the "claude/codex not installed"
/// shape) must not poison creation: CreateAgent quietly falls back to a
/// fresh spawn and still succeeds.
#[tokio::test]
async fn dead_prewarm_falls_back_to_cold_spawn() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let script = env.tmp.path().join("dying-agent.sh");
    let started = env.tmp.path().join("dying-agent-started");
    std::fs::write(
        &script,
        format!("#!/bin/sh\n: > '{}'\nexit 127\n", started.display()),
    )
    .unwrap();
    make_executable(&script);
    let mut daemon = env.spawn_daemon_with_agent_cmd(script.to_str().unwrap());

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;

    write_frame(
        &mut c,
        &ClientRequest::PrewarmAgent {
            worktree: worktree.id.clone(),
            kind: AgentKind::Claude,
            model: None,
            effort: None,
        },
    )
    .await
    .unwrap();
    wait_for_file(&started, "dying prewarm marker").await;

    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: "fallback-agent".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    assert!(
        matches!(
            find_ack(&events, 2),
            Some(ServerEvent::Ack {
                created: Some(EntityId::Agent(_)),
                ..
            })
        ),
        "CreateAgent must survive a dead prewarm: {events:#?}"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `kill -0` liveness probe (also true for an unreaped zombie).
/// An agent CLI that isn't installed must be refused up front. Before this
/// check the create "succeeded": the login shell printed `command not found`
/// into a PTY that died at once, leaving a dead session row indistinguishable
/// from a fresh one.
#[tokio::test]
async fn create_agent_refuses_when_the_cli_is_not_installed() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let shell = env.blind_shell();
    let mut daemon = env.spawn_daemon_with_shell(&shell);

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;

    for (req_id, kind, want) in [
        (10u64, AgentKind::Claude, "claude"),
        (11, AgentKind::Codex, "codex"),
        // Cursor's binary is `cursor-agent`; the message must name that, not
        // the kind, or the user goes looking for the wrong thing to install.
        (12, AgentKind::Cursor, "cursor-agent"),
        (13, AgentKind::Pi, "pi"),
    ] {
        write_frame(
            &mut c,
            &ClientRequest::CreateAgent {
                req_id,
                worktree: worktree.id.clone(),
                name: format!("agent-{req_id}"),
                kind,
                custom_harness: None,
                model: None,
                effort: None,
                auto_title: false,
                cloud_prompt: None,
                starting_prompt: None,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
            },
        )
        .await
        .unwrap();
        let events = read_events_until(&mut c, SPAWN_CHAIN_TIMEOUT, |evs| {
            evs.iter().any(|e| {
                matches!(e, ServerEvent::Error { req_id: Some(r), .. } if *r == req_id)
                    || matches!(e, ServerEvent::Ack { req_id: r, .. } if *r == req_id)
            })
        })
        .await;
        let message = events
            .iter()
            .find_map(|e| match e {
                ServerEvent::Error {
                    req_id: Some(r),
                    message,
                } if *r == req_id => Some(message.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{kind:?} create must be refused, got: {events:#?}"));
        assert!(
            message.starts_with(&format!("{want} was not found on your PATH")),
            "{kind:?}: {message}"
        );
    }

    // And no half-created rows left behind in the Sessions column.
    let events = subscribe(&mut c).await;
    let agents = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::Snapshot { agents, .. } => Some(agents.clone()),
            _ => None,
        })
        .expect("snapshot");
    assert!(agents.is_empty(), "refused creates left rows: {agents:#?}");

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// The mirror of the refusal: when the CLI *is* on the login shell's PATH the
/// check must stay out of the way — guards against the probe being so strict
/// (or so slow) that it blocks legitimate creates.
#[tokio::test]
async fn create_agent_succeeds_when_the_cli_is_on_the_login_shell_path() {
    let env = TestEnv::new();
    let repo = env.make_repo();

    // A stub `claude` that just sits there, reachable only via this shell.
    let bin = env.tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let stub = bin.join("claude");
    std::fs::write(&stub, "#!/bin/sh\nsleep 60\n").unwrap();
    make_executable(&stub);

    let shell = env.tmp.path().join("seeing-shell.sh");
    std::fs::write(
        &shell,
        format!(
            "#!/bin/sh\nPATH={}:/usr/bin:/bin\nexport PATH\nexec /bin/sh -c \"$4\"\n",
            bin.display()
        ),
    )
    .unwrap();
    make_executable(&shell);

    let mut daemon = env.spawn_daemon_with_shell(&shell);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;

    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 7,
            worktree: worktree.id.clone(),
            name: "real-agent".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, SPAWN_CHAIN_TIMEOUT, |evs| {
        find_ack(evs, 7).is_some()
            || evs.iter().any(|e| {
                matches!(
                    e,
                    ServerEvent::Error {
                        req_id: Some(7),
                        ..
                    }
                )
            })
    })
    .await;
    assert!(
        matches!(
            find_ack(&events, 7),
            Some(ServerEvent::Ack {
                created: Some(EntityId::Agent(_)),
                ..
            })
        ),
        "an installed CLI must still create: {events:#?}"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// With `orchestrator_remote_control` on, the orchestrator's Claude
/// launches with `--remote-control orchestrator` ahead of the rest of its
/// argv, and resumes with it too; a worker's launch never carries it.
#[tokio::test]
async fn the_orchestrator_launches_and_resumes_open_to_remote_control() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    env.write_config(r#"{"orchestrator_remote_control": true}"#);
    let boots = StubClaude::new(&env);
    let mut daemon = boots.spawn_daemon(&env);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: "orchestrator".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Orchestrator,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, SPAWN_CHAIN_TIMEOUT, |evs| {
        find_ack(evs, 2).is_some()
    })
    .await;
    let Some(ServerEvent::Ack {
        created: Some(EntityId::Agent(orchestrator)),
        ..
    }) = find_ack(&events, 2)
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let orchestrator = orchestrator.clone();
    let worker = create_agent_get_id(&mut c, &worktree.id, "worker", 3).await;

    // The test's data dir, `data`, makes this a dev instance.
    let launch = &boots.of(&orchestrator, 1).await[0];
    assert_eq!(
        launch[..2],
        ["--remote-control", "orchestrator-dev-data"],
        "{launch:?}"
    );
    let worker_launch = &boots.of(&worker, 1).await[0];
    assert!(
        !worker_launch.iter().any(|a| a == "--remote-control"),
        "a worker never opens to it: {worker_launch:?}"
    );

    // A turn saves the session id, so the restart resumes it.
    boots.run_turn(&orchestrator, "s1").await;
    status_seen(&mut c, &orchestrator, nebula_core::AgentStatus::Finished).await;
    write_frame(
        &mut c,
        &ClientRequest::RestartAgent {
            req_id: 4,
            id: orchestrator.clone(),
        },
    )
    .await
    .unwrap();
    let resume = &boots.of(&orchestrator, 2).await[1];
    assert_eq!(
        resume[..4],
        [
            "--remote-control",
            "orchestrator-dev-data",
            "--resume",
            "s1"
        ],
        "{resume:?}"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A SIDE CHAT (`ForkAgent`): a new session beside a Claude one, in its
/// checkout, whose first boot forks the conversation the session holds
/// (`--resume <id> --fork-session`) and whose later boots resume its own.
/// The session it forked keeps its conversation, and one with none yet is
/// refused.
#[tokio::test]
async fn a_side_chat_forks_the_sessions_conversation() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let boots = StubClaude::new(&env);
    let mut daemon = boots.spawn_daemon(&env);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let root = add_project_get_main_worktree(&mut c, &repo).await;
    let parent = create_agent_get_id(&mut c, &root.id, "worker", 2).await;
    boots.of(&parent, 1).await;

    let fork = |req_id: u64| ClientRequest::ForkAgent {
        req_id,
        id: parent.clone(),
    };
    write_frame(&mut c, &fork(3)).await.unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 3).is_some()).await;
    assert!(
        matches!(find_ack(&events, 3), Some(ServerEvent::Error { message, .. }) if message.contains("no conversation")),
        "nothing to fork before its first prompt: {events:?}"
    );

    boots.run_turn(&parent, "s1").await;
    status_seen(&mut c, &parent, nebula_core::AgentStatus::Finished).await;
    write_frame(&mut c, &fork(4)).await.unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 4).is_some()).await;
    let Some(ServerEvent::Ack {
        created: Some(EntityId::Agent(side)),
        ..
    }) = find_ack(&events, 4)
    else {
        panic!("the side chat was not created: {events:?}");
    };
    let side = side.clone();
    let first = &boots.of(&side, 1).await[0];
    let at = first.iter().position(|a| a == "--resume").unwrap();
    assert_eq!(first[at + 1], "s1", "{first:?}");
    assert!(first.iter().any(|a| a == "--fork-session"), "{first:?}");

    // Its own conversation from its first turn on: a restart resumes that.
    boots.run_turn(&side, "s2").await;
    status_seen(&mut c, &side, nebula_core::AgentStatus::Finished).await;
    write_frame(
        &mut c,
        &ClientRequest::RestartAgent {
            req_id: 5,
            id: side.clone(),
        },
    )
    .await
    .unwrap();
    let second = &boots.of(&side, 2).await[1];
    let at = second.iter().position(|a| a == "--resume").unwrap();
    assert_eq!(second[at + 1], "s2", "{second:?}");
    assert!(!second.iter().any(|a| a == "--fork-session"), "{second:?}");

    // The session it forked resumes its own, untouched.
    write_frame(
        &mut c,
        &ClientRequest::RestartAgent {
            req_id: 6,
            id: parent.clone(),
        },
    )
    .await
    .unwrap();
    let again = &boots.of(&parent, 2).await[1];
    let at = again.iter().position(|a| a == "--resume").unwrap();
    assert_eq!(again[at + 1], "s1", "{again:?}");
    assert!(!again.iter().any(|a| a == "--fork-session"), "{again:?}");

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A stand-in `claude`, reachable only through the login shell of the
/// daemon [`StubClaude::spawn_daemon`] starts: it logs its env and every
/// boot's argv, one argument a line, reports its SessionStart as Claude
/// does once it is up, then parks.
struct StubClaude {
    boots: PathBuf,
    shell: PathBuf,
    claude_dir: PathBuf,
}

impl StubClaude {
    fn new(env: &TestEnv) -> Self {
        let boots = env.tmp.path().join("boots");
        let bin = env.tmp.path().join("bin");
        std::fs::create_dir_all(&boots).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let stub = bin.join("claude");
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\nenv | grep '^NEBULA_' > '{d}'/$NEBULA_AGENT_ID.env\n\
                 {{ printf '%s\\n' \"$@\"; echo ---; }} >> '{d}'/$NEBULA_AGENT_ID.argv\n\
                 curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \
                 -H 'Content-Type: application/json' -d '{{}}' \
                 \"$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent=SessionStart\" \
                 >/dev/null 2>&1\n\
                 exec sleep 600\n",
                d = boots.display()
            ),
        )
        .unwrap();
        make_executable(&stub);
        let shell = env.tmp.path().join("seeing-shell.sh");
        std::fs::write(
            &shell,
            format!(
                "#!/bin/sh\nPATH={}:/usr/bin:/bin\nexport PATH\nexec /bin/sh -c \"$4\"\n",
                bin.display()
            ),
        )
        .unwrap();
        make_executable(&shell);
        Self {
            boots,
            shell,
            claude_dir: env.tmp.path().join("claude-config"),
        }
    }

    fn spawn_daemon(&self, env: &TestEnv) -> DaemonProc {
        env.spawn_daemon_in(
            &self.shell,
            None,
            &[(env::CLAUDE_CONFIG_DIR, self.claude_dir.to_str().unwrap())],
        )
    }

    /// The sessions that booted the stand-in so far.
    fn booted(&self) -> Vec<nebula_core::AgentId> {
        std::fs::read_dir(&self.boots)
            .unwrap()
            .filter_map(|e| {
                let name = e.unwrap().file_name().into_string().unwrap();
                name.strip_suffix(".argv")
                    .map(|id| nebula_core::AgentId(id.to_string()))
            })
            .collect()
    }

    /// Each boot's argv so far for `id`, once there are `n` of them, one line
    /// an item: an argument with lines in it is several.
    async fn of(&self, id: &nebula_core::AgentId, n: usize) -> Vec<Vec<String>> {
        let path = self.boots.join(format!("{}.argv", id.0));
        let deadline = tokio::time::Instant::now() + SPAWN_CHAIN_TIMEOUT;
        loop {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let boots: Vec<Vec<String>> = text
                .split_terminator("---\n")
                .map(|boot| boot.lines().map(str::to_string).collect())
                .collect();
            if boots.len() >= n {
                return boots;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "{n} boots never logged: {text}"
            );
            tokio::time::sleep(POLL_STEP).await;
        }
    }

    /// Post the hooks of one turn of `id`'s CLI on Claude session `sid`,
    /// which saves the id for a resume.
    async fn run_turn(&self, id: &nebula_core::AgentId, sid: &str) {
        let vars = read_env_file(&self.boots.join(format!("{}.env", id.0))).await;
        let port: u16 = vars[env::API_URL]
            .rsplit(':')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let payload = format!(r#"{{"session_id":"{sid}"}}"#);
        for event in ["SessionStart", "UserPromptSubmit", "Stop"] {
            let path = format!("/api/hooks/claude?agentId={}&hookEvent={event}", id.0);
            let (status, _) = hook_post_json(port, &path, &vars[env::API_TOKEN], &payload).await;
            assert_eq!(status, 200, "{event}");
        }
    }
}

/// `nebula orchestrator captain`, run by the orchestrator: the project's
/// captain is started in its root checkout, never out of the PREWARM POOL,
/// and the next call finds the same one. Its Claude launches and resumes
/// with the captain's guidance, without the editing tools, and with
/// `--add-dir` for each directory its project's space shares, as a
/// worker's in that space does. `nebula sessions` names its role. A
/// worker is refused, and a client older than captains sees the captain
/// as a worker.
#[tokio::test]
async fn nebula_orchestrator_captain_finds_or_starts_the_projects_captain() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let boots = StubClaude::new(&env);
    let mut daemon = boots.spawn_daemon(&env);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let root = add_project_get_main_worktree(&mut c, &repo).await;
    let home = std::env::var("HOME").unwrap();
    env.write_config(
        &serde_json::json!({
            "projects": { root.path.to_str().unwrap(): { "space": "Work" } },
            "spaces": { "work": { "add_dirs": ["~/notes", "/shared/docs"] } },
        })
        .to_string(),
    );
    let worker = create_agent_get_id(&mut c, &root.id, "worker", 2).await;
    let orchestrator = create_agent_as(
        &mut c,
        &root.id,
        "orchestrator",
        3,
        nebula_core::AgentRole::Orchestrator,
    )
    .await;
    let worker_launch = &boots.of(&worker, 1).await[0];
    assert_eq!(
        worker_launch[..2],
        [
            format!("--add-dir={home}/notes"),
            "--add-dir=/shared/docs".into()
        ],
        "{worker_launch:?}"
    );
    assert!(!worker_launch
        .iter()
        .any(|a| a.starts_with("--disallowedTools")));

    // A spare booted for the root checkout, which the captain must not take.
    write_frame(
        &mut c,
        &ClientRequest::PrewarmAgent {
            worktree: root.id.clone(),
            kind: AgentKind::Claude,
            model: None,
            effort: None,
        },
    )
    .await
    .unwrap();
    let deadline = tokio::time::Instant::now() + SLOW_TIMEOUT;
    let spare = loop {
        if let Some(spare) = boots
            .booted()
            .into_iter()
            .find(|id| *id != worker && *id != orchestrator)
        {
            break spare;
        }
        assert!(tokio::time::Instant::now() < deadline, "no spare booted");
        tokio::time::sleep(POLL_STEP).await;
    };

    let stderr = refusal(&agent_cli(
        &env,
        &worker,
        &["orchestrator", "captain", "repo"],
    ));
    assert!(stderr.contains("only available"), "stderr: {stderr}");

    let json = stdout_json(&agent_cli(
        &env,
        &orchestrator,
        &["orchestrator", "captain", "repo", "--json"],
    ));
    assert_eq!(json["name"], "captain");
    assert_eq!(json["worktree"], root.id.0.as_str(), "the root checkout");
    let captain = nebula_core::AgentId(json["id"].as_str().unwrap().to_string());
    assert_ne!(captain, spare, "never adopted from the prewarm pool");
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        agent_named(evs, "captain").is_some()
    })
    .await;
    assert_eq!(
        agent_named(&events, "captain").unwrap().role,
        nebula_core::AgentRole::Captain
    );

    let launch = &boots.of(&captain, 1).await[0];
    // Asks nothing and runs only what it is allowed, with edits nowhere in
    // its project's checkouts or its space's folders and no git write.
    let checkout = root.path.display().to_string();
    let edits = format!(
        "--disallowedTools=NotebookEdit,Edit(/{checkout}/**),Edit(/{checkout}-worktrees/**),\
         Edit(/{home}/notes/**),Edit(//shared/docs/**),"
    );
    let captain_flags = |argv: &[String]| {
        argv.len() > 2
            && argv[0] == "--permission-mode=dontAsk"
            && argv[1].starts_with("--allowedTools=Read,Grep,Glob,Bash(nebula spawn:*),")
            && argv[2].starts_with(&edits)
            && argv[2].contains(",Bash(git commit:*),")
    };
    assert!(captain_flags(launch), "{launch:?}");
    assert_eq!(
        launch[3..5],
        [
            format!("--add-dir={home}/notes"),
            "--add-dir=/shared/docs".into(),
        ],
        "{launch:?}"
    );
    let guided = |argv: &[String]| argv.join("\n").contains("You are this project's CAPTAIN");
    assert!(guided(launch), "{launch:?}");
    assert!(!guided(worker_launch));

    let out = agent_cli(&env, &orchestrator, &["orchestrator", "captain", "repo"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("the captain of repo is session {captain}\n"),
        "the same captain, found"
    );
    let out = env.cli().arg("sessions").output().unwrap();
    let listed = String::from_utf8_lossy(&out.stdout);
    let line = |id: &nebula_core::AgentId| {
        listed
            .lines()
            .find(|l| l.starts_with(&id.0))
            .unwrap_or_else(|| panic!("{id} not listed: {listed}"))
            .to_string()
    };
    assert!(line(&captain).contains(" role=captain "), "{listed}");
    assert!(line(&orchestrator).contains(" role=orchestrator "));
    assert!(!line(&worker).contains("role="));

    // Its resume carries the same flags.
    boots.run_turn(&captain, "c1").await;
    status_seen(&mut c, &captain, nebula_core::AgentStatus::Finished).await;
    write_frame(
        &mut c,
        &ClientRequest::RestartAgent {
            req_id: 4,
            id: captain.clone(),
        },
    )
    .await
    .unwrap();
    let resume = &boots.of(&captain, 2).await[1];
    assert!(captain_flags(resume), "{resume:?}");
    assert!(
        resume.windows(2).any(|w| w == ["--resume", "c1"]),
        "{resume:?}"
    );
    assert!(guided(resume));

    // A client from before captains is sent the captain as a worker.
    let mut old = connect(&env.sock()).await;
    write_frame(
        &mut old,
        &ClientRequest::Hello {
            protocol_version: nebula_core::CAPTAIN_PROTOCOL - 1,
        },
    )
    .await
    .unwrap();
    write_frame(&mut old, &ClientRequest::Subscribe)
        .await
        .unwrap();
    let events = read_events_until(&mut old, EVENT_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::Snapshot { .. }))
    })
    .await;
    let Some(ServerEvent::Snapshot { agents, .. }) = events
        .iter()
        .find(|e| matches!(e, ServerEvent::Snapshot { .. }))
    else {
        unreachable!()
    };
    let seen = agents.iter().find(|a| a.id == captain).unwrap();
    assert_eq!(seen.role, nebula_core::AgentRole::Worker);

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A stand-in `claude` that opens on Claude Code's question whether to
/// trust its folder, as Claude does in a folder it has no trust for, and
/// is past it once the test says the user answered: it clears the screen
/// and reports its SessionStart. Bracketed paste is on from the start, as
/// Claude turns it on before it asks. It reaches the daemon only through the login shell of
/// the daemon [`TrustStub::spawn_daemon`] starts. What is typed into it
/// once it is past the question lands in `typed`.
struct TrustStub {
    shell: PathBuf,
    answered: PathBuf,
    typed: PathBuf,
}

/// The question [`TrustStub::new`] opens on, as `printf` takes it.
const TRUST_SCREEN: &str = " Quick safety check: Is this a project you created or one you trust? \
     (Like\\r\\n your own code).\\r\\n\\r\\n \\342\\235\\257 No, exit\\r\\n   Yes, I trust \
     this folder\\r\\n";

impl TrustStub {
    fn new(env: &TestEnv) -> Self {
        Self::opening_on(env, TRUST_SCREEN)
    }

    /// The stand-in, opening on `screen` (as `printf` takes it) instead,
    /// still silent until the test says the user answered.
    fn opening_on(env: &TestEnv, screen: &str) -> Self {
        let bin = env.tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let answered = env.tmp.path().join("answered");
        let typed = env.tmp.path().join("typed");
        let stub = bin.join("claude");
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\nprintf '\\033[?2004h{screen}'\n\
                 while [ ! -e '{answered}' ]; do sleep 0.1; done\n\
                 printf '\\033[2J\\033[H\\033[?2004h> '\n\
                 curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \
                 -H 'Content-Type: application/json' -d '{{}}' \
                 \"$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent=SessionStart\" \
                 >/dev/null 2>&1\n\
                 stty raw -echo 2>/dev/null\n\
                 exec cat >> '{typed}'\n",
                answered = answered.display(),
                typed = typed.display(),
            ),
        )
        .unwrap();
        make_executable(&stub);
        let shell = env.tmp.path().join("seeing-shell.sh");
        std::fs::write(
            &shell,
            format!(
                "#!/bin/sh\nPATH={}:/usr/bin:/bin\nexport PATH\nexec /bin/sh -c \"$4\"\n",
                bin.display()
            ),
        )
        .unwrap();
        make_executable(&shell);
        Self {
            shell,
            answered,
            typed,
        }
    }

    fn spawn_daemon(&self, env: &TestEnv) -> DaemonProc {
        env.spawn_daemon_with_shell(&self.shell)
    }

    /// The user answers the question.
    fn answer(&self) {
        std::fs::write(&self.answered, b"").unwrap();
    }
}

/// Wait for `id`'s status to change to `want`. Each change these tests
/// wait on comes from a CLI booting through a login shell, some of them
/// right after a daemon restart, so they get a respawn chain's time.
async fn wait_status(
    c: &mut UnixStream,
    id: &nebula_core::AgentId,
    want: nebula_core::AgentStatus,
) {
    read_events_until(c, SPAWN_CHAIN_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status, .. }
                if agent == id && *status == want)
        })
    })
    .await;
}

/// A Claude session that opens on the question whether to trust its
/// folder reads as waiting on the user while it is up, not idle, and goes
/// back to what it was once Claude is past it.
#[tokio::test]
async fn a_claude_session_on_its_trust_question_needs_feedback() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let stub = TrustStub::new(&env);
    let mut daemon = stub.spawn_daemon(&env);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let id = create_agent_get_id(&mut c, &worktree.id, "untrusted", 2).await;
    wait_status(&mut c, &id, nebula_core::AgentStatus::NeedsFeedback).await;
    stub.answer();
    wait_status(&mut c, &id, nebula_core::AgentStatus::Fresh).await;

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `nebula session send` and `nebula orchestrator send` boot a session
/// with no live process, and when its Claude comes up on the question
/// whether to trust its folder they type nothing, failing with a message
/// that says the session is waiting on a dialog in its pane. Once Claude
/// is past the question, the text is typed.
#[tokio::test]
async fn both_sends_wait_for_claude_and_never_type_into_its_trust_question() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let stub = TrustStub::new(&env);
    let mut daemon = stub.spawn_daemon(&env);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let orchestrator = create_agent_as(
        &mut c,
        &worktree.id,
        "orchestrator",
        2,
        nebula_core::AgentRole::Orchestrator,
    )
    .await;
    let mine = create_agent_get_id(&mut c, &worktree.id, "mine", 3).await;
    let routed = create_agent_get_id(&mut c, &worktree.id, "routed", 4).await;
    // Both on the question, so the restart leaves both `disconnected`: one
    // still booting would come back `fresh`.
    read_events_until(&mut c, SPAWN_CHAIN_TIMEOUT, |evs| {
        [&mine, &routed].iter().all(|id| {
            evs.iter().any(|e| {
                matches!(e, ServerEvent::StatusChanged { agent, status, .. }
                    if agent == *id && *status == nebula_core::AgentStatus::NeedsFeedback)
            })
        })
    })
    .await;
    // A daemon restart leaves them with no process, so a send boots each.
    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
    let mut daemon = stub.spawn_daemon(&env);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;

    let text = "now add a test for it";
    let session_send = |id: &nebula_core::AgentId| {
        env.cli()
            .args(["session", "send", &id.0, text])
            .output()
            .unwrap()
    };
    let orchestrator_send = env
        .cli()
        .env(env::AGENT_ID, &orchestrator.0)
        .args(["orchestrator", "send", &routed.0, text])
        .output()
        .unwrap();
    for (out, said) in [
        (session_send(&mine), "nothing was typed"),
        (orchestrator_send, "nothing was sent"),
    ] {
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{stderr}");
        assert!(stderr.contains(said), "{stderr}");
        assert!(
            stderr.contains("waiting on a dialog in its pane"),
            "{stderr}"
        );
    }
    for id in [&mine, &routed] {
        let screen = env.cli().args(["session", "read", &id.0]).output().unwrap();
        let screen = String::from_utf8_lossy(&screen.stdout);
        assert!(screen.contains("trust this folder"), "{screen}");
        assert!(!screen.contains(text), "typed into the question: {screen}");
    }

    stub.answer();
    wait_status(&mut c, &mine, nebula_core::AgentStatus::Disconnected).await;
    let out = session_send(&mine);
    let said =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("typed the next turn"), "{said}");

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// The orchestrator and a captain launch on the Claude model and effort
/// Settings → Agents sets for new sessions, as the quick prompt does:
/// nothing else picks one for them.
#[tokio::test]
async fn the_orchestrator_and_a_captain_launch_on_the_configured_claude_model() {
    let env = TestEnv::new();
    env.write_config(r#"{"claude_model": "haiku", "claude_effort": "low"}"#);
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let orchestrator = create_agent_as(
        &mut c,
        &worktree.id,
        "orchestrator",
        2,
        nebula_core::AgentRole::Orchestrator,
    )
    .await;
    let json = stdout_json(&agent_cli(
        &env,
        &orchestrator,
        &["orchestrator", "captain", "repo", "--json"],
    ));
    let captain = nebula_core::AgentId(json["id"].as_str().unwrap().to_string());

    let mut seen = connect(&env.sock()).await;
    handshake(&mut seen).await;
    let events = subscribe(&mut seen).await;
    let Some(ServerEvent::Snapshot { agents, .. }) = events
        .iter()
        .find(|e| matches!(e, ServerEvent::Snapshot { .. }))
    else {
        unreachable!()
    };
    for id in [&orchestrator, &captain] {
        let row = agents.iter().find(|a| a.id == *id).unwrap();
        assert_eq!(
            (row.model.as_deref(), row.effort.as_deref()),
            (Some("haiku"), Some("low")),
            "{}",
            row.name
        );
    }

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A new captain opening on Claude's question whether to trust its
/// folder keeps `orchestrator captain` waiting, not failing, while the
/// question is up, and the command prints the captain once the user has
/// answered it, so the orchestrator's background `captain` goes on.
#[tokio::test]
async fn orchestrator_captain_waits_out_its_trust_question() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let stub = TrustStub::new(&env);
    let mut daemon = stub.spawn_daemon(&env);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let orchestrator = create_agent_as(
        &mut c,
        &worktree.id,
        "orchestrator",
        2,
        nebula_core::AgentRole::Orchestrator,
    )
    .await;

    let captain = env
        .cli()
        .env(env::AGENT_ID, &orchestrator.0)
        .args(["orchestrator", "captain", "repo"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let asking = |e: &ServerEvent| match e {
        ServerEvent::StatusChanged { agent, status, .. }
            if *status == nebula_core::AgentStatus::NeedsFeedback && *agent != orchestrator =>
        {
            Some(agent.clone())
        }
        _ => None,
    };
    let events = read_events_until(&mut c, SPAWN_CHAIN_TIMEOUT, |evs| {
        evs.iter().any(|e| asking(e).is_some())
    })
    .await;
    let asking = events.iter().find_map(asking).unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    stub.answer();
    let out = captain.wait_with_output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("the captain of repo is session {asking}\n")
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// An `orchestrator send` waits for the session's CLI to come up before
/// it types, and one whose client gave up meanwhile (the orchestrator's
/// command killed) types nothing once the CLI is up after all.
#[tokio::test]
async fn an_orchestrator_send_its_client_left_types_nothing() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let stub = TrustStub::opening_on(&env, " Starting\\r\\n");
    let mut daemon = stub.spawn_daemon(&env);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let orchestrator = create_agent_as(
        &mut c,
        &worktree.id,
        "orchestrator",
        2,
        nebula_core::AgentRole::Orchestrator,
    )
    .await;
    let routed = create_agent_get_id(&mut c, &worktree.id, "routed", 3).await;

    let text = "now add a test for it";
    let mut send = env
        .cli()
        .env(env::AGENT_ID, &orchestrator.0)
        .args(["orchestrator", "send", &routed.0, text])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        send.try_wait().unwrap().is_none(),
        "the send waits on the CLI"
    );
    send.kill().unwrap();
    send.wait().unwrap();

    stub.answer();
    let deadline = tokio::time::Instant::now() + SLOW_TIMEOUT;
    while !stub.typed.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the stand-in never came up"
        );
        tokio::time::sleep(POLL_STEP).await;
    }
    // Well past the daemon's 100 ms look and the Enter's 150 ms behind it.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let typed = std::fs::read_to_string(&stub.typed).unwrap();
    assert!(
        !typed.contains(text),
        "typed after its client left: {typed:?}"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

fn pid_alive(pid: i32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn wait_pid_dead(pid: i32, timeout: Duration, what: &str) {
    let deadline = tokio::time::Instant::now() + timeout;
    while pid_alive(pid) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what} (pid {pid}) still running after {timeout:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn create_agent_get_id(
    c: &mut UnixStream,
    worktree: &nebula_core::WorktreeId,
    name: &str,
    req_id: u64,
) -> nebula_core::AgentId {
    create_agent_as(c, worktree, name, req_id, nebula_core::AgentRole::Worker).await
}

/// [`create_agent_get_id`] for a session in `role`.
async fn create_agent_as(
    c: &mut UnixStream,
    worktree: &nebula_core::WorktreeId,
    name: &str,
    req_id: u64,
    role: nebula_core::AgentRole,
) -> nebula_core::AgentId {
    write_frame(
        c,
        &ClientRequest::CreateAgent {
            req_id,
            worktree: worktree.clone(),
            name: name.into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(c, EVENT_TIMEOUT, |evs| find_ack(evs, req_id).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(id)),
        ..
    } = find_ack(&events, req_id).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    id.clone()
}

/// Poll a pidfile the fake agent writes on boot.
async fn read_pidfile(path: &Path) -> i32 {
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        if let Ok(s) = std::fs::read_to_string(path) {
            if let Ok(pid) = s.trim().parse() {
                return pid;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "pidfile {path:?} never appeared"
        );
        tokio::time::sleep(POLL_STEP).await;
    }
}

/// Wait for a marker a PTY child writes once it is up: a spawn, so the
/// slow bound, which a busy machine needs.
async fn wait_for_file(path: &Path, what: &str) {
    let deadline = tokio::time::Instant::now() + SLOW_TIMEOUT;
    loop {
        if path.exists() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what} never appeared"
        );
        tokio::time::sleep(POLL_STEP).await;
    }
}

/// Archiving or deleting an agent must kill its CLI process — sessions must
/// not keep burning memory/CPU once the user has put them away.
#[tokio::test]
async fn archive_and_delete_kill_the_agent_process() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let pid_dir = env.tmp.path().join("pids");
    std::fs::create_dir_all(&pid_dir).unwrap();
    // Stand-in CLI: record the pid, then exec into a long sleep (same pid).
    let script = env.tmp.path().join("agent.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho $$ > '{}'/$NEBULA_AGENT_ID.pid\nexec sleep 600\n",
            pid_dir.display()
        ),
    )
    .unwrap();
    make_executable(&script);
    let mut daemon = env.spawn_daemon_with_agent_cmd(script.to_str().unwrap());

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;

    let a1 = create_agent_get_id(&mut c, &worktree.id, "to-archive", 2).await;
    let a2 = create_agent_get_id(&mut c, &worktree.id, "to-delete", 3).await;
    let pid1 = read_pidfile(&pid_dir.join(format!("{}.pid", a1.0))).await;
    let pid2 = read_pidfile(&pid_dir.join(format!("{}.pid", a2.0))).await;
    assert!(pid_alive(pid1) && pid_alive(pid2), "fake CLIs should be up");

    // ---- archive kills the CLI and broadcasts archived + not-alive ----
    write_frame(
        &mut c,
        &ClientRequest::ArchiveAgent {
            req_id: 4,
            id: a1.clone(),
        },
    )
    .await
    .unwrap();
    // The Ack and the EntityUpserted broadcast race on the client stream —
    // wait for both.
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        find_ack(evs, 4).is_some()
            && evs.iter().any(|e| {
                matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                    if a.id == a1 && a.archived)
            })
    })
    .await;
    let archived = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Agent(a),
            } if a.id == a1 && a.archived => Some(a.clone()),
            _ => None,
        })
        .expect("archive upsert");
    assert!(
        !archived.alive,
        "archived agent should not be alive: {archived:?}"
    );
    wait_pid_dead(pid1, EVENT_TIMEOUT, "archived agent CLI").await;
    assert!(pid_alive(pid2), "the other agent must be untouched");

    // ---- delete kills the CLI too ----
    write_frame(
        &mut c,
        &ClientRequest::DeleteAgent {
            req_id: 5,
            id: a2.clone(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        find_ack(evs, 5).is_some()
            && evs
                .iter()
                .any(|e| matches!(e, ServerEvent::EntityRemoved { id: EntityId::Agent(id) } if *id == a2))
    })
    .await;
    wait_pid_dead(pid2, EVENT_TIMEOUT, "deleted agent CLI").await;

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A wedged CLI that ignores SIGHUP still gets cleared on archive: the kill
/// watchdog SIGKILLs its whole process group (grandchildren included) after
/// the grace period.
#[tokio::test]
async fn archive_sigkills_an_agent_that_ignores_sighup() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let dir = env.tmp.path().to_path_buf();
    // HUP-immune stand-in with a background child; `trap '' HUP` is inherited
    // by `sleep`, so neither dies from the polite signal alone.
    let script = dir.join("stubborn-agent.sh");
    std::fs::write(
        &script,
        format!(
            concat!(
                "#!/bin/sh\n",
                "trap '' HUP\n",
                "echo $$ > '{d}/agent.pid'\n",
                "sleep 600 &\n",
                "echo $! > '{d}/child.pid'\n",
                "wait\n",
            ),
            d = dir.display()
        ),
    )
    .unwrap();
    make_executable(&script);
    let mut daemon = env.spawn_daemon_with_agent_cmd(script.to_str().unwrap());

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;

    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: "stubborn".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(agent_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let agent_id = agent_id.clone();
    let shell_pid = read_pidfile(&dir.join("agent.pid")).await;
    let child_pid = read_pidfile(&dir.join("child.pid")).await;

    write_frame(
        &mut c,
        &ClientRequest::ArchiveAgent {
            req_id: 3,
            id: agent_id,
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 3).is_some()).await;
    // SIGHUP alone can't clear these; the ~3s watchdog escalation must.
    wait_pid_dead(shell_pid, SLOW_TIMEOUT, "HUP-immune agent CLI").await;
    wait_pid_dead(child_pid, SLOW_TIMEOUT, "agent CLI's grandchild").await;

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// The launch resolves the CLI through the login shell the way a typed
/// command would, so a `claude` the rc files reroute — an alias, a function
/// — is what runs, not the binary on PATH behind it. That shell then keeps
/// the agent as a job in a process group of its own, so archiving has to
/// reach past the shell: the polite SIGHUP takes the shell alone (a
/// non-interactive one forwards nothing) and reparents the job to init, and
/// only a sweep taken before the signal still knows which group to hang up
/// — and, for one that shrugs that off too, to SIGKILL.
#[tokio::test]
async fn create_agent_runs_the_shells_own_claude_and_archive_clears_its_job() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let dir = env.tmp.path().to_path_buf();
    // The rc file: job control on, and a `claude` function that records its
    // argv and parks an HUP-immune job in a group of its own.
    let rc = dir.join("rc.sh");
    std::fs::write(
        &rc,
        format!(
            concat!(
                "set -m\n",
                "claude() {{\n",
                "  echo \"routed $*\" > '{d}/routed'\n",
                "  sh -c 'trap \"\" HUP; echo $$ > \"{d}/job.pid\"; sleep 600' &\n",
                "  wait\n",
                "}}\n",
            ),
            d = dir.display()
        ),
    )
    .unwrap();
    // A `$SHELL` that sources it ahead of the `-c` string, as `-l -i` would
    // source ~/.zshrc.
    let shell = dir.join("routing-shell.sh");
    std::fs::write(
        &shell,
        format!(
            "#!/bin/sh\nPATH=/usr/bin:/bin\nexport PATH\nexec /bin/bash -c \". '{}'; $4\"\n",
            rc.display()
        ),
    )
    .unwrap();
    make_executable(&shell);
    let mut daemon = env.spawn_daemon_with_shell(&shell);

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent_id = create_agent_get_id(&mut c, &worktree.id, "routed", 2).await;

    // The function ran, with the CLI's argv, in place of any binary.
    let job_pid = read_pidfile(&dir.join("job.pid")).await;
    let routed = std::fs::read_to_string(dir.join("routed")).unwrap();
    assert!(
        routed.starts_with("routed --append-system-prompt "),
        "{routed:?}"
    );
    assert!(pid_alive(job_pid), "the agent's job should be up");

    write_frame(
        &mut c,
        &ClientRequest::ArchiveAgent {
            req_id: 3,
            id: agent_id,
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 3).is_some()).await;
    // SIGHUP ends the shell and orphans the job; the watchdog's sweep must
    // still clear it once the grace period is up.
    wait_pid_dead(job_pid, SLOW_TIMEOUT, "agent job the shell left behind").await;

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// Some shell tools (Iris is the real report) autostart from .zshrc/.bashrc
/// by checking only "interactive shell + stdin is a TTY" and then `exec`ing
/// themselves. A `$SHELL -l -i -c <agent>` whose stdin is the session PTY
/// never reaches `<agent>`. Nebula keeps the login shell interactive for rc
/// files, but makes stdin non-TTY until the launch line itself reopens the
/// PTY, so aliases/functions from rc still work and the agent gets a real
/// terminal — by the PTY's own device path, not `/dev/tty`: macOS kqueue
/// refuses `/dev/tty`, and Claude Code (Bun) dies on its stdin watcher with
/// `EINVAL: invalid argument, kqueue` the moment it starts.
#[tokio::test]
async fn create_agent_survives_rc_autostart_that_execs_on_tty_stdin() {
    let launch = launch_claude_through_rc("").await;
    assert!(
        launch.ran.starts_with("claude tty --append-system-prompt "),
        "{:?}",
        launch.ran
    );
    assert!(
        !launch.iris_took_over,
        "the rc autostart hook should not have taken over the PTY"
    );
    assert_ne!(
        launch.stdin.trim_end(),
        "/dev/tty",
        "stdin must be the PTY's own device, which kqueue can watch"
    );
    assert_eq!(
        launch.pane_tty.trim_end(),
        "unset",
        "the launch line unsets the PTY's name before the agent runs"
    );
}

/// An rc file that clears `NEBULA_PANE_TTY` must not leave the agent on the
/// `/dev/null` the rc files ran with: the launch line falls back to
/// `/dev/tty` instead of failing on `exec <""`.
#[tokio::test]
async fn create_agent_gets_a_tty_when_rc_clears_the_pane_tty() {
    let launch = launch_claude_through_rc("unset NEBULA_PANE_TTY\n").await;
    assert!(
        launch.ran.starts_with("claude tty --append-system-prompt "),
        "{:?}",
        launch.ran
    );
    assert!(!launch.iris_took_over);
}

/// What the rc-defined `claude` saw when [`launch_claude_through_rc`] ran it.
struct RcLaunch {
    /// `claude <tty|notty> <args>`.
    ran: String,
    /// `tty`'s answer for its stdin.
    stdin: String,
    /// `NEBULA_PANE_TTY` as it saw it, or `unset`.
    pane_tty: String,
    /// The Iris-style autostart ran instead.
    iris_took_over: bool,
}

/// Create a Claude agent through a login shell whose rc file has an
/// Iris-style autostart (it `exec`s when interactive with a TTY stdin),
/// then `extra_rc`, then a `claude` function that records its stdin.
async fn launch_claude_through_rc(extra_rc: &str) -> RcLaunch {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let dir = env.tmp.path().to_path_buf();

    let rc = dir.join("rc.sh");
    std::fs::write(
        &rc,
        format!(
            concat!(
                "case $- in *i*) interactive=1;; *) interactive=0;; esac\n",
                "if [ \"$interactive\" = 1 ] && [ -t 0 ]; then\n",
                "  exec sh -c 'echo iris-autostarted > \"{d}/iris\"; sleep 600'\n",
                "fi\n",
                "{extra}",
                "claude() {{\n",
                "  if [ -t 0 ]; then tty=tty; else tty=notty; fi\n",
                "  tty > '{d}/claude-stdin'\n",
                "  echo \"${{NEBULA_PANE_TTY-unset}}\" > '{d}/claude-pane-tty'\n",
                "  echo \"claude $tty $*\" > '{d}/claude-ran'\n",
                "  sleep 600\n",
                "}}\n",
            ),
            d = dir.display(),
            extra = extra_rc,
        ),
    )
    .unwrap();
    let shell = dir.join("iris-shell.sh");
    std::fs::write(
        &shell,
        format!(
            "#!/bin/sh\nPATH=/usr/bin:/bin\nexport PATH\nexec /bin/bash -i -c \". '{}'; $4\"\n",
            rc.display()
        ),
    )
    .unwrap();
    make_executable(&shell);

    let mut daemon = env.spawn_daemon_with_shell(&shell);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent_id = create_agent_get_id(&mut c, &worktree.id, "iris-safe", 2).await;

    let marker = dir.join("claude-ran");
    tokio::time::timeout(SLOW_TIMEOUT, async {
        loop {
            if marker.exists() {
                break;
            }
            tokio::time::sleep(POLL_STEP).await;
        }
    })
    .await
    .expect("the rc-defined claude function should run");
    let launch = RcLaunch {
        ran: std::fs::read_to_string(&marker).unwrap(),
        stdin: std::fs::read_to_string(dir.join("claude-stdin")).unwrap(),
        pane_tty: std::fs::read_to_string(dir.join("claude-pane-tty")).unwrap(),
        iris_took_over: dir.join("iris").exists(),
    };

    write_frame(
        &mut c,
        &ClientRequest::ArchiveAgent {
            req_id: 3,
            id: agent_id,
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 3).is_some()).await;

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
    launch
}

/// One PrewarmWorktreeSessions must revive every dead session under the
/// worktree — no Attach involved — so the TUI can boot a worktree's
/// sessions the moment the user's selection rests on it. Archived agents
/// stay dead.
#[tokio::test]
async fn prewarm_worktree_sessions_boots_dead_sessions() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;

    // One agent, one terminal, one archived agent — every PTY dies with the
    // daemon below.
    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: "warmed".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(agent_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let agent_id = agent_id.clone();

    write_frame(
        &mut c,
        &ClientRequest::CreateTerminal {
            req_id: 3,
            worktree: worktree.id.clone(),
            name: None,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 3).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Terminal(term_id)),
        ..
    } = find_ack(&events, 3).unwrap()
    else {
        panic!("CreateTerminal failed: {events:#?}");
    };
    let term_id = term_id.clone();

    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 4,
            worktree: worktree.id.clone(),
            name: "shelved".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 4).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(archived_id)),
        ..
    } = find_ack(&events, 4).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let archived_id = archived_id.clone();
    write_frame(
        &mut c,
        &ClientRequest::ArchiveAgent {
            req_id: 5,
            id: archived_id.clone(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 5).is_some()).await;

    // Restart: rows persist, every PTY is dead.
    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
    let mut daemon2 = env.spawn_daemon();
    let mut c2 = connect(&env.sock()).await;
    handshake(&mut c2).await;
    write_frame(&mut c2, &ClientRequest::Subscribe)
        .await
        .unwrap();
    let events = read_events_until(&mut c2, EVENT_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::Snapshot { .. }))
    })
    .await;
    let ServerEvent::Snapshot {
        agents, terminals, ..
    } = &events[0]
    else {
        panic!("expected snapshot");
    };
    assert!(agents.iter().all(|a| !a.alive), "agents dead after restart");
    assert!(
        terminals.iter().all(|t| !t.alive),
        "terminals dead after restart"
    );

    // One prewarm revives the agent and the terminal (upserts flip alive)…
    write_frame(
        &mut c2,
        &ClientRequest::PrewarmWorktreeSessions {
            worktree: worktree.id.clone(),
            cols: 80,
            rows: 24,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c2, SLOW_TIMEOUT, |evs| {
        let agent_alive = evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id && a.alive)
        });
        let term_alive = evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Terminal(t) }
                if t.id == term_id && t.alive)
        });
        agent_alive && term_alive
    })
    .await;
    // …and never touches the archived agent.
    assert!(
        !events.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == archived_id && a.alive)
        }),
        "archived agent stayed dead: {events:#?}"
    );

    write_frame(&mut c2, &ClientRequest::Shutdown)
        .await
        .unwrap();
    wait_for_exit(&mut daemon2);
}

/// Switched off — `prewarm_sessions: false`, for a machine with less
/// memory to spare — the same request boots nothing: the worktree the
/// selection rests on stays cold, and a session forks only when the user
/// lands on it, whose Attach still revives that one row alone. On by
/// default, so a fresh install feels instant.
#[tokio::test]
async fn prewarm_worktree_sessions_boots_nothing_when_switched_off() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    env.write_config(r#"{"prewarm_sessions": false}"#);
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;

    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: "cold".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(agent_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let agent_id = agent_id.clone();

    write_frame(
        &mut c,
        &ClientRequest::CreateTerminal {
            req_id: 3,
            worktree: worktree.id.clone(),
            name: None,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 3).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Terminal(term_id)),
        ..
    } = find_ack(&events, 3).unwrap()
    else {
        panic!("CreateTerminal failed: {events:#?}");
    };
    let term_id = term_id.clone();

    // Restart: rows persist, every PTY is dead.
    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
    let mut daemon2 = env.spawn_daemon();
    let mut c2 = connect(&env.sock()).await;
    handshake(&mut c2).await;
    write_frame(&mut c2, &ClientRequest::Subscribe)
        .await
        .unwrap();
    let events = read_events_until(&mut c2, EVENT_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::Snapshot { .. }))
    })
    .await;
    let ServerEvent::Snapshot {
        agents, terminals, ..
    } = &events[0]
    else {
        panic!("expected snapshot");
    };
    assert!(agents.iter().all(|a| !a.alive), "agents dead after restart");
    assert!(
        terminals.iter().all(|t| !t.alive),
        "terminals dead after restart"
    );

    let agent_alive = |evs: &[ServerEvent]| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id && a.alive)
        })
    };
    let term_alive = |evs: &[ServerEvent]| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Terminal(t) }
                if t.id == term_id && t.alive)
        })
    };

    // The prewarm the TUI sends as the selection rests on the worktree
    // boots nothing. A running sweep forks its first session at once (the
    // stagger sits between boots), so a quiet window is a real absence.
    write_frame(
        &mut c2,
        &ClientRequest::PrewarmWorktreeSessions {
            worktree: worktree.id.clone(),
            cols: 80,
            rows: 24,
        },
    )
    .await
    .unwrap();
    let events = read_events_for(&mut c2, Duration::from_secs(2)).await;
    assert!(
        !agent_alive(&events) && !term_alive(&events),
        "prewarm switched off must boot nothing: {events:#?}"
    );

    // Landing on one session still boots that session — and only it.
    write_frame(
        &mut c2,
        &ClientRequest::Attach {
            session: SessionRef::Agent(agent_id.clone()),
            from_seq: None,
            cols: 80,
            rows: 24,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c2, SLOW_TIMEOUT, |evs| agent_alive(evs)).await;
    assert!(
        !term_alive(&events),
        "an explicit attach revives one row, not the worktree: {events:#?}"
    );

    write_frame(&mut c2, &ClientRequest::Shutdown)
        .await
        .unwrap();
    wait_for_exit(&mut daemon2);
}

/// The idle reaper kills sessions in worktrees no client is looking at once
/// they age past `session_idle_timeout` — but spares terminals with a
/// command still running, and never touches an attached session no matter
/// how long it idles. A reaped agent revives on the next attach.
#[tokio::test]
async fn idle_sessions_reap_unwatched_but_spare_busy_and_attached() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    env.write_config(r#"{"session_idle_timeout": "2s"}"#);
    let mut daemon = env.spawn_daemon_with("/bin/sh", &[(env::IDLE_REAP_MS, "200")]);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;

    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: "idler".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(agent_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let agent_id = agent_id.clone();

    write_frame(
        &mut c,
        &ClientRequest::CreateTerminal {
            req_id: 3,
            worktree: worktree.id.clone(),
            name: None,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 3).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Terminal(term_id)),
        ..
    } = find_ack(&events, 3).unwrap()
    else {
        panic!("CreateTerminal failed: {events:#?}");
    };
    let term_id = term_id.clone();

    // Give the terminal a running command, then stop looking at anything.
    let term_sref = SessionRef::Terminal(term_id.clone());
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: term_sref.clone(),
            from_seq: None,
            cols: 80,
            rows: 24,
        },
    )
    .await
    .unwrap();
    write_frame(
        &mut c,
        &ClientRequest::Input {
            session: term_sref.clone(),
            data: b"sleep 30\n".to_vec(),
        },
    )
    .await
    .unwrap();
    write_frame(
        &mut c,
        &ClientRequest::Detach {
            session: term_sref.clone(),
        },
    )
    .await
    .unwrap();

    // Unwatched: the idle agent is reaped after ~2s…
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id && !a.alive)
        })
    })
    .await;
    // …while the terminal's sleep keeps it alive.
    let term_reaped = |evs: &[ServerEvent]| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Terminal(t) }
                if t.id == term_id && !t.alive)
        })
    };
    assert!(!term_reaped(&events), "busy terminal spared: {events:#?}");

    // Attaching revives the agent; an attached session then idles forever.
    let agent_sref = SessionRef::Agent(agent_id.clone());
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: agent_sref.clone(),
            from_seq: None,
            cols: 80,
            rows: 24,
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id && a.alive)
        })
    })
    .await;
    // Well past the 2s timeout with sweeps every 200ms.
    tokio::time::sleep(Duration::from_secs(4)).await;
    write_frame(
        &mut c,
        &ClientRequest::RenameAgent {
            req_id: 6,
            id: agent_id.clone(),
            name: "still-here".into(),
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 6).is_some()).await;
    assert!(
        !events.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id && !a.alive)
        }),
        "attached agent never reaped: {events:#?}"
    );
    assert!(
        !term_reaped(&events),
        "in-view terminal spared: {events:#?}"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// The idle reaper never takes a session with a role of its own: the
/// orchestrator, idle and unwatched longer than a worker the reaper takes,
/// keeps its process.
#[tokio::test]
async fn idle_reaper_spares_the_orchestrator() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    env.write_config(r#"{"session_idle_timeout": "2s"}"#);
    let mut daemon = env.spawn_daemon_with("/bin/sh", &[(env::IDLE_REAP_MS, "200")]);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    // The orchestrator first, so it has idled longer than the worker by
    // the time the worker is reaped.
    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: "orchestrator".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Orchestrator,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let Some(ServerEvent::Ack {
        created: Some(EntityId::Agent(orchestrator)),
        ..
    }) = find_ack(&events, 2)
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let orchestrator = orchestrator.clone();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let worker = create_agent_get_id(&mut c, &worktree.id, "worker", 3).await;

    let reaped = |evs: &[ServerEvent], id: &nebula_core::AgentId| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if &a.id == id && !a.alive)
        })
    };
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| reaped(evs, &worker)).await;
    assert!(
        !reaped(&events, &orchestrator),
        "the orchestrator was reaped: {events:#?}"
    );
    let sessions = sessions_on(&env, "main").await.unwrap();
    let row = sessions.iter().find(|a| a.id == orchestrator).unwrap();
    assert!(row.alive, "the orchestrator keeps its process");

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A finished agent whose backgrounded tool call is still running — a job
/// it detached from its terminal, the way Claude Code runs a
/// `run_in_background` Bash call or a Monitor watch — is spared by the idle
/// reaper until that job ends, and then gets the full timeout over again.
/// A child the agent keeps inside its own terminal session (an MCP server)
/// does not count: that agent is reaped on schedule (#78).
#[tokio::test]
async fn idle_agent_with_a_detached_job_is_spared_until_it_ends() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    env.write_config(r#"{"session_idle_timeout": "2s"}"#);
    let mut daemon = env.spawn_daemon_with("/bin/sh", &[(env::IDLE_REAP_MS, "200")]);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let worker = create_agent_get_id(&mut c, &worktree.id, "worker", 2).await;
    let resident = create_agent_get_id(&mut c, &worktree.id, "resident", 3).await;

    // Each stand-in agent (`/bin/sh`) gets a child, then nobody looks at
    // either. The worker's is a job in a session of its own, as Claude
    // spawns a backgrounded Bash call; the resident's is an ordinary child
    // inside its session, the shape of an MCP server.
    let started = tokio::time::Instant::now();
    for (id, line) in [
        (
            &worker,
            "python3 -c 'import subprocess; subprocess.run([\"sleep\", \"5\"], start_new_session=True)'\n",
        ),
        (&resident, "sleep 30\n"),
    ] {
        let session = SessionRef::Agent(id.clone());
        write_frame(
            &mut c,
            &ClientRequest::Attach {
                session: session.clone(),
                from_seq: None,
                cols: 80,
                rows: 24,
            },
        )
        .await
        .unwrap();
        write_frame(
            &mut c,
            &ClientRequest::Input {
                session: session.clone(),
                data: line.as_bytes().to_vec(),
            },
        )
        .await
        .unwrap();
        write_frame(&mut c, &ClientRequest::Detach { session })
            .await
            .unwrap();
    }
    let reaped = |evs: &[ServerEvent], id: &nebula_core::AgentId| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if &a.id == id && !a.alive)
        })
    };

    // The resident goes after ~2s; the worker's job keeps it alive.
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| reaped(evs, &resident)).await;
    assert!(
        !reaped(&events, &worker),
        "worker spared while its job runs: {events:#?}"
    );

    // The job ends at ~5s, and the worker gets grace after that — not on
    // the next sweep. The reaper only notices a detached job's end on its
    // 200 ms beat, so keep this below the ideal 7 s while still proving the
    // timeout did not expire while the job was alive.
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| reaped(evs, &worker)).await;
    assert!(
        started.elapsed() >= Duration::from_secs(6),
        "reaped {:?} after the job started; the timeout restarts when it ends",
        started.elapsed()
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

fn wait_for_exit(daemon: &mut DaemonProc) {
    let deadline = std::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        match daemon.try_wait().unwrap() {
            Some(status) => {
                assert!(status.success(), "daemon exited with {status:?}");
                return;
            }
            None if std::time::Instant::now() < deadline => std::thread::sleep(POLL_STEP),
            None => {
                let _ = daemon.kill();
                panic!("daemon did not exit after Shutdown");
            }
        }
    }
}

/// What a stand-in wrote to `path`, once it has written a line there.
async fn read_line_from(path: &Path, what: &str) -> String {
    let deadline = tokio::time::Instant::now() + SLOW_TIMEOUT;
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if text.ends_with('\n') {
            return text.trim_end().to_string();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what} never written"
        );
        tokio::time::sleep(POLL_STEP).await;
    }
}

/// An installed `nebula` that rc files put first on PATH, as one adding
/// `~/.local/bin` does: the directory it is in.
fn installed_nebula(env: &TestEnv) -> PathBuf {
    let installed = env.tmp.path().join("installed");
    std::fs::create_dir_all(&installed).unwrap();
    let nebula = installed.join("nebula");
    std::fs::write(&nebula, "#!/bin/sh\necho installed\n").unwrap();
    make_executable(&nebula);
    installed
}

/// The SESSION BIN DIR's `nebula`, checked to be a symlink to the
/// daemon's own binary once a session has found it.
fn own_nebula(env: &TestEnv) -> PathBuf {
    env.runtime_dir.join("bin").join("nebula")
}

fn assert_is_the_daemons_binary(own: &Path) {
    assert_eq!(
        std::fs::canonicalize(own).unwrap(),
        std::fs::canonicalize(env!("CARGO_BIN_EXE_nebula")).unwrap(),
        "the daemon's own binary"
    );
}

/// An agent in a session runs its own daemon's `nebula`, not an installed
/// one the login shell's profile puts first: the CLI itself, and Claude's
/// tool shell, which rebuilds PATH from the rc files and then sources the
/// `CLAUDE_ENV_FILE` nebula hands it.
#[tokio::test]
async fn an_agent_runs_its_own_daemons_nebula() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let installed = installed_nebula(&env);
    let bin = env.tmp.path().join("bin");
    let seen = env.tmp.path().join("seen");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&seen).unwrap();
    let claude = bin.join("claude");
    std::fs::write(
        &claude,
        format!(
            "#!/bin/sh\ncommand -v nebula > '{seen}/agent'\n\
             PATH='{installed}':/usr/bin:/bin\n. \"$CLAUDE_ENV_FILE\"\n\
             command -v nebula > '{seen}/tool-shell'\nexec sleep 600\n",
            seen = seen.display(),
            installed = installed.display()
        ),
    )
    .unwrap();
    make_executable(&claude);
    let shell = env.tmp.path().join("profile-shell.sh");
    std::fs::write(
        &shell,
        format!(
            "#!/bin/sh\nPATH='{}':'{}':/usr/bin:/bin\nexport PATH\nexec /bin/sh -c \"$4\"\n",
            installed.display(),
            bin.display()
        ),
    )
    .unwrap();
    make_executable(&shell);
    let _daemon = env.spawn_daemon_with_shell(&shell);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    create_agent_get_id(&mut c, &worktree.id, "agent-1", 2).await;

    let own = own_nebula(&env);
    let agent = read_line_from(&seen.join("agent"), "the agent's nebula").await;
    assert_eq!(Path::new(&agent), own);
    let tool = read_line_from(&seen.join("tool-shell"), "the tool shell's nebula").await;
    assert_eq!(Path::new(&tool), own);
    assert_is_the_daemons_binary(&own);
}

/// A plain terminal's shell starts on the environment the daemon has,
/// untouched: no `ZDOTDIR` of nebula's, the user's own one as it was, no
/// `CLAUDE_ENV_FILE` and the inherited PATH, so a zsh reads its startup
/// files exactly as it would outside nebula. The stand-in is named `zsh`,
/// the shell nebula once started on a `ZDOTDIR` of its own. A daemon
/// started from inside another instance's session inherits that one's
/// `CLAUDE_ENV_FILE` and SESSION BIN DIR, and the terminal gets neither.
#[tokio::test]
async fn a_plain_terminal_starts_the_users_shell_untouched() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let seen = env.tmp.path().join("seen");
    let zdotdir = env.tmp.path().join("zdotdir");
    let shells = env.tmp.path().join("shells");
    let foreign = env.tmp.path().join("other-instance");
    for dir in [&seen, &zdotdir, &shells, &foreign.join("bin")] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let foreign_env_file = foreign.join("claude-env.sh");
    std::fs::write(&foreign_env_file, "").unwrap();
    std::fs::write(foreign.join("bin").join("nebula"), "").unwrap();
    let inherited = "/usr/bin:/bin";
    let daemon_path = format!("{}:{inherited}", foreign.join("bin").display());
    let zsh = shells.join("zsh");
    std::fs::write(
        &zsh,
        format!(
            "#!/bin/sh\n{{ echo \"$*\"; echo \"${{ZDOTDIR-unset}}\"; \
             echo \"${{NEBULA_ZDOTDIR-unset}}\"; echo \"${{CLAUDE_ENV_FILE-unset}}\"; \
             echo \"$PATH\"; }} > '{seen}/terminal.tmp'\n\
             mv '{seen}/terminal.tmp' '{seen}/terminal'\nexec sleep 600\n",
            seen = seen.display()
        ),
    )
    .unwrap();
    make_executable(&zsh);
    let _daemon = env.spawn_daemon_in(
        &zsh,
        Some("/bin/sh"),
        &[
            ("ZDOTDIR", zdotdir.to_str().unwrap()),
            (env::CLAUDE_ENV_FILE, foreign_env_file.to_str().unwrap()),
            ("PATH", &daemon_path),
        ],
    );
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    write_frame(
        &mut c,
        &ClientRequest::CreateTerminal {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: None,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    assert!(
        matches!(
            find_ack(&events, 2),
            Some(ServerEvent::Ack {
                created: Some(EntityId::Terminal(_)),
                ..
            })
        ),
        "CreateTerminal failed: {events:#?}"
    );

    let answer = seen.join("terminal");
    let deadline = tokio::time::Instant::now() + SLOW_TIMEOUT;
    while !answer.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the terminal's shell never started"
        );
        tokio::time::sleep(POLL_STEP).await;
    }
    let text = std::fs::read_to_string(&answer).unwrap();
    let want = format!("-l\n{}\nunset\nunset\n{inherited}\n", zdotdir.display());
    assert_eq!(text, want, "the shell's own environment");
}

/// `nebula reload` onto a binary somewhere else repoints the SESSION BIN
/// DIR's `nebula` at it, so the sessions the restart carries run the build
/// their daemon now runs.
#[tokio::test]
async fn a_reload_onto_another_binary_repoints_the_sessions_nebula() {
    let env = TestEnv::new();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let own = own_nebula(&env);
    assert_is_the_daemons_binary(&own);

    let moved = env.tmp.path().join("moved");
    std::fs::create_dir_all(&moved).unwrap();
    let moved = moved.join("nebula");
    std::fs::copy(env!("CARGO_BIN_EXE_nebula"), &moved).unwrap();
    let out = std::process::Command::new(&moved)
        .env(env::RUNTIME_DIR, &env.runtime_dir)
        .env(env::DATA_DIR, env.tmp.path().join("data"))
        .env_remove(env::CONFIG_FILE)
        .arg("reload")
        .output()
        .unwrap();
    assert!(out.status.success(), "reload failed: {out:?}");
    read_until_closed(&mut c).await;

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    assert_eq!(
        std::fs::canonicalize(&own).unwrap(),
        std::fs::canonicalize(&moved).unwrap(),
        "the binary the daemon restarted onto"
    );
    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A daemon started from inside a Claude Code session (`nebula` typed in
/// an agent's tool shell) must not hand that session's identity to the
/// agents it starts: an inherited `CLAUDE_CODE_CHILD_SESSION` turns the
/// agent's own transcript off, and Claude Code's `GIT_EDITOR=true` breaks a
/// bare `git commit`. A user setting under the same prefix still comes
/// through.
#[tokio::test]
async fn agents_never_inherit_the_claude_session_the_daemon_started_in() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let env_dir = env.tmp.path().join("agent-env");
    std::fs::create_dir_all(&env_dir).unwrap();
    let script = env.tmp.path().join("agent.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nenv | grep -E '^(NEBULA_|CLAUDE|AI_AGENT|GIT_EDITOR|COREPACK_)' > '{}'/$NEBULA_AGENT_ID.env\nexec sleep 600\n",
            env_dir.display()
        ),
    )
    .unwrap();
    make_executable(&script);
    let _daemon = env.spawn_daemon_with(
        script.to_str().unwrap(),
        &[
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_CHILD_SESSION", "1"),
            ("CLAUDE_CODE_SESSION_ID", "host-session"),
            ("CLAUDE_CODE_MESSAGING_SOCKET", "/tmp/host.sock"),
            ("CLAUDE_PID", "1"),
            ("AI_AGENT", "claude-code"),
            ("GIT_EDITOR", "true"),
            ("COREPACK_ENABLE_AUTO_PIN", "0"),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
        ],
    );

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent_id = create_agent_get_id(&mut c, &worktree.id, "agent-1", 2).await;

    let agent_env = read_env_file(&env_dir.join(format!("{}.env", agent_id.0))).await;
    for name in [
        "CLAUDECODE",
        "CLAUDE_CODE_CHILD_SESSION",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_CODE_MESSAGING_SOCKET",
        "CLAUDE_PID",
        "AI_AGENT",
        "GIT_EDITOR",
        "COREPACK_ENABLE_AUTO_PIN",
    ] {
        assert!(
            !agent_env.contains_key(name),
            "{name} leaked: {agent_env:?}"
        );
    }
    assert_eq!(
        agent_env.get("CLAUDE_CODE_USE_BEDROCK").map(String::as_str),
        Some("1"),
        "a user setting is kept: {agent_env:?}"
    );
}

/// Poll the env dump the fake agent CLI writes on boot, returning the
/// NEBULA_* variables the real CLI's hooks (and `nebula rename`) would see.
async fn read_env_file(path: &Path) -> std::collections::HashMap<String, String> {
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        if let Ok(s) = std::fs::read_to_string(path) {
            let map: std::collections::HashMap<String, String> = s
                .lines()
                .filter_map(|l| l.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            if map.contains_key(env::API_URL) && map.contains_key(env::API_TOKEN) {
                return map;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "agent env dump {path:?} never appeared"
        );
        tokio::time::sleep(POLL_STEP).await;
    }
}

/// Raw HTTP POST to the daemon's hook receiver, standing in for the curl
/// one-liner the installed hook runs; returns (status, body) — the body is
/// what the hook would pipe into the CLI's stdout.
async fn hook_post(port: u16, path_query: &str, token: &str) -> (u16, String) {
    hook_post_json(port, path_query, token, r#"{"session_id":"s1"}"#).await
}

/// `hook_post` with the payload the CLI would have piped in.
async fn hook_post_json(port: u16, path_query: &str, token: &str, payload: &str) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let req = format!(
        "POST {path_query} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf).to_string();
    let status: u16 = text.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

/// The whole auto-title loop over real processes: a default-named agent's
/// UserPromptSubmit hook response carries the titling instruction, the
/// `nebula rename` CLI (what the model runs) applies it exactly once and
/// broadcasts the new name, and afterwards the instruction stops and a
/// retitle attempt is declined without failing.
#[tokio::test]
async fn auto_title_instruction_and_rename_flow() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let env_dir = env.tmp.path().join("agent-env");
    std::fs::create_dir_all(&env_dir).unwrap();
    // Stand-in CLI: capture the NEBULA_* env its hooks would use, then park.
    let script = env.tmp.path().join("agent.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nenv | grep '^NEBULA_' > '{}'/$NEBULA_AGENT_ID.env\nexec sleep 600\n",
            env_dir.display()
        ),
    )
    .unwrap();
    make_executable(&script);
    let mut daemon = env.spawn_daemon_with_agent_cmd(script.to_str().unwrap());

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;

    // Created with the accepted default name → auto-title pending.
    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: worktree.id.clone(),
            name: "agent-1".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: true,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(agent_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let agent_id = agent_id.clone();

    let agent_env = read_env_file(&env_dir.join(format!("{}.env", agent_id.0))).await;
    let port: u16 = agent_env[env::API_URL]
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let token = agent_env[env::API_TOKEN].clone();
    let submit_path = format!(
        "/api/hooks/claude?agentId={}&hookEvent=UserPromptSubmit",
        agent_id.0
    );

    // First prompt on the untitled session: instruction rides the response.
    let (status, body) = hook_post(port, &submit_path, &token).await;
    assert_eq!(status, 200);
    assert_eq!(body, nebula_daemon::hooks::auto_title_injection());

    // The model obeys — `nebula rename` runs with the session's env.
    let out = agent_cli(&env, &agent_id, &["rename", "Fix", "Login", "Redirect"]);
    assert!(out.status.success(), "rename failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Fix Login Redirect"), "stdout: {stdout}");
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id && a.name == "Fix Login Redirect")
        })
    })
    .await;

    // Titled now: the next prompt injects no instruction — instead the
    // reply hands Claude the row's name as its own session title (CLAUDE
    // TITLE SYNC), so `/resume` and `/rc` show what the row shows.
    let (status, body) = hook_post(port, &submit_path, &token).await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        nebula_daemon::hooks::user_prompt_reply(false, Some("Fix Login Redirect"))
    );
    assert!(
        !body.contains("additionalContext"),
        "no instruction: {body}"
    );

    // A repeat attempt is declined as a settled answer (exit 0), not a fault.
    let out = agent_cli(&env, &agent_id, &["rename", "Another", "Title"]);
    assert!(out.status.success(), "declined rename must exit 0: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("already has a title"), "stdout: {stdout}");

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `nebula open <file>…` from inside a session, end to end over real
/// processes: the CLI (what the model runs) resolves the paths against its
/// own cwd, the daemon checks the caller and fans the files out to every
/// subscriber as one `FilesOpened` carrying the agent's checkout — and a
/// path that does not exist, or is an unsupported binary, fails in the CLI
/// before anything is sent.
#[tokio::test]
async fn nebula_open_cli_hands_the_files_to_every_subscriber() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let script = env.tmp.path().join("agent.sh");
    std::fs::write(&script, "#!/bin/sh\nexec sleep 600\n").unwrap();
    make_executable(&script);
    let mut daemon = env.spawn_daemon_with_agent_cmd(script.to_str().unwrap());

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent_id = create_agent_get_id(&mut c, &worktree.id, "agent-1", 2).await;

    let notes = repo.join("docs").join("notes.md");
    std::fs::create_dir_all(notes.parent().unwrap()).unwrap();
    std::fs::write(&notes, "# notes\n").unwrap();
    let main_rs = repo.join("main.rs");
    std::fs::write(&main_rs, "fn main() {}\n").unwrap();

    // Relative paths resolve against the CLI's cwd — the agent's.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nebula"))
        .args(["open", "docs/notes.md", "main.rs"])
        .current_dir(&repo)
        .env(env::RUNTIME_DIR, &env.runtime_dir)
        .env(env::AGENT_ID, &agent_id.0)
        .output()
        .unwrap();
    assert!(out.status.success(), "open failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("opened 2 files"), "stdout: {stdout}");

    let want: Vec<PathBuf> = [&notes, &main_rs]
        .iter()
        .map(|p| std::fs::canonicalize(p).unwrap())
        .collect();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::FilesOpened { .. }))
    })
    .await;
    let opened = events
        .iter()
        .find(|e| matches!(e, ServerEvent::FilesOpened { .. }))
        .unwrap();
    let ServerEvent::FilesOpened { agent, root, paths } = opened else {
        unreachable!()
    };
    assert_eq!(agent, &agent_id);
    assert_eq!(root, &worktree.path, "the agent's checkout rides along");
    assert_eq!(paths, &want, "absolute, in the order given");

    // A missing file is the CLI's error, before the daemon hears anything.
    let out = agent_cli(&env, &agent_id, &["open", "/nowhere/at/all.md"]);
    assert!(!out.status.success(), "a missing file must fail: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no such file"), "stderr: {stderr}");

    // Image and Mermaid files are previewable, so they pass through even
    // though image bytes fail git's text sniff.
    let png = repo.join("shot.png");
    std::fs::write(&png, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
    let mmd = repo.join("flow.mmd");
    std::fs::write(&mmd, "flowchart LR\nA-->B\n").unwrap();
    let out = agent_cli(
        &env,
        &agent_id,
        &["open", png.to_str().unwrap(), mmd.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "previewable visuals must open: {out:?}"
    );

    // Other binary files are still refused before the daemon hears them.
    let bin = repo.join("blob.bin");
    std::fs::write(&bin, b"a\0b").unwrap();
    let out = agent_cli(&env, &agent_id, &["open", bin.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "an unsupported binary must fail: {out:?}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unsupported binary file"),
        "stderr: {stderr}"
    );

    // Outside a session there is no row to open for.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nebula"))
        .args(["open", main_rs.to_str().unwrap()])
        .env(env::RUNTIME_DIR, &env.runtime_dir)
        .env_remove(env::AGENT_ID)
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "outside a session must fail: {out:?}"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `nebula worktree <name>` from inside a session, end to end over real
/// processes: the CLI (what the model runs) creates the checkout in nebula's
/// sibling layout and re-homes the row at once; the live PTY is left alone
/// until the turn's Stop hook — a tool hook still reporting the old
/// checkout's cwd in between must not drag the row back — and then respawns
/// inside the worktree.
#[tokio::test]
async fn nebula_worktree_cli_relocates_the_session_when_the_turn_ends() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let env_dir = env.tmp.path().join("agent-env");
    std::fs::create_dir_all(&env_dir).unwrap();
    // Stand-in CLI: dump the NEBULA_* env its hooks would use, log where
    // each boot runs (to a file, and to its own screen), then park.
    let script = env.tmp.path().join("agent.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nenv | grep '^NEBULA_' > '{d}'/$NEBULA_AGENT_ID.env\n\
             pwd >> '{d}'/$NEBULA_AGENT_ID.pwd\necho \"booted in $(pwd)\"\nexec sleep 600\n",
            d = env_dir.display()
        ),
    )
    .unwrap();
    make_executable(&script);
    let mut daemon = env.spawn_daemon_with_agent_cmd(script.to_str().unwrap());

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;

    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: main_worktree.id.clone(),
            name: "agent-1".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(agent_id)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let agent_id = agent_id.clone();
    let agent_env = read_env_file(&env_dir.join(format!("{}.env", agent_id.0))).await;
    let port: u16 = agent_env[env::API_URL]
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let token = agent_env[env::API_TOKEN].clone();
    let pwd_log = env_dir.join(format!("{}.pwd", agent_id.0));
    let boots = |path: &Path| -> Vec<String> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    };
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    while boots(&pwd_log).is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "first boot never logged"
        );
        tokio::time::sleep(POLL_STEP).await;
    }

    // A client sits on the pane throughout — the TUI, showing the session
    // that is about to run `nebula worktree`.
    let sref = SessionRef::Agent(agent_id.clone());
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: sref.clone(),
            from_seq: None,
            cols: 120,
            rows: 30,
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        String::from_utf8_lossy(&collected_output(evs)).contains("booted in")
    })
    .await;

    // A turn is under way — the one about to run the command.
    let hook = |event: &str| format!("/api/hooks/claude?agentId={}&hookEvent={event}", agent_id.0);
    let payload = format!(
        r#"{{"session_id":"s1","cwd":"{}","tool_name":"Bash"}}"#,
        repo.display()
    );
    let (status, _) = hook_post_json(port, &hook("UserPromptSubmit"), &token, &payload).await;
    assert_eq!(status, 200, "UserPromptSubmit");
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::Running, .. }
                if *agent == agent_id)
        })
    })
    .await;

    // The model obeys the guidance — `nebula worktree feat x` (the space
    // slugifies) with the session's env.
    let out = agent_cli(&env, &agent_id, &["worktree", "feat", "x"]);
    assert!(out.status.success(), "nebula worktree failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("feat-x") && stdout.contains("this turn ends"),
        "stdout: {stdout}"
    );

    // The row re-homes under the new checkout at once…
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id && a.worktree_id != main_worktree.id)
        })
    })
    .await;
    let feat = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Worktree(w),
            } if w.branch == "feat-x" => Some(w.clone()),
            _ => None,
        })
        .expect("feat-x worktree upsert");
    assert!(
        feat.path.ends_with("repo-worktrees/feat-x"),
        "nebula's sibling layout: {:?}",
        feat.path
    );
    assert!(feat.path.join(".git").exists(), "a real checkout");
    // …while the process is untouched: still the one boot, in the old checkout.
    assert_eq!(boots(&pwd_log).len(), 1, "no respawn before the turn ends");

    // Mid-turn the CLI's hooks keep reporting the old checkout's cwd; that
    // must not drag the row back. Then the Stop — same old cwd — ends the
    // turn and triggers the relocation.
    for event in ["PostToolUse", "Stop"] {
        let (status, _) = hook_post_json(port, &hook(event), &token, &payload).await;
        assert_eq!(status, 200, "{event}");
    }

    // The respawn: alive again under feat-x, booted inside it — and the
    // attached client follows it there with no second Attach: the daemon
    // rebinds the pane to the new PTY (a fresh Scrollback, then its output),
    // so the TUI never sits frozen on the old process's last frame.
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        let alive = evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id && a.worktree_id == feat.id && a.alive)
        });
        alive && String::from_utf8_lossy(&collected_output(evs)).contains("repo-worktrees/feat-x")
    })
    .await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ServerEvent::Scrollback { session, .. } if session == &sref)),
        "the rebind replays the new PTY's ring: {events:#?}"
    );
    // The turn's Stop did not finish the row on its way to the respawn:
    // the card would have dropped to the bottom of the grid and climbed
    // back once the relocated CLI's first hook landed. The respawn opens
    // on the relocation notice, so the row stays `running` throughout.
    assert!(
        !events.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status: nebula_core::AgentStatus::Finished, .. }
                if *agent == agent_id)
        }),
        "no finish between the Stop and the respawn: {events:#?}"
    );
    assert!(
        events.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent_id && a.worktree_id == feat.id && a.alive
                    && a.status == nebula_core::AgentStatus::Running)
        }),
        "the relocated row reads running: {events:#?}"
    );
    let deadline = tokio::time::Instant::now() + SLOW_TIMEOUT;
    loop {
        let b = boots(&pwd_log);
        if b.len() >= 2 {
            assert_eq!(b.len(), 2, "one respawn, not several: {b:?}");
            assert!(
                b[1].ends_with("repo-worktrees/feat-x"),
                "respawned inside the worktree: {b:?}"
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "respawn never booted in the worktree: {b:?}"
        );
        tokio::time::sleep(POLL_STEP).await;
    }

    // Already there now: a settled answer, and no second relocation.
    let out = agent_cli(&env, &agent_id, &["worktree", "feat-x"]);
    assert!(out.status.success(), "repeat must exit 0: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("already runs inside it"),
        "stdout: {stdout}"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `TestEnv::make_repo` at a caller-chosen path, for tests that need two.
fn make_repo_at(repo: &Path) {
    std::fs::create_dir_all(repo).unwrap();
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?} failed");
    };
    git(&["init", "-b", "main"]);
    git(&["config", "user.email", "test@nebula.dev"]);
    git(&["config", "user.name", "nebula-test"]);
    std::fs::write(repo.join("README.md"), "# test\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "init"]);
}

/// Mark a freshly written stub script runnable.
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Subscribe a handshaken client and wait for its first `Snapshot`; returns
/// everything received up to and including it.
async fn subscribe(c: &mut UnixStream) -> Vec<ServerEvent> {
    write_frame(c, &ClientRequest::Subscribe).await.unwrap();
    read_events_until(c, EVENT_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::Snapshot { .. }))
    })
    .await
}

/// A subscribed client that hangs up is let go at once, with nothing
/// broadcast in between: the daemon closes its side of the connection as
/// soon as it reads the hang-up. It used to keep the socket open until the
/// next broadcast, so an idle daemon collected one per one-shot client.
#[tokio::test]
async fn a_subscribed_client_that_hangs_up_is_let_go_by_an_idle_daemon() {
    use tokio::io::AsyncWriteExt;
    let env = TestEnv::new();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;

    // Hang up the writing half only: the daemon reads the end of the
    // requests, and this half stays open to see the daemon close its own.
    c.shutdown().await.unwrap();
    let closed = tokio::time::timeout(EVENT_TIMEOUT, async {
        while let Ok(Some(_)) = read_frame::<ServerEvent, _>(&mut c).await {}
    })
    .await;
    assert!(
        closed.is_ok(),
        "the daemon kept a hung-up subscriber's connection open"
    );

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `nebula spawn "<task>"` from inside a session, end to end over real
/// processes: the CLI (what the model runs) makes the daemon start a second
/// agent in the caller's worktree — booted at once, on the default name so
/// AUTO-TITLE applies, matching the caller's harness unless `--kind` names
/// another — while the caller's own process is left alone. The task itself
/// reaches argv only outside `NEBULA_AGENT_CMD`, so it is covered by the
/// registry's argv unit tests, not here.
#[tokio::test]
async fn nebula_spawn_cli_starts_a_sibling_session_in_the_same_worktree() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let env_dir = env.tmp.path().join("agent-env");
    std::fs::create_dir_all(&env_dir).unwrap();
    // Stand-in CLI: record every boot by agent id, then park.
    let script = env.tmp.path().join("agent.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nenv | grep '^NEBULA_' > '{d}'/$NEBULA_AGENT_ID.env\nexec sleep 600\n",
            d = env_dir.display()
        ),
    )
    .unwrap();
    make_executable(&script);
    // A Codex sibling installs Codex's managed hooks into Codex's home:
    // this test's own, never the developer's `~/.codex`.
    let codex_home = env.tmp.path().join("codex-home");
    let mut daemon = env.spawn_daemon_with(
        script.to_str().unwrap(),
        &[(env::CODEX_HOME, codex_home.to_str().unwrap())],
    );

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;

    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 2,
            worktree: main_worktree.id.clone(),
            name: "agent-1".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 2).is_some()).await;
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(caller)),
        ..
    } = find_ack(&events, 2).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let caller = caller.clone();
    read_env_file(&env_dir.join(format!("{}.env", caller.0))).await;

    // The model obeys the guidance — `nebula spawn fix the login redirect`
    // (the words join) with the session's env.
    let out = agent_cli(&env, &caller, &["spawn", "fix", "the", "login", "redirect"]);
    assert!(out.status.success(), "nebula spawn failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("started a new session") && stdout.contains("carry on"),
        "stdout: {stdout}"
    );

    // A second row, in the caller's worktree, on the default name, live.
    let sibling_of = |evs: &[ServerEvent], kind: AgentKind| {
        evs.iter().find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Agent(a),
            } if a.id != caller
                && a.worktree_id == main_worktree.id
                && a.kind == kind
                && a.alive =>
            {
                Some(a.clone())
            }
            _ => None,
        })
    };
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        sibling_of(evs, AgentKind::Claude).is_some()
    })
    .await;
    let sibling = sibling_of(&events, AgentKind::Claude).unwrap();
    assert_eq!(sibling.name, "agent-2", "the first free default name");
    assert!(
        stdout.contains(&format!("session {} in this worktree", sibling.id)),
        "stdout names the new session's id: {stdout}"
    );
    // …and its CLI really booted (the stub logged its own env).
    let sibling_env = read_env_file(&env_dir.join(format!("{}.env", sibling.id.0))).await;
    assert_eq!(sibling_env[env::AGENT_ID], sibling.id.0);
    // The caller was never respawned: still its one boot.
    assert_eq!(
        std::fs::read_dir(&env_dir).unwrap().count(),
        2,
        "exactly two boots: the caller's and the sibling's"
    );

    // `--kind` picks another harness (the stub stands in for every CLI).
    let out = agent_cli(
        &env,
        &caller,
        &["spawn", "--kind", "codex", "run the tests"],
    );
    assert!(out.status.success(), "nebula spawn --kind failed: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("new codex session"),
        "stdout names the harness"
    );
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        sibling_of(evs, AgentKind::Codex).is_some()
    })
    .await;
    assert_eq!(
        sibling_of(&events, AgentKind::Codex).unwrap().name,
        "agent-3"
    );

    // `--worktree` starts it in the project's worktree on that branch,
    // cutting one first (the words slugified as `nebula worktree` does).
    let out = agent_cli(
        &env,
        &caller,
        &["spawn", "--worktree", "feat login", "port the tests"],
    );
    assert!(
        out.status.success(),
        "nebula spawn --worktree failed: {out:?}"
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("branch \"feat-login\""),
        "stdout names the branch: {out:?}"
    );
    let worktree_of = |evs: &[ServerEvent]| {
        evs.iter().find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Worktree(w),
            } if w.branch == "feat-login" => Some(w.clone()),
            _ => None,
        })
    };
    let in_worktree = |evs: &[ServerEvent], worktree: &nebula_core::WorktreeId| {
        evs.iter().find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Agent(a),
            } if &a.worktree_id == worktree && a.alive => Some(a.clone()),
            _ => None,
        })
    };
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        worktree_of(evs).is_some_and(|w| in_worktree(evs, &w.id).is_some())
    })
    .await;
    let feat = worktree_of(&events).unwrap();
    assert!(!feat.is_main && feat.path.is_dir(), "a real new checkout");
    let spawned = in_worktree(&events, &feat.id).unwrap();
    assert_eq!(spawned.kind, AgentKind::Claude, "the caller's harness");
    assert_eq!(spawned.name, "agent-1", "named against its own worktree");
    let caller_row = events.iter().rev().find_map(|e| match e {
        ServerEvent::EntityUpserted {
            entity: Entity::Agent(a),
        } if a.id == caller => Some(a.clone()),
        _ => None,
    });
    assert!(
        caller_row.is_none_or(|a| a.worktree_id == main_worktree.id),
        "the caller stays in its own worktree"
    );
    // Asked again, the same branch reuses that checkout. `--json` prints
    // what `session start --json` does, the session's checkout included.
    let json = stdout_json(&agent_cli(
        &env,
        &caller,
        &["spawn", "--worktree", "feat-login", "--json", "review it"],
    ));
    assert_eq!(json["name"], "agent-2");
    assert_eq!(json["worktree"], feat.id.0.as_str());
    assert_eq!(json["path"], feat.path.to_str().unwrap());
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id.0 == json["id"] && a.worktree_id == feat.id && a.name == "agent-2"
                    && a.alive)
        })
    })
    .await;
    assert!(
        worktree_of(&events).is_none(),
        "no second worktree for the same branch"
    );
    // `--name` gives it the title it keeps.
    let json = stdout_json(&agent_cli(
        &env,
        &caller,
        &[
            "spawn",
            "--worktree",
            "feat-login",
            "--name",
            "Login review",
            "--json",
            "review it again",
        ],
    ));
    assert_eq!(json["name"], "Login review");
    let out = agent_cli(&env, &caller, &["spawn", "--name", " ", "x"]);
    assert!(!out.status.success(), "{out:?}");

    // A start point for a branch that already has a checkout is refused,
    // never silently dropped, and nothing is spawned for it.
    let out = agent_cli(
        &env,
        &caller,
        &["spawn", "--worktree", "feat-login", "--base", "main", "x"],
    );
    assert!(
        !out.status.success(),
        "--base on an existing worktree: {out:?}"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("already has a worktree"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // So is one for a branch kept without a checkout (a deleted worktree's).
    let kept = std::process::Command::new("git")
        .args(["-C", repo.to_str().unwrap(), "branch", "kept"])
        .output()
        .unwrap();
    assert!(kept.status.success(), "git branch: {kept:?}");
    let out = agent_cli(
        &env,
        &caller,
        &["spawn", "--worktree", "kept", "--base", "main", "x"],
    );
    assert!(!out.status.success(), "--base on a kept branch: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("already exists"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // `--base` means nothing without `--worktree`, or blank.
    let out = agent_cli(&env, &caller, &["spawn", "--base", "main", "x"]);
    assert!(!out.status.success(), "--base alone must fail: {out:?}");
    let out = agent_cli(
        &env,
        &caller,
        &["spawn", "--worktree", "feat-login", "--base", " ", "x"],
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("base is empty"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // A bad harness name and a blank task are the CLI's own refusals.
    let out = agent_cli(&env, &caller, &["spawn", "--kind", "gemini", "x"]);
    assert!(!out.status.success(), "unknown harness must fail: {out:?}");
    let out = agent_cli(&env, &caller, &["spawn", "   "]);
    assert!(!out.status.success(), "blank task must fail: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("task is empty"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let _ = daemon.kill();
}

/// `nebula orchestrator spawn`, run by the orchestrator: the new session
/// starts in the project's root checkout, or in the worktree on the branch
/// `--worktree` names (cut when there is none), and the CLI prints its id,
/// or with `--json` what `session start --json` prints. `--name` titles it
/// for good, and `--model` and `--effort` are what it launches with. A
/// worker's run of it is refused by the daemon.
#[tokio::test]
async fn nebula_orchestrator_spawn_cli_prints_the_session_it_started() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let script = env.tmp.path().join("agent.sh");
    std::fs::write(&script, "#!/bin/sh\nexec sleep 600\n").unwrap();
    make_executable(&script);
    let mut daemon = env.spawn_daemon_with_agent_cmd(script.to_str().unwrap());
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let worker = create_agent_get_id(&mut c, &main_worktree.id, "worker", 2).await;
    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 3,
            worktree: main_worktree.id.clone(),
            name: "orchestrator".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Orchestrator,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 3).is_some()).await;
    let Some(ServerEvent::Ack {
        created: Some(EntityId::Agent(orchestrator)),
        ..
    }) = find_ack(&events, 3)
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let orchestrator = orchestrator.clone();

    let stderr = refusal(&agent_cli(
        &env,
        &worker,
        &["orchestrator", "spawn", "--project", "repo", "x"],
    ));
    assert!(stderr.contains("only available"), "stderr: {stderr}");

    let out = agent_cli(
        &env,
        &orchestrator,
        &[
            "orchestrator",
            "spawn",
            "--project",
            "repo",
            "run the tests",
        ],
    );
    assert!(out.status.success(), "orchestrator spawn failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        agent_named(evs, "agent-1").is_some()
    })
    .await;
    let spawned = agent_named(&events, "agent-1").unwrap();
    assert_eq!(spawned.worktree_id, main_worktree.id, "the root checkout");
    assert_eq!(
        stdout,
        format!(
            "started a new claude session {} for that task\n",
            spawned.id
        )
    );

    let json = stdout_json(&agent_cli(
        &env,
        &orchestrator,
        &[
            "orchestrator",
            "spawn",
            "--project",
            "repo",
            "--worktree",
            "feat-tests",
            "--json",
            "port the tests",
        ],
    ));
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id.0 == json["id"])
        })
    })
    .await;
    let feat = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Worktree(w),
            } if w.branch == "feat-tests" => Some(w.clone()),
            _ => None,
        })
        .expect("the worktree the spawn cut");
    assert_eq!(json["name"], "agent-1", "named against its own worktree");
    assert_eq!(json["worktree"], feat.id.0.as_str());
    assert_eq!(json["path"], feat.path.to_str().unwrap());

    let json = stdout_json(&agent_cli(
        &env,
        &orchestrator,
        &[
            "orchestrator",
            "spawn",
            "--project",
            "repo",
            "--name",
            "Fix login",
            "--model",
            "opus",
            "--effort",
            "high",
            "--json",
            "fix the login redirect",
        ],
    ));
    assert_eq!(json["name"], "Fix login");
    let named = nebula_core::AgentId(json["id"].as_str().unwrap().to_string());
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        agent_named(evs, "Fix login").is_some()
    })
    .await;
    let row = agent_named(&events, "Fix login").unwrap();
    assert_eq!(row.id, named);
    assert_eq!(row.model.as_deref(), Some("opus"));
    assert_eq!(row.effort.as_deref(), Some("high"));
    // Not auto-titled: the session's own `nebula rename`, the one the
    // auto-title instruction asks for, is declined.
    let out = agent_cli(&env, &named, &["rename", "Login", "Redirect"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("already has a title"), "stdout: {stdout}");

    // A harness with no hooks never reports a status: the spawn says so on
    // stderr, as `session start` does, and stdout is still the one object.
    // `nebula spawn --kind` says the same.
    let spawn_on = |kind: &str| {
        let flags = ["--project", "repo", "--kind", kind, "--json", "x"];
        agent_cli(
            &env,
            &orchestrator,
            &[&["orchestrator", "spawn"], &flags[..]].concat(),
        )
    };
    let out = spawn_on("muse");
    stdout_json(&out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("reports no status"), "stderr: {stderr}");
    assert!(stderr.contains("--timeout"), "stderr: {stderr}");
    let out = spawn_on("claude");
    stdout_json(&out);
    assert!(out.stderr.is_empty(), "nothing to warn of: {out:?}");
    let out = agent_cli(&env, &worker, &["spawn", "--kind", "grok", "--json", "x"]);
    stdout_json(&out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("reports no status"), "stderr: {stderr}");

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `nebula ask` sends a labelled question to another local agent, waits on
/// the ordinary status hooks, queues behind a busy turn, and times out
/// clearly when the target never becomes idle.
#[tokio::test]
async fn nebula_ask_cli_round_trips_queues_and_times_out() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let env_dir = env.tmp.path().join("agent-env");
    let log_dir = env.tmp.path().join("agent-log");
    std::fs::create_dir_all(&env_dir).unwrap();
    std::fs::create_dir_all(&log_dir).unwrap();

    let script = env.tmp.path().join("answer-agent.sh");
    std::fs::write(
        &script,
        format!(
            concat!(
                "#!/bin/sh\n",
                "env | grep '^NEBULA_' > '{env_dir}'/$NEBULA_AGENT_ID.env\n",
                "cd '{log_dir}' || exit 1\n",
                "while IFS= read -r line; do\n",
                "  case \"$line\" in\n",
                "    *'[question from session'*)\n",
                "      echo \"$line\" >> $NEBULA_AGENT_ID.questions\n",
                "      : > $NEBULA_AGENT_ID.prompt\n",
                "      ;;\n",
                "  esac\n",
                "  printf '%s\\n' \"$line\" >> $NEBULA_AGENT_ID.prompt\n",
                "  case \"$line\" in\n",
                "    *'Answer concisely;'*)\n",
                "      {payload}\n",
                "      curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" ",
                "-H 'Content-Type: application/json' -d @$NEBULA_AGENT_ID.json ",
                "\"$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent=UserPromptSubmit\" ",
                ">/dev/null 2>&1\n",
                "      echo 'Research answer: use serde_json.'\n",
                "      curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" ",
                "-H 'Content-Type: application/json' -d '{{\"session_id\":\"ask-sid\"}}' ",
                "\"$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent=Stop\" ",
                ">/dev/null 2>&1\n",
                "      ;;\n",
                "  esac\n",
                "done\n",
            ),
            env_dir = env_dir.display(),
            log_dir = log_dir.display(),
            payload = prompt_payload_sh(
                "ask-sid",
                "$NEBULA_AGENT_ID.prompt",
                "$NEBULA_AGENT_ID.json"
            ),
        ),
    )
    .unwrap();
    make_executable(&script);

    let mut daemon = env.spawn_daemon_with_agent_cmd(script.to_str().unwrap());
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let caller = create_agent_get_id(&mut c, &worktree.id, "feature", 2).await;
    let target = create_agent_get_id(&mut c, &worktree.id, "research", 3).await;
    let target_env = read_env_file(&env_dir.join(format!("{}.env", target.0))).await;
    read_env_file(&env_dir.join(format!("{}.env", caller.0))).await;

    let ask = |args: &[&str]| agent_cli(&env, &caller, args);
    let out = ask(&[
        "ask",
        "research",
        "which library should the feature use?",
        "--timeout",
        "5",
    ]);
    assert!(out.status.success(), "ask failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Research answer: use serde_json."),
        "stdout: {stdout}"
    );

    let port: u16 = target_env[env::API_URL]
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let token = target_env[env::API_TOKEN].clone();
    let hook = |event: &str| format!("/api/hooks/claude?agentId={}&hookEvent={event}", target.0);
    let target_log = log_dir.join(format!("{}.questions", target.0));
    let question_count = || {
        std::fs::read_to_string(&target_log)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    };
    let before_busy = question_count();

    let (status, _) = hook_post_json(
        port,
        &hook("UserPromptSubmit"),
        &token,
        r#"{"session_id":"ask-sid","prompt":"busy"}"#,
    )
    .await;
    assert_eq!(status, 200);
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent, status, .. }
                if agent == &target && *status == nebula_core::AgentStatus::Running)
        })
    })
    .await;

    let mut queued = env.cli();
    let queued = queued
        .args([
            "ask",
            "research",
            "answer after your current turn",
            "--timeout",
            "5",
        ])
        .env(env::AGENT_ID, &caller.0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        question_count(),
        before_busy,
        "busy target should not receive the queued question yet"
    );
    let (status, _) =
        hook_post_json(port, &hook("Stop"), &token, r#"{"session_id":"ask-sid"}"#).await;
    assert_eq!(status, 200);
    let out = queued.wait_with_output().unwrap();
    assert!(out.status.success(), "queued ask failed: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Research answer: use serde_json."),
        "queued stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    let (status, _) = hook_post_json(
        port,
        &hook("UserPromptSubmit"),
        &token,
        r#"{"session_id":"ask-sid","prompt":"stuck"}"#,
    )
    .await;
    assert_eq!(status, 200);
    let out = ask(&["ask", "--timeout", "1", "research", "will this time out?"]);
    assert!(!out.status.success(), "timeout ask should fail: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("timed out"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `nebula read --report` and `nebula orchestrator read --report` print a
/// captain's last reply alone, every line of it, where a plain read buries
/// it under the tool results, task notifications and reminders around it.
/// `--report` beside `--lines` or `--bytes` is refused.
#[tokio::test]
async fn both_reads_print_a_report_alone() {
    use nebula_core::AgentStatus::{Finished, Running};
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;
    let worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let orchestrator = create_agent_as(
        &mut c,
        &worktree.id,
        "orchestrator",
        2,
        nebula_core::AgentRole::Orchestrator,
    )
    .await;
    let captain = create_agent_get_id(&mut c, &worktree.id, "captain", 3).await;

    let transcript = env.tmp.path().join("sess-captain.jsonl");
    std::fs::write(
        &transcript,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"The user asks: fix the login redirect."}}"#,
            "\n",
            r#"{"type":"assistant","message":{"id":"m1","role":"assistant","content":[{"type":"text","text":"Starting a worker."}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"Command running in background with ID: b1."}]}}"#,
            "\n",
            r#"{"type":"queue-operation","operation":"enqueue","content":"<task-notification> <task-id>b1</task-id> </task-notification>"}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":"<task-notification> <task-id>b1</task-id> </task-notification>"}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":"<system-reminder>A background-task event.</system-reminder>"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"id":"m2","role":"assistant","content":[{"type":"text","text":"The redirect keeps its query string now.\n\nNotes: the e2e tests need a free port."}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"id":"m2","role":"assistant","content":[{"type":"text","text":"DONE fixed the login redirect"}]}}"#,
            "\n",
        ),
    )
    .unwrap();
    let payload = format!(
        r#"{{"session_id":"s1","transcript_path":"{}"}}"#,
        transcript.display()
    );
    // The transcript is noted after the status the hook drives, so the
    // Stop behind it says the first hook is all in.
    type_into(
        &mut c,
        &captain,
        &hook_curl_with("UserPromptSubmit", &payload),
    )
    .await;
    status_seen(&mut c, &captain, Running).await;
    post_hook(&mut c, &captain, "Stop", Finished).await;

    let report =
        "The redirect keeps its query string now.\n\nNotes: the e2e tests need a free port.\nDONE fixed the login redirect\n";
    let id = captain.0.as_str();
    let plain = shell_cli(&env, &["read", id]);
    assert!(
        String::from_utf8_lossy(&plain.stdout).contains("<task-notification>"),
        "a plain read keeps the noise: {plain:?}"
    );
    for out in [
        shell_cli(&env, &["read", id, "--report"]),
        agent_cli(
            &env,
            &orchestrator,
            &["orchestrator", "read", id, "--report"],
        ),
    ] {
        assert!(out.status.success(), "the read failed: {out:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), report);
    }
    for args in [
        &["read", id, "--report", "--lines", "5"][..],
        &["orchestrator", "read", id, "--report", "--bytes", "5"],
    ] {
        let stderr = refusal(&agent_cli(&env, &orchestrator, args));
        assert!(stderr.contains("cannot be used with"), "{stderr}");
    }

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// Run the `nebula` CLI the way a hook would inside an agent session: the
/// test daemon's runtime dir plus the session's `NEBULA_AGENT_ID`.
fn agent_cli(
    env: &TestEnv,
    agent_id: &nebula_core::AgentId,
    args: &[&str],
) -> std::process::Output {
    env.cli()
        .args(args)
        .env(env::AGENT_ID, &agent_id.0)
        .output()
        .unwrap()
}

/// Run the `nebula` CLI against this env's daemon and data dir.
fn shell_cli(env: &TestEnv, args: &[&str]) -> std::process::Output {
    env.cli().args(args).output().unwrap()
}

/// What a `--json` run printed, having succeeded.
fn stdout_json(out: &std::process::Output) -> serde_json::Value {
    assert!(out.status.success(), "the CLI failed: {out:?}");
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("stdout is not one JSON value ({e}): {out:?}"))
}

/// A CLI run that must fail, and what it said on stderr.
fn refusal(out: &std::process::Output) -> String {
    assert!(!out.status.success(), "the CLI must fail: {out:?}");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The newest upsert of the agent row called `name`.
fn agent_named(events: &[ServerEvent], name: &str) -> Option<nebula_core::Agent> {
    events.iter().rev().find_map(|e| match e {
        ServerEvent::EntityUpserted {
            entity: Entity::Agent(a),
        } if a.name == name => Some(a.clone()),
        _ => None,
    })
}

/// The unarchived sessions in the worktree on `branch`, as a fresh
/// subscriber's Snapshot has them, or `None` when there is no such
/// worktree.
async fn sessions_on(env: &TestEnv, branch: &str) -> Option<Vec<nebula_core::Agent>> {
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    write_frame(&mut c, &ClientRequest::Subscribe)
        .await
        .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter()
            .any(|e| matches!(e, ServerEvent::Snapshot { .. }))
    })
    .await;
    let Some(ServerEvent::Snapshot {
        worktrees, agents, ..
    }) = events
        .into_iter()
        .find(|e| matches!(e, ServerEvent::Snapshot { .. }))
    else {
        unreachable!("read until a Snapshot");
    };
    let worktree = worktrees.iter().find(|w| w.branch == branch)?;
    Some(
        agents
            .into_iter()
            .filter(|a| a.worktree_id == worktree.id && !a.archived)
            .collect(),
    )
}

/// `git worktree add -b <branch> <checkout>` from outside nebula.
fn git_worktree_add(repo: &Path, branch: &str, checkout: &Path) {
    let added = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "add", "-b", branch])
        .arg(checkout)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(added, "external git worktree add {branch} failed");
}

/// `nebula session start`, end to end: the session lands in the checkout
/// the directory is in, under the name it was given (or the first free
/// default), working, on the quick prompt's harness unless a flag names
/// one, and the CLI prints the id a script keeps hold of it by. A
/// repository nebula does not know is refused at once naming `nebula add`,
/// one nested in a checkout it does know included: the session never lands
/// in the checkout around it.
#[tokio::test]
async fn nebula_session_start_cli_starts_a_session_in_the_checkout_it_is_told() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let path = main_worktree.path.display().to_string();

    // A directory inside the checkout, spelled as a shell would: on macOS
    // the temp dir is a symlink the daemon's own paths have resolved.
    let src = repo.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let out = shell_cli(
        &env,
        &[
            "session",
            "start",
            "--in",
            src.to_str().unwrap(),
            "--name",
            "Fix Login",
            "fix",
            "the",
            "login",
            "redirect",
        ],
    );
    assert!(out.status.success(), "session start --in failed: {out:?}");
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        agent_named(evs, "Fix Login").is_some_and(|a| a.alive)
    })
    .await;
    let session = agent_named(&events, "Fix Login").unwrap();
    assert_eq!(session.worktree_id, main_worktree.id);
    assert_eq!(
        session.kind,
        AgentKind::Claude,
        "the quick prompt's harness"
    );
    assert_eq!(
        session.status,
        nebula_core::AgentStatus::Running,
        "a session handed its first prompt is working from the start"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&session.id.0)
            && stdout.contains("\"Fix Login\"")
            && stdout.contains(&path),
        "stdout names the id, the name and the checkout: {stdout}"
    );

    // No `--in`: the directory the command runs in. No name: the first
    // free default. `--json` is the same answer as one object, and nothing
    // else on stdout. `--kind` names another harness than the quick
    // prompt's.
    let out = env
        .cli()
        .current_dir(&src)
        .args(["session", "start", "--json", "--kind", "cursor", "run it"])
        .output()
        .unwrap();
    let json = stdout_json(&out);
    assert_eq!(json["name"], "agent-1");
    assert_eq!(json["worktree"], main_worktree.id.0.as_str());
    assert_eq!(json["path"], path.as_str());
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        agent_named(evs, "agent-1").is_some()
    })
    .await;
    let second = agent_named(&events, "agent-1").unwrap();
    assert_eq!(json["id"], second.id.0.as_str());
    assert_eq!(second.kind, AgentKind::Cursor);

    // A directory in no repository: refused, with the way to fix it.
    let plain = env.tmp.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let start_in = |dir: &Path| {
        shell_cli(
            &env,
            &["session", "start", "--in", dir.to_str().unwrap(), "x"],
        )
    };
    let stderr = refusal(&start_in(&plain));
    assert!(stderr.contains("`nebula add "), "stderr: {stderr}");
    // A repository of its own inside the checkout is not the checkout:
    // refused the same way, and nothing starts in the one around it.
    let nested = repo.join("vendor").join("other");
    make_repo_at(&nested);
    let stderr = refusal(&start_in(&nested));
    assert!(
        stderr.contains("`nebula add ") && !stderr.contains("adopted"),
        "stderr: {stderr}"
    );
    assert_eq!(
        sessions_on(&env, "main").await.unwrap().len(),
        2,
        "no session landed in the enclosing checkout"
    );
    // One that is not there at all fails before any IPC.
    let stderr = refusal(&start_in(Path::new("does-not-exist")));
    assert!(stderr.contains("does not exist"), "stderr: {stderr}");
    let stderr = refusal(&shell_cli(
        &env,
        &["session", "start", "--in", repo.to_str().unwrap(), "  "],
    ));
    assert!(stderr.contains("task is empty"), "stderr: {stderr}");

    // A harness with no hooks never reports a status: the start says so
    // on stderr, and stdout is still the one object.
    let start_on = |kind: &str| {
        let flags = ["--in", repo.to_str().unwrap(), "--json", "--kind", kind];
        shell_cli(&env, &[&["session", "start"], &flags[..], &["x"]].concat())
    };
    let out = start_on("muse");
    stdout_json(&out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("reports no status"), "stderr: {stderr}");
    let out = start_on("claude");
    stdout_json(&out);
    assert!(out.stderr.is_empty(), "nothing to warn of: {out:?}");

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A checkout `git worktree add` made a moment ago is no row until
/// the worktree sync adopts it. `nebula session start --in` waits for that
/// row: beside the repo, where nothing holds the path yet, and under the
/// root checkout, which already does and must not be taken for it.
#[tokio::test]
async fn nebula_session_start_cli_waits_for_a_checkout_git_just_made() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    // A slow sync beat: the checkout is certainly no row when the CLI asks.
    let mut daemon = env.spawn_daemon_with("/bin/sh", &[(env::WORKTREE_SYNC_MS, "1500")]);
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;

    for (branch, checkout) in [
        (
            "beside",
            env.tmp.path().join("repo-worktrees").join("beside"),
        ),
        ("nested", repo.join(".worktrees").join("nested")),
    ] {
        git_worktree_add(&repo, branch, &checkout);
        let json = stdout_json(&shell_cli(
            &env,
            &[
                "session",
                "start",
                "--in",
                checkout.to_str().unwrap(),
                "--json",
                "carry on",
            ],
        ));
        assert_ne!(
            json["worktree"],
            main_worktree.id.0.as_str(),
            "{branch}: not the root checkout"
        );
        assert_eq!(
            PathBuf::from(json["path"].as_str().unwrap()),
            checkout.canonicalize().unwrap()
        );
        let sessions = sessions_on(&env, branch)
            .await
            .unwrap_or_else(|| panic!("{branch} was never adopted"));
        assert_eq!(sessions[0].id.0, json["id"].as_str().unwrap());
    }

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// Type `line` and Enter into the stand-in shell behind `agent`'s PTY.
async fn type_into(c: &mut UnixStream, agent: &nebula_core::AgentId, line: &str) {
    write_frame(
        c,
        &ClientRequest::Input {
            session: SessionRef::Agent(agent.clone()),
            data: format!("{line}\n").into_bytes(),
        },
    )
    .await
    .unwrap();
}

/// The shell line that posts the Claude hook `event` from inside an agent
/// PTY, with the env nebula injected there: what the installed hook runs.
fn hook_curl(event: &str) -> String {
    hook_curl_with(event, r#"{"session_id":"s1"}"#)
}

/// A line for the stand-in agent's shell that reports itself as the prompt
/// it took, as a CLI's `UserPromptSubmit` hook does, then runs `rest`: the
/// report is read from a file the line names, which holds the whole line.
fn reported_turn(env: &TestEnv, rest: &str) -> String {
    reported_turn_as(env, None, rest)
}

/// [`reported_turn`] reporting `took` as the prompt instead, when given:
/// what a CLI that lost part of the line would report.
fn reported_turn_as(env: &TestEnv, took: Option<&str>, rest: &str) -> String {
    static TURNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = TURNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let file = env.tmp.path().join(format!("turn-{n}.json"));
    let line = format!(
        "{}; {rest}",
        hook_curl_with("UserPromptSubmit", &format!("@{}", file.display()))
    );
    let payload = serde_json::json!({ "session_id": "s1", "prompt": took.unwrap_or(&line) });
    std::fs::write(&file, payload.to_string()).unwrap();
    line
}

/// [`hook_curl`] posting the JSON `payload`, which must hold no `'`.
fn hook_curl_with(event: &str, payload: &str) -> String {
    format!(
        "curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \
         -H 'Content-Type: application/json' -d '{payload}' \
         \"$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent={event}\""
    )
}

/// Read on until the subscribed client sees `agent` turn `status`.
async fn status_seen(
    c: &mut UnixStream,
    agent: &nebula_core::AgentId,
    status: nebula_core::AgentStatus,
) {
    read_events_until(c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::StatusChanged { agent: a, status: s, .. }
                if a == agent && *s == status)
        })
    })
    .await;
}

/// Have the stand-in shell behind `agent`'s PTY post the Claude hook
/// `event` with its own injected env, as the installed hook would, and
/// read on until the subscribed client sees the status that drives.
async fn post_hook(
    c: &mut UnixStream,
    agent: &nebula_core::AgentId,
    event: &str,
    status: nebula_core::AgentStatus,
) {
    type_into(c, agent, &hook_curl(event)).await;
    status_seen(c, agent, status).await;
}

/// What a CLI left running printed once it ended. One that never ends is
/// killed, and fails the test.
async fn output_of(cli: std::process::Child) -> std::process::Output {
    output_within(cli, SLOW_TIMEOUT).await
}

/// [`output_of`], for a CLI given `limit` to end.
async fn output_within(mut cli: std::process::Child, limit: Duration) -> std::process::Output {
    let deadline = tokio::time::Instant::now() + limit;
    while cli.try_wait().unwrap().is_none() {
        if tokio::time::Instant::now() >= deadline {
            let _ = cli.kill();
            panic!("the CLI never returned");
        }
        tokio::time::sleep(POLL_STEP).await;
    }
    cli.wait_with_output().unwrap()
}

/// The `nebula` CLI left running against this env's daemon, its output
/// kept for [`output_of`].
fn spawn_cli(env: &TestEnv, args: &[&str]) -> std::process::Child {
    env.cli()
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

/// The rows `nebula session read <id>` prints, asked again until one of
/// them is exactly `row`.
async fn read_until(env: &TestEnv, id: &str, row: &str) -> Vec<String> {
    let deadline = tokio::time::Instant::now() + SLOW_TIMEOUT;
    loop {
        let out = shell_cli(env, &["session", "read", id]);
        assert!(out.status.success(), "nebula session read failed: {out:?}");
        let rows: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect();
        if rows.iter().any(|r| r == row) {
            return rows;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the screen never showed `{row}`: {rows:#?}"
        );
        tokio::time::sleep(POLL_STEP).await;
    }
}

/// `nebula session wait <id>`, end to end: it returns the status the
/// session stopped in, driven by the same hook POSTs the installed hooks
/// make: `finished` after a Stop and `needs_feedback` after a
/// PermissionRequest, which `--until-done` waits through to the Stop
/// after it. A session that is not running when asked returns at
/// once, `--timeout` gives up on one that stays running with an exit code
/// of its own, and an archived one is an error, not a wait without end. An
/// id nothing has fails naming `nebula sessions`, and with no daemon nothing is
/// started.
#[tokio::test]
async fn nebula_session_wait_cli_returns_the_status_the_session_stopped_in() {
    use nebula_core::AgentStatus::{Finished, NeedsFeedback, Running};
    let env = TestEnv::new();
    let repo = env.make_repo();

    let stderr = refusal(&shell_cli(&env, &["session", "wait", "any"]));
    assert!(
        stderr.contains("no nebula daemon is running"),
        "stderr: {stderr}"
    );
    assert!(!env.sock().exists(), "`session wait` must start no daemon");

    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent = create_agent_get_id(&mut c, &main_worktree.id, "worker", 2).await;
    let id = agent.0.as_str();

    let stderr = refusal(&shell_cli(&env, &["session", "wait", "no-such-id"]));
    assert!(stderr.contains("`nebula sessions`"), "stderr: {stderr}");

    // Not running when asked: back at once, with the status it is in.
    let out = shell_cli(&env, &["session", "wait", id]);
    assert!(out.status.success(), "session wait failed: {out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "fresh\n");

    for (hook, status, word) in [
        ("Stop", Finished, "finished\n"),
        ("PermissionRequest", NeedsFeedback, "needs_feedback\n"),
    ] {
        post_hook(&mut c, &agent, "UserPromptSubmit", Running).await;
        // Running, and staying so: --timeout gives up, as `timeout(1)`
        // does, and prints no status.
        let out = shell_cli(&env, &["session", "wait", id, "--timeout", "1"]);
        assert_eq!(out.status.code(), Some(124), "a timeout's exit: {out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("still running after 1 s"), "{stderr}");
        assert!(out.stdout.is_empty(), "no status on a timeout: {out:?}");

        // A wait with no timeout ends when the session stops.
        let waiting = env
            .cli()
            .args(["session", "wait", id])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        post_hook(&mut c, &agent, hook, status).await;
        let out = output_of(waiting).await;
        assert!(out.status.success(), "session wait failed: {out:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), word, "after {hook}");
    }

    // `--until-done` waits through the dialog, and ends with the turn.
    post_hook(&mut c, &agent, "UserPromptSubmit", Running).await;
    let mut waiting = env
        .cli()
        .args(["session", "wait", id, "--until-done"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    post_hook(&mut c, &agent, "PermissionRequest", NeedsFeedback).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        waiting.try_wait().unwrap().is_none(),
        "--until-done returned on a dialog"
    );
    // Given up on, it says the dialog is what it is waiting through.
    let out = shell_cli(
        &env,
        &["session", "wait", id, "--until-done", "--timeout", "1"],
    );
    assert_eq!(out.status.code(), Some(124), "a timeout's exit: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("still needs_feedback after 1 s"),
        "{stderr}"
    );
    post_hook(&mut c, &agent, "Stop", Finished).await;
    let out = output_of(waiting).await;
    assert!(out.status.success(), "session wait failed: {out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "finished\n");

    // Archiving stops the process and leaves the status: running here, so
    // a wait that only read the status would never end.
    post_hook(&mut c, &agent, "UserPromptSubmit", Running).await;
    write_frame(
        &mut c,
        &ClientRequest::ArchiveAgent {
            req_id: 3,
            id: agent.clone(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 3).is_some()).await;
    let out = shell_cli(&env, &["session", "wait", id]);
    let stderr = refusal(&out);
    assert!(stderr.contains("is archived"), "stderr: {stderr}");
    assert!(
        out.stdout.is_empty(),
        "no status for an archived row: {out:?}"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `nebula session wait --until-done` against the daemon, end to end: a
/// Stop that a subagent's own SubagentStart, landing just after it, heals
/// back to running is not the end of the work, and the wait returns only
/// once the session has stayed finished for DONE_SETTLE.
#[tokio::test]
async fn nebula_session_wait_until_done_outwaits_a_stop_a_subagent_heals() {
    use nebula_core::AgentStatus::{Finished, Running};
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent = create_agent_get_id(&mut c, &main_worktree.id, "worker", 2).await;
    let id = agent.0.as_str();

    post_hook(&mut c, &agent, "UserPromptSubmit", Running).await;
    let mut waiting = spawn_cli(&env, &["session", "wait", id, "--until-done"]);
    post_hook(&mut c, &agent, "Stop", Finished).await;
    let subagent = r#"{"session_id":"s1","agent_id":"sub1"}"#;
    type_into(&mut c, &agent, &hook_curl_with("SubagentStart", subagent)).await;
    status_seen(&mut c, &agent, Running).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        waiting.try_wait().unwrap().is_none(),
        "--until-done returned on a Stop the subagent healed"
    );

    // The subagent ends, and Claude parks at its input box.
    type_into(&mut c, &agent, &hook_curl_with("SubagentStop", subagent)).await;
    let idle = r#"{"session_id":"s1","notification_type":"idle_prompt"}"#;
    type_into(&mut c, &agent, &hook_curl_with("Notification", idle)).await;
    status_seen(&mut c, &agent, Finished).await;
    let finished = tokio::time::Instant::now();
    let out = output_of(waiting).await;
    assert!(out.status.success(), "session wait failed: {out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "finished\n");
    assert!(
        finished.elapsed() >= nebula_core::DONE_SETTLE - Duration::from_millis(500),
        "returned {:?} after the finish, inside the settle",
        finished.elapsed()
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `nebula session wait --until-done` on a session that ended its turn
/// with a job still running in the background (Claude's
/// `run_in_background` Bash: a process leading a session of its own under
/// the agent) waits for that job too, since the agent wakes when it ends,
/// and returns DONE_SETTLE after it.
#[tokio::test]
async fn nebula_session_wait_until_done_waits_for_a_job_left_in_the_background() {
    use nebula_core::AgentStatus::{Finished, Running};
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent = create_agent_get_id(&mut c, &main_worktree.id, "worker", 2).await;
    let id = agent.0.as_str();

    post_hook(&mut c, &agent, "UserPromptSubmit", Running).await;
    let job = Duration::from_secs(3);
    type_into(
        &mut c,
        &agent,
        &format!(
            "python3 -c 'import subprocess; subprocess.run([\"sleep\", \"{}\"], \
             start_new_session=True)' &",
            job.as_secs()
        ),
    )
    .await;
    post_hook(&mut c, &agent, "Stop", Finished).await;
    let stopped = tokio::time::Instant::now();
    let waiting = spawn_cli(&env, &["session", "wait", id, "--until-done"]);
    let out = output_within(waiting, job + SLOW_TIMEOUT).await;
    assert!(out.status.success(), "session wait failed: {out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "finished\n");
    // Without the job it would have returned at the settle, 5 s after the
    // Stop; the job holds it a settle past its own end.
    assert!(
        stopped.elapsed() >= job + nebula_core::DONE_SETTLE - Duration::from_millis(500),
        "returned {:?} after the Stop, while the job ran",
        stopped.elapsed()
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `nebula session wait` rides out an IN-PLACE RESTART: `nebula reload`
/// drops every client's connection, and a wait, `--until-done` or plain,
/// reaches the new image, subscribes again, and returns when the session
/// finishes there instead of failing with the old connection.
#[tokio::test]
async fn nebula_session_wait_survives_a_daemon_reload() {
    use nebula_core::AgentStatus::{Finished, Running};
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent = create_agent_get_id(&mut c, &main_worktree.id, "worker", 2).await;
    let id = agent.0.as_str();

    post_hook(&mut c, &agent, "UserPromptSubmit", Running).await;
    let mut until_done = spawn_cli(&env, &["session", "wait", id, "--until-done"]);
    let mut plain = spawn_cli(&env, &["session", "wait", id]);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let out = env.cli().arg("reload").output().unwrap();
    assert!(out.status.success(), "reload failed: {out:?}");
    read_until_closed(&mut c).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    for (waiting, name) in [(&mut until_done, "--until-done"), (&mut plain, "plain")] {
        assert!(
            waiting.try_wait().unwrap().is_none(),
            "the {name} wait ended with the old daemon's connection"
        );
    }

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;
    post_hook(&mut c, &agent, "Stop", Finished).await;
    for waiting in [until_done, plain] {
        let out = output_of(waiting).await;
        assert!(out.status.success(), "session wait failed: {out:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "finished\n");
    }

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// Play the daemon to a CLI connected on `c`, once it has said hello and
/// been answered: take its Subscribe and answer with a tree of one Claude
/// session, `a1`, in `status`.
async fn serve_one_agent(
    env: &TestEnv,
    c: &mut UnixStream,
    status: nebula_core::AgentStatus,
) -> nebula_core::Agent {
    let subscribe = read_frame::<ClientRequest, _>(c).await.unwrap();
    assert!(
        matches!(subscribe, Some(ClientRequest::Subscribe)),
        "{subscribe:?}"
    );
    let project = nebula_core::Project {
        id: nebula_core::ProjectId("p1".into()),
        name: "repo".into(),
        repo_path: env.tmp.path().join("repo"),
        sort_order: 0,
    };
    let worktree = nebula_core::Worktree {
        id: nebula_core::WorktreeId("w1".into()),
        project_id: project.id.clone(),
        path: project.repo_path.clone(),
        branch: "main".into(),
        is_main: true,
        sort_order: 0,
        removed: false,
    };
    let agent = nebula_core::Agent {
        id: nebula_core::AgentId("a1".into()),
        worktree_id: worktree.id.clone(),
        name: "worker".into(),
        status,
        archived: false,
        archived_at: 0,
        unseen: false,
        status_changed_at: nebula_core::clock::now_ms(),
        kind: AgentKind::Claude,
        custom_harness: None,
        model: None,
        effort: None,
        session_id: None,
        cloud_session_id: None,
        issue_url: None,
        role: nebula_core::AgentRole::Worker,
        sort_order: 0,
        alive: true,
        recent_prompts: Vec::new(),
    };
    write_frame(
        c,
        &ServerEvent::Snapshot {
            projects: vec![project],
            worktrees: vec![worktree],
            agents: vec![agent.clone()],
            terminals: Vec::new(),
            links: Vec::new(),
            pr_seen: Vec::new(),
            ui_state: None,
        },
    )
    .await
    .unwrap();
    agent
}

/// `nebula session wait --until-done` against a daemon older than
/// AWAIT_DONE_PROTOCOL, played here by a socket that speaks the protocol
/// before it: the CLI never sends that daemon the AwaitDone it could not
/// read, and holds the status its subscription reports to DONE_SETTLE
/// itself, starting over when a heal sends it back to running.
#[tokio::test]
async fn nebula_session_wait_until_done_settles_on_its_own_against_an_older_daemon() {
    use nebula_core::AgentStatus::{Finished, Running};
    let env = TestEnv::new();
    std::fs::create_dir_all(&env.runtime_dir).unwrap();
    let listener = tokio::net::UnixListener::bind(env.sock()).unwrap();
    let waiting = spawn_cli(&env, &["session", "wait", "a1", "--until-done"]);
    let (mut c, _) = tokio::time::timeout(EVENT_TIMEOUT, listener.accept())
        .await
        .expect("the CLI never connected")
        .unwrap();
    let hello = read_frame::<ClientRequest, _>(&mut c).await.unwrap();
    assert!(
        matches!(hello, Some(ClientRequest::Hello { .. })),
        "{hello:?}"
    );
    write_frame(
        &mut c,
        &ServerEvent::HelloOk {
            protocol_version: nebula_core::AWAIT_DONE_PROTOCOL - 1,
            daemon_pid: std::process::id(),
        },
    )
    .await
    .unwrap();
    let agent = serve_one_agent(&env, &mut c, Running).await;
    let status = |status| ServerEvent::StatusChanged {
        agent: agent.id.clone(),
        status,
        changed_at: nebula_core::clock::now_ms(),
        unseen: false,
    };

    // Finished, healed back to running a second later, finished again.
    write_frame(&mut c, &status(Finished)).await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    write_frame(&mut c, &status(Running)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    write_frame(&mut c, &status(Finished)).await.unwrap();
    let finished = tokio::time::Instant::now();
    let out = output_of(waiting).await;
    assert!(out.status.success(), "session wait failed: {out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "finished\n");
    assert!(
        finished.elapsed() >= nebula_core::DONE_SETTLE - Duration::from_millis(500),
        "returned {:?} after the last finish, inside the settle",
        finished.elapsed()
    );
    // All it said after the Subscribe, up to its hang-up: nothing.
    let after = read_frame::<ClientRequest, _>(&mut c).await.unwrap();
    assert!(after.is_none(), "the CLI asked the older daemon {after:?}");
}

/// `nebula session send` against a daemon older than
/// TURN_RECEIPT_PROTOCOL, played here by a socket that speaks the protocol
/// before it: that daemon could not say whether the text arrived whole, so
/// the CLI types nothing and asks it nothing past the tree.
#[tokio::test]
async fn nebula_session_send_types_nothing_through_an_older_daemon() {
    use nebula_core::AgentStatus::Finished;
    let env = TestEnv::new();
    std::fs::create_dir_all(&env.runtime_dir).unwrap();
    let listener = tokio::net::UnixListener::bind(env.sock()).unwrap();
    let sending = spawn_cli(&env, &["session", "send", "a1", "carry on"]);
    let (mut c, _) = tokio::time::timeout(EVENT_TIMEOUT, listener.accept())
        .await
        .expect("the CLI never connected")
        .unwrap();
    let hello = read_frame::<ClientRequest, _>(&mut c).await.unwrap();
    assert!(
        matches!(hello, Some(ClientRequest::Hello { .. })),
        "{hello:?}"
    );
    write_frame(
        &mut c,
        &ServerEvent::HelloOk {
            protocol_version: nebula_core::TURN_RECEIPT_PROTOCOL - 1,
            daemon_pid: std::process::id(),
        },
    )
    .await
    .unwrap();
    serve_one_agent(&env, &mut c, Finished).await;
    let asked = read_frame::<ClientRequest, _>(&mut c).await.unwrap();
    assert!(asked.is_none(), "the CLI asked the older daemon {asked:?}");
    let stderr = refusal(&output_of(sending).await);
    assert!(
        stderr.contains("nothing was sent") && stderr.contains("`nebula reload`"),
        "stderr: {stderr}"
    );
}

/// `nebula orchestrator send`, end to end: it returns once the session
/// reports working on the prompt, so a `session wait --until-done` run
/// right after it waits for that turn, not the one before. A prompt nothing
/// reports is typed all the same, with exit code 3, and a session with a
/// dialog open is refused with nothing typed.
#[tokio::test]
async fn nebula_orchestrator_send_cli_returns_once_the_turn_is_taken() {
    use nebula_core::AgentStatus::{Finished, NeedsFeedback, Running};
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let worker = create_agent_get_id(&mut c, &main_worktree.id, "worker", 2).await;
    let orchestrator = create_agent_as(
        &mut c,
        &main_worktree.id,
        "orchestrator",
        3,
        nebula_core::AgentRole::Orchestrator,
    )
    .await;
    let id = worker.0.as_str();
    post_hook(&mut c, &worker, "UserPromptSubmit", Running).await;
    post_hook(&mut c, &worker, "Stop", Finished).await;

    // The prompt is a line for the stand-in agent's shell: a turn of 2 s.
    let turn = reported_turn(&env, &format!("sleep 2; {}", hook_curl("Stop")));
    let out = agent_cli(&env, &orchestrator, &["orchestrator", "send", id, &turn]);
    assert!(out.status.success(), "orchestrator send failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("it is working on it"), "{stdout}");
    let sent = tokio::time::Instant::now();
    let row = sessions_on(&env, "main")
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.id == worker)
        .unwrap();
    assert_eq!(
        row.status, Running,
        "send returned before the turn was taken"
    );
    let out = shell_cli(&env, &["session", "wait", id, "--until-done"]);
    assert!(out.status.success(), "session wait failed: {out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "finished\n");
    assert!(
        sent.elapsed() >= Duration::from_secs(2),
        "the wait returned {:?} after the send, on the turn before",
        sent.elapsed()
    );

    let out = agent_cli(&env, &orchestrator, &["orchestrator", "send", id, "true"]);
    assert_eq!(out.status.code(), Some(3), "an unreported prompt: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("has not reported taking it after 5 s")
            && stderr.contains("nothing says it arrived whole"),
        "{stderr}"
    );

    // The session reports taking only the end of what was typed, as Claude
    // Code did with a 1,536-byte line: the send fails, saying how each
    // starts.
    let cut = reported_turn_as(
        &env,
        Some("e the docs for the new flow"),
        &hook_curl("Stop"),
    );
    let stderr = refusal(&agent_cli(
        &env,
        &orchestrator,
        &["orchestrator", "send", id, &cut],
    ));
    assert!(
        stderr.contains("took only part of the text typed into it")
            && stderr.contains("its prompt starts \"e the docs for the new flow…\"")
            && stderr.contains("send the whole text again"),
        "{stderr}"
    );
    status_seen(&mut c, &worker, Finished).await;

    post_hook(&mut c, &worker, "UserPromptSubmit", Running).await;
    post_hook(&mut c, &worker, "PermissionRequest", NeedsFeedback).await;
    let marker = env.tmp.path().join("typed");
    let line = format!("touch {}", marker.display());
    let stderr = refusal(&agent_cli(
        &env,
        &orchestrator,
        &["orchestrator", "send", id, &line],
    ));
    assert!(stderr.contains("dialog open"), "stderr: {stderr}");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!marker.exists(), "the prompt was typed into the dialog");

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// The work an `orchestrator send` gave a captain comes back to the
/// orchestrator, end to end, though it starts no wait on it: once the
/// captain is done, nebula types a note into the orchestrator, at rest
/// with its input box empty, naming the captain and how to read what it
/// did. A send it does wait on is told by that wait, and no note follows.
#[tokio::test]
async fn work_an_orchestrator_sends_and_never_waits_on_is_typed_back_to_it() {
    use nebula_core::AgentRole::{Captain, Orchestrator};
    use nebula_core::AgentStatus::{Finished, Running};
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let captain = create_agent_as(&mut c, &main_worktree.id, "captain", 2, Captain).await;
    let orchestrator =
        create_agent_as(&mut c, &main_worktree.id, "orchestrator", 3, Orchestrator).await;
    // The orchestrator's stand-in shell prompts with Claude's input-box
    // mark on a row of its own, so an empty prompt reads as an empty box.
    type_into(&mut c, &orchestrator, "PS1='\\n❯ '").await;
    for agent in [&captain, &orchestrator] {
        post_hook(&mut c, agent, "UserPromptSubmit", Running).await;
        post_hook(&mut c, agent, "Stop", Finished).await;
    }
    let turn = reported_turn(&env, &format!("sleep 1; {}", hook_curl("Stop")));
    let send = || {
        agent_cli(
            &env,
            &orchestrator,
            &["orchestrator", "send", &captain.0, &turn],
        )
    };
    let out = send();
    assert!(out.status.success(), "orchestrator send failed: {out:?}");

    let note = format!(
        "[nebula] The captain of repo, {id}, is done with what you sent it. Read what it did \
         with nebula orchestrator read {id} --report.",
        id = captain.0
    );
    let deadline = tokio::time::Instant::now() + SPAWN_CHAIN_TIMEOUT;
    let typed = |rows: &[String]| rows.join("").contains(&note);
    loop {
        // Typed, and its Enter after it.
        let rows = screen_of(&env, &orchestrator.0);
        if typed(&rows) && rows.last().is_some_and(|r| r == "❯") {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the orchestrator was never told the captain is done: {rows:#?}"
        );
        tokio::time::sleep(POLL_STEP).await;
    }

    // Once it has moved on, a send it waits on is told by its wait alone.
    type_into(&mut c, &orchestrator, "clear").await;
    post_hook(&mut c, &orchestrator, "UserPromptSubmit", Running).await;
    post_hook(&mut c, &orchestrator, "Stop", Finished).await;
    let out = send();
    assert!(out.status.success(), "orchestrator send failed: {out:?}");
    let out = shell_cli(&env, &["session", "wait", &captain.0, "--until-done"]);
    assert!(out.status.success(), "session wait failed: {out:?}");
    tokio::time::sleep(nebula_core::DONE_SETTLE + Duration::from_secs(2)).await;
    let rows = screen_of(&env, &orchestrator.0);
    assert!(!typed(&rows), "a note followed the wait: {rows:#?}");

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// The rows `nebula session read <id>` prints now.
fn screen_of(env: &TestEnv, id: &str) -> Vec<String> {
    let out = shell_cli(env, &["session", "read", id]);
    assert!(out.status.success(), "nebula session read failed: {out:?}");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// `nebula session read <id>`, end to end: it prints what the stand-in
/// agent's terminal shows now, one screen of it.
#[tokio::test]
async fn nebula_session_read_cli_prints_the_screen() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent = create_agent_get_id(&mut c, &main_worktree.id, "worker", 2).await;
    let id = agent.0.as_str();

    let stderr = refusal(&shell_cli(&env, &["session", "read", "no-such-id"]));
    assert!(stderr.contains("`nebula sessions`"), "stderr: {stderr}");

    // Sixty rows through a 24-row PTY: the first ones scroll away.
    type_into(
        &mut c,
        &agent,
        "i=1; while [ $i -le 60 ]; do echo row-$i-; i=$((i+1)); done",
    )
    .await;
    let screen = read_until(&env, id, "row-60-").await;
    assert!(screen.len() <= 24, "one screen at most: {screen:#?}");
    let shown: Vec<&String> = screen
        .iter()
        .filter(|r| r.starts_with("row-") && r.ends_with('-'))
        .collect();
    let wanted: Vec<String> = (61 - shown.len()..=60)
        .map(|i| format!("row-{i}-"))
        .collect();
    assert_eq!(
        shown,
        wanted.iter().collect::<Vec<_>>(),
        "the end of the output"
    );
    assert!(shown.len() < 60, "the first rows scrolled away: {shown:#?}");
    assert!(
        !screen.last().unwrap().is_empty(),
        "trailing blank rows are dropped: {screen:#?}"
    );

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A stand-in agent CLI with an input box, which reads the way the real
/// ones read theirs: raw, a burst of bytes at a time. `boot` is shell that
/// runs first; what is typed meanwhile is lost. Then the box is up:
/// bracketed paste on, and nothing more printed. From there a carriage
/// return that arrives alone submits what the box holds, and one glued to
/// text is part of the text: how Claude Code leaves a long line unsubmitted
/// when its Enter is written straight behind it.
///
/// A burst is what arrives with no tenth of a second between its bytes, up
/// to 200 of them. The terminal's own timer tells (`min 200 time 1`: one
/// read waits for the first byte and returns at the first such gap), so
/// the stand-in starts no process between a text and its Enter. Reading
/// the rest of a burst with a second `stty` and `dd` did, and under load
/// those two can take longer than the pause `send` leaves.
///
/// It runs in the directory returned: every boot dumps its NEBULA_* env to
/// `<agent id>.env`, and every submitted turn is appended to `turns.log`
/// and reported through the prompt hook with the prompt it took, paste
/// marks dropped, as a CLI reports it, unless a file `mute` is there (a CLI
/// that reports nothing).
fn input_box_agent(env: &TestEnv, boot: &str) -> PathBuf {
    let dir = env.tmp.path().join("input-box");
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("agent.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n\
             cd '{dir}' || exit 1\n\
             env | grep '^NEBULA_' > \"$NEBULA_AGENT_ID.env\"\n\
             stty raw -echo\n\
             {boot}\n\
             stty min 0 time 0\n\
             dd bs=65536 count=1 >/dev/null 2>&1\n\
             : > box\n\
             stty min 200 time 1\n\
             printf '\\033[?2004h> '\n\
             while :; do\n\
               dd bs=65536 count=1 2>/dev/null > burst\n\
               [ -s burst ] || exit 0\n\
               if [ \"$(od -An -tx1 burst | tr -d ' \\n')\" = 0d ]; then\n\
                 {{ cat box; printf '\\n'; }} >> turns.log\n\
                 {payload}\n\
                 : > box\n\
                 [ -e mute ] || {hook} >/dev/null 2>&1\n\
               else\n\
                 cat burst >> box\n\
               fi\n\
             done\n",
            dir = dir.display(),
            hook = hook_curl_with("UserPromptSubmit", "@prompt.json"),
            payload = prompt_payload_sh("s1", "box", "prompt.json"),
        ),
    )
    .unwrap();
    make_executable(&script);
    dir
}

/// Shell that writes to the file `to` the `UserPromptSubmit` payload a CLI
/// posts for the prompt in the file `from`, as it reports one: escapes and
/// paste marks dropped, the rest JSON-escaped.
fn prompt_payload_sh(session_id: &str, from: &str, to: &str) -> String {
    format!(
        "{{ printf '{{\"session_id\":\"{session_id}\",\"prompt\":\"'; tr -d '\\033' < {from} \
         | sed -e 's/\\[20[01]~//g' -e 's/\\\\/\\\\\\\\/g' -e 's/\"/\\\\\"/g' \
         | awk 'NR > 1 {{ printf \"\\\\n\" }} {{ printf \"%s\", $0 }}'; printf '\"}}'; }} > {to}"
    )
}

/// The turns an [`input_box_agent`] in `dir` was sent, once there are `n`.
async fn turns_sent(dir: &Path, n: usize) -> String {
    let deadline = tokio::time::Instant::now() + SLOW_TIMEOUT;
    loop {
        let sent = std::fs::read_to_string(dir.join("turns.log")).unwrap_or_default();
        if sent.lines().count() >= n {
            return sent;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the stand-in never got turn {n}: {sent:?}"
        );
        tokio::time::sleep(POLL_STEP).await;
    }
}

/// `nebula session send <id> <text>`, end to end, into a stand-in with an
/// input box. The text is typed once the box is up and quiet, and its
/// Enter after a pause, so the turn is submitted: a short line as it is,
/// several lines or a long one as one paste. The send returns when the
/// session reports the whole turn, and with an exit code of its own when
/// it never does. Nothing is
/// typed into a session that is working (or starts to while the send
/// waits), has a dialog open or is archived, nor is text that carries a
/// control character.
#[tokio::test]
async fn nebula_session_send_cli_types_the_next_turn_into_a_session_at_rest() {
    use nebula_core::AgentStatus::{Finished, NeedsFeedback};
    let env = TestEnv::new();
    let repo = env.make_repo();
    let dir = input_box_agent(&env, ":");
    let mut daemon = env.spawn_daemon_with_agent_cmd(dir.join("agent.sh").to_str().unwrap());
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent = create_agent_get_id(&mut c, &main_worktree.id, "worker", 2).await;
    let id = agent.0.as_str();
    let agent_env = read_env_file(&dir.join(format!("{id}.env"))).await;
    let port: u16 = agent_env[env::API_URL]
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let token = agent_env[env::API_TOKEN].clone();
    let hook = |event: &str| format!("/api/hooks/claude?agentId={id}&hookEvent={event}");
    let send = |text: &[&str]| shell_cli(&env, &[&["session", "send", id], text].concat());

    // Several words need no quotes: they are one line, typed and entered.
    // The stand-in reports the turn through its hook, as a CLI does, so
    // the send returns with the session running.
    let out = send(&["now", "add", "a", "test"]);
    assert!(out.status.success(), "session send failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(id) && stdout.contains("\"worker\"") && stdout.contains("working on it"),
        "stdout: {stdout}"
    );
    assert_eq!(turns_sent(&dir, 1).await, "now add a test\n");
    let out = shell_cli(&env, &["session", "wait", id, "--timeout", "1"]);
    assert_eq!(
        out.status.code(),
        Some(124),
        "it waits for this turn: {out:?}"
    );

    // Working: its next turn has to wait for this one.
    let stderr = refusal(&send(&["too", "soon"]));
    assert!(stderr.contains("is working"), "stderr: {stderr}");
    // A dialog is open: text and its Enter would answer it blindly.
    assert_eq!(
        hook_post(port, &hook("PermissionRequest"), &token).await.0,
        200
    );
    status_seen(&mut c, &agent, NeedsFeedback).await;
    let stderr = refusal(&send(&["no,", "do", "not"]));
    assert!(stderr.contains("dialog"), "stderr: {stderr}");

    // At rest again. Line breaks: the lines arrive as the one paste the
    // box asked for, and the Enter after it submits them. Text that starts
    // with a hyphen, as a file handed over whole can, is text and no flag.
    assert_eq!(hook_post(port, &hook("Stop"), &token).await.0, 200);
    status_seen(&mut c, &agent, Finished).await;
    let out = send(&["- line one\n- line two"]);
    assert!(out.status.success(), "session send failed: {out:?}");
    assert_eq!(
        turns_sent(&dir, 3).await,
        "now add a test\n\x1b[200~- line one\n- line two\x1b[201~\n",
        "nothing typed while it was refused, then the paste"
    );

    // One line too long to type whole goes as one paste too, and the
    // stand-in's report of it holds the whole line: Claude Code lost the
    // start of a 1,536-byte line typed plain.
    assert_eq!(hook_post(port, &hook("Stop"), &token).await.0, 200);
    status_seen(&mut c, &agent, Finished).await;
    let long = "word ".repeat(300) + "end";
    let out = send(&[&long]);
    assert!(out.status.success(), "session send failed: {out:?}");
    assert!(
        turns_sent(&dir, 4)
            .await
            .ends_with(&format!("\n\x1b[200~{long}\x1b[201~\n")),
        "the long line, as one paste"
    );

    // The session starts working while a send waits for its box to go
    // quiet: the status is read again before anything is typed.
    assert_eq!(hook_post(port, &hook("Stop"), &token).await.0, 200);
    status_seen(&mut c, &agent, Finished).await;
    let sending = env
        .cli()
        .args(["session", "send", id, "too", "late"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        hook_post(port, &hook("UserPromptSubmit"), &token).await.0,
        200
    );
    let stderr = refusal(&output_of(sending).await);
    assert!(stderr.contains("is working"), "stderr: {stderr}");

    // A turn that is never reported: the text is typed all the same, and
    // the exit says the turn was not confirmed.
    assert_eq!(hook_post(port, &hook("Stop"), &token).await.0, 200);
    status_seen(&mut c, &agent, Finished).await;
    std::fs::write(dir.join("mute"), "").unwrap();
    let out = send(&["quiet", "one"]);
    assert_eq!(out.status.code(), Some(3), "typed, not confirmed: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not reported"), "stderr: {stderr}");
    assert!(turns_sent(&dir, 5).await.ends_with("\nquiet one\n"));

    // Text that is not text is refused before anything is asked.
    let stderr = refusal(&send(&["a\x1b[201~b"]));
    assert!(stderr.contains("control character"), "stderr: {stderr}");
    let stderr = refusal(&send(&["  "]));
    assert!(stderr.contains("text is empty"), "stderr: {stderr}");
    let stderr = refusal(&shell_cli(&env, &["session", "send", "no-such-id", "x"]));
    assert!(stderr.contains("`nebula sessions`"), "stderr: {stderr}");

    write_frame(
        &mut c,
        &ClientRequest::ArchiveAgent {
            req_id: 3,
            id: agent.clone(),
        },
    )
    .await
    .unwrap();
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 3).is_some()).await;
    let stderr = refusal(&send(&["late"]));
    assert!(stderr.contains("is archived"), "stderr: {stderr}");
    assert_eq!(turns_sent(&dir, 5).await.lines().count(), 5, "nothing more");

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A session the idle reaper took, end to end. `nebula session read`
/// refuses it and boots nothing. `nebula session send` brings it back and
/// types only once the input box is up: the stand-in boots the way a real
/// session does, bracketed paste switched on by the login shell's line
/// editor, then off, then on again with the box, and loses what is typed
/// before that.
#[tokio::test]
async fn nebula_session_send_cli_brings_a_reaped_session_back_before_it_types() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let dir = input_box_agent(
        &env,
        "printf '\\033[?2004h%% '\n\
         for i in 1 2 3; do sleep 0.1; printf .; done\n\
         printf '\\033[?2004l'\n\
         for i in 1 2 3 4; do sleep 0.2; printf 'loading %s\\r\\n' $i; done",
    );
    env.write_config(r#"{"session_idle_timeout": "2s"}"#);
    let mut daemon = env.spawn_daemon_with(
        dir.join("agent.sh").to_str().unwrap(),
        &[(env::IDLE_REAP_MS, "200")],
    );
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;
    let agent = create_agent_get_id(&mut c, &main_worktree.id, "idler", 2).await;
    let id = agent.0.as_str();

    // Nobody is looking: the reaper takes it.
    read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Agent(a) }
                if a.id == agent && !a.alive)
        })
    })
    .await;
    // The reaper reads its timeout afresh on every sweep: switched off, it
    // leaves the revived session alone for the rest of the test.
    env.write_config(r#"{"session_idle_timeout": "off"}"#);

    let stderr = refusal(&shell_cli(&env, &["session", "read", id]));
    assert!(
        stderr.contains("no live terminal")
            && stderr.contains("`nebula session send`")
            && stderr.contains("`nebula read`"),
        "stderr: {stderr}"
    );
    let session = &sessions_on(&env, "main").await.unwrap()[0];
    assert!(!session.alive, "reading boots nothing: {session:?}");

    let out = shell_cli(&env, &["session", "send", id, "carry", "on"]);
    assert!(out.status.success(), "session send failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("brought it back") && stdout.contains("working on it"),
        "stdout: {stdout}"
    );
    let session = &sessions_on(&env, "main").await.unwrap()[0];
    assert!(session.alive, "the session is back: {session:?}");
    assert_eq!(turns_sent(&dir, 1).await, "carry on\n");

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `nebula session delete <id>`, end to end: the row goes and the CLI says
/// so. With no session filed under it any more, a worktree whose checkout
/// `git worktree remove` takes away is dropped by the worktree sync.
#[tokio::test]
async fn nebula_session_delete_cli_removes_the_row_and_lets_a_removed_checkout_go() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    add_project_get_main_worktree(&mut c, &repo).await;

    // A session in a checkout of its own, started as a script would.
    let checkout = env.tmp.path().join("repo-worktrees").join("side");
    git_worktree_add(&repo, "side", &checkout);
    let started = stdout_json(&shell_cli(
        &env,
        &[
            "session",
            "start",
            "--in",
            checkout.to_str().unwrap(),
            "--name",
            "Side Job",
            "--json",
            "do it",
        ],
    ));
    let id = started["id"].as_str().unwrap();

    let out = shell_cli(&env, &["session", "delete", id]);
    assert!(out.status.success(), "session delete failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("deleted session")
            && stdout.contains(id)
            && stdout.contains("\"Side Job\""),
        "stdout: {stdout}"
    );
    let sessions = sessions_on(&env, "side")
        .await
        .expect("the worktree outlives its session");
    assert!(sessions.is_empty(), "the row is gone: {sessions:?}");
    let stderr = refusal(&shell_cli(&env, &["session", "delete", id]));
    assert!(stderr.contains("`nebula sessions`"), "stderr: {stderr}");

    // The checkout goes, and with nothing filed under it, so does its row.
    let removed = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["worktree", "remove", "--force"])
        .arg(&checkout)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(removed, "git worktree remove failed");
    let deadline = tokio::time::Instant::now() + SLOW_TIMEOUT;
    while sessions_on(&env, "side").await.is_some() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the worktree row outlived its checkout"
        );
        tokio::time::sleep(POLL_STEP).await;
    }

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// `nebula add <dir>` and the bare `nebula <dir>` shorthand: the one-shot CLI
/// resolves the path against its own cwd (the daemon's differs), registers
/// the repo over IPC, and surfaces daemon rejections as nonzero exits.
#[tokio::test]
async fn cli_add_project() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let mut daemon = env.spawn_daemon();
    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    subscribe(&mut c).await;

    let run_cli =
        |args: &[&str], cwd: &Path| env.cli().args(args).current_dir(cwd).output().unwrap();

    // `nebula add .` from inside the repo: cwd-relative resolution, project
    // named after the directory.
    let out = run_cli(&["add", "."], &repo);
    assert!(out.status.success(), "add . failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("added project"), "stdout: {stdout}");
    let canon = repo.canonicalize().unwrap();
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Project(p) }
                if p.repo_path == canon && p.name == "repo")
        })
    })
    .await;

    // The same repo again: the daemon's dedupe comes back as a failure.
    let out = run_cli(&["add", repo.to_str().unwrap()], env.tmp.path());
    assert!(!out.status.success(), "duplicate add must fail: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("already added"), "stderr: {stderr}");

    // Bare `nebula <dir>` shorthand on a second repo.
    let repo2 = env.tmp.path().join("repo2");
    std::fs::create_dir_all(&repo2).unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["commit", "--allow-empty", "-m", "init"],
    ] {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo2)
            .args(&args)
            .env("GIT_AUTHOR_NAME", "nebula-test")
            .env("GIT_AUTHOR_EMAIL", "test@nebula.dev")
            .env("GIT_COMMITTER_NAME", "nebula-test")
            .env("GIT_COMMITTER_EMAIL", "test@nebula.dev")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?} failed");
    }
    let out = run_cli(&[repo2.to_str().unwrap()], env.tmp.path());
    assert!(out.status.success(), "bare add failed: {out:?}");
    let canon2 = repo2.canonicalize().unwrap();
    read_events_until(&mut c, EVENT_TIMEOUT, |evs| {
        evs.iter().any(|e| {
            matches!(e, ServerEvent::EntityUpserted { entity: Entity::Project(p) }
                if p.repo_path == canon2 && p.name == "repo2")
        })
    })
    .await;

    // A directory that isn't a git repo is rejected by the daemon.
    let plain = env.tmp.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let out = run_cli(&["add", plain.to_str().unwrap()], env.tmp.path());
    assert!(!out.status.success(), "non-repo add must fail: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not a git repository"), "stderr: {stderr}");

    // A path that doesn't exist fails client-side, before any IPC.
    let out = run_cli(&["add", "does-not-exist"], env.tmp.path());
    assert!(!out.status.success(), "missing dir must fail: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("does not exist"), "stderr: {stderr}");

    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
}

/// A Claude Cloud row: `claude --cloud <task>` creates the session, prints
/// its id and exits, and the daemon captures the id off the PTY — the only
/// handle it ever gets. That is where the local side ends: the agent runs
/// in the cloud sandbox, and the row's pane is a panel linking to it.
/// Nothing is attached, teleported or re-homed on the user's behalf — the
/// stub runs exactly once, the checkout never switches branch — and the
/// paths that would boot a bare local CLI in the row's name (a restart, an
/// attach finding no PTY) refuse instead.
#[tokio::test]
async fn cloud_row_captures_its_session_id_and_runs_nothing_locally() {
    let env = TestEnv::new();
    let repo = env.make_repo();
    let state = env.tmp.path().join("cloud-stub");
    std::fs::create_dir_all(&state).unwrap();
    let stub = env.tmp.path().join("cloud-stub.sh");
    std::fs::write(
        &stub,
        format!(
            r#"#!/bin/sh
n=$(cat "{state}/runs" 2>/dev/null || echo 0)
n=$((n + 1))
echo "$n" > "{state}/runs"
pwd >> "{state}/cwds"
printf 'Created cloud session: Greet the world\r\n'
printf 'View: https://claude.ai/code/session_016SiQW5Lem2LbnUf1A3undt?from=cli&m=0\r\n'
printf 'Resume with: claude --teleport session_016SiQW5Lem2LbnUf1A3undt\r\n'
exit 0
"#,
            state = state.display()
        ),
    )
    .unwrap();
    make_executable(&stub);
    let runs = || {
        std::fs::read_to_string(state.join("runs"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let mut daemon = env.spawn_daemon_with_agent_cmd(stub.to_str().unwrap());

    let mut c = connect(&env.sock()).await;
    handshake(&mut c).await;
    let main_worktree = add_project_get_main_worktree(&mut c, &repo).await;

    // The create: the stub prints the session lines and exits at once.
    const CLOUD_ID: &str = "session_016SiQW5Lem2LbnUf1A3undt";
    write_frame(
        &mut c,
        &ClientRequest::CreateAgent {
            req_id: 10,
            worktree: main_worktree.id.clone(),
            // The stand-in name the TUI sends with AUTO-TITLE on: the
            // sandbox fires no hook, so the title comes off the create's
            // own output instead (issue #92).
            name: "agent".into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: true,
            cloud_prompt: Some("  Hello,\n  world  ".into()),
            starting_prompt: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, SLOW_TIMEOUT, |evs| {
        find_ack(evs, 10).is_some()
            && evs.iter().any(|e| {
                matches!(
                    e,
                    ServerEvent::EntityUpserted {
                        entity: Entity::Agent(a)
                    } if a.cloud_session_id.as_deref() == Some(CLOUD_ID)
                )
            })
    })
    .await;
    // The task is the row's first prompt from the create's own upsert on,
    // condensed like a typed one: the card says what the session was
    // asked to do before the CLI has printed a thing.
    let created = events
        .iter()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Agent(a),
            } if a.cloud_session_id.is_none() => Some(a.clone()),
            _ => None,
        })
        .expect("the create's own upsert");
    assert_eq!(
        created
            .recent_prompts
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>(),
        vec!["Hello, world"],
        "{created:#?}"
    );
    let ServerEvent::Ack {
        created: Some(EntityId::Agent(agent_id)),
        ..
    } = find_ack(&events, 10).unwrap()
    else {
        panic!("CreateAgent failed: {events:#?}");
    };
    let agent_id = agent_id.clone();

    // The create's exit is the end of the local story: the row goes dead
    // and stays dead — nothing re-enters the session in its name.
    let died = |evs: &[ServerEvent]| {
        evs.iter().any(|e| {
            matches!(
                e,
                ServerEvent::EntityUpserted {
                    entity: Entity::Agent(a)
                } if a.id == agent_id && !a.alive && a.cloud_session_id.as_deref() == Some(CLOUD_ID)
            )
        })
    };
    let mut events = read_events_until(&mut c, SLOW_TIMEOUT, died).await;
    assert!(died(&events), "the create pane should exit: {events:#?}");
    // Hold still past any cadence a follow could have had.
    events.extend(read_events_for(&mut c, EVENT_TIMEOUT).await);
    assert_eq!(
        runs(),
        "1",
        "the create is the only CLI run — no attach, no teleport"
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            ServerEvent::EntityUpserted { entity: Entity::Worktree(w) } if w.branch.starts_with("cloud-")
        )),
        "no cloud-<id> worktree is cut: {events:#?}"
    );
    let row = events
        .iter()
        .rev()
        .find_map(|e| match e {
            ServerEvent::EntityUpserted {
                entity: Entity::Agent(a),
            } if a.id == agent_id => Some(a.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        row.worktree_id, main_worktree.id,
        "the row stays where it was created"
    );
    assert_eq!(row.cloud_session_id.as_deref(), Some(CLOUD_ID));
    assert!(!row.alive);
    // Claude Cloud's title for the session is the row's name, read off
    // the `Created cloud session:` line, and the task stays its prompt.
    assert_eq!(row.name, "Greet the world");
    assert_eq!(
        row.recent_prompts
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>(),
        vec!["Hello, world"]
    );

    // The create ran in the user's checkout, and left its branch alone.
    let cwds = std::fs::read_to_string(state.join("cwds")).unwrap();
    let cwds: Vec<PathBuf> = cwds
        .lines()
        .map(|l| std::fs::canonicalize(l).unwrap())
        .collect();
    assert_eq!(
        cwds,
        vec![std::fs::canonicalize(&main_worktree.path).unwrap()]
    );
    let main_branch = std::process::Command::new("git")
        .args(["-C", repo.to_str().unwrap(), "branch", "--show-current"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&main_branch.stdout).trim(), "main");

    // A restart has nothing local to restart, and an attach finding no
    // PTY must not boot a bare CLI wearing the row's name: both refuse,
    // and the run count stays put.
    write_frame(
        &mut c,
        &ClientRequest::RestartAgent {
            req_id: 11,
            id: agent_id.clone(),
        },
    )
    .await
    .unwrap();
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| find_ack(evs, 11).is_some()).await;
    match find_ack(&events, 11) {
        Some(ServerEvent::Error { message, .. }) => {
            assert!(message.contains("runs in Claude Cloud"), "{message}")
        }
        other => panic!("a cloud row's restart must be refused: {other:?}"),
    }
    write_frame(
        &mut c,
        &ClientRequest::Attach {
            session: SessionRef::Agent(agent_id.clone()),
            from_seq: None,
            cols: 80,
            rows: 24,
        },
    )
    .await
    .unwrap();
    // The refusal names the session, so the pane waiting on it can say why.
    let refused = |e: &ServerEvent| {
        matches!(
            e,
            ServerEvent::AttachRefused { session: SessionRef::Agent(id), message }
                if *id == agent_id && message.contains("runs in Claude Cloud")
        )
    };
    let events = read_events_until(&mut c, EVENT_TIMEOUT, |evs| evs.iter().any(refused)).await;
    assert!(
        events.iter().any(refused),
        "a cloud row's attach must be refused: {events:#?}"
    );
    assert_eq!(runs(), "1", "neither verb spawned a CLI");

    // Shutting down must not spawn anything further.
    write_frame(&mut c, &ClientRequest::Shutdown).await.unwrap();
    wait_for_exit(&mut daemon);
    assert_eq!(runs(), "1");
}
