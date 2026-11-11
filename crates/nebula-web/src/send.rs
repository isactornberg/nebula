//! A turn typed into a session from the web app, the way `nebula session
//! send` types one (`nebula_client::send`), its refusals worded for the
//! page. It holds a connection of its own for that, since it reads the
//! daemon's events for as long as the session takes to be ready and to
//! report.

use crate::Refusal;
use axum::http::StatusCode;
use nebula_client::send::{
    send_turn, typeable, NotReady, Receipt, Unrest, Untypeable, Untyped, READY_WAIT,
};
use nebula_client::{Client, Closed, Tree};
use nebula_core::{Agent, MAX_CLOUD_PROMPT_BYTES, TURN_RECEIPT_PROTOCOL, TURN_RECEIPT_WAIT};

/// Who a turn is for.
#[derive(Clone, Copy)]
pub enum To<'a> {
    Session(&'a str),
    Orchestrator,
}

fn conflict(message: impl Into<String>) -> Refusal {
    Refusal::new(StatusCode::CONFLICT, message)
}

/// A refusal of the daemon's, `message` as it words it, or the daemon gone.
fn failed(err: anyhow::Error) -> Refusal {
    if err.is::<Closed>() {
        return Refusal::down();
    }
    Refusal::new(
        StatusCode::BAD_GATEWAY,
        crate::sentence(&format!("{err:#}")),
    )
}

/// `text` as it is typed ([`typeable`]), or why it is not.
fn text_of(text: &str) -> Result<&str, Refusal> {
    let bad = |message: &str| Refusal::new(StatusCode::BAD_REQUEST, message);
    typeable(text).map_err(|why| match why {
        Untypeable::Empty => bad("There is nothing to send."),
        Untypeable::Control => bad(
            "The text has a control character in it: only line breaks are typed, and a tab only \
             in text with a line break.",
        ),
        Untypeable::TooLong => Refusal::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "The text is too long: it can be {} KiB at most.",
                MAX_CLOUD_PROMPT_BYTES / 1024
            ),
        ),
    })
}

/// Why there is no row for `to`.
fn missing(to: To) -> Refusal {
    match to {
        To::Session(id) => crate::no_session(id),
        To::Orchestrator => conflict("There is no orchestrator yet."),
    }
}

/// The row a turn is for, by the tree as it stands.
fn row<'a>(tree: &'a Tree, to: To) -> Result<&'a Agent, Refusal> {
    match to {
        To::Session(id) => tree.agents.iter().find(|a| a.id.0 == id),
        To::Orchestrator => tree.orchestrator_agent(),
    }
    .ok_or_else(|| missing(to))
}

/// What a session that is not at rest is doing, as the page says it.
fn unrest(why: Unrest) -> &'static str {
    match why {
        Unrest::Archived => "is archived: unarchive it in the TUI first (`u` on its card)",
        Unrest::Working => "is working: wait for it first, then send",
        Unrest::Dialog => {
            "has a dialog open, which typed text and its Enter would answer blindly: answer it \
             first"
        }
    }
}

/// Why nothing was typed into `to`, `who` by name.
fn untyped(to: To, who: &str, why: Untyped) -> Refusal {
    let lower = who.to_lowercase();
    match why {
        Untyped::NotAtRest(why) => conflict(format!("{who} {}.", unrest(why))),
        Untyped::CheckoutGone(path) => conflict(format!(
            "It has no live terminal, and its checkout {} is gone, so it cannot be brought back.",
            path.display()
        )),
        Untyped::NotUp(err) if err.is::<Closed>() => Refusal::down(),
        Untyped::NotUp(err) => conflict(crate::sentence(&format!("nothing was typed: {err:#}"))),
        Untyped::NotReady(NotReady::Dialog) => conflict(format!(
            "Nothing was typed: {lower} is waiting on a dialog, which typed text would answer: \
             answer it, then send again."
        )),
        Untyped::NotReady(NotReady::TimedOut) => conflict(format!(
            "Nothing was typed: {lower} is not ready for input after {} s.",
            READY_WAIT.as_secs()
        )),
        Untyped::MovedOn(why) => conflict(format!("Nothing was typed: {lower} {}.", unrest(why))),
        Untyped::Deleted => missing(to),
        Untyped::Failed(err) => failed(err),
    }
}

/// Type `text` into the session `to` names as its next turn, and return
/// once its CLI reports taking the whole of it.
pub async fn send(to: To<'_>, text: &str) -> Result<(), Refusal> {
    let text = text_of(text)?;
    let mut client = Client::connect("nothing was sent")
        .await
        .map_err(|_| Refusal::down())?;
    if client.protocol < TURN_RECEIPT_PROTOCOL {
        return Err(conflict(
            "Nothing was sent: the running daemon cannot tell whether a prompt arrived whole. \
             `nebula reload` moves it onto this build, its sessions kept.",
        ));
    }
    let who = match to {
        To::Session(_) => "The session",
        To::Orchestrator => "The orchestrator",
    };
    let agent = row(&client.tree, to)?.id.clone();
    let sent = send_turn(&mut client, &agent, text)
        .await
        .map_err(|why| untyped(to, who, why))?;
    match sent.receipt {
        Receipt::Whole => Ok(()),
        Receipt::Exited => Err(Refusal::new(
            StatusCode::BAD_GATEWAY,
            format!("{who} lost its terminal after the text was typed: its process ended."),
        )),
        Receipt::Unconfirmed => Err(Refusal::new(
            StatusCode::GATEWAY_TIMEOUT,
            format!(
                "The text was typed, but {} has not reported taking it after {} s, so nothing \
                 says it arrived whole.",
                who.to_lowercase(),
                TURN_RECEIPT_WAIT.as_secs()
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_text_is_typed() {
        assert_eq!(text_of("  carry on \n").ok(), Some("carry on"));
        assert_eq!(text_of("one\n\ttwo").ok(), Some("one\n\ttwo"));
        for (text, status) in [
            ("  \n ", StatusCode::BAD_REQUEST),
            ("a\tb", StatusCode::BAD_REQUEST),
            ("a\x1b[201~b", StatusCode::BAD_REQUEST),
        ] {
            assert_eq!(
                text_of(text).err().map(|r| r.status),
                Some(status),
                "{text:?}"
            );
        }
        let long = "x".repeat(MAX_CLOUD_PROMPT_BYTES + 1);
        assert_eq!(
            text_of(&long).err().map(|r| r.status),
            Some(StatusCode::PAYLOAD_TOO_LARGE)
        );
    }
}
