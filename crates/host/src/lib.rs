//! `clauseo-host`: the session host. One thread runs matters, threads, turns and agent
//! sessions, and serves commands, queries and subscriptions to every caller.
//!
//! # Owns
//!
//! - Its own thread, `clauseo-host`, and its executor: sessions, commands and their receipts,
//!   permission requests and elicitations, text coalescing and the rate cap, subscriptions, and
//!   the checkpoints before and after each turn, undo, restore, and a `CheckpointRow` for every
//!   checkpoint. `docs/decisions/0011-host-turn-lifecycle.md` records the turn order, the
//!   versions rules and the reason for each choice.
//! - The next `seq` of every stream, the patch window and epoch of every loaded stream, and the
//!   sent view of what is in flight to the store.
//! - One `clauseo-matter` thread per open matter, one `clauseo-snapshot` thread, and a
//!   short-lived `clauseo-stat` thread per `ListMatters` that reads each matter's folder state,
//!   and a short-lived `clauseo-scan` thread per `ScanFolder` (one at a time) that walks the
//!   folder before it becomes a matter and posts its counts back.
//! - What a cancelled turn left running (`agent/leftovers.rs`, decision 0011): a short-lived
//!   `clauseo-sweep` thread lists the agent's descendants as each prompt goes out, and another
//!   ends those new since that started before the turn's end when it ends cancelled, but the
//!   session's MCP servers. An agent's stop lists them on one first, for its tree kill.
//! - The `clauseo-search` thread (`search.rs`, decision 0043): it owns `search.sqlite` in the
//!   data folder, opens it (or rebuilds it from the log through the store) without holding up
//!   start-up, and indexes each returned commit's envelopes, which the actor hands over a bounded
//!   channel with `try_send` and never waits for; a full channel makes it catch up from its
//!   per-stream watermarks. A forgotten matter's rows are deleted; its import indexes it again.
//! - The Scratch matter: a first start creates it.
//! - The matter rail (`actor/rail.rs`, `actor/matter_details.rs`): each matter's details on
//!   its row (`UpdateMatter`, `OpenMatter`), its status computed from its threads, its last
//!   activity, `ForgetMatter` (which deletes nothing; an import of the folder brings the matter
//!   back), a new matter in the default location (`GetDefaultLocation` shows where), and one
//!   error code per refused folder.
//!   `docs/decisions/0022-matters-for-the-rail.md` records the rules. Each thread's seen mark
//!   (`MarkSeen`, `actor/seen.rs`) and last stop, and the matter list and thread lists sent to
//!   app subscribers as their rows change (`actor/app_lists.rs`):
//!   `docs/decisions/0023-seen-marks-and-app-lists.md`. Each thread's triage (`SnoozeThread`,
//!   `MarkThreadDone`, the morning sweep `SweepDone`, and the snooze timer that logs a snooze's
//!   wake at its time; `actor/triage.rs`): `docs/decisions/0045-thread-triage.md`.
//! - The profile hook: [`ProfileHook`], with [`NoProfile`] as the default. Its `_meta` goes on
//!   `session/new` and `session/load`, and its variables into each launch, with the host's
//!   switches over them; its servers after the lawyer's, its `CODEX_CONFIG` keys and its skill
//!   files, with the per-session placeholders filled (`agent/loads/profile.rs`,
//!   `agent/loads/browser.rs`; decision record 0030). The tools each thread's header shows are
//!   an unlogged overlay (`actor/header_tools.rs`).
//! - What a session loads (`agent/loads.rs`): only the MCP servers the lawyer turned on, under
//!   Claude Code's strict MCP config with `settingSources: ["user"]` and claude.ai connectors
//!   off. When Claude Code's enterprise `managed-mcp.json` is present, none of its servers and
//!   no strict config go: the firm controls MCP there. Every session's `_meta` also turns
//!   bypass off (`allowDangerouslySkipPermissions: false`), and a Claude session that starts in
//!   another mode than `default` (the user's `permissions.defaultMode`, a resumed transcript's
//!   mode) gets `session/set_mode default` before its first prompt; a failed switch fails the
//!   launch.
//! - What a Codex agent is launched with (`agent/codex.rs`): its ask mode
//!   (`INITIAL_AGENT_MODE=workspace-write`), its home under the data folder as `CODEX_HOME` with a
//!   `config.toml` of the same switches for app-server's start-up, a
//!   `CODEX_CONFIG` with `clauseo_limits::CODEX_OVERRIDES` (and on Windows the unelevated
//!   sandbox, record 0025) and the hooks seam's object, and no
//!   MCP filtering, over any settings value; its refusal in a folder holding `.codex` (#184);
//!   and, before its first prompt, `session/set_mode` to the ask mode and the guard's trust check.
//!   No agent inherits a `GIT_*` variable. `docs/decisions/0025-codex-in-acp-and-host.md`.
//! - The session start: `session/load` when the agent offers it, else `session/resume`, else
//!   `session/new`.
//! - The pre-write guard ([`pre_write_guard`]): the check an agent's `PreToolUse` hook runs
//!   before each file edit and shell command, in every permission mode. It refuses a write to a
//!   file of the matter that Word holds open (an Office owner file beside it, or on Windows a
//!   file another program holds with a share mode that refuses others), a Codex patch into the
//!   matter's `.codex` folder, and a Codex shell command naming a `.codex` folder. When
//!   [`HostConfig::with_pre_write_guard`] names the program to run, Claude Code's entry goes in
//!   the session's `_meta`, with the matter folder, and Codex's in its `CODEX_CONFIG`.
//!
//! # Interface
//!
//! - [`Host::start`] with a [`HostConfig`] ([`HostConfig::new`] takes the settings and the
//!   [`Launcher`](clauseo_process::Launcher); [`HostConfig::with_profile_hook`],
//!   [`HostConfig::with_refused_folders`], [`HostConfig::with_home_folder`],
//!   [`HostConfig::with_managed_mcp_path`], [`HostConfig::with_pre_write_guard`],
//!   [`HostConfig::with_clock`], [`HostConfig::with_ids`] and
//!   [`HostConfig::with_limits_reader`] set the rest). It returns once
//!   start-up is done: the store is open in `Effective::data_dir()`, the Scratch matter exists,
//!   and start-up reconciliation has committed. A second start on the same data folder fails
//!   with `store.locked`.
//! - [`HostHandle`], `Clone` and `Send`: [`HostHandle::dispatch`], [`HostHandle::query`],
//!   [`HostHandle::subscribe`], [`HostHandle::subscribe_events`],
//!   [`HostHandle::subscribe_app`], [`HostHandle::subscribe_limits`], [`HostHandle::ack`],
//!   [`HostHandle::register_folder`], [`HostHandle::register_file`],
//!   [`HostHandle::search`] (or `HostQuery::Search`; a newer `generation` from the same
//!   window answers an older search `Cancelled`, interrupting it if it runs),
//!   [`HostHandle::locate_file`] and [`HostHandle::matter_folder`] (the desktop's `open_file`
//!   and `reveal`, 0026; the local desktop only), and [`HostHandle::shutdown`]. Each call is
//!   one request over a `bounded(256)` channel with a `futures` oneshot for the reply.
//!   [`HostJoin::join`] waits for the thread to end.
//! - [`Subscription`] yields [`EncodedEvent`]s: each `HostEvent` serialised once and shared by
//!   every subscriber of the stream (or of the limits); [`EncodedEvent::json`] is sent on as it
//!   is and [`EncodedEvent::decode`] gives the type back. [`EventSubscription`] yields typed
//!   `HostEvent`s: the replay, one `CaughtUp`, then live `Events`. [`AppSubscription`]
//!   yields the app-wide events of no stream as [`EncodedEvent`]s: `SettingsChanged`,
//!   `MattersChanged` and `ThreadsChanged`, starting with the current state of each.
//! - `HostQuery::ListAgents` lists the settings agents with their kinds.
//! - The display settings: `HostQuery::GetSettings` reads them and `HostCommand::SetUiSettings`
//!   changes them through `Effective::apply_ui_patch`, on the actor; a patch naming a key policy
//!   locks is `settings.locked`. `docs/decisions/0017-settings-over-the-wire.md` records it.
//!   The tool switches are `SetToolSettings` through `Effective::apply_tools_patch`, the same
//!   way; a session reads them at its start, so a change reaches the next session
//!   (`docs/decisions/0030-launch-profile.md`, "Tool switches over the wire"). The privacy
//!   switches are `SetPrivacySettings` and `FirstAct` (`actor/settings/privacy.rs`): off is
//!   saved before the usage counts folder is deleted, and on is refused with `store.io` while it
//!   cannot be (`docs/decisions/0036-counts.md`).
//! - The usage counts: `HostConfig::with_counts` hands the host a `clauseo_counts::Counts`, and
//!   `actor/counting.rs` bumps closed events into it at each call site, once per operation that
//!   succeeded; with none, or counts off, it counts nothing. Counting never blocks or fails a
//!   command.
//! - [`session_folder`]: the canonical spelling of a matter folder that an agent session
//!   works in (its process's folder and its `cwd`), for whatever else must compare with it.
//! - [`Error`] wraps the errors of `protocol`, `store`, `matters`, `process`, `acp`, `reducer`
//!   and `settings`, plus `Closed` and `Internal`; [`Error::code`] gives its `ErrorCode`, and
//!   every public call returns it as a `HostError`. After shutdown or a host panic, calls and
//!   `join` return `internal`.
//! - [`Clock`] ([`SystemClock`], [`Now`]) and [`IdSource`] ([`RandomV7`]) are the host's only
//!   sources of time and ids. The `test-support` feature adds `ManualClock`, `SequentialIds`,
//!   `HostConfig::with_patch_window_bytes`, `HostConfig::with_app_subscription_capacity`,
//!   `HostConfig::with_codex_hooks`, `HostConfig::with_search_channel_capacity` and
//!   `HostConfig::with_search_holds` (with [`SearchHold`]),
//!   `EncodedEvent::ptr_eq` and
//!   `HostHandle::abort_for_test`, which ends the thread with no flush and no graceful stop.
//! - [`codex_hook_config`]: the `hooks` value for Codex's config that runs the same guard,
//!   trusted by its own `state` entry; each Codex launch's `CODEX_CONFIG` carries it when
//!   [`HostConfig::with_pre_write_guard`] names the program.
//! - [`pre_write_guard::run`] reads one hook input object and returns 0, or
//!   [`pre_write_guard::DENY`] with the reason on stderr (0 with a line starting
//!   [`pre_write_guard::IGNORED`] when it could not read the input);
//!   [`pre_write_guard::FLAG`] (then
//!   `--matter <folder>`) is what the hook passes. It runs in the hook's own process, with no
//!   host thread.
//! - The limits (`actor/limits.rs`): the host owns the `clauseo_limits::Limits` model, starts
//!   its reads on the actor's pool (the reader is [`HostConfig::with_limits_reader`]'s, by
//!   default `clauseo_limits::ProcessReaders`), feeds it live rate-limit signals, turn ends,
//!   pending questions and `RefreshLimits`, keeps one timer at its next deadline, stores each
//!   changed `AccountLimits` (never in the log), and publishes `AccountLimitsChanged` and
//!   `LimitsViewChanged` to limits subscribers ([`HostHandle::subscribe_limits`], a
//!   [`Subscription`]). A turn stopped at the limit with no reset time logs its
//!   `TurnStoppedAtLimit` in a commit of its own after its agent's next read (an agent id with
//!   no kind keeps it in the end commit), and a Codex prompt failing with a JSON-RPC error waits
//!   at most 10 s for one read, which decides whether it ends at the limit or failed.
//!
//! # Invariants
//!
//! - One actor on a `futures` `LocalPool`. Its loop awaits nothing but its input channel
//!   (requests, agent events, timers, matter, snapshot and store results); every wait runs as a
//!   spawned task that posts its result back. async-io's timers and readiness run on async-io's
//!   own thread.
//! - Committed before published: an event reaches a subscriber, and a command its answer, only
//!   after the commit holding it returned `Ok`. Text deltas are the one exception: they reach
//!   view subscribers at most once per 16 ms (at once after a quiet 16 ms) and are committed in
//!   250 ms durable batches.
//! - `seq` is gapless per stream. Commits are pipelined (each takes `pending + 1` and is sent
//!   without waiting for the ones before it) and applied in `seq` order. After a failed commit
//!   nothing more is sent for that stream until its in-flight commits have returned, and no
//!   `seq` is reused.
//! - No clock read outside `SystemClock::now`; every delay runs on the host's [`Clock`].
//! - Agents start only through the [`Launcher`](clauseo_process::Launcher) in the config.
//! - No `tokio` and no `tauri`. Matter work runs on the `clauseo-matter` threads and zstd on
//!   `clauseo-snapshot`, never on the actor. The actor never touches the search index.
//!
//! # Depends on
//!
//! `protocol`, `reducer`, `store`, `matters`, `process`, `acp`, `settings`, `search`, `limits` (its
//! model, its readers and `CODEX_OVERRIDES`), `profile` (the placeholders and the skill writes,
//! 0030), `counts` (the usage counts, 0036), `sandbox` (the browser's socket folder and port
//! claims, 0029), `getrandom` (Windows browser session names), `async-channel`, `async-io`,
//! `futures`, `uuid`, `serde_json`, `tracing`, `thiserror`, `dunce` and `sha2` (the Codex
//! hook's trust hash, 0024). Never `tauri`, `tokio` or `agents`.
//!
//! # Start reading
//!
//! `handle.rs` (the public calls and the subscription streams), `actor.rs` (the thread, the
//! loop and the methods the rest call), then `actor/`: `input.rs`, `startup.rs`, `commands.rs`,
//! `streams.rs`, `subs.rs` and the turn path in `turn.rs`. Then the leaf modules: `pipeline.rs`
//! and `sent.rs` (seq and the sent view), `view.rs`, `window.rs`, `encode.rs`, `coalesce.rs`,
//! `rate.rs`, `build.rs`, `agent.rs`, `matter_worker.rs`, `snapshot.rs`, `config.rs`,
//! `clock.rs`, `ids.rs` and `error.rs`.

