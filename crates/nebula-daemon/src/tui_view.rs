//! What the TUIs show. Each TUI reports the session on its screen
//! (`ClientRequest::ReportShowing`), and every subscriber is told the one
//! the TUI the user touched last shows (`ServerEvent::TuiShowing`): what
//! the web app's **Follow TUI** follows. A `ShowSession` goes out only
//! while some TUI is there to put the session up.

use crate::registry::Daemon;
use anyhow::{bail, Context, Result};
use nebula_core::{AgentId, ServerEvent};
use std::collections::BTreeMap;

/// Every connected TUI's report, by connection.
#[derive(Default)]
pub(crate) struct TuiViews {
    reports: BTreeMap<u64, Report>,
    /// Touches counted so far. A report keeps the count of the touch that
    /// last came with it, so the highest is the TUI touched last.
    touches: u64,
}

struct Report {
    session: Option<AgentId>,
    touched: u64,
}

/// What every subscriber is told: the session on screen in the TUI touched
/// last, and whether any TUI is connected.
type Front = (Option<AgentId>, bool);

impl TuiViews {
    /// Connection `conn` shows `session`. A TUI first heard from without a
    /// touch counts as touched before every one that was.
    fn report(&mut self, conn: u64, session: Option<AgentId>, touched: bool) {
        let touched = if touched {
            self.touches += 1;
            self.touches
        } else {
            self.reports.get(&conn).map_or(0, |r| r.touched)
        };
        self.reports.insert(conn, Report { session, touched });
    }

    /// The TUI touched last, and among TUIs never touched the newest
    /// connection: `max_by_key` keeps the last of equal keys, and the map
    /// is in connection order.
    fn front(&self) -> Front {
        let front = self.reports.values().max_by_key(|r| r.touched);
        (front.and_then(|r| r.session.clone()), front.is_some())
    }
}

fn showing((session, tui_open): Front) -> ServerEvent {
    ServerEvent::TuiShowing { session, tui_open }
}

impl Daemon {
    /// A TUI's report of the session on its screen.
    pub fn report_showing(&self, conn: u64, session: Option<AgentId>, touched: bool) {
        self.change_tui_views(|views| views.report(conn, session, touched));
    }

    /// Connection `conn` hung up: a TUI no longer, when it was one.
    pub fn tui_gone(&self, conn: u64) {
        self.change_tui_views(|views| {
            views.reports.remove(&conn);
        });
    }

    /// What the TUIs show now, as a subscriber is told it after a Snapshot.
    pub fn tui_showing(&self) -> ServerEvent {
        showing(self.tui_views.lock().front())
    }

    /// The web app's **Open in terminal**: every TUI puts session `id` up.
    pub fn show_session(&self, id: &AgentId) -> Result<()> {
        if self.tui_views.lock().reports.is_empty() {
            bail!("no TUI is open to show the session in");
        }
        let agent = self.store.get_agent(id)?.context("session not found")?;
        if agent.archived {
            bail!("session '{}' is archived", agent.name);
        }
        self.broadcast(ServerEvent::ShowSession { id: id.clone() });
        Ok(())
    }

    /// Apply `change`, and tell every subscriber when what they follow
    /// moved. The broadcast goes out under the lock, so two TUIs reporting
    /// at once reach every subscriber in the order their reports landed.
    fn change_tui_views(&self, change: impl FnOnce(&mut TuiViews)) {
        let mut views = self.tui_views.lock();
        let before = views.front();
        change(&mut views);
        let after = views.front();
        if after != before {
            self.broadcast(showing(after));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> Option<AgentId> {
        Some(AgentId(s.into()))
    }

    #[test]
    fn no_tui_is_no_session_and_none_open() {
        assert_eq!(TuiViews::default().front(), (None, false));
    }

    /// The TUI the user touched last is the one followed, however the
    /// others' screens change meanwhile, and the next touch elsewhere
    /// moves it.
    #[test]
    fn the_tui_touched_last_is_followed() {
        let mut views = TuiViews::default();
        views.report(1, id("a"), true);
        views.report(2, id("b"), true);
        assert_eq!(views.front(), (id("b"), true));
        // TUI 1's screen changes on its own (a show, a removal): it is
        // not touched by that, so TUI 2 stays in front.
        views.report(1, id("c"), false);
        assert_eq!(views.front(), (id("b"), true));
        views.report(1, id("c"), true);
        assert_eq!(views.front(), (id("c"), true));
        // A TUI's own screen changing keeps it in front with the new one.
        views.report(1, None, false);
        assert_eq!(views.front(), (None, true));
    }

    /// A TUI just connected and not touched yet takes the front only from
    /// others never touched either, and the newest of those wins.
    #[test]
    fn an_untouched_tui_follows_every_touched_one() {
        let mut views = TuiViews::default();
        views.report(1, id("a"), false);
        assert_eq!(views.front(), (id("a"), true));
        views.report(2, id("b"), false);
        assert_eq!(views.front(), (id("b"), true));
        views.report(1, id("a"), true);
        views.report(3, id("c"), false);
        assert_eq!(views.front(), (id("a"), true));
    }
}
