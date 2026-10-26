//! The state the driver's handlers and its command loop share, behind one mutex that is never
//! held across an `.await`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use agent_client_protocol::Responder;
use clauseo_protocol::{CompactString, PromptCapabilities, TurnId};

use crate::Error;
use crate::event::{AcpEvent, FrameDirection, Origin, PendingKey, RawFrame};
use crate::line::Gate;
use crate::types::AuthMethod;

/// Shared by the line reader, the inbound handler, the command loop and every `AcpClient`.
pub(crate) struct Shared {
    events: async_channel::Sender<AcpEvent>,
    pub(crate) gate: Gate,
    frame_tap: Option<async_channel::Sender<RawFrame>>,
    frames_dropped: Arc<AtomicU64>,
    /// Wakes the command loop to end the connection with [`Shared::fatal`].
    stop: async_channel::Sender<()>,
    state: Mutex<State>,
    /// Why the connection must end with an error, when something decided so.
    fatal: Mutex<Option<Error>>,
}

impl Shared {
    pub(crate) fn new(
        events: async_channel::Sender<AcpEvent>,
        frame_tap: Option<async_channel::Sender<RawFrame>>,
        frames_dropped: Arc<AtomicU64>,
        stop: async_channel::Sender<()>,
    ) -> Self {
        Self {
            events,
            gate: Gate::default(),
            frame_tap,
            frames_dropped,
            stop,
            state: Mutex::new(State::default()),
            fatal: Mutex::new(None),
        }
    }

    /// Sends an event; while the channel is full, the line reader stops. A dropped receiver
    /// drops the event.
    pub(crate) async fn emit(&self, event: AcpEvent) {
        match self.events.try_send(event) {
            Ok(()) | Err(async_channel::TrySendError::Closed(_)) => {}
            Err(async_channel::TrySendError::Full(event)) => {
                self.gate.close();
                // An error means the host dropped the receiver; the event goes nowhere.
                match self.events.send(event).await {
                    Ok(()) | Err(async_channel::SendError(_)) => {}
                }
                self.gate.open();
            }
        }
    }

    /// Copies a line to the frame tap, counting it when the tap is full or closed.
    pub(crate) fn tap(&self, direction: FrameDirection, line: &str) {
        if let Some(tap) = &self.frame_tap {
            let frame = RawFrame {
                direction,
                line: line.to_owned(),
            };
            if tap.try_send(frame).is_err() {
                self.frames_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records why the connection ends; the first reason wins.
    pub(crate) fn set_fatal(&self, error: Error) {
        let mut fatal = self.fatal.lock().unwrap_or_else(PoisonError::into_inner);
        if fatal.is_none() {
            *fatal = Some(error);
        }
    }

    /// Ends the connection with `error`, unless an earlier reason was recorded.
    pub(crate) fn stop(&self, error: Error) {
        self.set_fatal(error);
        // Full means a stop is already pending; closed means the loop has ended.
        match self.stop.try_send(()) {
            Ok(()) | Err(_) => {}
        }
    }

    pub(crate) fn fatal(&self) -> Option<Error> {
        self.fatal
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Sessions, pending agent requests and what `initialize` said.
#[derive(Default)]
pub(crate) struct State {
    pub(crate) sessions: BTreeMap<CompactString, SessionState>,
    pub(crate) permissions: BTreeMap<PendingKey, PendingPermission>,
    pub(crate) elicitations: BTreeMap<PendingKey, PendingElicitation>,
    next_key: u64,
    /// From `initialize`: what the prompt may carry.
    pub(crate) prompt_capabilities: PromptCapabilities,
    /// From `initialize`: the auth methods, for `Error::AuthRequired` and `authenticate`.
    pub(crate) auth_methods: Vec<AuthMethod>,
}

impl State {
    pub(crate) fn next_key(&mut self) -> PendingKey {
        self.next_key += 1;
        PendingKey(self.next_key)
    }
}

/// One session started or loaded on this connection.
pub(crate) struct SessionState {
    pub(crate) cwd: PathBuf,
    pub(crate) origin: Origin,
    /// The turn whose `session/prompt` waits for its response.
    pub(crate) prompt: Option<TurnId>,
}

impl SessionState {
    pub(crate) fn new(cwd: PathBuf, origin: Origin) -> Self {
        Self {
            cwd,
            origin,
            prompt: None,
        }
    }
}

/// A `session/request_permission` waiting for the host's answer.
pub(crate) struct PendingPermission {
    pub(crate) session: CompactString,
    /// The offered option ids.
    pub(crate) options: Vec<CompactString>,
    pub(crate) responder: Responder<serde_json::Value>,
}

/// An `elicitation/create` waiting for the host's answer.
pub(crate) struct PendingElicitation {
    /// A URL elicitation's accept carries no `content`.
    pub(crate) url: bool,
    pub(crate) responder: Responder<serde_json::Value>,
}
