//! `clauseo-limits`: reads each agent account's plan usage with the vendor's own tools, and keeps
//! the limits model: the merge of live signals, the refresh policy and the view.
//!
//! # Owns
//!
//! - Plan usage readers: the Claude usage helper (`helper/claude-usage.mjs`, embedded and written
//!   into claude-agent-acp's install folder) and the Codex reader (`codex app-server` through
//!   `codex_appserver`'s client). Each runs the vendor's binary with the agent's settings
//!   environment, so the vendor reads its own login. Clauseo reads no token and calls no usage
//!   endpoint.
//! - The limits model: [`Limits`], a pure state machine with no I/O and no clock. It takes
//!   inputs and `now`, and answers with [`Action`]s: which reads to start, what to persist and
//!   publish.
//! - The merge of live `_claude/rateLimit` signals into the last snapshot, the event-driven
//!   refresh policy, and the mapping from `AccountLimits` to `LimitsView` at a given `now`.
//!
//! # Interface
//!
//! - [`Limits`]: `new`, `input`, `view`, `next_deadline`, `next_read_at`, `at_limit`; with
//!   [`Input`], [`Action`], [`LiveSignal`], [`LiveStatus`] and [`LimitHit`].
//! - [`ReadLimits`], [`ReadOutcome`], [`ReadFailure`], and [`ProcessReaders`], which reads over a
//!   `process::Launcher` and a `process::Timer`.
//! - [`Error`]: why a reader could not be set up (a refused settings path, a failed file step);
//!   a read reports it as `ReadFailure::LaunchFailed` with its text.
//!
//! # Invariants
//!
//! - Clauseo never reads a credential: no `auth.json`, no `.credentials.json`, no keychain, no
//!   token. An `AccountRef` is an email the agent reports, or a fixed ref by billing.
//! - The model starts no timer and no retry: the host waits for [`Limits::next_deadline`].
//! - Every helper and app-server line is untrusted: size caps, unknown fields ignored, no panic.
//! - Files change only through `write.rs`, and only inside the adapter's install folder and
//!   `<data_dir>/limits/run` (made if missing; nothing in Clauseo writes there).
//!
//! # Depends on
//!
//! `protocol` (the limits types), `process` (the launcher, the timer, the redacted stderr tail),
//! `settings` (the agents' launch entries, the data folder), `codex_appserver` (the Codex
//! client), `serde_json`, `futures`, `tracing`, `thiserror`. Never `tauri`, `tokio` or
//! `keyring`.
//!
//! # Start reading
//!
//! `policy.rs` (the model's inputs and actions), then `model.rs` (the merge), `view.rs`, and
//! `readers.rs` with `readers/paths.rs` (the install folder), `readers/run.rs` (one run under a
//! deadline), `readers/claude.rs` and `readers/codex.rs` (with `readers/codex/map.rs`).

mod error;
mod model;
mod policy;
mod readers;
mod view;
mod words;
mod write;

pub use error::Error;
pub use model::{LimitHit, LiveSignal, LiveStatus};
pub use policy::{Action, Input, Limits};
pub use readers::{ProcessReaders, ReadFailure, ReadLimits, ReadOutcome};
