//! Client-side IPC: connect to the daemon, auto-spawning it when absent,
//! over [`nebula_client::connection`]'s socket and version handshake, and
//! the one-shot commands that each send it one request.

use anyhow::{bail, Context, Result};
use nebula_client::connection::{
    handshake, peer_pid, spawn_reader, try_connect, CLOSED_BEFORE_REPLY,
};
use nebula_core::codec::{read_frame, write_frame};
use nebula_core::{
    env, paths, Agent, AgentId, AgentKind, AgentRole, ClientRequest, EnterOutcome, EntityId, Link,
    Project, ProjectId, ServerEvent, SessionRef, SessionSummary, TerminalId, TerminalTab, Worktree,
    WorktreeId, CAPTAIN_PROTOCOL, READ_REPORT_PROTOCOL,
};
use std::collections::HashMap;
use std::time::Duration;

/// Request id for the one-shot CLIs: each opens a fresh connection, sends a
/// single request and waits for its reply, so there is never a second id.
const ONE_SHOT_REQ_ID: u64 = 1;
const SESSION_CONTEXT_PROTOCOL: u32 = 50;
/// How often the connect and shutdown waits re-check the daemon.
const POLL_STEP: Duration = Duration::from_millis(50);

pub use nebula_client::connection::Connection;

/// Connect, auto-spawning `current_exe() daemon` when nothing is listening.
pub async fn connect_or_spawn() -> Result<Connection> {
    let sock = paths::socket_path();

    if let Ok(conn) = try_connect(&sock).await {
        return handshake(conn).await;
    }

    if matches!(
        nebula_core::host::wsl_flavor(),
        Some(nebula_core::host::WslFlavor::Wsl1)
    ) {
        bail!(
            "nebula supports WSL2, but this looks like WSL1. \
             WSL1 does not provide the Linux kernel features the daemon needs \
             for its Unix socket, pidfile lock, and PTYs. Convert the distro \
             with `wsl.exe --set-version <distro> 2`, then run `nebula` again."
        );
    }

    spawn_daemon()?;

    // Poll-connect while the daemon boots.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        match try_connect(&sock).await {
            Ok(conn) => return handshake(conn).await,
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(POLL_STEP).await;
            }
            Err(e) => {
                return Err(e).with_context(|| {
                    format!(
                        "daemon did not come up on {} — check {}",
                        sock.display(),
                        paths::daemon_log_path().display()
                    )
                })
            }
        }
    }
}

fn spawn_daemon() -> Result<()> {
    use std::os::unix::process::CommandExt;
    let exe = nebula_core::paths::current_exe().context("resolve current_exe")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // New *session*, not just a new process group: besides outliving this
    // client and skipping its terminal signals (Ctrl+C etc.), the daemon must
    // hold no controlling terminal. It shells out to the user's interactive
    // shell (CLI probes, login-shell agent wrap), and an interactive zsh that
    // can reach a tty via /dev/tty grabs its foreground process group —
    // SIGTTIN-stopping the TUI running on this terminal mid-frame.
    unsafe {
        cmd.pre_exec(own_session);
    }
    cmd.spawn().context("spawn nebula daemon")?;
    Ok(())
}

