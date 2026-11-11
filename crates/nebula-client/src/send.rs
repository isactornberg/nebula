//! Typing a session's next turn, which `nebula session send` and the web
//! app's send both do, each telling the user why it typed nothing in its
//! own words.
//!
//! The session must be at rest: one working would take the text mid-turn,
//! and one with a dialog open would take the text and its Enter as the
//! answer (typing "no, do not create it" into a Claude permission dialog
//! approved the tool). One with no live PTY (the IDLE REAPER took it, or
//! the daemon restarted) is brought back first: the daemon drops `Input`
//! for a session it has not spawned, and `RestartAgent` resumes the
//! conversation where the harness resumes one (another boots fresh). The
//! text is typed once its CLI is up past any question it asks before it
//! starts (`AwaitUp`) and its program takes input ([`ready`]), as the bytes
//! the FOLLOW-UP COMPOSER types (`TypeTurn`), and the send is over once the
//! CLI reports the prompt it took, which turns the row `running`.

use std::path::PathBuf;
use std::time::Duration;

use nebula_core::{Agent, AgentId, AgentStatus, ClientRequest, OutputTail, MAX_CLOUD_PROMPT_BYTES};
use tokio::time::Instant;

use crate::{Client, Tree};

/// How long a session gets to be ready for input. A CLI resuming a long
/// conversation takes seconds to draw its box.
pub const READY_WAIT: Duration = Duration::from_secs(30);

/// How often its output is asked for meanwhile.
const READY_POLL: Duration = Duration::from_millis(100);

/// How long a session must have printed nothing to count as waiting for
/// input.
const READY_QUIET: Duration = Duration::from_millis(700);

/// Why text is not typed.
#[derive(Debug, PartialEq, Eq)]
pub enum Untypeable {
    /// Nothing is left of it once trimmed.
    Empty,
    /// It holds a control character, which is a key, not text: an
    /// `ESC[201~` would end the paste the text goes in as, and what follows
    /// it would be typed as keystrokes. A tab is text only inside that
    /// paste: a short line goes down unbracketed
    /// (`nebula_core::turn::turn_bytes`), where a tab is a key press to the
    /// CLI.
    Control,
    /// It is longer than a starting prompt can be,
    /// [`MAX_CLOUD_PROMPT_BYTES`].
    TooLong,
}

/// `text` as it is typed, trimmed, or why it is not.
pub fn typeable(text: &str) -> Result<&str, Untypeable> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Untypeable::Empty);
    }
    let pasted = text.contains('\n');
    if text.contains(|c: char| c.is_control() && c != '\n' && !(pasted && c == '\t')) {
        return Err(Untypeable::Control);
    }
    if text.len() > MAX_CLOUD_PROMPT_BYTES {
        return Err(Untypeable::TooLong);
    }
    Ok(text)
}

/// Why a session is not at rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unrest {
    Archived,
    Working,
    /// A dialog is open in it, which typed text and its Enter would answer
    /// blindly.
    Dialog,
}

fn unrest(row: &Agent) -> Option<Unrest> {
    if row.archived {
        return Some(Unrest::Archived);
    }
    match row.status {
        AgentStatus::Running => Some(Unrest::Working),
        AgentStatus::NeedsFeedback => Some(Unrest::Dialog),
        _ => None,
    }
}

/// Why a session's program took no input within [`READY_WAIT`].
#[derive(Debug, PartialEq, Eq)]
pub enum NotReady {
    /// Its screen shows a choice dialog, which typed text would answer.
    Dialog,
    /// It printed on, or never showed an input box.
    TimedOut,
}

/// Why nothing was typed.
#[derive(Debug)]
pub enum Untyped {
    /// The session is not at rest.
    NotAtRest(Unrest),
    /// It has no live PTY, and its checkout, where its CLI boots, is gone.
    CheckoutGone(PathBuf),
    /// Its CLI did not come up: `AwaitUp`'s refusal, or the daemon gone.
    NotUp(anyhow::Error),
    NotReady(NotReady),
    /// It started working or opened a dialog while it was waited for.
    MovedOn(Unrest),
    /// Its row was deleted.
    Deleted,
    /// The daemon refused a request, or hung up. A prompt the session
    /// reported taking only part of is the refusal of `TypeTurn`.
    Failed(anyhow::Error),
}

