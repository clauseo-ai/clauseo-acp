//! `clauseo-acp`: the client side of ACP v1 over any pair of async pipes, on the host's executor.
//!
//! # Owns
//!
//! - The one wrapper over `agent-client-protocol` 3.3.0 (schema 1.11.0): no other crate but
//!   `fake_agent` names the SDK, and no SDK type appears in this interface. A later SDK major
//!   changes only this crate.
//! - What Clauseo advertises in `initialize` (`driver.rs`, `client_capabilities`), the version
//!   check, and the refusal of `fs/*` and `terminal/*` (-32601).
//! - Turning agent traffic into [`AcpEvent`]s built from `protocol`'s types (`map.rs`), the
//!   typed plan limit signals ([`LimitSignal`]), and the stdout line filter with its caps
//!   (`line.rs`): a line at most 8 MiB, a text chunk at most 1 MiB.
//!
//! # Interface
//!
//! [`connect`] takes the agent's stdin and stdout and gives an [`AcpClient`] (one async method
//! per ACP call the first app needs), the event receiver, and the [`Driver`] future the host
//! spawns. Events carry an [`Origin`]; agent requests carry a [`PendingKey`] to answer with.
//! Every failure is an [`Error`] with [`Error::code`].
//!
//! # Invariants
//!
//! - No spawn, no sleep, no clock, no executor: all work happens inside [`Driver`]. The host
//!   owns timers, including the cancel fallback.
//! - The event channel holds 1,024 events. When it is full the driver stops reading the agent,
//!   which slows it. The host must keep draining events while it awaits any call.
//! - No call blocks another: a pending prompt never holds back a permission answer or a cancel.
//!   A call's future may be dropped; its answer is then discarded.
//! - Origins: `Turn(t)` while turn `t`'s prompt is in flight; `Late(t)` from `cancel()` until the
//!   next prompt starts; `Replay` between sending `session/load` and its response; else `Idle`.
//!   A response's callback runs before any later agent message, so tags never guess from timing.
//! - `_meta` is read only at `_claude/rateLimit` (on `usage_update`) and
//!   `jetbrains.air.sessionFailure` (on `session_info_update` and the prompt response). A
//!   `session/new` `_meta` is sent verbatim and never read.
//! - A malformed line, an unknown update or an unknown content block never ends the connection:
//!   they become a [`ProtocolWarning`] or a `ContentBlock::Unsupported`.
//! - No Clauseo id is minted here: agent ids stay strings, and `host` maps [`PendingKey`]s.
//!
//! # Depends on
//!
//! `protocol`, `agent-client-protocol` (features `unstable_session_fork`,
//! `unstable_llm_providers`, `unstable_end_turn_token_usage`, `unstable_plan_operations`),
//! `futures`, `async-channel`, `serde`, `serde_json`, `memchr`, `thiserror`. Never `tauri`,
//! `tokio` or `process`: the host hands over the pipes.
//!
//! # Start reading
//!
//! `driver.rs` (the connection and the command loop), then `inbound.rs` (what the agent sends),
//! `line.rs` (the stdout line path), `map.rs` (the mappings) and `state.rs`. Tests: unit tests
//! in `src/*/tests.rs`; the stdio tests against `fake_agent` in `tests/stdio/`, with this
//! crate's own scenarios in `tests/scenarios/`.

#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::iter_over_hash_type,
    clippy::wildcard_enum_match_arm
)]

mod client;
mod driver;
mod error;
mod event;
mod inbound;
mod line;
mod map;
mod state;
mod types;

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use agent_client_protocol::{Client, Lines};

pub use client::AcpClient;
pub use driver::Driver;
pub use error::Error;
pub use event::{
    AcpEvent, AgentUpdate, FrameDirection, LimitSignal, LimitStatus, MessageRole, Origin,
    PendingKey, ProtocolWarning, RawFrame, ToolCall,
};
pub use types::{
    AgentInfo, AuthKind, AuthMethod, ConnectOptions, LoadSession, McpServer, NewSession, PromptEnd,
    SessionMode, SessionModes, SessionStart, SteerResult,
};

/// How many events the channel holds before the driver stops reading the agent.
pub const EVENT_CAPACITY: usize = 1024;

/// Starts a connection over the agent's stdin (`writer`) and stdout (`reader`). Nothing happens
/// until the host spawns the [`Driver`]; calls made before then wait for it.
pub fn connect<W, R>(
    writer: W,
    reader: R,
    options: ConnectOptions,
) -> (AcpClient, async_channel::Receiver<AcpEvent>, Driver)
where
    W: futures::io::AsyncWrite + Send + Unpin + 'static,
    R: futures::io::AsyncRead + Send + Unpin + 'static,
{
    let (events, receiver) = async_channel::bounded(EVENT_CAPACITY);
    let (commands, command_receiver) = async_channel::unbounded();
    let (stop, stop_receiver) = async_channel::bounded(1);
    let frames_dropped = Arc::new(AtomicU64::new(0));
    let shared = Arc::new(state::Shared::new(
        events,
        options.frame_tap,
        Arc::clone(&frames_dropped),
        stop,
    ));
    let transport = Lines::new(
        line::LineSink::new(writer, Arc::clone(&shared)),
        line::incoming(reader, Arc::clone(&shared)),
    );
    let on_close = Arc::clone(&shared);
    let looping = Arc::clone(&shared);
    let connection = Client
        .builder()
        .name("clauseo")
        .with_handler(inbound::Inbound::new(Arc::clone(&shared)))
        .on_close(async move |_| {
            // The agent's stdout ended before every client was dropped.
            on_close.stop(Error::Closed);
            Ok(())
        })
        .connect_with(transport, async move |cx| {
            driver::run(cx, command_receiver, stop_receiver, looping).await
        });
    let driver = Driver(Box::pin(async move {
        match connection.await {
            Ok(()) => Ok(()),
            Err(_) => Err(shared.fatal().unwrap_or(Error::Closed)),
        }
    }));
    (AcpClient::new(commands, frames_dropped), receiver, driver)
}