/// `pre_exec` hook putting the child in a session of its own (`setsid`).
/// Avoids a libc dependency for one call (same pattern as nebula-core's
/// geteuid).
pub(crate) fn own_session() -> std::io::Result<()> {
    extern "C" {
        fn setsid() -> i32;
    }
    if unsafe { setsid() } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Channel-based IPC handle for the TUI event loop: outbound requests go
/// through `tx`; inbound events arrive on `rx`. Reader/writer tasks own the
/// socket halves.
pub struct IpcChannels {
    pub tx: tokio::sync::mpsc::Sender<ClientRequest>,
    pub rx: tokio::sync::mpsc::Receiver<ServerEvent>,
    /// The protocol the DAEMON said it speaks, so a request it would not
    /// know is never sent to it.
    pub protocol_version: u32,
    writer: tokio::task::JoinHandle<()>,
}

impl IpcChannels {
    /// Wait, up to `limit`, for every request already sent to reach the
    /// socket. The last one before the process goes is the `SaveUiState`
    /// the next launch restores from, and the writer task would otherwise
    /// race the exit (or the exec) for it.
    pub async fn flush(self, limit: Duration) {
        drop(self.tx);
        let _ = tokio::time::timeout(limit, self.writer).await;
    }
}

pub fn split_connection(conn: Connection) -> IpcChannels {
    let protocol_version = conn.protocol_version;
    let (read_half, mut write_half) = conn.stream.into_split();
    let event_rx = spawn_reader(read_half);
    let (req_tx, mut req_rx) = tokio::sync::mpsc::channel::<ClientRequest>(256);

    let writer = tokio::spawn(async move {
        while let Some(req) = req_rx.recv().await {
            if write_frame(&mut write_half, &req).await.is_err() {
                break;
            }
        }
    });

    IpcChannels {
        tx: req_tx,
        rx: event_rx,
        protocol_version,
        writer,
    }
}

/// The agent id a one-shot CLI runs as, from the raw `NEBULA_AGENT_ID`
/// value. Unset and empty are the same miss, and the error names the
/// `nebula <verb>` that needs it. Pure so the message is testable.
fn agent_id_from(value: Option<String>, verb: &str) -> Result<String> {
    value.filter(|v| !v.is_empty()).with_context(|| {
        format!(
            "{} is not set — `nebula {verb}` only works from inside a \
             nebula agent session",
            env::AGENT_ID
        )
    })
}

/// [`agent_id_from`] over the live environment.
fn current_agent_id(verb: &str) -> Result<String> {
    agent_id_from(env::non_empty(env::AGENT_ID), verb)
}

/// What the daemon said back to a one-shot request.
enum Reply {
    /// Done, and what the request created, for one that creates a row.
    Ack(Option<EntityId>),
    /// The daemon's own message for a request it declined.
    Error(String),
}

/// Read events until the daemon answers `req_id` — an Ack or an Error —
/// skipping anything else it broadcasts meanwhile. Only the hang-up is an
/// `Err`; the daemon's refusal comes back as a [`Reply::Error`] so each
/// caller can decide whether that is a failure.
async fn await_reply(conn: &mut Connection, req_id: u64) -> Result<Reply> {
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Ack { req_id: r, created }) if r == req_id => {
                return Ok(Reply::Ack(created))
            }
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => return Ok(Reply::Error(message)),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

fn require_protocol(conn: &Connection, needed: u32, command: &str) -> Result<()> {
    if conn.protocol_version < needed {
        bail!(
            "`nebula {command}` needs daemon protocol v{needed}, but the running daemon speaks v{} — run `nebula kill` and relaunch",
            conn.protocol_version
        );
    }
    Ok(())
}

/// [`await_reply`] for the callers where the daemon declining *is* the
/// failure: its message becomes the error.
async fn await_ack(conn: &mut Connection, req_id: u64) -> Result<Option<EntityId>> {
    match await_reply(conn, req_id).await? {
        Reply::Ack(created) => Ok(created),
        Reply::Error(message) => bail!("{message}"),
    }
}

/// [`await_ack`] for a request that starts a session: the session its Ack
/// names.
async fn await_started(conn: &mut Connection, req_id: u64) -> Result<AgentId> {
    match await_ack(conn, req_id).await? {
        Some(EntityId::Agent(id)) => Ok(id),
        _ => bail!("the daemon acknowledged the session without naming it"),
    }
}

/// The session `id` a spawn just started and the worktree it runs in, read
/// off a Snapshot asked for after the spawn's Ack: by then the row, and any
/// worktree the spawn cut, are in the store.
async fn started_session(conn: &mut Connection, id: &AgentId) -> Result<(Agent, Worktree)> {
    write_frame(&mut conn.stream, &ClientRequest::Subscribe).await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Snapshot {
                worktrees, agents, ..
            }) => {
                let agent = agents
                    .into_iter()
                    .find(|a| &a.id == id)
                    .with_context(|| format!("session {id} was started, then went"))?;
                let worktree = worktrees
                    .into_iter()
                    .find(|w| w.id == agent.worktree_id)
                    .with_context(|| format!("session {id} was started in no worktree"))?;
                return Ok((agent, worktree));
            }
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

/// What `--json` prints for a session a command started, the one shape
/// `nebula session start`, `nebula spawn` and `nebula orchestrator spawn`
/// share: the session's `id` and `name`, and the `worktree` id and checkout
/// `path` it runs in.
pub(crate) fn started_json(id: &AgentId, name: &str, worktree: &Worktree) -> Result<String> {
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "id": id,
        "name": name,
        "worktree": worktree.id,
        "path": worktree.path.to_string_lossy(),
    }))?)
}

/// What `--json` prints for the session `id` a spawn just started, from
/// `read`, its row and worktree read back after the spawn's Ack. The spawn
/// has happened whatever the read did, so one that failed still prints the
/// id, the rest null, and says why on stderr: a failing exit would have
/// the caller start the work again, in a second session.
fn started_json_or_id(id: &AgentId, read: Result<(Agent, Worktree)>) -> Result<String> {
    match read {
        Ok((agent, home)) => started_json(id, &agent.name, &home),
        Err(err) => {
            eprintln!(
                "warning: session {id} was started, but reading it back failed ({err:#}): only \
                 its id is printed"
            );
            Ok(serde_json::to_string_pretty(&serde_json::json!({
                "id": id,
                "name": null,
                "worktree": null,
                "path": null,
            }))?)
        }
    }
}

/// How `nebula rename` treats a session that already carries a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameMode {
    /// Title only an untitled session; the daemon declines otherwise.
    Auto,
    /// Overwrite whatever title the session has (`--force`).
    Force,
}

/// One-shot client for `nebula rename`, run from inside an agent session's
/// CLI: resolve the agent from NEBULA_AGENT_ID and ask the daemon to title
/// it. Never spawns a daemon — no daemon means no session worth titling.
///
/// Daemon-reported outcomes (renamed, or "already titled" on the non-force
/// path) both print and exit 0: for the model running this, a declined
/// auto-title is a settled answer, not a failure to retry.
pub async fn rename_current_agent(title: &str, mode: RenameMode) -> Result<()> {
    let agent_id = current_agent_id("rename")?;
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no nebula daemon is running — title unchanged");
    };
    let mut conn = handshake(stream).await?;
    let req_id = ONE_SHOT_REQ_ID;
    let id = AgentId(agent_id);
    let name = title.to_string();
    let request = match mode {
        RenameMode::Force => ClientRequest::RenameAgent { req_id, id, name },
        RenameMode::Auto => ClientRequest::AutoRenameAgent { req_id, id, name },
    };
    write_frame(&mut conn.stream, &request).await?;
    match await_reply(&mut conn, req_id).await? {
        Reply::Ack(_) => println!("session renamed to \"{title}\""),
        Reply::Error(message) => println!("nebula: {message}"),
    }
    Ok(())
}

