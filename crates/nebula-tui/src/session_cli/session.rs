//! `nebula session <start | wait | read | send | delete>`: start a session
//! from a shell, and follow one by the id `nebula tree` prints. Wait until
//! it stops, read its screen, type its next turn, remove it.
//!
//! None of them sends `Attach`. An Attach resizes the session's PTY to the
//! asker's pane, which would redraw the program under a TUI that is showing
//! it. So the screen is laid out from the end of the ring (`TailOutput`,
//! what a terminal's card on the grid reads) and text is typed with a bare
//! `Input`, the way the FOLLOW-UP COMPOSER types a turn.

use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{bail, Result};
use nebula_core::{
    Agent, AgentId, AgentStatus, ClientRequest, OutputTail, ServerEvent, SessionRef,
    MAX_CLOUD_PROMPT_BYTES,
};
use tokio::time::Instant;

use super::start::{start, StartOpts};
use super::{printable, session_by_id, Client};
use crate::app::Tree;
use crate::event_loop::turn_bytes;

/// What one `TailOutput` asks for: all the daemon hands out of a ring at
/// once, which is its own cap.
const MAX_TAIL_BYTES: u32 = 64 * 1024;

/// How long `session send` gives a session to be ready for input. A CLI
/// resuming a long conversation takes seconds to draw its box.
const READY_WAIT: Duration = Duration::from_secs(30);

/// How often it asks for that session's output meanwhile.
const READY_POLL: Duration = Duration::from_millis(100);

/// How long a session must have printed nothing to count as waiting for
/// input.
const READY_QUIET: Duration = Duration::from_millis(700);

/// The pause between the text and the Enter that submits it.
const ENTER_PAUSE: Duration = Duration::from_millis(150);

/// How long `session send` waits for the session to report the turn it was
/// sent. The row turns `running` on the CLI's own hook, a moment after the
/// Enter; a harness with no hooks never reports, and costs this much.
const TAKEN_WAIT: Duration = Duration::from_secs(5);

/// `session wait`'s exit when `--timeout` ran out, as `timeout(1)` exits.
const EXIT_TIMED_OUT: u8 = 124;

/// `session send`'s exit when the text was typed and the session did not
/// report starting on it. Not 2, which clap exits with for a usage error,
/// where nothing was typed.
const EXIT_UNCONFIRMED: u8 = 3;

/// What `nebula session` was asked to do.
pub enum SessionOp {
    /// Start a session on a task.
    Start(StartOpts),
    /// Block until the session `id` is no longer running, for at most
    /// `timeout`.
    Wait {
        id: String,
        timeout: Option<Duration>,
    },
    /// Print its screen.
    Read { id: String },
    /// Type `text` as its next turn.
    Send { id: String, text: String },
    /// Remove its row and stop its process.
    Delete { id: String },
}

/// CLI: `nebula session <start | wait | read | send | delete>`. The exit
/// code is the command's own where it has one to give: `wait`'s timeout and
/// `send`'s unconfirmed turn are told apart from a failure.
pub async fn run(op: SessionOp) -> Result<ExitCode> {
    match op {
        SessionOp::Wait { id, timeout } => return wait(&id, timeout).await,
        SessionOp::Send { id, text } => return send(&id, &text).await,
        SessionOp::Start(opts) => start(opts).await?,
        SessionOp::Read { id } => read(&id).await?,
        SessionOp::Delete { id } => delete(&id).await?,
    }
    Ok(ExitCode::SUCCESS)
}

/// Where a wait on the session `id` stands: `None` while it is running,
/// the status it stopped in once it is not, and what became of a row there
/// is nothing to wait for. Archiving stops the process and leaves the
/// status as it was, so a wait on an archived row would never end.
fn settled(tree: &Tree, id: &str) -> Option<Result<AgentStatus, &'static str>> {
    match session_by_id(tree, id) {
        Err(_) => Some(Err("was deleted")),
        Ok(row) if row.archived => Some(Err("is archived")),
        Ok(row) => (row.status != AgentStatus::Running).then_some(Ok(row.status)),
    }
}

