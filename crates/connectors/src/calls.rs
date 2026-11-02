//! The calls with the credential: `mcp_post`, the account, and the recharge and its poll. Each
//! retries once after a 401: with the newer token, or after a refresh; a second 401 ends the
//! credential.

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use crate::account::{Account, parse_account};
use crate::cancel::CancelFlag;
use crate::connectors::{ConnectorHandle, lock};
use crate::error::{AuthError, Error, parse_error};
use crate::http::{self, POLL_TIMEOUT, Reply, SendBody};
use crate::mcp::{McpBody, McpReply, McpRequestHeaders, parse_event_stream};
use crate::recharge::{Recharge, RechargeStatus, insufficient_credits_402, parse_recharge};
use crate::secret::Secret;

/// The account is read at most once a minute; a copy younger than this is served from memory.
pub const ACCOUNT_MIN_INTERVAL: Duration = Duration::from_mins(1);
/// How long `wait_recharge` waits after a network error before polling again.
const POLL_BACKOFF: Duration = Duration::from_secs(5);
/// The least time between the starts of two recharge polls, so a connector (or a proxy) that
/// answers `awaiting_payment` at once is not polled in a loop.
const POLL_MIN_INTERVAL: Duration = Duration::from_secs(2);
/// How often a waiting `wait_recharge` looks at the cancel flag and the deadline.
const POLL_TICK: Duration = Duration::from_millis(20);

/// POSTs one MCP message to the connector. Sends `Authorization` only with a credential
/// (`initialize` and `tools/list` need none), `MCP-Protocol-Version` and `Mcp-Session-Id`
/// only when given. A JSON answer is `Json`, an event stream `Events` (one value per `data:`
/// event), a 202 or empty body `Empty`.
pub(crate) fn mcp_post(
    handle: &ConnectorHandle,
    body: &[u8],
    headers: &McpRequestHeaders,
) -> Result<McpReply, Error> {
    let mut extra = vec![("Accept", "application/json, text/event-stream")];
    if let Some(version) = &headers.protocol_version {
        extra.push(("MCP-Protocol-Version", version.as_str()));
    }
    if let Some(session) = &headers.session_id {
        extra.push(("Mcp-Session-Id", session.as_str()));
    }
    let url = handle.inner.urls.mcp_url.clone();
    let timeout = handle.inner.shared.timeout();
    let reply = authed(
        handle,
        "POST",
        &url,
        &extra,
        SendBody::Json(body),
        timeout,
        true,
    )?;
    mcp_reply(&reply)
}

/// The account (`GET /v1/case-law/account?recent_days=30`). A copy younger than
/// [`ACCOUNT_MIN_INTERVAL`] is returned without a request. A 402 is
/// `Error::InsufficientCredits`.
pub(crate) fn account(handle: &ConnectorHandle) -> Result<Account, Error> {
    let mut cache = lock(&handle.inner.account);
    let now = handle.inner.shared.config.clock.now();
    let generation = handle.inner.generation.load(Ordering::SeqCst);
    if let Some((cached_generation, account)) = cache.as_ref()
        && *cached_generation == generation
        && now
            .duration_since(account.fetched_at)
            .is_ok_and(|age| age < ACCOUNT_MIN_INTERVAL)
    {
        return Ok(account.clone());
    }
    let url = handle.account_url();
    let timeout = handle.inner.shared.timeout();
    let headers = [("Accept", "application/json")];
    let reply = authed(
        handle,
        "GET",
        &url,
        &headers,
        SendBody::None,
        timeout,
        false,
    )?;
    let shared = &handle.inner.shared;
    let account = match reply.status {
        200 => parse_account(&reply.body, handle.inner.known, &shared.config.origin, now)?,
        402 => return Err(out_of_credit(handle, &reply)),
        status => return Err(Error::Http { status }),
    };
    *cache = Some((generation, account.clone()));
    Ok(account)
}

