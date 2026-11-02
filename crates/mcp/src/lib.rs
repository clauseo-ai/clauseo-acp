//! `clauseo-mcp`: the sidecar binary the agents start as an MCP server. One mode today,
//! `browser-guard`; R24 adds `connector`.
//!
//! # Owns
//!
//! - The browser guard ([`browser_guard`]): the process between an agent and agent-browser's
//!   built-in MCP server (`agent-browser mcp --tools core`, v0.41.1). It starts agent-browser with
//!   an environment built from scratch, relays stdio both ways, and refuses tool calls whose
//!   arguments would undo Clauseo's isolation. `docs/decisions/0029-browser.md` is the record.
//! - The one place the crate creates folders (`write.rs`) and starts a process
//!   (`browser_guard/relay.rs`, `spawn_child`).
//!
//! # Interface
//!
//! - [`browser_guard::run`]: the mode, given the arguments after `browser-guard`. It returns the
//!   exit code, or a [`Failure`] for `main` to print. `main.rs` holds the table of modes.
//! - The command line: `clauseo-mcp browser-guard --agent-browser <file> --cwd <folder> --
//!   <agent-browser args...>`. Exit 2 for a bad command line or a missing or invalid variable;
//!   1 when a folder cannot be made or agent-browser cannot start, or when agent-browser was
//!   killed by a signal; otherwise agent-browser's own exit code.
//!
//! # Invariants
//!
//! - Stdout carries only JSON-RPC lines, whole, one writer at a time. Diagnostics go to stderr.
//! - The child sees exactly the variables `browser_guard/env.rs` builds; nothing is inherited.
//! - Every line the agent sends reaches agent-browser only as the guard's own serialisation of
//!   the JSON value it checked, so both parse the same thing.
//! - The filter fails closed: anything it cannot read is answered with an error and dropped.
//! - Plain threads, no async runtime. Only `main` exits the process.
//!
//! # Depends on
//!
//! `serde_json` alone. `tests/guard/deps.rs` fails if an HTTP client, a TLS crate, tokio or
//! tauri becomes reachable.
//!
//! # Start reading
//!
//! `browser_guard.rs`, then `browser_guard/filter.rs` (the rules), `env.rs`, `relay.rs`.

pub mod browser_guard;
mod write;

/// A mode's failure: the exit code and the message `main` prints on stderr.
#[derive(Debug)]
pub struct Failure {
    /// The process exit code.
    pub code: i32,
    /// One or more lines for stderr, without a trailing newline.
    pub message: String,
}