/// CLI: `nebula spawn "<task>" [--kind <claude|codex|cursor>] [--worktree
/// <branch> [--base <ref>]] [--json]` from inside an agent session — ask
/// the daemon to start a new agent beside this one, in the same worktree or
/// in the project's worktree on `worktree` (created when the branch has
/// none), opening on `task` as its first prompt. The caller is untouched:
/// no relocation, no turn-end wait. Never spawns a daemon: no daemon means
/// no session to sit beside.
///
/// What this prints is read by the model that ran it, so it says what
/// happened, the new session's id to follow it by, and that this session
/// carries on; `json` prints [`started_json`] instead. A daemon-side
/// refusal (a blank task, a missing CLI, a worktree git could not create)
/// is a nonzero exit the model reports.
pub async fn spawn_sibling_for_current_agent(
    task: &str,
    kind: Option<AgentKind>,
    worktree: Option<String>,
    base: Option<String>,
    name: Option<&str>,
    json: bool,
) -> Result<()> {
    let agent_id = current_agent_id("spawn")?;
    let task = task.trim();
    if task.is_empty() {
        bail!("the task is empty — `nebula spawn \"<task>\"` needs the work the new session starts on");
    }
    // The words become a branch as `nebula worktree`'s do, so both
    // commands land on the same branch for the same words. Unlike there, no
    // name is invented for a blank one: a spawn names where its work goes.
    let worktree = match worktree.as_deref().map(crate::branch_name::slugify) {
        Some(slug) if slug.is_empty() => {
            bail!("the branch is empty — `nebula spawn --worktree <branch>` needs a branch name")
        }
        other => other,
    };
    let base = match base.as_deref().map(str::trim) {
        Some("") => bail!("the base is empty — `--base <ref>` needs a branch, tag or commit"),
        other => other.map(str::to_string),
    };
    let name = match name.map(str::trim) {
        Some("") => bail!("the name is empty: `--name <title>` needs the session's title"),
        other => other,
    };
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no nebula daemon is running — no session started");
    };
    let mut conn = handshake(stream).await?;
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::SpawnSiblingAgent {
            req_id,
            id: AgentId(agent_id),
            kind,
            starting_prompt: task.to_string(),
            worktree: worktree.clone(),
            base,
        },
    )
    .await?;
    let id = await_started(&mut conn, req_id).await?;
    // The title it keeps: a rename by name takes it off AUTO-TITLE, as the
    // user's own rename does.
    if let Some(name) = name {
        let rename = req_id + 1;
        write_frame(
            &mut conn.stream,
            &ClientRequest::RenameAgent {
                req_id: rename,
                id: id.clone(),
                name: name.to_string(),
            },
        )
        .await?;
        await_ack(&mut conn, rename)
            .await
            .with_context(|| format!("started session {id}, but it was not named"))?;
    }
    if let Some(kind) = kind {
        crate::session_cli::warn_if_statusless(&crate::config::Config::load(), kind);
    }
    if json {
        let read = started_session(&mut conn, &id).await;
        println!("{}", started_json_or_id(&id, read)?);
        return Ok(());
    }
    let harness = kind.map(|k| format!("{} ", k.as_str())).unwrap_or_default();
    let place = match &worktree {
        Some(branch) => format!("the worktree on branch \"{branch}\""),
        None => "this worktree".to_string(),
    };
    println!(
        "started a new {harness}session {id} in {place}; it is working on that task now and \
         shows in the sessions list. This session is unaffected — carry on."
    );
    Ok(())
}

/// CLI: `nebula open <file>…` from inside an agent session — resolve the
/// paths here, where the cwd is the agent's, and hand them to the daemon,
/// which raises every attached TUI's FILE TABS on them. Never spawns a
/// daemon: no daemon means nobody is looking. What this prints is read by
/// the model that ran it.
pub async fn open_files_for_current_agent(files: &[String]) -> Result<()> {
    let agent_id = current_agent_id("open")?;
    if files.is_empty() {
        bail!("nothing to open — `nebula open <file>…` needs at least one file");
    }
    let mut resolved = Vec::with_capacity(files.len());
    for file in files {
        let path = std::fs::canonicalize(file)
            .with_context(|| format!("can't open {file}: no such file"))?;
        if !path.is_file() {
            bail!("can't open {file}: not a file");
        }
        // The preview pane can render text, local images and Mermaid
        // diagrams. Other binaries are refused here, before IPC.
        let openable = crate::tree_browser::is_openable_file(&path)
            .with_context(|| format!("can't open {file}: unreadable"))?;
        if !openable {
            bail!(
                "can't open {file}: unsupported binary file — nebula opens text, images and \
                 Mermaid diagrams; name this path in your reply instead"
            );
        }
        resolved.push(path);
    }
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no nebula daemon is running — nothing opened");
    };
    let mut conn = handshake(stream).await?;
    let req_id = ONE_SHOT_REQ_ID;
    let count = resolved.len();
    write_frame(
        &mut conn.stream,
        &ClientRequest::OpenFiles {
            req_id,
            id: AgentId(agent_id),
            paths: resolved,
        },
    )
    .await?;
    await_ack(&mut conn, req_id).await?;
    let noun = if count == 1 { "file" } else { "files" };
    println!(
        "opened {count} {noun} in nebula's file tabs — the user is looking at them now, one tab \
         each, with a preview and an editor. Don't paste their contents into your reply; carry on."
    );
    Ok(())
}

async fn session_context_connection(command: &str) -> Result<Connection> {
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no nebula daemon is running");
    };
    let conn = handshake(stream).await?;
    require_protocol(&conn, SESSION_CONTEXT_PROTOCOL, command)?;
    Ok(conn)
}

async fn fetch_sessions(conn: &mut Connection) -> Result<Vec<SessionSummary>> {
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(&mut conn.stream, &ClientRequest::ListSessions { req_id }).await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::SessionList {
                req_id: r,
                sessions,
            }) if r == req_id => return Ok(sessions),
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

/// Each session's role by its id, off the Snapshot a Subscribe answers
/// with: `nebula sessions` shows it, and the list's rows have no room for
/// it on the wire.
async fn session_roles(conn: &mut Connection) -> Result<HashMap<String, AgentRole>> {
    write_frame(&mut conn.stream, &ClientRequest::Subscribe).await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Snapshot { agents, .. }) => {
                return Ok(agents.into_iter().map(|a| (a.id.0, a.role)).collect())
            }
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

pub async fn list_sessions(json: bool) -> Result<()> {
    let mut conn = session_context_connection("sessions").await?;
    let sessions = fetch_sessions(&mut conn).await?;
    let roles = session_roles(&mut conn).await?;
    if json {
        println!("{}", sessions_json(&sessions, &roles)?);
    } else {
        for session in &sessions {
            println!("{}", session_line(session, roles.get(&session.id).copied()));
        }
    }
    Ok(())
}

