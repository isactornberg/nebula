//! The BOARD in the TUI: the ORCHESTRATOR VIEW's column of what needs
//! attention across every project, left of the session on screen
//! (`ui::launcher_view::board` draws it). The model is
//! [`nebula_client::board`]'s, which says what its sections and rows are;
//! this module sizes the column and reads the model off the [`App`]: the
//! session the view shows, the BOARD READS the beat took
//! (`event_loop::background::request_board_reads`), and the checkout a row
//! names from the TUI's pull request cache.

use crate::app::App;
use nebula_core::{Agent, SessionRef};

pub use nebula_client::board::{Board, BoardRead, BoardRow, RowKind, Section};
pub use nebula_core::why::report_word;

/// The view's body is at least this wide for the board to be drawn;
/// narrower, the header's strip of sessions stands in for it.
pub const MIN_SCREEN_W: u16 = 100;
/// The board's width: a third of the body, between these.
const MIN_W: u16 = 36;
const MAX_W: u16 = 56;

/// Does a body `width` wide have room for the board?
pub fn fits(width: u16) -> bool {
    width >= MIN_SCREEN_W
}

/// The board's width in a body `width` wide.
pub fn width(width: u16) -> u16 {
    (width / 3).clamp(MIN_W, MAX_W)
}

/// The checkout a session's row names under its title: its branch, and
/// the pull request on it. Only a session in a WORKTREE of its own, or on
/// a branch with a pull request, has one; the root checkout with no pull
/// request says nothing the project does not.
pub fn checkout_of(app: &App, a: &Agent) -> Option<(String, Option<crate::launcher::RowPr>)> {
    let own = !app
        .tree
        .worktrees
        .iter()
        .find(|w| w.id == a.worktree_id)?
        .is_main;
    let row = crate::launcher::row(app, &a.id)?;
    (own || row.pr.is_some()).then_some((row.branch, row.pr))
}

/// Does `a` wait on the user: a dialog, or a finish that asks in the state
/// it is in now ([`nebula_client::board::waits_on_user`])?
pub fn waits_on_user(app: &App, a: &Agent) -> bool {
    nebula_client::board::waits_on_user(&app.pane.board_reads, a)
}

/// What the board read off `a`'s screen in the state it is in now.
pub fn read_of<'a>(app: &'a App, a: &Agent) -> Option<&'a BoardRead> {
    nebula_client::board::read_of(&app.pane.board_reads, a)
}

/// The board: every live session but the orchestrator, from the strip's
/// attention order ([`Board::build`]), with the session the ORCHESTRATOR
/// VIEW shows as the one on screen.
pub fn board(app: &App) -> Board {
    let showing = match app.pane.term.as_ref().map(|t| &t.sref) {
        Some(SessionRef::Agent(id)) if app.orchestrator_view() => Some(id),
        _ => None,
    };
    Board::build(
        &app.tree,
        &app.strip_sessions(),
        showing,
        &app.pane.board_reads,
        |a| checkout_of(app, a).is_some(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_board_is_a_third_of_the_body_between_its_bounds() {
        assert!(!fits(99));
        assert!(fits(100));
        assert_eq!(width(100), MIN_W);
        assert_eq!(width(120), 40);
        assert_eq!(width(240), MAX_W);
    }
}
