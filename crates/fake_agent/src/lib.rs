//! `fake_agent`: a scripted ACP v1 agent over stdio, for other crates' tests.
//!
//! # Owns
//!
//! - The scenario format: one `.jsonl` file per scenario, a header line, then one step per line.
//!   `scenarios/README.md` is the full reference: every step and its fields, the pattern rules,
//!   labels, includes, `raw`, the exit codes, and how to add a scenario.
//! - The bundled scenarios in `scenarios/`, compiled in (`bundled.rs` lists them once).
//! - The runner: a blocking line loop over stdin and stdout, no async runtime, no SDK role.
//!
//! # Interface
//!
//! - [`Scenario::parse`] and [`Scenario::load`]: parse and validate a scenario.
//! - [`SCENARIOS`]: the bundled names, sorted. [`scenario_path`]: a bundled scenario's source file.
//! - [`bin_path`]: the built binary, for another crate's tests. [`Error`]: every failure.
//! - [`cli`]: the command line, for the binary's `main` only.
//! - The exit codes: [`EXIT_OK`] 0, [`EXIT_WRITE`] 1, [`EXIT_LOAD`] 2, [`EXIT_MISMATCH`] 3.
//! - The binary: `fake_agent --scenario <name or path>`, `--check <path>...` (prints
//!   `ok <count>`), `--list`, `--hold`. Exit [`EXIT_OK`] when stdin closes, [`EXIT_LOAD`] for
//!   bad arguments or a scenario that fails to load, [`EXIT_MISMATCH`] for a mismatch,
//!   [`EXIT_WRITE`] when `--list` or `--check` cannot write its result to stdout, or an `exit`
//!   step's code (never 2 or 3, refused at load). Every failure is one stderr line
//!   `fake_agent: <file>:<line>: <what>`.
//!
//! # Invariants
//!
//! - Stdout carries only JSON-RPC lines (and `send_line` text), each flushed. Diagnostics go to
//!   stderr.
//! - The agent waits only for the next client line: no timers, sleeps, randomness or clock.
//! - Every sent message without `"raw": true` survives the SDK's v1 schema for its method at load
//!   (`validate.rs`): deserialised, serialised again, and every value the scenario wrote is still
//!   there. A method the SDK does not know must be marked raw.
//! - `write_file`, `delete_file` and `move_file` change files only under the current session's
//!   `cwd`, with relative paths and no `..`. `spawn_child` writes its `pid_file` relative to the
//!   process folder (the agent's working directory), not the session `cwd`, also with no `..`.
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
mod write;

pub use bundled::{SCENARIOS, bin_path, scenario_path};
pub use cli::cli;
pub use error::Error;
pub use scenario::Scenario;

/// Exit code: stdin closed under `on_stdin_close: "exit"` (at any step, so steps may be left),
/// or `--list` or `--check` succeeded.
pub const EXIT_OK: i32 = 0;
/// Exit code: `--list` or `--check` could not write its result to stdout.
pub const EXIT_WRITE: i32 = 1;
/// Exit code: bad arguments, or a scenario that fails to load. An `exit` step may not use it.
pub const EXIT_LOAD: i32 = 2;
/// Exit code: a client line did not match the scenario, or a step failed while running. An
/// `exit` step may not use it.
pub const EXIT_MISMATCH: i32 = 3;

/// `what` as one stderr line: `fake_agent: <what>\n`.
pub(crate) fn diagnostic(what: impl std::fmt::Display) -> String {
    format!("fake_agent: {what}\n")
}

/// Parks the calling thread forever: `--hold`, and stdin closed under `"hang"`.
pub(crate) fn park_forever() -> ! {
    loop {
        std::thread::park();
    }
}