/// `nebula sessions --json`: the daemon's rows, each agent's with its
/// `role` beside them.
fn sessions_json(
    sessions: &[SessionSummary],
    roles: &HashMap<String, AgentRole>,
) -> Result<String> {
    let mut rows = Vec::new();
    for session in sessions {
        let mut row = serde_json::to_value(session)?;
        if let (Some(role), Some(obj)) = (roles.get(&session.id), row.as_object_mut()) {
            obj.insert("role".into(), role.as_str().into());
        }
        rows.push(row);
    }
    Ok(serde_json::to_string_pretty(
        &serde_json::json!({ "sessions": rows }),
    )?)
}

pub async fn read_session(session: &str, lines: u32, report: bool) -> Result<()> {
    let mut conn = session_context_connection("read").await?;
    if report {
        require_protocol(&conn, READ_REPORT_PROTOCOL, "read --report")?;
    }
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::ReadSession {
            req_id,
            target: session.to_string(),
            max_lines: lines,
            report,
        },
    )
    .await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::SessionText {
                req_id: r, text, ..
            }) if r == req_id => {
                print!("{text}");
                return Ok(());
            }
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

pub async fn ask_session(
    session: &str,
    question: &str,
    timeout_secs: u64,
    wait: bool,
) -> Result<()> {
    let mut conn = session_context_connection("ask").await?;
    let question = question.trim();
    if question.is_empty() {
        bail!("question is empty");
    }
    let caller = env::non_empty(env::AGENT_ID).map(AgentId);
    let timeout_ms = timeout_secs.saturating_mul(1000);
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::AskSession {
            req_id,
            caller,
            target: session.to_string(),
            question: question.to_string(),
            timeout_ms,
            wait,
        },
    )
    .await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::SessionAnswer {
                req_id: r, answer, ..
            }) if r == req_id => {
                println!("{answer}");
                return Ok(());
            }
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

/// One row of `nebula sessions`: a session with a role other than a
/// worker's says it (`role=captain`) before its summary.
fn session_line(session: &SessionSummary, role: Option<AgentRole>) -> String {
    let role = role
        .filter(|r| *r != AgentRole::Worker)
        .map(|r| format!(" role={}", r.as_str()))
        .unwrap_or_default();
    format!(
        "{} name={} project={} branch={} worktree={} harness={} status={}{}{role} summary={}",
        session.id,
        shell_word(&session.name),
        shell_word(&session.project),
        shell_word(&session.branch),
        session.worktree.display(),
        session.harness,
        session.status,
        if session.alive { " alive" } else { "" },
        shell_word(&session.summary)
    )
}

fn shell_word(value: &str) -> String {
    if value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
    {
        value.to_string()
    } else {
        format!("{value:?}")
    }
}

#[derive(Debug, Clone)]
struct OrchestratorTree {
    projects: Vec<Project>,
    worktrees: Vec<Worktree>,
    agents: Vec<Agent>,
    terminals: Vec<TerminalTab>,
    links: Vec<Link>,
}

async fn orchestrator_connection() -> Result<(AgentId, Connection)> {
    let agent_id = AgentId(current_agent_id("orchestrator")?);
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no nebula daemon is running");
    };
    Ok((agent_id, handshake(stream).await?))
}

async fn fetch_orchestrator_tree(
    caller: &AgentId,
    conn: &mut Connection,
) -> Result<OrchestratorTree> {
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::OrchestratorList {
            req_id,
            caller: caller.clone(),
        },
    )
    .await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::OrchestratorList {
                req_id: r,
                projects,
                worktrees,
                agents,
                terminals,
                links,
            }) if r == req_id => {
                return Ok(OrchestratorTree {
                    projects,
                    worktrees,
                    agents,
                    terminals,
                    links,
                });
            }
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

pub async fn orchestrator_list(json: bool) -> Result<()> {
    let (caller, mut conn) = orchestrator_connection().await?;
    let tree = fetch_orchestrator_tree(&caller, &mut conn).await?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "projects": tree.projects,
                "worktrees": tree.worktrees,
                "agents": tree.agents,
                "terminals": tree.terminals,
                "links": tree.links,
            }))?
        );
    } else {
        print_orchestrator_tree(&tree);
    }
    Ok(())
}

pub async fn orchestrator_read(session: &str, bytes: u32, report: bool) -> Result<()> {
    let (caller, mut conn) = orchestrator_connection().await?;
    if report {
        require_protocol(&conn, READ_REPORT_PROTOCOL, "orchestrator read --report")?;
    }
    let session = parse_session_ref(session);
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::OrchestratorRead {
            req_id,
            caller,
            session: session.clone(),
            max_bytes: bytes,
            report,
        },
    )
    .await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::OutputTail {
                req_id: r,
                session: _,
                tail,
            }) if r == req_id => {
                match tail {
                    Some(tail) => print!("{}", String::from_utf8_lossy(&tail.data)),
                    None => println!("(session is not live)"),
                }
                return Ok(());
            }
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