/// `session wait`: block until the session is no longer running, then
/// print the status it stopped in, alone on stdout for a script to branch
/// on. One that is not running when asked returns at once.
async fn wait(id: &str, timeout: Option<Duration>) -> Result<ExitCode> {
    let mut client = Client::connect("there is no session to wait for").await?;
    session_by_id(&client.tree, id)?;
    match client.wait_for(timeout, |tree| settled(tree, id)).await? {
        Some(Ok(status)) => println!("{}", status.as_str()),
        Some(Err(gone)) => bail!("session {id} {gone}: there is no turn to wait for"),
        None => {
            eprintln!(
                "session {id} is still running after {} s",
                timeout.unwrap_or_default().as_secs()
            );
            return Ok(ExitCode::from(EXIT_TIMED_OUT));
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The end of `agent`'s ring, or `None` when it has no live PTY. With
/// `after_seq`, no bytes when the ring has not grown past it. `TailOutput`
/// is answered by an `OutputTail`, never an Ack.
async fn tail(
    client: &mut Client,
    agent: &AgentId,
    after_seq: Option<u64>,
) -> Result<Option<OutputTail>> {
    let req_id = client.next_req_id();
    client
        .send(&ClientRequest::TailOutput {
            req_id,
            session: SessionRef::Agent(agent.clone()),
            max_bytes: MAX_TAIL_BYTES,
            after_seq,
        })
        .await?;
    loop {
        match client.next_event().await? {
            ServerEvent::OutputTail {
                req_id: r, tail, ..
            } if r == req_id => return Ok(tail),
            _ => {}
        }
    }
}

/// A throwaway screen the PTY's size, with `tail` run through it.
fn screen_of(tail: &OutputTail) -> vt100::Parser {
    // vt100's grid arithmetic needs two cells a side (`parse_tail` floors
    // the same way).
    let mut parser = vt100::Parser::new(tail.rows.max(2), tail.cols.max(2), 0);
    parser.process(&tail.data);
    parser
}

/// The text `tail`'s screen shows, a row each, as the PTY has it.
fn screen_rows(tail: &OutputTail) -> Vec<String> {
    // Not `terminal_tail::parse_tail`: it stops at the cursor's row and
    // drops blank rows, which would cut the options off a dialog.
    let parser = screen_of(tail);
    let mut rows: Vec<String> = parser
        .screen()
        .rows(0, tail.cols.max(2))
        .map(|row| row.trim_end().to_string())
        .collect();
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

/// Why `row` has no live PTY, for a `session read` that found none. Only
/// a session `session send` would bring back is told so, and where what it
/// said is still to be read.
fn no_terminal(row: &Agent) -> &'static str {
    if row.cloud_session_id.is_some() {
        "it runs in Claude Cloud"
    } else if row.archived {
        "it is archived"
    } else {
        "it was reaped while idle, or the daemon restarted. `nebula session send` brings it \
         back, and what it said is in its CLI's transcript, found by the `session_id` \
         `nebula tree --json` prints"
    }
}

/// `session read`: print the session's screen as plain text. A session
/// with no live PTY has no screen, and reading boots nothing.
async fn read(id: &str) -> Result<()> {
    let mut client = Client::connect("there is no session to read").await?;
    let row = session_by_id(&client.tree, id)?.clone();
    let Some(tail) = tail(&mut client, &row.id, None).await? else {
        bail!(
            "session {id} has no live terminal to read: {}",
            no_terminal(&row)
        );
    };
    let mut text = screen_rows(&tail).join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    // A reader that has seen enough (`| head`, `| grep -q`) is no failure.
    match std::io::stdout().lock().write_all(text.as_bytes()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(e.into()),
        _ => Ok(()),
    }
}

/// `text` as `session send` types it, or why it does not. A control
/// character is a key, not text: an `ESC[201~` would end the paste the
/// text goes in as, and what follows it would be typed as keystrokes. A
/// tab is text only inside that paste: one line goes down unbracketed
/// ([`turn_bytes`]), where a tab is a key press to the CLI. The size is the
/// bound a starting prompt has.
fn typeable(text: &str) -> Result<&str> {
    let text = text.trim();
    if text.is_empty() {
        bail!("the text is empty: `nebula session send <id> <text>` needs what to type");
    }
    let pasted = text.contains('\n');
    if text.contains(|c: char| c.is_control() && c != '\n' && !(pasted && c == '\t')) {
        bail!(
            "the text has a control character in it: only line breaks are typed, and a tab only \
             in text with a line break (in one line it is a key press to the CLI)"
        );
    }
    if text.len() > MAX_CLOUD_PROMPT_BYTES {
        bail!(
            "the text is too long (max {} KiB)",
            MAX_CLOUD_PROMPT_BYTES / 1024
        );
    }
    Ok(text)
}

/// Why `session send` types nothing into `row`, when it does not. A send
/// is the session's next turn, so it needs a session at rest. One with a
/// dialog open would take the text and its Enter as the answer: typing "no,
/// do not create it" into a Claude permission dialog approved the tool.
fn not_at_rest(row: &Agent) -> Option<&'static str> {
    if row.archived {
        return Some("is archived: unarchive it first (`u` on its card)");
    }
    match row.status {
        AgentStatus::Running => {
            Some("is working: wait for it first (`nebula session wait`), then send")
        }
        AgentStatus::NeedsFeedback => Some(
            "has a dialog open, which typed text and its Enter would answer blindly: answer it \
             in the TUI",
        ),
        _ => None,
    }
}

/// Why `row`, which has no live PTY, cannot be brought back, when it
/// cannot: a CLI boots in the session's checkout, and that one is gone.
fn no_checkout(tree: &Tree, row: &Agent) -> Option<String> {
    let home = tree.worktrees.iter().find(|w| w.id == row.worktree_id)?;
    (!home.path.is_dir()).then(|| {
        format!(
            "session {id} has no live terminal, and its checkout {} is gone, so it cannot be \
             brought back: `nebula session delete {id}` removes it",
            home.path.display(),
            id = row.id
        )
    })
}

/// Whether the program whose output ends in `tail`, `quiet` since it last
/// printed, is taking input: it has printed nothing for [`READY_QUIET`].
/// One that is `booting` (this send started it, or the row is `fresh`, so
/// it may still be coming up) must also have bracketed paste on, as an
/// agent CLI's input box does, when the whole ring is in hand (the tail
/// starts at the PTY's first byte). A ring longer than the tail cannot say
/// which mode is on, and belongs to a session long past booting, so there
/// quiet alone decides. So it does for a session at rest after a turn: it
/// has an input box, and a CLI that never switches bracketed paste on must
/// still be reachable.
///
/// Bracketed paste alone is not the box: measured on Claude Code 2.1.287
/// under zsh, a restarted session switched it on at 1.3 s (the login
/// shell's own line editor), off at 1.5 s, and on again at 1.7 s when the
/// box came up. Text typed at 1.3 s was lost.
fn ready(tail: &OutputTail, quiet: Duration, booting: bool) -> bool {
    let whole = tail.end_seq == tail.data.len() as u64;
    quiet >= READY_QUIET && (!booting || !whole || screen_of(tail).screen().bracketed_paste())
}

/// Wait until the program behind `agent` takes input ([`ready`]), asking
/// for the end of its ring every [`READY_POLL`]. `false` when
/// [`READY_WAIT`] runs out first.
async fn wait_until_ready(client: &mut Client, agent: &AgentId, booting: bool) -> Result<bool> {
    let started = Instant::now();
    let mut printed_at = started;
    let mut latest: Option<OutputTail> = None;
    loop {
        let seen = latest.as_ref().map(|t| t.end_seq);
        let now = Instant::now();
        match tail(client, agent, seen).await? {
            // Nothing new: an ask past `seen` is answered with no bytes.
            Some(tail) if Some(tail.end_seq) == seen => {}
            Some(tail) => {
                latest = Some(tail);
                printed_at = now;
            }
            // No PTY at this moment is one that died starting, which the
            // daemon may yet replace: never ready, and the cap still runs.
            None => latest = None,
        }
        if latest
            .as_ref()
            .is_some_and(|t| ready(t, now - printed_at, booting))
        {
            return Ok(true);
        }
        if now - started >= READY_WAIT {
            return Ok(false);
        }
        tokio::time::sleep(READY_POLL).await;
    }
}

/// `session send`: type `text` into the session as its next turn, the
/// bytes the FOLLOW-UP COMPOSER types ([`turn_bytes`]), then the carriage
/// return that submits it.
///
/// The session must be at rest ([`not_at_rest`]) and its program taking
/// input ([`ready`]). One with no live PTY (the IDLE REAPER took it, or
/// the daemon restarted) is brought back first: the daemon drops `Input`
/// for a session it has not spawned, and `RestartAgent` resumes the
/// conversation where the harness resumes one (another boots fresh).
///
/// Typing changes no status: the row turns `running` when the CLI reports
/// the prompt. So this returns once it has, and a `session wait` run
/// straight after waits for this turn and not the last. When it has not
/// within [`TAKEN_WAIT`], the exit says so.
async fn send(id: &str, text: &str) -> Result<ExitCode> {
    let text = typeable(text)?;
    let mut client = Client::connect("nothing was sent").await?;
    let row = session_by_id(&client.tree, id)?;
    if let Some(why) = not_at_rest(row) {
        bail!("session {id} {why}");
    }
    let (agent, name) = (row.id.clone(), printable(&row.name));
    let fresh = row.status == AgentStatus::Fresh;
    let gone = no_checkout(&client.tree, row);
    // `RestartAgent` kills whatever PTY the session has before it spawns
    // one, so it is sent only when there is none. Another client booting
    // the session in this same instant can lose that boot to this one.
    let revived = tail(&mut client, &agent, None).await?.is_none();
    if revived {
        if let Some(gone) = gone {
            bail!("{gone}");
        }
        let restart = |req_id| ClientRequest::RestartAgent {
            req_id,
            id: agent.clone(),
        };
        client.request(restart).await?;
    }
    if !wait_until_ready(&mut client, &agent, revived || fresh).await? {
        bail!(
            "session {id} is not ready for input after {} s: nothing was typed",
            READY_WAIT.as_secs()
        );
    }
    // That wait takes most of a second at the least, and a session can
    // start working or open a dialog meanwhile. Every poll of it folded the
    // daemon's events, so the tree is current.
    if let Some(why) = not_at_rest(session_by_id(&client.tree, id)?) {
        bail!("nothing was typed: session {id} {why}");
    }
    let pty = SessionRef::Agent(agent.clone());
    client
        .send(&ClientRequest::Input {
            session: pty.clone(),
            data: turn_bytes(text),
        })
        .await?;
    // The Enter is a keypress only when it arrives on its own. Measured on
    // Claude Code 2.1.287: a 250-character line with the Enter written
    // straight behind it stayed in the box unsubmitted, 2 times of 2; with
    // the Enter 50 ms or 300 ms later it was submitted, 4 of 4.
    tokio::time::sleep(ENTER_PAUSE).await;
    client
        .send(&ClientRequest::Input {
            session: pty,
            data: b"\r".to_vec(),
        })
        .await?;
    let typed = if revived {
        format!(
            "session {id} \"{name}\" had no live terminal: brought it back and typed its next turn"
        )
    } else {
        format!("typed the next turn into session {id} \"{name}\"")
    };
    let running = |tree: &Tree| {
        let row = session_by_id(tree, id).ok()?;
        (row.status == AgentStatus::Running).then_some(())
    };
    if client.wait_for(Some(TAKEN_WAIT), running).await?.is_some() {
        println!("{typed}; it is working on it.");
        return Ok(ExitCode::SUCCESS);
    }
    if tail(&mut client, &agent, None).await?.is_none() {
        bail!("session {id} lost its terminal after the text was typed: its process ended");
    }
    eprintln!(
        "{typed}, but it has not reported starting on it after {} s.",
        TAKEN_WAIT.as_secs()
    );
    Ok(ExitCode::from(EXIT_UNCONFIRMED))
}

/// `session delete`: remove the session's row and stop its process, like
/// `d` on its card. A worktree whose checkout is gone keeps its row for as
/// long as a session is filed under it, so this is what lets a script that
/// removed a checkout have the row follow.
async fn delete(id: &str) -> Result<()> {
    let mut client = Client::connect("nothing was deleted").await?;
    let row = session_by_id(&client.tree, id)?;
    let (agent, name) = (row.id.clone(), printable(&row.name));
    client
        .request(|req_id| ClientRequest::DeleteAgent { req_id, id: agent })
        .await?;
    println!("deleted session {id} \"{name}\": its process is stopped and its row is gone.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{agent, sample};
    use super::*;

    fn tail_of(data: &[u8], cols: u16, rows: u16) -> OutputTail {
        OutputTail {
            cols,
            rows,
            end_seq: data.len() as u64,
            data: data.to_vec(),
        }
    }

    // A wait is over when the row is anything but running. A row that was
    // archived or went away is told apart from one that stopped.
    #[test]
    fn a_wait_settles_on_any_status_but_running_and_on_a_row_that_is_gone() {
        let mut tree = sample();
        assert_eq!(settled(&tree, "a1"), None, "running: keep waiting");
        for status in [
            AgentStatus::Finished,
            AgentStatus::NeedsFeedback,
            AgentStatus::Terminated,
            AgentStatus::Disconnected,
            AgentStatus::Fresh,
        ] {
            tree.agents[0].status = status;
            assert_eq!(settled(&tree, "a1"), Some(Ok(status)));
        }
        // Archiving leaves the status alone, running included.
        tree.agents[0].status = AgentStatus::Running;
        tree.agents[0].archived = true;
        assert_eq!(settled(&tree, "a1"), Some(Err("is archived")));
        tree.agents.clear();
        assert_eq!(settled(&tree, "a1"), Some(Err("was deleted")));
    }

    // The screen the PTY has: rows below the cursor and blank rows between
    // others kept (a dialog's options sit there), trailing blank rows
    // dropped, and only what is on the screen.
    #[test]
    fn the_screen_is_read_whole_and_its_trailing_blank_rows_dropped() {
        let data = b"one  \r\n\r\ntwo\x1b[6;1Hstatus\x1b[3;4H";
        assert_eq!(
            screen_rows(&tail_of(data, 20, 8)),
            ["one", "", "two", "", "", "status"]
        );
        assert!(screen_rows(&tail_of(b"", 20, 8)).is_empty());
        assert_eq!(screen_rows(&tail_of(b"hi", 0, 0)), ["hi"]);
        let scrolled = b"1\r\n2\r\n3\r\n4\r\n$ ";
        assert_eq!(screen_rows(&tail_of(scrolled, 20, 3)), ["3", "4", "$"]);
    }

    // Text is typed only when it is text: not empty, no key in it, and no
    // longer than a starting prompt.
    #[test]
    fn text_with_a_control_character_or_past_the_bound_is_not_typed() {
        assert_eq!(typeable("  yes, go ahead \n").unwrap(), "yes, go ahead");
        assert_eq!(typeable("one\n\ttwo").unwrap(), "one\n\ttwo");
        let refused = |text: &str| typeable(text).unwrap_err().to_string();
        assert!(refused(" \n ").contains("empty"));
        // A tab is a key wherever the text is one line, which it is once
        // the line break at its end is trimmed.
        for key in [
            "a\x1b[201~b",
            "a\rb",
            "a\x03",
            "a\x7f",
            "a\u{9b}b",
            "a\tb",
            "a\tb\n",
        ] {
            assert!(refused(key).contains("control character"), "{key:?}");
        }
        assert!(typeable(&"x".repeat(MAX_CLOUD_PROMPT_BYTES)).is_ok());
        assert!(refused(&"x".repeat(MAX_CLOUD_PROMPT_BYTES + 1)).contains("too long"));
    }

    // A turn goes to a session at rest: not one working, one with a dialog
    // open, or an archived one.
    #[test]
    fn only_a_session_at_rest_is_typed_into() {
        let mut row = agent("a1", "main", "x");
        for status in [
            AgentStatus::Fresh,
            AgentStatus::Finished,
            AgentStatus::Terminated,
            AgentStatus::Disconnected,
        ] {
            row.status = status;
            assert_eq!(not_at_rest(&row), None, "{status:?}");
        }
        row.status = AgentStatus::Running;
        assert!(not_at_rest(&row).unwrap().contains("is working"));
        row.status = AgentStatus::NeedsFeedback;
        assert!(not_at_rest(&row).unwrap().contains("dialog"));
        row.status = AgentStatus::Finished;
        row.archived = true;
        assert!(not_at_rest(&row).unwrap().contains("archived"));
    }

    // Reading a session with no terminal offers `session send` only where
    // a send would bring it back.
    #[test]
    fn a_cloud_or_archived_session_is_not_told_that_send_brings_it_back() {
        let mut row = agent("a1", "main", "x");
        let reaped = no_terminal(&row);
        assert!(
            reaped.contains("`nebula session send`") && reaped.contains("`session_id`"),
            "{reaped}"
        );
        row.archived = true;
        assert!(!no_terminal(&row).contains("send"));
        row.archived = false;
        row.cloud_session_id = Some("session_1".into());
        assert!(!no_terminal(&row).contains("send"));
    }

    // A session with no live PTY is brought back in its checkout: one whose
    // checkout is gone is refused, naming the command that removes it.
    #[test]
    fn a_session_whose_checkout_is_gone_is_not_brought_back() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tree = sample();
        let row = tree.agents[0].clone();
        tree.worktrees[0].path = tmp.path().to_path_buf();
        assert_eq!(no_checkout(&tree, &row), None);
        tree.worktrees[0].path = tmp.path().join("removed");
        let gone = no_checkout(&tree, &row).unwrap();
        assert!(
            gone.contains("removed") && gone.contains("`nebula session delete a1`"),
            "{gone}"
        );
    }

    // A booting program is ready once it is quiet with bracketed paste on.
    // Neither alone is enough, and the mode is the one the output ends in.
    #[test]
    fn a_booting_program_is_ready_once_quiet_with_bracketed_paste_on() {
        let boxed = tail_of(b"\x1b[?2004h> ", 80, 24);
        assert!(ready(&boxed, READY_QUIET, true));
        assert!(!ready(&boxed, READY_QUIET / 2, true), "still printing");
        let plain = tail_of(b"loading\r\n$ ", 80, 24);
        assert!(!ready(&plain, READY_WAIT, true), "quiet is no input box");
        assert!(
            !ready(&tail_of(b"", 80, 24), READY_WAIT, true),
            "nothing printed"
        );
        // The login shell's line editor switched it on, then off again.
        let shell = tail_of(b"\x1b[?2004h% \x1b[?2004lstarting", 80, 24);
        assert!(!ready(&shell, READY_QUIET, true));
    }

    // A session at rest after a turn has its input box: quiet alone
    // decides, so a CLI that never switches bracketed paste on is reached.
    #[test]
    fn a_session_at_rest_after_a_turn_is_ready_once_quiet() {
        let plain = tail_of(b"done\r\n> ", 80, 24);
        assert!(ready(&plain, READY_QUIET, false));
        assert!(!ready(&plain, READY_QUIET / 2, false), "still printing");
    }

    // A ring longer than the tail hides the mode: quiet alone decides.
    #[test]
    fn a_ring_longer_than_the_tail_is_ready_once_quiet() {
        let cut = OutputTail {
            end_seq: 1_000_000,
            ..tail_of(b"> ", 80, 24)
        };
        assert!(ready(&cut, READY_QUIET, true));
        assert!(!ready(&cut, READY_QUIET / 2, true));
    }
}