/// What became of the text once it was typed.
#[derive(Debug, PartialEq, Eq)]
pub enum Receipt {
    /// The CLI reported taking the whole of it.
    Whole,
    /// It reported nothing within `TURN_RECEIPT_WAIT`, so nothing says the
    /// text arrived whole.
    Unconfirmed,
    /// Its process ended after the text was typed.
    Exited,
}

#[derive(Debug)]
pub struct Typed {
    /// The session had no live PTY, and was brought back for the turn.
    pub revived: bool,
    pub receipt: Receipt,
}

/// Type `text` ([`typeable`]) into session `agent` as its next turn, over
/// a daemon at `TURN_RECEIPT_PROTOCOL` or later.
pub async fn send_turn(client: &mut Client, agent: &AgentId, text: &str) -> Result<Typed, Untyped> {
    let at = row(&client.tree, agent)?;
    if let Some(why) = unrest(at) {
        return Err(Untyped::NotAtRest(why));
    }
    let fresh = at.status == AgentStatus::Fresh;
    let gone = no_checkout(&client.tree, at);
    // `RestartAgent` kills whatever PTY the session has before it spawns
    // one, so it is sent only when there is none. Another client booting
    // the session in this same instant can lose that boot to this one.
    let revived = tail(client, agent, None).await?.is_none();
    if revived {
        if let Some(gone) = gone {
            return Err(Untyped::CheckoutGone(gone));
        }
        let restart = |req_id| ClientRequest::RestartAgent {
            req_id,
            id: agent.clone(),
        };
        client.request(restart).await.map_err(Untyped::Failed)?;
    }
    // Past any question the CLI asks before it starts, which the text and
    // its Enter would answer blindly: Claude's whether to trust the folder
    // defaults to quitting.
    let up = |req_id| ClientRequest::AwaitUp {
        req_id,
        id: agent.clone(),
    };
    client.request(up).await.map_err(Untyped::NotUp)?;
    wait_until_ready(client, agent, revived || fresh)
        .await?
        .map_err(Untyped::NotReady)?;
    // That wait takes most of a second at the least, and a session can
    // start working or open a dialog meanwhile. Every poll of it folded the
    // daemon's events, so the tree is current.
    if let Some(why) = unrest(row(&client.tree, agent)?) {
        return Err(Untyped::MovedOn(why));
    }
    let confirmed = client
        .type_turn(|req_id| ClientRequest::TypeTurn {
            req_id,
            id: agent.clone(),
            text: text.to_string(),
        })
        .await
        .map_err(Untyped::Failed)?;
    let receipt = if confirmed {
        Receipt::Whole
    } else if tail(client, agent, None).await?.is_none() {
        Receipt::Exited
    } else {
        Receipt::Unconfirmed
    };
    Ok(Typed { revived, receipt })
}

fn row<'a>(tree: &'a Tree, agent: &AgentId) -> Result<&'a Agent, Untyped> {
    tree.agents
        .iter()
        .find(|a| a.id == *agent)
        .ok_or(Untyped::Deleted)
}

/// The checkout `row`, which has no live PTY, would be brought back in,
/// when it is gone.
fn no_checkout(tree: &Tree, row: &Agent) -> Option<PathBuf> {
    let home = tree.worktrees.iter().find(|w| w.id == row.worktree_id)?;
    (!home.path.is_dir()).then(|| home.path.clone())
}

async fn tail(
    client: &mut Client,
    agent: &AgentId,
    after_seq: Option<u64>,
) -> Result<Option<OutputTail>, Untyped> {
    client.tail(agent, after_seq).await.map_err(Untyped::Failed)
}

/// Whether the program whose output ends in `tail`, `quiet` since it last
/// printed, is taking input: it has printed nothing for [`READY_QUIET`],
/// and its screen shows no choice dialog (`nebula_core::dialog`), which the
/// text and its Enter would answer: Claude can open a one-time notice past
/// the point it reports it is up.
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
    let paste = || {
        // vt100's grid arithmetic needs two cells a side (`screen_text`
        // floors the same way).
        let mut screen = vt100::Parser::new(tail.rows.max(2), tail.cols.max(2), 0);
        screen.process(&tail.data);
        screen.screen().bracketed_paste()
    };
    quiet >= READY_QUIET && (!booting || !whole || paste()) && !on_dialog(tail)
}