#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::iter_over_hash_type,
    clippy::wildcard_enum_match_arm
)]

mod actor;
mod agent;
mod blob_read;
pub mod browser;
mod build;
mod clock;
mod coalesce;
mod config;
mod encode;
mod error;
mod file_link;
mod handle;
mod ids;
mod matter_worker;
mod pipeline;
pub mod pre_write_guard;
mod principal;
mod rate;
mod search;
mod sent;
mod snapshot;
mod sources;
mod undo_note;
mod view;
mod window;
mod write;

pub use agent::loads::codex_hook_config;
pub use agent::session_folder;
pub use clauseo_acp::McpServer;
#[cfg(feature = "test-support")]
pub use clock::ManualClock;
pub use clock::{Clock, Now, SystemClock};
#[cfg(feature = "test-support")]
pub use config::TestProxy;
pub use config::{EgressProxy, HostConfig, NoProfile, ProfileHook, SessionKey};
pub use encode::EncodedEvent;
pub use error::Error;
pub use handle::{
    AppSubscription, EventSubscription, Host, HostHandle, HostJoin, LocatedFile, Subscription,
};
#[cfg(feature = "test-support")]
pub use ids::SequentialIds;
pub use ids::{IdSource, RandomV7};
#[cfg(feature = "test-support")]
pub use search::SearchHold;
