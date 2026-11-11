//! A session's screen as text. The daemon keeps bytes, not a screen (its
//! `pty::cursor` says why: the emulator lives in the client), so a client
//! that reads one lays the end of the session's ring out for itself.

/// How much of a session's ring a read of its screen asks the daemon for
/// (`TailOutput`): all it hands out at once, which is its own cap, since a
/// CLI's redraws can take more bytes than a shell's prompt does to lay one
/// screen out. `nebula session read` and the BOARD both ask this much.
pub const SCREEN_TAIL_BYTES: u32 = 64 * 1024;

/// `data`'s screen as text, every row of it (the TUI's card tail stops at
/// the cursor's row and drops blank rows, which would cut the options off a
/// dialog), each with its trailing blanks trimmed, the blank rows under
/// the last one written dropped.
pub fn screen_text(data: &[u8], cols: u16, rows: u16) -> Vec<String> {
    let mut parser = vt100::Parser::new(rows.max(2), cols.max(2), 0);
    parser.process(data);
    let mut out: Vec<String> = parser
        .screen()
        .rows(0, cols.max(2))
        .map(|row| row.trim_end().to_string())
        .collect();
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}