/// CLI: `nebula orchestrator send <session> <prompt>`: type the prompt into
/// the session and return, as `nebula session send` does, once it reports
/// taking the whole of it, so a `session wait` run straight after waits for
/// this turn and not the last. One that took only part of it fails the
/// command. A session already working takes it when it next reads input.
/// One with a dialog open is refused: the prompt and its Enter would answer
/// the dialog blindly. When no report comes within
/// [`nebula_core::TURN_RECEIPT_WAIT`] the prompt was typed all the same,
/// and the exit says so.
pub async fn orchestrator_send(session: &str, prompt: &str) -> Result<std::process::ExitCode> {
    use crate::session_cli::{
        session_by_id, Client, CANNOT_CONFIRM, DIALOG_OPEN, EXIT_UNCONFIRMED,
    };
    use nebula_core::AgentStatus::{NeedsFeedback, Running};
    use nebula_core::{TURN_RECEIPT_PROTOCOL, TURN_RECEIPT_WAIT};
    let caller = AgentId(current_agent_id("orchestrator")?);
    let mut client = Client::connect("nothing was sent").await?;
    if client.protocol < TURN_RECEIPT_PROTOCOL {
        bail!("nothing was sent: {CANNOT_CONFIRM}");
    }
    let row = session_by_id(&client.tree, session)?;
    if row.status == NeedsFeedback {
        bail!("session {session} {DIALOG_OPEN}");
    }
    let working = row.status == Running;
    let target = row.id.clone();
    let message = prompt.to_string();
    let confirmed = client
        .type_turn(|req_id| ClientRequest::OrchestratorSend {
            req_id,
            caller,
            target,
            message,
        })
        .await?;
    if confirmed {
        println!("sent to {session}; it is working on it.");
        return Ok(std::process::ExitCode::SUCCESS);
    }
    let waited = TURN_RECEIPT_WAIT.as_secs();
    if working {
        eprintln!(
            "sent to {session} while it was working: it takes it when it next reads input, \
             and had not after {waited} s, so nothing says it arrived whole."
        );
    } else {
        eprintln!(
            "sent to {session}, but it has not reported taking it after {waited} s, so nothing \
             says it arrived whole."
        );
    }
    Ok(std::process::ExitCode::from(EXIT_UNCONFIRMED))
}

/// What `nebula orchestrator spawn` was asked for: the task, and each flag
/// under its own name.
#[derive(Debug)]
pub struct OrchestratorSpawnOpts {
    pub project: String,
    pub worktree: Option<String>,
    pub base: Option<String>,
    pub kind: AgentKind,
    pub name: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub task: String,
    pub json: bool,
}

/// A flag's value with its whitespace trimmed, or `None` for a blank one.
fn given(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// CLI: `nebula orchestrator spawn`: start a session on the task in the
/// named project, in its root checkout or the worktree `--worktree` names.
/// A `--name` is kept: the session is not auto-titled over it. A
/// `--model` and `--effort` are what the harness launches with, the effort
/// fitted to the model as every launch surface fits it; left out, the
/// harness launches on its own defaults.
pub async fn orchestrator_spawn(opts: OrchestratorSpawnOpts) -> Result<()> {
    let OrchestratorSpawnOpts {
        kind, task, json, ..
    } = opts;
    let model = given(opts.model);
    let effort = given(opts.effort)
        .and_then(|e| crate::config::fit_effort(kind, model.as_deref(), Some(e), None));
    let (caller, mut conn) = orchestrator_connection().await?;
    let tree = fetch_orchestrator_tree(&caller, &mut conn).await?;
    let project_id = resolve_project(&tree, &opts.project)?;
    let (worktree_id, branch) = match opts.worktree.as_deref() {
        Some(target) => resolve_worktree_or_branch(&tree, &project_id, target),
        None => (None, None),
    };
    let req_id = ONE_SHOT_REQ_ID + 1;
    write_frame(
        &mut conn.stream,
        &ClientRequest::OrchestratorSpawn {
            req_id,
            caller,
            project: project_id,
            worktree: worktree_id,
            branch,
            base: opts.base,
            name: given(opts.name).unwrap_or_default(),
            kind,
            custom_harness: None,
            model,
            effort,
            starting_prompt: task,
        },
    )
    .await?;
    let id = await_started(&mut conn, req_id).await?;
    crate::session_cli::warn_if_statusless(&crate::config::Config::load(), kind);
    if json {
        let read = started_session(&mut conn, &id).await;
        println!("{}", started_json_or_id(&id, read)?);
    } else {
        println!("started a new {} session {id} for that task", kind.as_str());
    }
    Ok(())
}

/// CLI: `nebula orchestrator captain <project>`: the project's CAPTAIN,
/// found or started by the daemon, printed by its id.
pub async fn orchestrator_captain(project: &str, json: bool) -> Result<()> {
    let (caller, mut conn) = orchestrator_connection().await?;
    require_protocol(&conn, CAPTAIN_PROTOCOL, "orchestrator captain")?;
    let tree = fetch_orchestrator_tree(&caller, &mut conn).await?;
    let project_id = resolve_project(&tree, project)?;
    let req_id = ONE_SHOT_REQ_ID + 1;
    write_frame(
        &mut conn.stream,
        &ClientRequest::OrchestratorCaptain {
            req_id,
            caller,
            project: project_id,
        },
    )
    .await?;
    let id = await_started(&mut conn, req_id).await?;
    if json {
        let read = started_session(&mut conn, &id).await;
        println!("{}", started_json_or_id(&id, read)?);
    } else {
        println!("the captain of {project} is session {id}");
    }
    Ok(())
}

pub async fn orchestrator_review(session: &str) -> Result<()> {
    let (caller, mut conn) = orchestrator_connection().await?;
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::OrchestratorOpenReview {
            req_id,
            caller,
            sessions: vec![parse_session_ref(session)],
            tabs: Vec::new(),
        },
    )
    .await?;
    await_ack(&mut conn, req_id).await?;
    println!("opened review in nebula");
    Ok(())
}

fn parse_session_ref(raw: &str) -> SessionRef {
    if let Some(id) = raw
        .strip_prefix("terminal:")
        .or_else(|| raw.strip_prefix("term:"))
    {
        SessionRef::Terminal(TerminalId(id.to_string()))
    } else {
        SessionRef::Agent(AgentId(raw.to_string()))
    }
}

fn resolve_project(tree: &OrchestratorTree, raw: &str) -> Result<ProjectId> {
    let matches = tree
        .projects
        .iter()
        .filter(|p| p.id.as_str() == raw || p.name == raw)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [project] => Ok(project.id.clone()),
        [] => bail!("project not found: {raw}"),
        _ => bail!("project name is ambiguous: {raw}; use its id"),
    }
}

