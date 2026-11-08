//! Cursor's ask-question dialog, read off the screen. cursor-agent fires no
//! hook for its `AskQuestion` tool — no `preToolUse`, no `postToolUse`, even
//! though both fire for its shell calls — and drives no OSC 9;4 progress
//! bar, so the dialog it draws is the only sign the turn is waiting on you
//! (verified against cursor-agent 2026.10.01 by capturing hook payloads and
//! raw PTY bytes).
//!
//! The dialog is a bordered box whose last line is a fixed key legend. It
//! is matched on the screen rather than in the byte stream because the CLI
//! (an Ink app) repaints only the tail of its frame when the lines above
//! did not change: a repaint without the legend says nothing about whether
//! the box is still up. A narrow pane wraps the legend inside the border,
//! so the border and the whitespace are dropped before matching.
//!
//! Opening is reported at once. Closing waits for the legend to stay gone
//! across output [`CLOSE_SETTLE`] apart: a macOS PTY hands over at most
//! 1 KiB per read, the box alone is a few KiB, and a repaint split across
//! two flushes shows the box erased in between. Once answered, the CLI's
//! spinner repaints every quarter second, which is what confirms the close;
//! a turn that ends before that is finished by its `stop` hook instead.
//!
//! The same scanner watches a starting Claude or cursor-agent session for
//! the question whether to trust its folder ([`QuestionScanner::trust`],
//! `nebula_core::trust`), which comes before the CLI runs any hook, until
//! its first hook says it is past it.

use std::time::{Duration, Instant};

/// The dialog's key legend, as the CLI prints it.
const LEGEND: &str = "Space select · Enter next/submit · Esc to skip";

/// How long the legend has to stay off the screen before the dialog counts
/// as closed. Above any split repaint's gap; under the spinner's period.
pub const CLOSE_SETTLE: Duration = Duration::from_millis(150);

/// Side length the screen is floored at — see `pty::cursor`.
const MIN_SIDE: u16 = 2;

pub struct QuestionScanner {
    /// No scrollback: only what is on screen now matters.
    parser: vt100::Parser,
    open: bool,
    /// While open: when the dialog was first seen missing.
    missing_since: Option<Instant>,
    /// Whether the screen shows what is watched for.
    shown: fn(&vt100::Screen) -> bool,
}

impl QuestionScanner {
    /// A screen at the PTY's size with `history` replayed into it, watched
    /// for cursor-agent's ask-question dialog. A dialog already up in it
    /// counts as open without an edge: whoever wrote the history already
    /// acted on it.
    pub fn new(cols: u16, rows: u16, history: &[u8]) -> Self {
        Self::watching(cols, rows, history, |screen| {
            screen_text(screen).contains(LEGEND)
        })
    }

    /// [`Self::new`], watching for the question whether to trust the
    /// folder instead, asked in the dialog the screen ends on
    /// (`nebula_core::trust::question_on`), and for any other choice
    /// dialog (`nebula_core::dialog`) a starting CLI opens before it runs
    /// a hook, such as Claude's one-time notices.
    pub fn trust(cols: u16, rows: u16, history: &[u8]) -> Self {
        Self::watching(cols, rows, history, |screen| {
            let rows = screen_rows(screen);
            nebula_core::dialog::dialog_in(&rows).is_some()
                || nebula_core::trust::question_on(&rows).is_some()
        })
    }

