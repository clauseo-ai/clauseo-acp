//! The relay's lifecycle lines, one set per session (host.log keeps them): the relay opened, the
//! first `initialize` answered, the first `tools/list` answered and the first `tools/call` with
//! its outcome class, each with the milliseconds since the relay opened on the host clock; and,
//! as they happen, the server refusing the upstream `initialize` and the server's tool list
//! differing from the copy served. Fixed fields only: never the token, a query, arguments or a
//! reply's text.

use std::time::Duration;

use clauseo_connectors::{AuthError, Error};

use super::{Session, lock};
use crate::log_file::LIFECYCLE;

/// When the relay opened, and which of its first lines are written.
#[derive(Debug)]
pub(crate) struct Timeline {
    opened: Duration,
    marks: std::sync::Mutex<Marks>,
}

#[derive(Debug, Default)]
struct Marks {
    initialize: bool,
    tools_list: bool,
    tools_call: bool,
}

/// A first answer the relay gives on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    Initialize,
    ToolsList,
}

/// How a `tools/call` ended, as the lifecycle line names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallClass {
    Ok,
    OutOfCredit,
    CredentialEnded,
    Unreachable,
    Error,
}

impl CallClass {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            CallClass::Ok => "ok",
            CallClass::OutOfCredit => "out_of_credit",
            CallClass::CredentialEnded => "credential_ended",
            CallClass::Unreachable => "unreachable",
            CallClass::Error => "error",
        }
    }

    /// The class of a call that failed before or at the connector.
    pub(crate) fn of_error(err: &Error) -> CallClass {
        match err {
            Error::Auth(AuthError::NotAdded | AuthError::SignedOut | AuthError::KeyRevoked) => {
                CallClass::CredentialEnded
            }
            Error::Auth(AuthError::Network(_)) | Error::Network(_) => CallClass::Unreachable,
            Error::Auth(AuthError::Store(_))
            | Error::Http { .. }
            | Error::Parse(_)
            | Error::InsufficientCredits(_)
            | Error::NotOurPayLink
            | Error::BadKeyFormat
            | Error::RechargeTimedOut
            | Error::Cancelled
            | Error::Store(_)
            | Error::BadOrigin => CallClass::Error,
        }
    }
}

impl Timeline {
    pub(crate) fn new(opened: Duration) -> Timeline {
        Timeline {
            opened,
            marks: std::sync::Mutex::new(Marks::default()),
        }
    }
}

/// Milliseconds since `session`'s relay opened, on the host clock.
fn since_open_ms(session: &Session) -> u64 {
    let elapsed = session
        .hub
        .clock
        .now()
        .mono
        .saturating_sub(session.timeline.opened);
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

/// Logs the relay's opening.
pub(crate) fn opened(session: &Session) {
    tracing::info!(target: LIFECYCLE, session = %session.key.id(), "case-law relay opened");
}

/// Logs `mark` the first time it happens in `session`.
pub(crate) fn first(session: &Session, mark: Mark) {
    let first = {
        let mut marks = lock(&session.timeline.marks);
        let seen = match mark {
            Mark::Initialize => &mut marks.initialize,
            Mark::ToolsList => &mut marks.tools_list,
        };
        !std::mem::replace(seen, true)
    };
    if !first {
        return;
    }
    let ms = since_open_ms(session);
    let session = session.key.id();
    match mark {
        Mark::Initialize => {
            tracing::info!(target: LIFECYCLE, %session, ms, "case-law relay: initialize answered");
        }
        Mark::ToolsList => {
            tracing::info!(target: LIFECYCLE, %session, ms, "case-law relay: tools/list answered");
        }
    }
}

/// Logs `session`'s first `tools/call` as it ends, with its class.
pub(crate) fn first_call(session: &Session, class: CallClass) {
    if std::mem::replace(&mut lock(&session.timeline.marks).tools_call, true) {
        return;
    }
    tracing::info!(
        target: LIFECYCLE,
        session = %session.key.id(),
        ms = since_open_ms(session),
        outcome = class.as_str(),
        "case-law relay: first tools/call"
    );
}

/// Logs that the server answered the upstream `initialize` with an error.
pub(crate) fn server_refused(session: &Session) {
    tracing::info!(
        target: LIFECYCLE,
        session = %session.key.id(),
        ms = since_open_ms(session),
        class = "jsonrpc_error",
        "case-law relay: the server refused initialize"
    );
}

/// Logs that the server's tool list differed from the copy this connection had; `told` says
/// whether the agent had listed it and so was sent `notifications/tools/list_changed`.
pub(crate) fn list_differed(session: &Session, told: bool) {
    tracing::info!(
        target: LIFECYCLE,
        session = %session.key.id(),
        ms = since_open_ms(session),
        told,
        "case-law relay: the server's tool list differed from the copy served"
    );
}
