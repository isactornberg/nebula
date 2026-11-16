//! What GitHub and Linear say about a project's work: its pull requests
//! and issues, asked through `gh` in the checkout and Linear's GraphQL API,
//! and parsed into plain data. No UI and no daemon state live here, so the
//! TUI and the daemon ask the same calls and read the same answers. And
//! WEB PUSH to a phone paired to `nebula serve` ([`push`]): the server keeps
//! its subscription and the daemon sends to it.

pub mod issues;
pub mod linear;
pub mod pull_request;
pub mod push;