    fn watching(cols: u16, rows: u16, history: &[u8], shown: fn(&vt100::Screen) -> bool) -> Self {
        let (cols, rows) = grid_size(cols, rows);
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(history);
        let open = shown(parser.screen());
        Self {
            parser,
            open,
            missing_since: None,
            shown,
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let (cols, rows) = grid_size(cols, rows);
        self.parser.screen_mut().set_size(rows, cols);
    }

    /// Feed a chunk of child output. `Some(open)` only on an edge.
    pub fn feed(&mut self, data: &[u8], now: Instant) -> Option<bool> {
        self.parser.process(data);
        if (self.shown)(self.parser.screen()) {
            self.missing_since = None;
            if self.open {
                return None;
            }
            self.open = true;
            return Some(true);
        }
        if !self.open {
            return None;
        }
        let since = *self.missing_since.get_or_insert(now);
        if now.duration_since(since) < CLOSE_SETTLE {
            return None;
        }
        self.open = false;
        self.missing_since = None;
        Some(false)
    }
}

/// The screen's text with the border and the whitespace dropped to one
/// space between words, so a dialog a narrow pane wraps still matches.
fn screen_text(screen: &vt100::Screen) -> String {
    let mut text = String::new();
    let mut gap = false;
    for c in screen.contents().chars() {
        if c.is_whitespace() || c == '│' {
            gap = true;
            continue;
        }
        if gap && !text.is_empty() {
            text.push(' ');
        }
        gap = false;
        text.push(c);
    }
    text
}

/// The screen's rows, as the PTY lays them out.
fn screen_rows(screen: &vt100::Screen) -> Vec<String> {
    let (_, cols) = screen.size();
    screen.rows(0, cols).collect()
}

fn grid_size(cols: u16, rows: u16) -> (u16, u16) {
    (cols.max(MIN_SIDE), rows.max(MIN_SIDE))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dialog as cursor-agent paints it at `width` columns: the
    /// previous frame erased, then the box. The legend wraps inside the
    /// border when the box is too narrow for it.
    fn dialog(width: usize) -> Vec<u8> {
        let inner = width - 4;
        let legend = format!("↑/↓ option · ←/→ question · {LEGEND}");
        let mut lines = vec!["Pick a color".to_string(), String::new()];
        lines.push("› [ ] red".into());
        lines.push("  [ ] blue".into());
        lines.push(String::new());
        let mut line = String::new();
        for word in legend.split(' ') {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > inner {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
        let mut out = String::from("\x1b[2J\x1b[H");
        out.push_str(&format!(" ┌{}┐\r\n", "─".repeat(inner + 2)));
        for l in lines {
            let pad = inner - l.chars().count();
            out.push_str(&format!(" │ {l}{} │\r\n", " ".repeat(pad)));
        }
        out.push_str(&format!(" └{}┘\r\n", "─".repeat(inner + 2)));
        out.push_str("  → Add a follow-up\r\n");
        out.into_bytes()
    }

    /// One spinner repaint: the frame the CLI draws once the dialog is gone.
    fn composing() -> Vec<u8> {
        "\x1b[2J\x1b[H ⠋ Composing\r\n  → Add a follow-up\r\n"
            .as_bytes()
            .to_vec()
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_dialog_opens_at_once() {
        let t = Instant::now();
        let mut s = QuestionScanner::new(120, 40, b"");
        assert_eq!(s.feed(b" \xe2\xa0\x8b Composing\r\n", t), None);
        assert_eq!(s.feed(&dialog(120), t), Some(true));
        assert_eq!(
            s.feed(&dialog(120), t + ms(500)),
            None,
            "a repaint is no edge"
        );
    }

    #[test]
    fn a_wrapped_legend_still_matches() {
        let t = Instant::now();
        for width in [40, 50, 64] {
            let mut s = QuestionScanner::new(width as u16, 40, b"");
            assert_eq!(s.feed(&dialog(width), t), Some(true), "{width} cols");
        }
    }

    #[test]
    fn closing_waits_for_the_legend_to_stay_gone() {
        let t = Instant::now();
        let mut s = QuestionScanner::new(120, 40, b"");
        s.feed(&dialog(120), t);
        assert_eq!(s.feed(&composing(), t + ms(1000)), None, "first sighting");
        assert_eq!(
            s.feed(&composing(), t + ms(1100)),
            None,
            "inside the settle"
        );
        assert_eq!(s.feed(&composing(), t + ms(1250)), Some(false));
        assert_eq!(s.feed(&composing(), t + ms(1500)), None);
    }

    #[test]
    fn a_repaint_split_across_flushes_is_not_a_close() {
        let t = Instant::now();
        let mut s = QuestionScanner::new(120, 40, b"");
        s.feed(&dialog(120), t);
        let frame = dialog(120);
        let (head, tail) = frame.split_at(frame.len() / 2);
        assert_eq!(s.feed(head, t + ms(1000)), None);
        assert!(
            !screen_text(s.parser.screen()).contains(LEGEND),
            "the head erased the box"
        );
        assert_eq!(s.feed(tail, t + ms(1001)), None);
        // The half-drawn moment does not count toward a later close.
        assert_eq!(s.feed(&composing(), t + ms(1200)), None);
    }

    #[test]
    fn a_tail_only_repaint_leaves_it_open() {
        let t = Instant::now();
        let mut s = QuestionScanner::new(120, 40, b"");
        s.feed(&dialog(120), t);
        // Ink repaints just the last line when nothing above it changed.
        let tail = b"\x1b[1A\x1b[2K\x1b[G  \xe2\x86\x92 Add a follow-up \xc2\xb7 6%\r\n";
        assert_eq!(s.feed(tail, t + ms(500)), None);
        assert_eq!(s.feed(tail, t + ms(900)), None);
    }

    #[test]
    fn a_dialog_in_the_history_is_open_without_an_edge() {
        let t = Instant::now();
        let mut s = QuestionScanner::new(120, 40, &dialog(120));
        assert_eq!(s.feed(&dialog(120), t), None);
        s.feed(&composing(), t + ms(100));
        assert_eq!(s.feed(&composing(), t + ms(400)), Some(false));
    }

    /// Claude Code 2.1's question whether to trust the folder, as it
    /// draws it at 80 columns, and the box it draws once that is answered.
    #[test]
    fn the_trust_question_opens_and_closes_its_scanner() {
        let t = Instant::now();
        let question =
            "\x1b[2J\x1b[H Accessing workspace:\r\n\r\n /Users/me/src/relay-cli\r\n\r\n \
             Quick safety check: Is this a project you created or one you trust? (Like\r\n \
             your own code, a well-known open source project, or work from your team).\r\n\r\n \
             ❯ No, exit\r\n   Yes, I trust this folder\r\n";
        let mut s = QuestionScanner::trust(80, 24, b"");
        assert_eq!(s.feed(question.as_bytes(), t), Some(true));
        let prompt = "\x1b[2J\x1b[H╭─╮\r\n│ > │\r\n";
        assert_eq!(s.feed(prompt.as_bytes(), t + ms(10)), None);
        assert_eq!(s.feed(prompt.as_bytes(), t + ms(200)), Some(false));
        // Cursor's ask-question legend is no trust question.
        let mut s = QuestionScanner::trust(120, 40, b"");
        assert_eq!(s.feed(&dialog(120), t), None);
    }

    /// Any other choice dialog a starting Claude opens before its first
    /// hook is watched for the same way: it waits on the user as the trust
    /// question does. The input box is not one.
    #[test]
    fn a_notice_before_the_first_hook_opens_the_trust_scanner() {
        let t = Instant::now();
        let notice = "\x1b[2J\x1b[H \u{2500}\u{2500}\u{2500}\r\n  Make auto mode your default \
                      permission mode?\r\n\r\n    \u{276f} Yes, set auto mode\r\n      No, keep \
                      manual mode\r\n";
        let mut s = QuestionScanner::trust(80, 24, b"");
        assert_eq!(s.feed(notice.as_bytes(), t), Some(true));
        let input_box =
            "\x1b[2J\x1b[H\u{2500}\u{2500}\u{2500}\r\n\u{276f} \r\n\u{2500}\u{2500}\u{2500}\r\n";
        s.feed(input_box.as_bytes(), t + ms(10));
        assert_eq!(s.feed(input_box.as_bytes(), t + ms(200)), Some(false));
    }

    #[test]
    fn a_squeezed_pane_does_not_panic() {
        let t = Instant::now();
        for (cols, rows) in [(0, 0), (1, 1), (2, 2)] {
            let mut s = QuestionScanner::new(cols, rows, &dialog(120));
            s.resize(cols, rows);
            s.feed(&dialog(120), t);
        }
    }
}
