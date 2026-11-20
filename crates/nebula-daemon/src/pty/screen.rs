//! The screen a session's child shows now: a vt100 parser at the PTY's
//! size, fed every byte the child writes. It answers cursor position
//! reports (`CSI 6 n`): a child asking where its cursor is — crossterm's
//! `cursor::position()`, which inline ratatui viewports and line editors
//! call on startup — blocks on the reply and gives up after two seconds
//! ("The cursor position could not be read within a normal duration"),
//! and unlike the kitty and DA1 queries the answer is screen state. And it
//! is what a read of the screen gets (`TailOutput`): the ring's last bytes
//! cannot rebuild it once a CLI has redrawn one line for long enough, as a
//! status line redrawn for days fills any tail with nothing else.

pub struct Screen {
    /// No scrollback: only what is on screen is ever read.
    parser: vt100::Parser,
}

impl Screen {
    /// A screen at the PTY's size with `history` replayed into it: the
    /// ring so far, the same replay an attaching client parses, or a
    /// screen [`Self::state`] wrote out.
    pub fn new(cols: u16, rows: u16, history: &[u8]) -> Self {
        let (cols, rows) = grid_size(cols, rows);
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(history);
        Self { parser }
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.parser.process(data);
    }

    /// Follow the PTY's size: it is the one the child lays out against.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let (cols, rows) = grid_size(cols, rows);
        self.parser.screen_mut().set_size(rows, cols);
    }

    /// The reply to `CSI 6 n`: `CSI row ; col R`, 1-based.
    pub fn report(&self) -> Vec<u8> {
        let screen = self.parser.screen();
        let (row, col) = screen.cursor_position();
        // After a write to the last column vt100 parks the cursor one past
        // it until the wrap happens; a terminal reports the last column.
        let col = col.min(screen.size().1 - 1);
        format!("\x1b[{};{}R", row + 1, col + 1).into_bytes()
    }

    /// The screen as the bytes that draw it, cleared first: its rows with
    /// their looks, the cursor where it is, and the input modes. Laid out
    /// at the PTY's size they are this screen.
    pub fn state(&self) -> Vec<u8> {
        self.parser.screen().state_formatted()
    }

    /// Each row's text, its trailing blanks trimmed, the blank rows under
    /// the last one written dropped.
    pub fn rows(&self) -> Vec<String> {
        let screen = self.parser.screen();
        let mut out: Vec<String> = screen
            .rows(0, screen.size().1)
            .map(|row| row.trim_end().to_string())
            .collect();
        while out.last().is_some_and(String::is_empty) {
            out.pop();
        }
        out
    }
}

/// A client can ask for a pane squeezed to nothing, and vt100's grid
/// arithmetic underflows below two cells a side (a wrap on one row, a wide
/// character in one column) — a panic that would take the pump with it.
const MIN_SIDE: u16 = 2;

fn grid_size(cols: u16, rows: u16) -> (u16, u16) {
    (cols.max(MIN_SIDE), rows.max(MIN_SIDE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_places_the_cursor() {
        let t = Screen::new(80, 24, b"$ muse\r\nhello");
        assert_eq!(t.report(), b"\x1b[2;6R");
    }

    #[test]
    fn fed_output_moves_it() {
        let mut t = Screen::new(80, 24, b"");
        assert_eq!(t.report(), b"\x1b[1;1R");
        t.feed(b"\x1b[10;20H");
        assert_eq!(t.report(), b"\x1b[10;20R");
    }

    #[test]
    fn a_pending_wrap_reports_the_last_column() {
        let mut t = Screen::new(5, 3, b"abcde");
        assert_eq!(t.report(), b"\x1b[1;5R");
        t.feed(b"f");
        assert_eq!(t.report(), b"\x1b[2;2R");
    }

    #[test]
    fn a_resize_clamps_the_cursor_into_the_new_grid() {
        let mut t = Screen::new(80, 24, b"\x1b[20;70H");
        t.resize(40, 10);
        assert_eq!(t.report(), b"\x1b[10;40R");
    }

    #[test]
    fn a_squeezed_pane_is_floored_at_two_cells_a_side() {
        // No panic, and the cursor ends in the corner of the floored grid.
        let nasty = "wrap wrap\r\n漢字😀\x1b[999;999Hx\n\n".as_bytes();
        for (cols, rows) in [(0, 0), (1, 1), (1, 5), (5, 1), (2, 2)] {
            let mut t = Screen::new(cols, rows, nasty);
            t.resize(cols, rows);
            t.feed(nasty);
            let corner = format!("\x1b[{};{}R", rows.max(2), cols.max(2));
            assert_eq!(t.report(), corner.as_bytes(), "{cols}x{rows}");
        }
    }

    #[test]
    fn its_state_draws_the_same_screen_however_long_the_redraws_ran() {
        let mut t = Screen::new(40, 6, b"\x1b[1mreply\x1b[0m\r\n> \r\n");
        for i in 0..10_000 {
            t.feed(format!("\x1b[6;1H\x1b[2Kstatus {i}").as_bytes());
        }
        let again = Screen::new(40, 6, &t.state());
        assert_eq!(again.rows(), ["reply", ">", "", "", "", "status 9999"]);
        assert_eq!(again.state(), t.state());
        assert_eq!(again.report(), t.report());
    }
}