fn on_dialog(tail: &OutputTail) -> bool {
    let rows = crate::screen::screen_text(&tail.data, tail.cols, tail.rows);
    nebula_core::dialog::dialog_in(&rows).is_some()
}

/// Wait until the program behind `agent` takes input ([`ready`]), asking
/// for the end of its ring every [`READY_POLL`]; why it does not, when
/// [`READY_WAIT`] runs out first.
async fn wait_until_ready(
    client: &mut Client,
    agent: &AgentId,
    booting: bool,
) -> Result<Result<(), NotReady>, Untyped> {
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
            return Ok(Ok(()));
        }
        if now - started >= READY_WAIT {
            return Ok(Err(match latest {
                Some(tail) if on_dialog(&tail) => NotReady::Dialog,
                _ => NotReady::TimedOut,
            }));
        }
        tokio::time::sleep(READY_POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::tests::{agent, sample};

    fn tail_of(data: &[u8], cols: u16, rows: u16) -> OutputTail {
        OutputTail {
            cols,
            rows,
            end_seq: data.len() as u64,
            data: data.to_vec(),
        }
    }

    // Text is typed only when it is text: not empty, no key in it, and no
    // longer than a starting prompt.
    #[test]
    fn text_with_a_control_character_or_past_the_bound_is_not_typed() {
        assert_eq!(typeable("  yes, go ahead \n"), Ok("yes, go ahead"));
        assert_eq!(typeable("one\n\ttwo"), Ok("one\n\ttwo"));
        assert_eq!(typeable(" \n "), Err(Untypeable::Empty));
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
            assert_eq!(typeable(key), Err(Untypeable::Control), "{key:?}");
        }
        assert!(typeable(&"x".repeat(MAX_CLOUD_PROMPT_BYTES)).is_ok());
        assert_eq!(
            typeable(&"x".repeat(MAX_CLOUD_PROMPT_BYTES + 1)),
            Err(Untypeable::TooLong)
        );
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
            assert_eq!(unrest(&row), None, "{status:?}");
        }
        row.status = AgentStatus::Running;
        assert_eq!(unrest(&row), Some(Unrest::Working));
        row.status = AgentStatus::NeedsFeedback;
        assert_eq!(unrest(&row), Some(Unrest::Dialog));
        row.status = AgentStatus::Finished;
        row.archived = true;
        assert_eq!(unrest(&row), Some(Unrest::Archived));
    }

    // A session with no live PTY is brought back in its checkout, so one
    // whose checkout is gone is not.
    #[test]
    fn a_session_whose_checkout_is_gone_is_not_brought_back() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tree = sample();
        let row = tree.agents[0].clone();
        tree.worktrees[0].path = tmp.path().to_path_buf();
        assert_eq!(no_checkout(&tree, &row), None);
        tree.worktrees[0].path = tmp.path().join("removed");
        assert_eq!(no_checkout(&tree, &row), Some(tmp.path().join("removed")));
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

    // A choice dialog the CLI opened past its boot, a one-time notice or a
    // permission dialog, is never typed into, however quiet it is.
    #[test]
    fn a_session_showing_a_choice_dialog_is_not_ready() {
        let notice = tail_of(
            "\x1b[?2004h Try the new renderer?\r\n\r\n \u{276f} 1. Yes\r\n   2. Not now\r\n"
                .as_bytes(),
            80,
            24,
        );
        assert!(!ready(&notice, READY_WAIT, true));
        assert!(!ready(&notice, READY_WAIT, false));
        let boxed = tail_of(
            "\x1b[?2004h\u{2500}\u{2500}\u{2500}\r\n \u{276f} \r\n\u{2500}\u{2500}\u{2500}"
                .as_bytes(),
            80,
            24,
        );
        assert!(ready(&boxed, READY_QUIET, true), "the input box is up");
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
