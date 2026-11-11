//! The bytes that type a turn into an agent CLI: what nebula writes down a
//! session's PTY for a prompt sent to it, ahead of the Enter that submits
//! it. The daemon's sends and the TUI's composers type through this alike.

/// Bracketed-paste markers around a pasted block, so the child (claude,
/// vim…) takes it as one paste rather than typing to auto-indent.
pub const PASTE_START: &[u8] = b"\x1b[200~";
pub const PASTE_END: &[u8] = b"\x1b[201~";

/// The longest one-line turn that goes down as typed keys. The kernel hands
/// the CLI a burst from the PTY in reads of its own size (1,022 bytes on
/// macOS), and Claude Code takes a read of more than 800 characters for a
/// paste it found itself, then drops that read when the rest of the burst
/// follows in another: a 1,536-byte line arrived as its last 514 bytes
/// (2.1.322). A bracketed paste arrives whole at any length, and the model
/// is shown the same text either way: as typed up to 800 characters, in
/// `<pasted_content>` past them. Counted in bytes, never fewer than the
/// characters Claude counts.
pub const TYPED_MAX: usize = 800;

pub fn bracketed(text: &str) -> Vec<u8> {
    let mut data = PASTE_START.to_vec();
    data.extend_from_slice(text.as_bytes());
    data.extend_from_slice(PASTE_END);
    data
}

/// `text` as the bytes that type a turn: a bracketed paste when it has line
/// breaks, so the CLI takes it as one block instead of auto-indenting it
/// into mush, or is longer than [`TYPED_MAX`]; plain otherwise, so a short
/// line reaches the CLI as the user would type it.
pub fn turn_bytes(text: &str) -> Vec<u8> {
    if text.contains('\n') || text.len() > TYPED_MAX {
        bracketed(text)
    } else {
        text.as_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_line_is_typed_and_a_long_or_broken_one_is_pasted() {
        assert_eq!(turn_bytes("fix the login loop"), b"fix the login loop");
        let longest = "x".repeat(TYPED_MAX);
        assert_eq!(turn_bytes(&longest), longest.as_bytes());
        let long = "x".repeat(TYPED_MAX + 1);
        assert_eq!(turn_bytes(&long), bracketed(&long));
        assert_eq!(turn_bytes("a\nb"), b"\x1b[200~a\nb\x1b[201~");
        // Bytes, not characters: 401 `é` are 802 bytes.
        let wide = "é".repeat(TYPED_MAX / 2 + 1);
        assert_eq!(turn_bytes(&wide), bracketed(&wide));
    }
}
