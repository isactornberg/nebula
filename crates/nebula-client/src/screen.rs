//! A session's screen as text. The daemon hands a screen out as the bytes
//! that draw it (`TailOutput`), so a client that reads one lays them out
//! for itself, with [`screen_text`], as the daemon does to read a turn's
//! report line.

/// How much of a session's ring a read of its screen asks for
/// (`TailOutput`), which only a daemon from before it kept each session's
/// screen reads: it then sends the ring's end, all it hands out at once.
/// `nebula session read` and the BOARD both ask this much.
pub const SCREEN_TAIL_BYTES: u32 = 64 * 1024;

pub use nebula_core::why::screen_text;
