//! The upstream `initialize`, owed to the server once the relay answered the agent's own
//! (`local.rs`). It is sent at once on a thread of its own, and every forwarded message of the
//! connection waits for it ([`ensure`]), so the server has its `initialize`, then
//! `notifications/initialized`, before any call, and the call carries the server's
//! `MCP-Protocol-Version` and `Mcp-Session-Id`. A connection whose agent sent no `initialize`
//! forwards as before.
//!
//! One attempt at a time, shared: every message that waits on an attempt gets its outcome, and
//! one that waited on a failed attempt gets its failure and does not try again; the next message
//! to arrive after it starts a new one. A message waits at most the hub's warm-up deadline (20
//! s), then is answered as unreachable. No lock is held across the network.
//!
//! An attempt whose credential ended (a second 401, a refresh refused) sends `CredentialEnded` at
//! once. One the server answers with a JSON-RPC error is [`WarmFail::Refused`], logged on the
//! lifecycle target. Once the server has its `initialize`, the attempt's thread reads the
//! server's whole `tools/list` (every page), keeps both results as the copy the relay answers
//! from (`copies.rs`, which writes the cache file), and tells this connection's agent when the
//! list it was given is not the server's (`local::refresh`).

use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use clauseo_connectors::{Error, McpRequestHeaders};
use serde_json::{Value, json};

use super::super::super::texts;
use super::super::conn::Conn;
use super::super::copies::{self, MAX_CACHE_BYTES, Served};
use super::super::{lock, timeline};
use super::{credential_ended, describe, local, reply};

/// Why a message got no upstream `initialize`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WarmFail {
    /// It failed before or at the connector, or did not finish in time.
    Call(Error),
    /// The server answered it with a JSON-RPC error.
    Refused,
}

/// What the connection owes the server, and the waits on it.
#[derive(Debug, Default)]
pub(crate) struct Warm {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct State {
    stage: Stage,
    /// The last attempt started.
    attempt: u64,
}

#[derive(Debug, Default)]
enum Stage {
    /// No `initialize` answered locally: messages forward as they come.
    #[default]
    Cold,
    /// The agent's `initialize`, not yet sent.
    Owed(Owed),
    /// Attempt `attempt` is under way.
    Running { owed: Owed, attempt: u64 },
    /// Attempt `attempt` failed; the next message to arrive tries again.
    Failed {
        owed: Owed,
        attempt: u64,
        why: WarmFail,
    },
    /// The server has its `initialize`.
    Done,
}

/// The agent's `initialize`, vetted and with the negotiated version.
#[derive(Debug, Clone)]
struct Owed {
    id: Value,
    body: Arc<[u8]>,
    version: &'static str,
}

/// The id of the relay's own `tools/list` requests.
const LIST_ID: &str = "clauseo-relay-tools-list";
/// The most `tools/list` pages read; a server with more is not cached.
const MAX_PAGES: usize = 16;
/// The notification the warm-up sends after the server's `initialize` answer.
const INITIALIZED: &[u8] = br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

impl Warm {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        lock(&self.state)
    }
}

/// Records the agent's `initialize` (its `id`, the `body` to send, the `version` it was told)
/// and starts the attempt; a later `initialize` changes nothing.
pub(super) fn arm(conn: &Arc<Conn>, id: Value, body: Vec<u8>, version: &'static str) {
    let mut state = conn.warm.state();
    if !matches!(state.stage, Stage::Cold) {
        return;
    }
    state.stage = Stage::Owed(Owed {
        id,
        body: Arc::from(body),
        version,
    });
    launch(conn, &mut state);
}

/// Whether the connection's `initialize` was answered locally.
pub(super) fn armed(conn: &Conn) -> bool {
    !matches!(conn.warm.state().stage, Stage::Cold)
}

/// Waits until the server has its `initialize`: `Ok` at once when nothing is owed. Starts an
/// attempt when none is under way, then waits on it up to the deadline.
pub(super) fn ensure(conn: &Arc<Conn>) -> Result<(), WarmFail> {
    let mut state = conn.warm.state();
    let attempt = match &state.stage {
        Stage::Cold | Stage::Done => return Ok(()),
        Stage::Running { attempt, .. } => *attempt,
        Stage::Owed(_) | Stage::Failed { .. } => launch(conn, &mut state),
    };
    conn.session.warm_waits.fetch_add(1, Ordering::SeqCst);
    let deadline = conn.session.hub.warm_deadline();
    let running = |state: &mut State| matches!(state.stage, Stage::Running { attempt: theirs, .. } if theirs == attempt);
    let (mut state, _timeout) = conn
        .warm
        .changed
        .wait_timeout_while(state, deadline, running)
        .unwrap_or_else(PoisonError::into_inner);
    let settled = settle(&mut state, attempt);
    conn.warm.changed.notify_all();
    settled
}