fn resolve_worktree_or_branch(
    tree: &OrchestratorTree,
    project: &ProjectId,
    raw: &str,
) -> (Option<WorktreeId>, Option<String>) {
    match tree
        .worktrees
        .iter()
        .find(|w| &w.project_id == project && (w.id.as_str() == raw || w.branch == raw))
    {
        Some(worktree) => (Some(worktree.id.clone()), None),
        None => (None, Some(raw.to_string())),
    }
}

fn print_orchestrator_tree(tree: &OrchestratorTree) {
    for project in &tree.projects {
        println!(
            "project {} name={} path={}",
            project.id,
            project.name,
            project.repo_path.display()
        );
        for worktree in tree.worktrees.iter().filter(|w| w.project_id == project.id) {
            println!(
                "  worktree {} branch={}{} path={}",
                worktree.id,
                worktree.branch,
                if worktree.is_main { " root" } else { "" },
                worktree.path.display()
            );
            for agent in tree.agents.iter().filter(|a| a.worktree_id == worktree.id) {
                println!(
                    "    agent {} name={} kind={} status={}{}{} role={}",
                    agent.id,
                    agent.name,
                    agent.kind.as_str(),
                    agent.status.as_str(),
                    if agent.alive { " alive" } else { "" },
                    if agent.unseen { " unseen" } else { "" },
                    agent.role.as_str()
                );
            }
            for term in tree
                .terminals
                .iter()
                .filter(|t| t.worktree_id == worktree.id)
            {
                println!(
                    "    terminal {} name={}{}{}",
                    term.id,
                    term.name,
                    if term.alive { " alive" } else { "" },
                    if term.run_command.is_some() {
                        " run"
                    } else {
                        ""
                    }
                );
            }
        }
    }
}

/// CLI: `nebula worktree [name] [--base <ref>]` from inside an agent
/// session — take (or create) the named worktree of this session's project
/// and have the daemon move the session into it. A blank name is invented
/// the way the TUI's new-worktree prompt invents one, and spaces slugify to
/// hyphens the same way. Never spawns a daemon: no daemon means no session
/// to move.
///
/// What this prints is read by the model that ran it, so the relocating
/// case spells out what happens next: the daemon kills and resumes this
/// very process once the turn ends, and the answer has to be finished by
/// then. A daemon-side failure is a nonzero exit — the model reports it and
/// stays put.
pub async fn enter_worktree_for_current_agent(name: &str, base: Option<String>) -> Result<()> {
    let agent_id = current_agent_id("worktree")?;
    let branch = match crate::branch_name::slugify(name) {
        slug if slug.is_empty() => crate::branch_name::random_name(&[]),
        slug => slug,
    };
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no nebula daemon is running — nothing to move");
    };
    let mut conn = handshake(stream).await?;
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::EnterWorktree {
            req_id,
            id: AgentId(agent_id),
            branch,
            base,
        },
    )
    .await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::WorktreeEntered {
                req_id: r,
                worktree,
                outcome,
            }) if r == req_id => {
                println!(
                    "worktree \"{}\" is ready at {}",
                    worktree.branch,
                    worktree.path.display()
                );
                match outcome {
                    EnterOutcome::AlreadyThere => {
                        println!("this session already runs inside it — nothing to move.");
                    }
                    EnterOutcome::Relocating => {
                        println!(
                            "this session is now associated with it; nebula will relocate the \
                             session into it the moment this turn ends."
                        );
                        println!(
                            "Finish now: tell the user in one line that the session is moving \
                             into the worktree, and make no further tool calls or edits — you \
                             will be resumed inside the worktree with a prompt to continue."
                        );
                    }
                    EnterOutcome::NextLaunch => {
                        println!(
                            "this session is now associated with it and runs there from its \
                             next launch."
                        );
                    }
                }
                return Ok(());
            }
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

/// A directory named on the command line, as the daemon knows paths:
/// absolute and canonical (on macOS `/tmp` is `/private/tmp`), with a leading
/// `~/` expanded, since a quoted argument never is.
pub(crate) fn existing_dir(path: &str) -> Result<std::path::PathBuf> {
    let expanded = match (path.strip_prefix("~/"), env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => std::path::PathBuf::from(path),
    };
    let dir = std::fs::canonicalize(&expanded)
        .with_context(|| format!("{} does not exist", expanded.display()))?;
    if !dir.is_dir() {
        bail!("{} is not a directory", dir.display());
    }
    Ok(dir)
}

/// One-shot client for `nebula add <dir>` (and bare `nebula <dir>`): resolve
/// the path locally — the daemon's cwd is not ours, so relative paths must be
/// absolutized here — and ask the daemon to register it as a project. The
/// daemon owns the rest: normalizing to the repo toplevel, naming the project
/// after the directory, rejecting non-repos and duplicates. Spawns a daemon
/// when none is running, same as launching the TUI would.
pub async fn add_project(path: &str) -> Result<()> {
    let dir = existing_dir(path)?;
    let mut conn = connect_or_spawn().await?;
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::AddProject {
            req_id,
            path: dir.clone(),
            name: None,
            create_missing: false,
        },
    )
    .await?;
    await_ack(&mut conn, req_id).await?;
    println!("added project {}", dir.display());
    Ok(())
}

/// Ask a running daemon to shut down. Ok(false) when none is running.
///
/// A daemon on a different protocol version closes the socket right after
/// the handshake, so `Shutdown` can never reach it — exactly the situation
/// `nebula kill` exists to fix. Fall back to SIGTERM, which its handler turns
/// into the same clean shutdown, at the pid the kernel says is listening on
/// the socket — the very daemon the handshake just failed with. Only when the
/// kernel can't say does the pidfile decide, guarded by the daemon's flock so
/// a stale pid is never signalled. The pidfile can't come first: macOS's
/// tmp_cleaner deletes regular files in /tmp untouched for three days but
/// spares sockets, so a daemon that has been up that long — on any version
/// before the one that refreshes its pidfile — is still listening with no
/// pidfile beside it (#68), and a refreshing daemon re-creates the file
/// locked a moment before it writes the pid in.
pub async fn kill_daemon() -> Result<bool> {
    kill_daemon_at(&paths::socket_path(), &paths::pidfile_path()).await
}

