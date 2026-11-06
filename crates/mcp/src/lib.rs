//! `clauseo-mcp`: the sidecar binary the agents start as an MCP server. Two modes:
//! `browser-guard` and `connector`; on Unix a third, `browser-reaper`, which only the guard
//! starts (`browser_guard/reaper.rs`).
//!
//! # Owns
//!
//! - The browser guard ([`browser_guard`]): the process between an agent and agent-browser's
//!   built-in MCP server (`agent-browser mcp --tools core`, v0.41.1). It starts Chrome on the
//!   host's claimed debugging port, then agent-browser connected to it, with an environment built
//!   from scratch; relays stdio both ways; refuses tool calls whose arguments would undo
//!   Clauseo's isolation; and leaves no Chrome or agent-browser daemon behind when it ends.
//!   `docs/decisions/0029-browser.md` is the record.
//! - The connector relay client ([`connector`]): `clauseo-mcp connector <name>`, the stdio MCP
//!   server for a connector. It relays each JSON-RPC line, unchanged, over loopback TCP to the
//!   host's per-launch relay, and answers "not reachable" itself when there is no relay. It reads
//!   the relay address from `CLAUSEO_RELAY` and the token from the file `CLAUSEO_RELAY_TOKEN_FILE`
//!   names; it never puts the token on stdout, stderr or argv.
//! - The one place the crate creates folders (`write.rs`; the Unix socket folder under `/tmp`
//!   is made with mode 0700 and checked in `browser_guard/socket.rs`), and the three places it
//!   starts a process (`browser_guard/relay.rs` `spawn_child`, `chrome.rs` `spawn`, and on Unix
//!   `reaper.rs` `Reaper::start`).
//!
//! # Interface
//!
//! - [`browser_guard::run`]: the mode, given the arguments after `browser-guard`. It returns the
//!   exit code, or a [`Failure`] for `main` to print. `main.rs` holds the table of modes.
//! - The command line: `clauseo-mcp browser-guard --agent-browser <file> --cwd <folder>
//!   --cdp-port <port> -- <agent-browser args...>`, with `--daemon-port <port>` on Windows.
//!   Exit 2 for a bad command line or a missing or invalid variable; 1 when a folder cannot be
//!   made or Chrome or agent-browser cannot start, when Chrome does not listen on the claimed
//!   port within 15 s, when Chrome exits or the guard's parent goes, when agent-browser was
//!   killed (by a signal, by the guard after its exit wait, for a line over 64 MiB, or for a
//!   listener on a port the host did not claim); otherwise agent-browser's own exit code. Exit 2
//!   also for an unsafe Unix socket folder.
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
//! `serde_json`; `nix` on Unix and `windows` on Windows. `tests/guard/deps.rs` fails if an HTTP
//! client, a socket or TLS crate, an async runtime or tauri becomes reachable.
//!
//! # Start reading
//!
//! `browser_guard.rs`, then `browser_guard/filter.rs` (the rules), `env.rs`, `chrome.rs`,
//! `relay.rs`.

pub mod browser_guard;
pub mod connector;
mod write;

/// A mode's failure: the exit code and the message `main` prints on stderr.
#[derive(Debug)]
pub struct Failure {
    /// The process exit code.
    pub code: i32,
    /// One or more lines for stderr, without a trailing newline.
    pub message: String,
}
