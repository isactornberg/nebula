//! A session's screen as text. The daemon keeps bytes, not a screen (its
//! `pty::cursor` says why: the emulator lives in the client), so a client
//! that reads one lays the end of the session's ring out for itself, with
//! [`screen_text`], as the daemon lays one out to read a turn's report line.

/// How much of a session's ring a read of its screen asks the daemon for
/// (`TailOutput`): all it hands out at once, which is its own cap, since a
/// CLI's redraws can take more bytes than a shell's prompt does to lay one
/// screen out. `nebula session read` and the BOARD both ask this much.
pub const SCREEN_TAIL_BYTES: u32 = 64 * 1024;

pub use nebula_core::why::screen_text;