async fn kill_daemon_at(sock: &std::path::Path, pidfile: &std::path::Path) -> Result<bool> {
    let Ok(stream) = try_connect(sock).await else {
        // Nothing listening — but a wedged or mid-boot daemon may still hold
        // the pidfile lock; fall through to the same check.
        return kill_by_pidfile(pidfile).await;
    };
    let listener_pid = peer_pid(&stream);
    if let Ok(mut conn) = handshake(stream).await {
        write_frame(&mut conn.stream, &ClientRequest::Shutdown).await?;
        wait_for_daemon_exit(pidfile, conn.daemon_pid as i32).await;
        return Ok(true);
    }
    let Some(pid) = listener_pid else {
        if kill_by_pidfile(pidfile).await? {
            return Ok(true);
        }
        bail!(
            "a nebula daemon is listening on {} but this build can't talk to it or find its \
             pid — look it up with `pgrep -f 'nebula daemon'` and stop it with `kill <pid>` \
             (never `kill -9`, which skips flushing its database)",
            sock.display()
        );
    };
    terminate(pid)?;
    wait_for_daemon_exit(pidfile, pid).await;
    Ok(true)
}

/// Outcome of `shutdown_if_idle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleShutdown {
    /// Nothing listening on the socket.
    NoDaemon,
    /// The daemon held no live PTYs and was shut down cleanly.
    ShutDown,
    /// Live sessions exist; the daemon was left running.
    SessionsLive { count: usize },
    /// A daemon is listening but its protocol version differs, so its
    /// session state can't be inspected.
    Skewed,
}

/// Shut the daemon down only when it holds no live PTYs — the post-upgrade
/// handoff. An idle daemon can die safely (the next client launch spawns one
/// from the new binary on disk); live sessions would be killed with it, so
/// their daemon is left alone and the restart stays the user's call.
pub async fn shutdown_if_idle() -> Result<IdleShutdown> {
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        return Ok(IdleShutdown::NoDaemon);
    };
    let Ok(mut conn) = handshake(stream).await else {
        return Ok(IdleShutdown::Skewed);
    };
    write_frame(&mut conn.stream, &ClientRequest::Subscribe).await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Snapshot {
                agents, terminals, ..
            }) => {
                let live = agents.iter().filter(|a| a.alive).count()
                    + terminals.iter().filter(|t| t.alive).count();
                if live > 0 {
                    return Ok(IdleShutdown::SessionsLive { count: live });
                }
                write_frame(&mut conn.stream, &ClientRequest::Shutdown).await?;
                wait_for_daemon_exit(&paths::pidfile_path(), conn.daemon_pid as i32).await;
                return Ok(IdleShutdown::ShutDown);
            }
            Some(_) => continue,
            None => bail!("daemon closed the connection before sending a snapshot"),
        }
    }
}

/// SIGTERM the daemon recorded in the pidfile (its SIGTERM handler runs the
/// same clean shutdown as `Shutdown`). Ok(false) when no daemon holds it.
async fn kill_by_pidfile(path: &std::path::Path) -> Result<bool> {
    if !daemon_holds_pidfile_lock(path) {
        return Ok(false);
    }
    let pid: i32 = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .filter(|pid| *pid > 0)
        .context("daemon is running but its pidfile is unreadable — kill it manually")?;
    terminate(pid)?;
    wait_for_daemon_exit(path, pid).await;
    Ok(true)
}

/// SIGTERM `pid`. A daemon that exits between being found and being
/// signalled — mid-shutdown under a second `nebula kill` — has done what
/// was asked, so only a process still running makes a failed signal an error.
fn terminate(pid: i32) -> Result<()> {
    if send_signal(pid, SIGTERM) != 0 && process_running(pid) {
        bail!("failed to signal daemon pid {pid} — kill it manually");
    }
    Ok(())
}

/// Liveness = flock possession (mirrors the daemon's PidfileLock): if we can
/// take the lock ourselves, nobody holds it. Released on drop.
fn daemon_holds_pidfile_lock(path: &std::path::Path) -> bool {
    use std::os::fd::AsRawFd;
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    else {
        return false;
    };
    flock_try_exclusive(file.as_raw_fd()) != 0
}

/// Poll until the daemon has released its pidfile lock and exited, so a
/// relaunch right after `nebula kill` can't race the old daemon's teardown —
/// which ends by unlinking the socket path, a new daemon's socket included.
/// The process is watched as well as the lock because the lock may be on a
/// pidfile the tmp cleaner already deleted, where no client can see it.
async fn wait_for_daemon_exit(pidfile: &std::path::Path, pid: i32) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while (daemon_holds_pidfile_lock(pidfile) || process_running(pid))
        && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(POLL_STEP).await;
    }
}

/// Whether `pid` has yet to exit. A zombie still answers signal 0, but it
/// has exited and holds nothing — it is only waiting for its parent to reap
/// it, and a daemon's parent is whichever TUI spawned it, possibly still open.
fn process_running(pid: i32) -> bool {
    if send_signal(pid, 0) != 0 {
        return false;
    }
    // Linux: the state letter follows the parenthesised command name, which
    // can itself hold spaces and parens — hence the last `)`. No `ps` needed.
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        let state = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.trim_start().chars().next());
        return state != Some('Z');
    }
    match std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
    {
        Ok(out) => !String::from_utf8_lossy(&out.stdout)
            .trim_start()
            .starts_with('Z'),
        Err(_) => true,
    }
}

// Tiny extern shims, same dep-light idiom as nebula_core::paths.
fn flock_try_exclusive(fd: i32) -> i32 {
    extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    unsafe { flock(fd, LOCK_EX | LOCK_NB) }
}

const SIGTERM: i32 = 15;

