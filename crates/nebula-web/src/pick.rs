//! Picking an option of the dialog open in a session, with the keys its
//! CLI takes for one, as measured on Claude Code 2.1.322 and Codex 0.163.
//! In one of Claude's numbered dialogs (a permission dialog, an
//! AskUserQuestion menu) the option's digit picks it at once, with no
//! Enter after it, so a stray Enter cannot answer whatever comes next.
//! Claude's unnumbered ones (whether to trust the folder, a one-time
//! notice) ignore digits, and what a digit does in Codex is not the same
//! from one of its dialogs to the next. Every dialog of both moves its
//! selection with the arrow keys and picks it with Enter, so that is what
//! they are given, the Enter only once the screen shows the selection on
//! the option: a CLI reads an Enter against the selection it last drew.

use crate::api::Harness;
use crate::hub::Hub;
use crate::Refusal;
use axum::http::StatusCode;
use nebula_core::dialog::{choice_in, Choice};
use nebula_core::AgentId;
use std::time::Duration;
use tokio::time::Instant;

const UP: &[u8] = b"\x1b[A";
const DOWN: &[u8] = b"\x1b[B";

/// The pause after each arrow.
const KEY_PAUSE: Duration = Duration::from_millis(50);
/// How long the selection gets to land on the option, and how often the
/// screen is read meanwhile.
const MOVE_WAIT: Duration = Duration::from_secs(3);
const MOVE_POLL: Duration = Duration::from_millis(100);

/// What to type for an option.
#[derive(Debug, PartialEq, Eq)]
enum Keys {
    /// Its digit, which picks it.
    Digit(u8),
    /// `steps` presses of `arrow`, which move the selection onto it, then
    /// an Enter.
    Move { arrow: &'static [u8], steps: usize },
}

/// The keys that pick option `n` of `choice` in a `harness` session;
/// `None` when it offers no option `n`.
fn keys(harness: Harness, choice: &Choice, n: u32) -> Option<Keys> {
    let place = |n| choice.options.iter().position(|(o, _)| *o == n);
    let to = place(n)?;
    if harness == Harness::Claude && choice.numbered && n <= 9 {
        return Some(Keys::Digit(b'0' + n as u8));
    }
    let from = place(choice.selected)?;
    Some(if to >= from {
        Keys::Move {
            arrow: DOWN,
            steps: to - from,
        }
    } else {
        Keys::Move {
            arrow: UP,
            steps: from - to,
        }
    })
}

/// The dialog session `id`'s screen shows now, if any.
async fn open_dialog(hub: &Hub, id: &AgentId) -> Result<Option<Choice>, Refusal> {
    let tail = hub.screen(id.clone()).await?;
    Ok(tail.and_then(|t| choice_in(&nebula_client::screen::screen_text(&t.data, t.cols, t.rows))))
}

/// Pick option `n` of the dialog open in session `id`, as its screen shows
/// it right before the keys are typed: refused when none is open, or the
/// one open offers no option `n`.
pub async fn pick(hub: &Hub, id: &AgentId, harness: Harness, n: u32) -> Result<(), Refusal> {
    let conflict = |message: String| Refusal::new(StatusCode::CONFLICT, message);
    let Some(choice) = open_dialog(hub, id).await? else {
        return Err(conflict("No dialog is open in the session.".into()));
    };
    let Some(keys) = keys(harness, &choice, n) else {
        return Err(conflict(format!("The dialog has no option {n}.")));
    };
    let (arrow, steps) = match keys {
        Keys::Digit(digit) => return Ok(hub.input(id.clone(), vec![digit]).await?),
        Keys::Move { arrow, steps } => (arrow, steps),
    };
    for _ in 0..steps {
        hub.input(id.clone(), arrow.to_vec()).await?;
        tokio::time::sleep(KEY_PAUSE).await;
    }
    let deadline = Instant::now() + MOVE_WAIT;
    loop {
        let now = open_dialog(hub, id).await?;
        match now {
            Some(now) if now.question == choice.question && now.options == choice.options => {
                if now.selected == n {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(Refusal::new(
                        StatusCode::GATEWAY_TIMEOUT,
                        format!(
                            "The dialog's selection did not reach option {n} within {} s, so \
                             nothing was picked.",
                            MOVE_WAIT.as_secs()
                        ),
                    ));
                }
            }
            _ => {
                return Err(conflict(format!(
                    "The dialog closed before option {n} was picked: it was answered \
                     elsewhere, or the session moved on."
                )))
            }
        }
        tokio::time::sleep(MOVE_POLL).await;
    }
    Ok(hub.input(id.clone(), b"\r".to_vec()).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(options: u32, selected: u32, numbered: bool) -> Choice {
        Choice {
            question: "Go?".into(),
            options: (1..=options).map(|n| (n, format!("option {n}"))).collect(),
            selected,
            numbered,
        }
    }

    /// A Claude dialog that draws its numbers takes the digit alone; one
    /// past 9 has none, and a digit is the first of its own.
    #[test]
    fn a_claude_dialog_that_draws_its_numbers_takes_the_digit() {
        let numbered = choice(4, 1, true);
        assert_eq!(keys(Harness::Claude, &numbered, 3), Some(Keys::Digit(b'3')));
        assert_eq!(keys(Harness::Claude, &numbered, 5), None);
        let long = choice(10, 1, true);
        assert_eq!(
            keys(Harness::Claude, &long, 10),
            Some(Keys::Move {
                arrow: DOWN,
                steps: 9
            })
        );
    }

    /// Everything else moves the selection from where it is.
    #[test]
    fn every_other_dialog_moves_its_selection_onto_the_option() {
        let trust = choice(2, 1, false);
        assert_eq!(
            keys(Harness::Claude, &trust, 2),
            Some(Keys::Move {
                arrow: DOWN,
                steps: 1
            })
        );
        let codex = choice(3, 3, true);
        assert_eq!(
            keys(Harness::Codex, &codex, 1),
            Some(Keys::Move {
                arrow: UP,
                steps: 2
            })
        );
        assert_eq!(
            keys(Harness::Codex, &codex, 3),
            Some(Keys::Move {
                arrow: DOWN,
                steps: 0
            }),
            "already selected: an Enter alone"
        );
        assert_eq!(keys(Harness::Codex, &codex, 4), None);
        assert_eq!(keys(Harness::Codex, &codex, 0), None);
    }
}