/// What a message that waited on attempt `attempt` gets, once its wait ended. Its own attempt
/// still running is past the deadline: it fails, for every message waiting on it. Any other
/// stage it finds (a newer attempt, started after its own ended, included) it leaves as it is.
fn settle(state: &mut State, attempt: u64) -> Result<(), WarmFail> {
    match &state.stage {
        Stage::Cold | Stage::Done => Ok(()),
        Stage::Failed {
            attempt: theirs,
            why,
            ..
        } if *theirs == attempt => Err(why.clone()),
        Stage::Running {
            owed,
            attempt: theirs,
        } if *theirs == attempt => {
            let why = WarmFail::Call(Error::Network("initialize timed out".to_owned()));
            state.stage = Stage::Failed {
                owed: owed.clone(),
                attempt,
                why: why.clone(),
            };
            Err(why)
        }
        Stage::Owed(_) | Stage::Running { .. } | Stage::Failed { .. } => Err(WarmFail::Call(
            Error::Network("initialize did not finish".to_owned()),
        )),
    }
}

/// Starts a new attempt for the owed `initialize`, on its own thread; returns its number.
fn launch(conn: &Arc<Conn>, state: &mut State) -> u64 {
    let owed = match std::mem::take(&mut state.stage) {
        Stage::Owed(owed) | Stage::Running { owed, .. } | Stage::Failed { owed, .. } => owed,
        other @ (Stage::Cold | Stage::Done) => {
            state.stage = other;
            return state.attempt;
        }
    };
    state.attempt = state.attempt.wrapping_add(1);
    let attempt = state.attempt;
    state.stage = Stage::Running {
        owed: owed.clone(),
        attempt,
    };
    let runner = Arc::clone(conn);
    let sent = owed.clone();
    let spawned = std::thread::Builder::new()
        .name("clauseo-relay-warm".to_owned())
        .spawn(move || {
            let dispatch = runner.session.dispatch.clone();
            tracing::dispatcher::with_default(&dispatch, || run(&runner, &sent, attempt));
        });
    if spawned.is_err() {
        tracing::warn!("case-law relay: no thread for the warm-up");
        state.stage = Stage::Failed {
            owed,
            attempt,
            why: WarmFail::Call(Error::Network("no thread".to_owned())),
        };
    }
    attempt
}

/// One attempt: the upstream `initialize`, its outcome for the waiting messages, then the
/// server's tool list.
fn run(conn: &Arc<Conn>, owed: &Owed, attempt: u64) {
    let session = &conn.session;
    let outcome = initialize(conn, owed, attempt);
    match &outcome {
        Ok(_) => {}
        Err(WarmFail::Refused) => timeline::server_refused(session),
        Err(WarmFail::Call(err)) => {
            if !credential_ended(session, err) {
                tracing::info!(session = %session.key.id(), error = %describe(err), "case-law relay: the server's initialize did not go through");
            }
        }
    }
    let result = {
        let mut state = conn.warm.state();
        let result = finish(&mut state, attempt, outcome);
        conn.warm.changed.notify_all();
        result
    };
    if let Some(result) = result {
        refresh(conn, owed.version, result);
    }
}

/// Records how attempt `attempt` ended. Success marks the `initialize` done when the stage is
/// still the attempt's (running, or failed past its deadline); a failure marks the attempt's own
/// running stage failed. A newer attempt's stage is left as it is. Returns the server's
/// `initialize` result to keep, if any.
fn finish(
    state: &mut State,
    attempt: u64,
    outcome: Result<Option<Value>, WarmFail>,
) -> Option<Value> {
    let ours = match &state.stage {
        Stage::Running {
            attempt: theirs, ..
        }
        | Stage::Failed {
            attempt: theirs, ..
        } => *theirs == attempt,
        Stage::Cold | Stage::Owed(_) | Stage::Done => false,
    };
    match outcome {
        Ok(result) => {
            if ours {
                state.stage = Stage::Done;
            }
            result
        }
        Err(why) => {
            if let Stage::Running {
                owed,
                attempt: theirs,
            } = &state.stage
                && *theirs == attempt
            {
                state.stage = Stage::Failed {
                    owed: owed.clone(),
                    attempt,
                    why,
                };
            }
            None
        }
    }
}

