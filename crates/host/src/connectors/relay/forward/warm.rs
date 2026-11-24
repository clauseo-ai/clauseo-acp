//! The upstream `initialize`, owed to the server once the relay answered the agent's own
//! (`local.rs`). A warm-up thread sends it as soon as the agent's is answered, and every
//! forwarded message of the connection waits for it first ([`ensure`]), so the server has its
//! `initialize`, then `notifications/initialized`, before any call, and the call carries the
//! server's `MCP-Protocol-Version` and `Mcp-Session-Id`. When the warm-up fails (the server
//! unreachable, a 5xx), it is still owed: the next forwarded message sends it again, and a
//! `tools/call` that cannot get it is refunded and answered as unreachable.
//!
//! The lock is held across the upstream `initialize`: a call waiting on it waits for the same
//! request, not a second one. A connection whose agent sent no `initialize` forwards as before.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use clauseo_connectors::{Error, McpRequestHeaders};
use serde_json::Value;

use super::super::conn::Conn;
use super::super::lock;
use super::reply;

/// What the connection still owes the server. `armed` says, without the lock, whether an
/// `initialize` was answered locally, so the agent's later lines never wait on the warm-up.
#[derive(Debug, Default)]
pub(crate) struct Warm {
    stage: Mutex<Stage>,
    armed: AtomicBool,
}

#[derive(Debug, Default)]
enum Stage {
    /// No `initialize` answered locally: messages forward as they come.
    #[default]
    Cold,
    /// The agent's `initialize`, vetted and with the negotiated version, still to send.
    Owed {
        id: Value,
        body: Vec<u8>,
        version: &'static str,
    },
    /// The server has its `initialize`.
    Done,
}

/// The notification the warm-up sends after the server's `initialize` answer.
const INITIALIZED: &[u8] = br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

/// Records the agent's `initialize` (its `id`, the `body` to send, the `version` it was told);
/// `true` for the connection's first, which starts the warm-up. A later one changes nothing.
pub(super) fn arm(conn: &Conn, id: Value, body: Vec<u8>, version: &'static str) -> bool {
    if conn.warm.armed.swap(true, Ordering::SeqCst) {
        return false;
    }
    *lock(&conn.warm.stage) = Stage::Owed { id, body, version };
    true
}

/// Whether the connection's `initialize` was answered locally. Never waits.
pub(super) fn armed(conn: &Conn) -> bool {
    conn.warm.armed.load(Ordering::SeqCst)
}

/// Starts the warm-up thread; a thread that cannot start leaves the `initialize` owed to the
/// first forwarded message.
pub(super) fn start(conn: &Arc<Conn>) {
    let conn = Arc::clone(conn);
    let started = std::thread::Builder::new()
        .name("clauseo-relay-warm".to_owned())
        .spawn(move || {
            let dispatch = conn.session.dispatch.clone();
            tracing::dispatcher::with_default(&dispatch, || {
                if let Err(err) = ensure(&conn) {
                    tracing::info!(session = %conn.session.key.id(), error = %super::describe(&err), "case-law relay: the server's initialize did not go through; the first call tries again");
                }
            });
        });
    if started.is_err() {
        tracing::warn!("case-law relay: no thread for the warm-up");
    }
}

/// Sends the owed `initialize`, if any, and keeps the server's version and session id. `Ok` when
/// nothing is owed any more.
pub(super) fn ensure(conn: &Conn) -> Result<(), Error> {
    let mut stage = lock(&conn.warm.stage);
    let Stage::Owed { id, body, version } = &*stage else {
        return Ok(());
    };
    let hub = &conn.session.hub;
    let reply = hub
        .connector
        .mcp_post(body, &McpRequestHeaders::default())?;
    reply::keep_session_id(conn, reply.session_id.as_deref());
    if reply.status >= 400 {
        return Err(Error::Http {
            status: reply.status,
        });
    }
    let raws = reply::values(reply.body);
    let result = reply::answer_to(&conn.session, id, &raws).and_then(|(_, found)| found.result);
    let Some(result) = result else {
        return Err(Error::Network("no initialize answer".to_owned()));
    };
    let theirs = reply::keep_protocol_version(conn, result);
    if theirs.as_deref().is_some_and(|theirs| theirs != *version) {
        tracing::warn!(session = %conn.session.key.id(), "case-law relay: the server chose another protocol version than the agent was told");
    }
    *stage = Stage::Done;
    let headers = {
        let upstream = lock(&conn.upstream);
        McpRequestHeaders {
            protocol_version: upstream.protocol_version.clone(),
            session_id: upstream.session_id.clone(),
        }
    };
    if let Err(err) = hub.connector.mcp_post(INITIALIZED, &headers) {
        tracing::info!(session = %conn.session.key.id(), error = %super::describe(&err), "case-law relay: notifications/initialized did not go through");
    }
    Ok(())
}