fn send_signal(pid: i32, sig: i32) -> i32 {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe { kill(pid, sig) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::PROTOCOL_VERSION;
    use tokio::net::UnixStream;

    #[test]
    fn a_session_with_a_role_of_its_own_says_it() {
        let session = SessionSummary {
            session: SessionRef::Agent(AgentId("a1".into())),
            id: "a1".into(),
            name: "captain".into(),
            project_id: ProjectId("p".into()),
            project: "api".into(),
            worktree_id: WorktreeId("w".into()),
            worktree: "/src/api".into(),
            branch: "main".into(),
            harness: "claude".into(),
            status: "idle".into(),
            alive: true,
            summary: "fix it".into(),
        };
        assert_eq!(
            session_line(&session, Some(AgentRole::Captain)),
            "a1 name=captain project=api branch=main worktree=/src/api harness=claude \
             status=idle alive role=captain summary=\"fix it\""
        );
        let plain = session_line(&session, Some(AgentRole::Worker));
        assert!(!plain.contains("role="), "{plain}");
        assert_eq!(plain, session_line(&session, None), "a terminal has none");

        let roles = HashMap::from([("a1".to_string(), AgentRole::Captain)]);
        let json: serde_json::Value =
            serde_json::from_str(&sessions_json(&[session], &roles).unwrap()).unwrap();
        assert_eq!(json["sessions"][0]["role"], "captain");
        assert_eq!(json["sessions"][0]["name"], "captain");
    }

    // A spawn whose read-back failed still prints the id it started, in
    // the shape `--json` promises, so the caller follows that session
    // instead of starting another.
    #[test]
    fn a_failed_read_back_still_prints_the_started_id() {
        let id = AgentId("a1".into());
        let printed = started_json_or_id(&id, Err(anyhow::anyhow!("closed"))).unwrap();
        let json: serde_json::Value = serde_json::from_str(&printed).unwrap();
        assert_eq!(json["id"], "a1");
        for key in ["name", "worktree", "path"] {
            assert!(json[key].is_null(), "{key}: {json}");
        }
    }

    // Unset and empty are the same miss, and the error has to name the
    // command the model just ran so it knows why it can't work here.
    #[test]
    fn agent_id_requires_a_non_empty_value_and_names_the_verb() {
        let err = agent_id_from(None, "rename").unwrap_err().to_string();
        assert!(err.contains("`nebula rename`"), "{err}");
        assert!(err.contains(env::AGENT_ID), "{err}");
        let err = agent_id_from(Some(String::new()), "worktree")
            .unwrap_err()
            .to_string();
        assert!(err.contains("`nebula worktree`"), "{err}");
        assert_eq!(agent_id_from(Some("a1".into()), "rename").unwrap(), "a1");
    }

    /// Env var that turns [`fake_skewed_daemon`] from a no-op into a daemon.
    const FAKE_DAEMON_SOCK: &str = "NEBULA_TEST_FAKE_DAEMON_SOCK";

    /// A child re-exec of this test binary playing the daemon from #68: on
    /// an older protocol, with no pidfile, answering every `Hello` with
    /// `Incompatible` and hanging up — so only a signal stops it. A no-op
    /// when run any other way.
    #[test]
    #[ignore = "run as a child process by kill_stops_a_skewed_daemon_whose_pidfile_is_gone"]
    fn fake_skewed_daemon() {
        let Some(sock) = std::env::var_os(FAKE_DAEMON_SOCK) else {
            return;
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let listener = tokio::net::UnixListener::bind(sock).unwrap();
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let _ = read_frame::<ClientRequest, _>(&mut stream).await;
                let _ = write_frame(
                    &mut stream,
                    &ServerEvent::Incompatible {
                        daemon_protocol_version: PROTOCOL_VERSION - 1,
                    },
                )
                .await;
            }
        });
    }

    /// Kills the child if the test fails before it has exited.
    struct Reap(std::process::Child);

    impl Drop for Reap {
        fn drop(&mut self) {
            if !matches!(self.0.try_wait(), Ok(Some(_))) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    // #68: the pidfile is gone (macOS's tmp_cleaner) and the daemon speaks a
    // protocol this build doesn't, so neither `Shutdown` over the socket nor
    // SIGTERM via the pidfile can reach it — and `nebula kill` used to report
    // "no nebula daemon running" about a daemon still listening.
    #[tokio::test]
    async fn kill_stops_a_skewed_daemon_whose_pidfile_is_gone() {
        use std::os::unix::process::ExitStatusExt;
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let pidfile = dir.path().join("daemon.pid");
        let mut daemon = Reap(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "ipc::tests::fake_skewed_daemon", "--ignored"])
                .env(FAKE_DAEMON_SOCK, &sock)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let pid = daemon.0.id();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let stream = loop {
            match UnixStream::connect(&sock).await {
                Ok(stream) => break stream,
                Err(e) if tokio::time::Instant::now() > deadline => {
                    panic!("fake daemon never listened: {e}")
                }
                Err(_) => tokio::time::sleep(POLL_STEP).await,
            }
        };

        // What the TUI shows first: the pid, straight off the socket.
        let err = handshake(stream).await.err().expect("skewed").to_string();
        assert!(err.contains(&format!("`kill {pid}`")), "{err}");

        let started = std::time::Instant::now();
        assert!(
            kill_daemon_at(&sock, &pidfile).await.unwrap(),
            "a listening daemon is not \"no daemon running\""
        );
        // Its parent — this test — hasn't reaped it, so it is a zombie now:
        // exited, but still answering signal 0. The wait must count that as
        // gone rather than sit out its deadline.
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "kill waited {:?}",
            started.elapsed()
        );
        let status = {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            loop {
                if let Some(status) = daemon.0.try_wait().unwrap() {
                    break status;
                }
                assert!(tokio::time::Instant::now() < deadline, "daemon exited");
                tokio::time::sleep(POLL_STEP).await;
            }
        };
        assert_eq!(status.signal(), Some(SIGTERM), "{status:?}");
    }
}
