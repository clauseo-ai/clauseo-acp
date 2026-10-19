//! `fake_agent`: a scripted ACP v1 agent over stdio, for other crates' tests.
//!
//! # Owns
//!
//! - The scenario format: one `.jsonl` file per scenario, line 1 the header
//!   (`{"scenario", "description", "on_stdin_close": "exit" | "hang"}`), every other line one step
//!   with exactly one step key (`expect`, `expect_response`, `all`, `respond`, `update`, `notify`,
//!   `request`, `send_line`, `write_file`, `delete_file`, `move_file`, `stderr`, `spawn_child`,
//!   `exit`, `include`) and an optional `note`. Items of `all` are `{"expect": ...}` or
//!   `{"expect_response": ...}`. Files in `scenarios/common/` have no header and may not include.
//! - The bundled scenarios in `scenarios/`, compiled in (`bundled.rs` lists them once).
//! - The runner: a blocking line loop over stdin and stdout, no async runtime, no SDK role.
//!
//! # Interface
//!
//! - [`Scenario::parse`] and [`Scenario::load`]: parse and validate a scenario.
//! - [`SCENARIOS`]: the bundled names, sorted. [`scenario_path`]: a bundled scenario's source file.
//! - [`bin_path`]: the built binary, for another crate's tests. [`Error`]: every failure.
//! - [`cli`]: the command line, for the binary's `main` only.
//! - The binary: `fake_agent --scenario <name or path>`, `--check <path>...` (prints
//!   `ok <count>`), `--list`, `--hold`. Exit 0 when stdin closes, 2 for bad arguments or a
//!   scenario that fails to load, 3 for a mismatch, or an `exit` step's code. Every failure is
//!   one stderr line `fake_agent: <file>:<line>: <what>`.
//!
//! # Invariants
//!
//! - Stdout carries only JSON-RPC lines (and `send_line` text), each flushed. Diagnostics go to
//!   stderr.
//! - The agent waits only for the next client line: no timers, sleeps, randomness or clock.
//! - Every sent message without `"raw": true` survives the SDK's v1 schema for its method at load
//!   (`validate.rs`): deserialised, serialised again, and every value the scenario wrote is still
//!   there. A method the SDK does not know must be marked raw.
//! - File steps write only under the current session's `cwd`, with relative paths and no `..`.
//! - Only `main` exits the process.
//!
//! # Depends on
//!
//! `agent-client-protocol` (no features) for the schema, `serde`, `serde_json`, `thiserror`.
//! No Clauseo crate; `manifest_has_only_allowed_dependencies` enforces it.
//!
//! # Start reading
//!
//! `scenario.rs` (the model), then `run.rs` (the loop), then `validate.rs`.

mod bundled;
mod cli;
mod error;
mod pattern;
mod run;
mod scenario;
mod validate;

pub use bundled::{SCENARIOS, bin_path, scenario_path};
pub use cli::cli;
pub use error::Error;
pub use scenario::Scenario;

/// Parks the calling thread forever: `--hold`, and stdin closed under `"hang"`.
pub(crate) fn park_forever() -> ! {
    loop {
        std::thread::park();
    }
}