/// Sends the owed `initialize`, keeps the server's version and session id, then sends
/// `notifications/initialized`. The `Ok` is the server's `initialize` result to keep (`Null`
/// when it is over [`MAX_CACHE_BYTES`]), or `None` when the server chose another version than
/// the agent was told: then its answers are not that version's, and nothing is kept.
///
/// The server's session id and version are kept only while `attempt` is the connection's latest:
/// an older attempt that answers late leaves a newer one's alone, and keeps nothing.
fn initialize(conn: &Conn, owed: &Owed, attempt: u64) -> Result<Option<Value>, WarmFail> {
    let session = &conn.session;
    let hub = &session.hub;
    let reply = hub
        .connector
        .mcp_post(&owed.body, &McpRequestHeaders::default())
        .map_err(WarmFail::Call)?;
    // Checked under the warm-up's lock, held while the session id and version are written (the
    // upstream lock is only ever taken after it).
    let latest = conn.warm.state();
    if !current(&latest, attempt) {
        return Ok(None);
    }
    reply::keep_session_id(conn, reply.session_id.as_deref());
    if reply.status >= 400 {
        return Err(WarmFail::Call(Error::Http {
            status: reply.status,
        }));
    }
    let raws = reply::values(reply.body);
    let Some((_, found)) = reply::answer_to(session, &owed.id, &raws) else {
        return Err(WarmFail::Call(Error::Network(
            "no initialize answer".to_owned(),
        )));
    };
    let Some(result) = found.result else {
        return Err(WarmFail::Refused);
    };
    let theirs = reply::keep_protocol_version(conn, result);
    drop(latest);
    let kept = if theirs.as_deref() != Some(owed.version) {
        tracing::warn!(session = %session.key.id(), "case-law relay: the server chose another protocol version than the agent was told");
        None
    } else if result.get().len() <= MAX_CACHE_BYTES {
        Some(serde_json::from_str(result.get()).unwrap_or(Value::Null))
    } else {
        Some(Value::Null)
    };
    if let Err(err) = hub.connector.mcp_post(INITIALIZED, &headers(conn)) {
        tracing::info!(session = %session.key.id(), error = %describe(&err), "case-law relay: notifications/initialized did not go through");
    }
    Ok(kept)
}

/// Whether attempt `attempt` is the connection's latest.
fn current(state: &State, attempt: u64) -> bool {
    state.attempt == attempt
}

/// The headers the server's `initialize` set.
fn headers(conn: &Conn) -> McpRequestHeaders {
    let upstream = lock(&conn.upstream);
    McpRequestHeaders {
        protocol_version: upstream.protocol_version.clone(),
        session_id: upstream.session_id.clone(),
    }
}

/// Reads the server's whole tool list for `version` and, when it and `initialize` are usable,
/// keeps both as that version's copy (the cache file too) and gives the list to this connection.
fn refresh(conn: &Conn, version: &'static str, initialize: Value) {
    let session = &conn.session;
    let hub = &session.hub;
    let Some(tools_list) = tools_list(conn, version) else {
        return;
    };
    let initialize = if initialize.is_object() {
        Arc::new(initialize)
    } else {
        hub.copies.current(version).initialize
    };
    if !copies::usable(&initialize, &tools_list) {
        tracing::warn!(session = %session.key.id(), "case-law relay: the server's tool list is not usable; the copy is kept");
        return;
    }
    let tools_list = Arc::new(tools_list);
    let served = Served {
        initialize,
        tools_list: Arc::clone(&tools_list),
    };
    hub.copies
        .store(version, served, hub.clock.now().wall.as_millis());
    local::refresh(conn, tools_list);
}

/// The server's tools for `version`, every page, as one `tools/list` result; `None` on any
/// failure. Each request carries `MCP-Protocol-Version: <version>`: the server shapes the list by
/// that header.
fn tools_list(conn: &Conn, version: &'static str) -> Option<Value> {
    let session = &conn.session;
    let headers = McpRequestHeaders {
        protocol_version: Some(version.to_owned()),
        session_id: lock(&conn.upstream).session_id.clone(),
    };
    let id = json!(LIST_ID);
    let mut tools = Vec::new();
    let mut size = 0usize;
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let params = cursor
            .take()
            .map_or_else(|| json!({}), |next| json!({"cursor": next}));
        let request =
            json!({"jsonrpc": "2.0", "id": LIST_ID, "method": "tools/list", "params": params});
        let reply = match session
            .hub
            .connector
            .mcp_post(&texts::line(&request), &headers)
        {
            Ok(reply) if reply.status < 400 => reply,
            Ok(reply) => {
                tracing::info!(session = %session.key.id(), status = reply.status, "case-law relay: the server's tools/list failed");
                return None;
            }
            Err(err) => {
                tracing::info!(session = %session.key.id(), error = %describe(&err), "case-law relay: the server's tools/list failed");
                return None;
            }
        };
        let raws = reply::values(reply.body);
        let result = reply::answer_to(session, &id, &raws).and_then(|(_, found)| found.result)?;
        size = size.saturating_add(result.get().len());
        if size > MAX_CACHE_BYTES {
            tracing::warn!(session = %session.key.id(), "case-law relay: the server's tool list is too large to keep");
            return None;
        }
        let Ok(Value::Object(mut page)) = serde_json::from_str::<Value>(result.get()) else {
            return None;
        };
        let Some(Value::Array(more)) = page.remove("tools") else {
            return None;
        };
        tools.extend(more);
        match page.remove("nextCursor") {
            Some(Value::String(next)) if !next.is_empty() => cursor = Some(next),
            _ => return Some(json!({ "tools": tools })),
        }
    }
    tracing::warn!(session = %session.key.id(), "case-law relay: the server's tool list has too many pages to keep");
    None
}

#[cfg(test)]
mod tests;
