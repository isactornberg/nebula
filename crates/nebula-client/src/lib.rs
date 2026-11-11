//! What every client of the DAEMON shares, the TUI and a server in front
//! of the daemon alike: the connection and its handshake, a subscribed
//! client that keeps the entity tree current, the orders read off that
//! tree, the BOARD, a session's screen as text, typing a session's next
//! turn, and finding the `nebula serve` in front of the daemon.

pub mod board;
pub mod client;
pub mod connection;
pub mod screen;
pub mod send;
pub mod serve;
pub mod tree;

pub use client::{Client, Closed};
pub use tree::Tree;