/// Opens a recharge for `pack` (`POST /v1/case-law/recharges {"pack": ...}`) and returns it
/// with its pay link, which must be ours (`Error::NotOurPayLink` otherwise).
pub(crate) fn create_recharge(handle: &ConnectorHandle, pack: &str) -> Result<Recharge, Error> {
    let body = serde_json::to_vec(&serde_json::json!({ "pack": pack }))
        .map_err(|err| parse_error("recharge request", &err))?;
    let url = format!("{}/recharges", handle.inner.urls.api_base);
    let timeout = handle.inner.shared.timeout();
    let headers = [("Accept", "application/json")];
    let reply = authed(
        handle,
        "POST",
        &url,
        &headers,
        SendBody::Json(&body),
        timeout,
        false,
    )?;
    match reply.status {
        200 | 201 => recharge_from(handle, &reply),
        402 => Err(out_of_credit(handle, &reply)),
        status => Err(Error::Http { status }),
    }
}

/// Polls `GET /v1/case-law/recharges/{id}?wait=40` until the recharge is no longer awaiting
/// payment, the recharge deadline passes (`RechargeTimedOut`) or `cancel` is set
/// (`Cancelled`, within about 20 ms, even mid-poll). Polls start at least 2 s apart. A network
/// error or a 5xx waits 5 s and polls again. An answer for another recharge is `Parse`.
pub(crate) fn wait_recharge(
    handle: &ConnectorHandle,
    recharge_id: &str,
    cancel: &CancelFlag,
) -> Result<Recharge, Error> {
    if recharge_id.is_empty()
        || recharge_id.len() > 128
        || !recharge_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(Error::Parse("recharge: not a recharge id".to_owned()));
    }
    let config = &handle.inner.shared.config;
    let deadline = config
        .clock
        .now()
        .checked_add(config.recharge_deadline)
        .ok_or_else(|| Error::Parse("recharge: the deadline is out of range".to_owned()))?;
    let url = format!(
        "{}/recharges/{recharge_id}?wait=40",
        handle.inner.urls.api_base
    );
    loop {
        let remaining = time_left(handle, deadline, cancel)?;
        let started = config.clock.now();
        let failure = match poll_once(handle, &url, remaining.min(POLL_TIMEOUT), cancel, deadline)?
        {
            Ok(reply) if reply.status == 200 => {
                let recharge = recharge_from(handle, &reply)?;
                if recharge.recharge_id != recharge_id {
                    return Err(Error::Parse(
                        "recharge: the answer is for another recharge".to_owned(),
                    ));
                }
                if recharge.status != RechargeStatus::AwaitingPayment {
                    return Ok(recharge);
                }
                let took = config
                    .clock
                    .now()
                    .duration_since(started)
                    .unwrap_or_default();
                let gap = POLL_MIN_INTERVAL.saturating_sub(took);
                if !gap.is_zero() {
                    let remaining = time_left(handle, deadline, cancel)?;
                    if cancel.wait_timeout(remaining.min(gap)) {
                        return Err(Error::Cancelled);
                    }
                }
                continue;
            }
            Ok(reply) if reply.status >= 500 => format!("HTTP {}", reply.status),
            Ok(reply) => {
                return Err(Error::Http {
                    status: reply.status,
                });
            }
            Err(Error::Network(err)) => err,
            Err(err) => return Err(err),
        };
        tracing::info!(connector = handle.id(), error = %failure, "the recharge poll failed; retrying");
        let remaining = time_left(handle, deadline, cancel)?;
        if cancel.wait_timeout(remaining.min(POLL_BACKOFF)) {
            return Err(Error::Cancelled);
        }
    }
}

/// What is left before `deadline`, or why the poll stops.
fn time_left(
    handle: &ConnectorHandle,
    deadline: SystemTime,
    cancel: &CancelFlag,
) -> Result<Duration, Error> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    match deadline.duration_since(handle.inner.shared.config.clock.now()) {
        Ok(left) if !left.is_zero() => Ok(left),
        _ => Err(Error::RechargeTimedOut),
    }
}

