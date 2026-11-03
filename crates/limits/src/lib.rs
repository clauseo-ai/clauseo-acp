//! `clauseo-limits`: reads each agent account's plan usage with the vendor's own tools, and keeps
//! the limits model: the merge of live signals, the refresh policy and the view.
//!
//! # Owns
//!
//! - Plan usage readers: the Claude usage helper (`helper/claude-usage.mjs`, embedded and written
//!   into claude-agent-acp's install folder, which must hold the Claude Agent SDK as a regular
//!   `node_modules/@anthropic-ai/claude-agent-sdk/package.json` and the regular entry file it
//!   names) and the Codex reader
//!   (`codex app-server` through `codex_appserver`'s client). Each runs the vendor's binary with
//!   the agent's settings environment, so the vendor reads its own login. Clauseo reads no token
//!   and calls no usage endpoint.
//! - The limits model: [`Limits`], a pure state machine with no I/O and no clock. It takes
//!   inputs and `now`, and answers with [`Action`]s: which reads to start, what to persist and
//!   publish.
//! - The merge of live `_claude/rateLimit` signals into the last snapshot, the event-driven
//!   refresh policy, and the mapping from `AccountLimits` to `LimitsView` at a given `now`.
//!
//! # Interface
//!
//! - [`Limits`]: `new`, `input`, `view`, `next_deadline`, `next_read_at`,
//!   `at_limit(agent, now)`; with [`Input`], [`Action`], [`LiveSignal`], [`LiveStatus`] and
//!   [`LimitHit`].
//! - [`ReadLimits`], [`ReadOutcome`], [`ReadFailure`], and [`ProcessReaders`], which reads over a
//!   `process::Launcher` and a `process::Timer`. A refused settings path or a failed file step
//!   comes back as `ReadFailure::LaunchFailed` with its text.
//! - [`CODEX_OVERRIDES`]: the `-c` overrides the Codex reader passes `app-server`, which R15's
//!   `CODEX_CONFIG` for codex-acp reuses.
//! - Hidden, for G16's live tests only (`tests/live.rs`): `ProcessReaders::with_stdout_tap` and
//!   `StdoutTap`, a copy of each read's raw, unredacted stdout. They exist only with the
//!   `live-tap` feature, which only this crate's own dev-dependency turns on. That holds per
//!   package: once another crate depends on `clauseo-limits`, a workspace run
//!   (`cargo clippy --workspace --all-targets`, `cargo nextest run --workspace`) builds the
//!   library with the tap on for every crate. A per-package build still catches a production
//!   caller: ci.yml's `cargo clippy -p clauseo-desktop --all-targets --features marks` and every
//!   release build compile `limits` without it.
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
//! `policy.rs` (the model's inputs and actions), `kinds.rs` (what differs per agent kind), then
//! `model.rs` (the merge), `view.rs`, and `readers.rs` with `readers/paths.rs` (the install
//! folder), `readers/run.rs` (one run under a deadline), `readers/claude.rs` and
//! `readers/codex.rs` (with `readers/codex/map.rs`).

mod error;
mod kinds;
mod model;
mod policy;
mod readers;
mod view;
mod words;
mod write;

pub use model::{LimitHit, LiveSignal, LiveStatus};
pub use policy::{Action, Input, Limits};
#[cfg(feature = "live-tap")]
#[doc(hidden)]
pub use readers::StdoutTap;
pub use readers::{CODEX_OVERRIDES, ProcessReaders, ReadFailure, ReadLimits, ReadOutcome};