/// One poll on its own thread, so the cancel flag and the deadline are watched while it
/// waits. The outer result stops the poll; the inner one is the poll's own outcome.
fn poll_once(
    handle: &ConnectorHandle,
    url: &str,
    timeout: Duration,
    cancel: &CancelFlag,
    deadline: SystemTime,
) -> Result<Result<Reply, Error>, Error> {
    let (sender, receiver) = mpsc::channel();
    let owned = handle.clone();
    let url = url.to_owned();
    std::thread::Builder::new()
        .name("connector-recharge-poll".to_owned())
        .spawn(move || {
            let headers = [("Accept", "application/json")];
            let reply = authed(
                &owned,
                "GET",
                &url,
                &headers,
                SendBody::None,
                timeout,
                false,
            );
            // The receiver is gone when the wait was cancelled; the answer is dropped.
            let _sent = sender.send(reply);
        })
        .map_err(|err| Error::Network(format!("no poll thread ({:?})", err.kind())))?;
    loop {
        match receiver.recv_timeout(POLL_TICK) {
            Ok(reply) => return Ok(reply),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                time_left(handle, deadline, cancel)?;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Ok(Err(Error::Network("the poll thread ended".to_owned())));
            }
        }
    }
}

fn recharge_from(handle: &ConnectorHandle, reply: &Reply) -> Result<Recharge, Error> {
    let shared = &handle.inner.shared;
    parse_recharge(&reply.body, handle.inner.known, &shared.config.origin)
}

fn out_of_credit(handle: &ConnectorHandle, reply: &Reply) -> Error {
    let shared = &handle.inner.shared;
    insufficient_credits_402(&reply.body, handle.inner.known, &shared.config.origin)
}

/// One request with the bearer token, and the 401 retry. With `anonymous_ok`, a connector
/// with no credential is called without `Authorization`, and its 401 is handed back.
pub(crate) fn authed(
    handle: &ConnectorHandle,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: SendBody<'_>,
    timeout: Duration,
    anonymous_ok: bool,
) -> Result<Reply, Error> {
    let token = {
        let mut slot = lock(&handle.inner.slot);
        match crate::refresh::fresh_token(handle, &mut slot) {
            Ok(token) => Some(token),
            Err(AuthError::NotAdded | AuthError::SignedOut | AuthError::KeyRevoked)
                if anonymous_ok =>
            {
                None
            }
            Err(err) => return Err(Error::Auth(err)),
        }
    };
    let reply = send_with(handle, method, url, headers, body, timeout, token.as_ref())?;
    let Some(used) = token.filter(|_| reply.status == 401) else {
        return Ok(reply);
    };
    let next = crate::refresh::after_401(handle, &used)?;
    let reply = send_with(handle, method, url, headers, body, timeout, Some(&next))?;
    if reply.status == 401 {
        return Err(Error::Auth(crate::refresh::after_second_401(handle, &next)));
    }
    Ok(reply)
}

fn send_with(
    handle: &ConnectorHandle,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: SendBody<'_>,
    timeout: Duration,
    token: Option<&Secret>,
) -> Result<Reply, Error> {
    let auth = token.map(ConnectorHandle::bearer_header);
    let mut all: Vec<(&str, &str)> = Vec::with_capacity(headers.len().saturating_add(1));
    if let Some(auth) = &auth {
        all.push(("Authorization", auth.as_str()));
    }
    all.extend_from_slice(headers);
    http::send(&handle.inner.shared.agent, method, url, &all, body, timeout).map_err(Error::Network)
}

/// An MCP answer as [`McpReply`]. A body that is neither JSON nor an event stream is a parse
/// error on a 2xx and `Http` otherwise.
fn mcp_reply(reply: &Reply) -> Result<McpReply, Error> {
    let body = if reply.status == 202 || reply.body.is_empty() {
        McpBody::Empty
    } else if reply.content_type.as_deref() == Some("text/event-stream") {
        let text = std::str::from_utf8(&reply.body)
            .map_err(|_not_utf8| Error::Parse("event stream: not UTF-8".to_owned()))?;
        McpBody::Events(parse_event_stream(text)?)
    } else {
        match serde_json::from_slice(&reply.body) {
            Ok(value) => McpBody::Json(value),
            Err(err) if (200..300).contains(&reply.status) => {
                return Err(parse_error("MCP answer", &err));
            }
            Err(_) => {
                return Err(Error::Http {
                    status: reply.status,
                });
            }
        }
    };
    Ok(McpReply {
        status: reply.status,
        session_id: reply.session_id.clone(),
        body,
    })
}
