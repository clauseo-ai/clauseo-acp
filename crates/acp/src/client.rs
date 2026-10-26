//! [`AcpClient`]: the host's handle. Each call is a command to the driver's loop, answered on a
//! oneshot; dropping a call's future is allowed, and its answer is then discarded.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use clauseo_protocol::{
    AgentChoiceId, AgentSessionId, ConfigOption, ConfigValue, ContentBlock, ElicitationResponse,
    TurnId,
};
use futures::channel::oneshot;

use crate::Error;
use crate::event::PendingKey;
use crate::types::{AgentInfo, LoadSession, NewSession, PromptEnd, SessionStart, SteerResult};

/// Where a command's answer goes.
pub(crate) type Reply<T> = oneshot::Sender<Result<T, Error>>;

/// One call, for the driver's loop.
pub(crate) enum Command {
    Initialize(Reply<AgentInfo>),
    Authenticate {
        method_id: String,
        reply: Reply<()>,
    },
    NewSession {
        request: NewSession,
        reply: Reply<SessionStart>,
    },
    LoadSession {
        request: LoadSession,
        reply: Reply<SessionStart>,
    },
    Prompt {
        session: AgentSessionId,
        turn: TurnId,
        content: Vec<ContentBlock>,
        reply: Reply<PromptEnd>,
    },
    /// A `prompt()` future was dropped before its answer.
    PromptDropped {
        session: AgentSessionId,
        turn: TurnId,
    },
    Cancel {
        session: AgentSessionId,
        reply: Reply<Vec<PendingKey>>,
    },
    AnswerPermission {
        key: PendingKey,
        option_id: AgentChoiceId,
        reply: Reply<()>,
    },
    AnswerElicitation {
        key: PendingKey,
        response: ElicitationResponse,
        reply: Reply<()>,
    },
    SetConfigOption {
        session: AgentSessionId,
        option_id: AgentChoiceId,
        value: ConfigValue,
        reply: Reply<Vec<ConfigOption>>,
    },
    SetMode {
        session: AgentSessionId,
        mode_id: AgentChoiceId,
        reply: Reply<()>,
    },
}

/// The host's handle on one agent connection. Cheap to clone; when every clone is dropped the
/// driver closes the agent's stdin and ends.
///
/// The host must keep draining the event receiver while it awaits any call: when the event
/// channel is full the driver stops reading the agent, and a response then waits too.
#[derive(Debug, Clone)]
pub struct AcpClient {
    commands: async_channel::Sender<Command>,
    frames_dropped: Arc<AtomicU64>,
}

impl AcpClient {
    pub(crate) fn new(
        commands: async_channel::Sender<Command>,
        frames_dropped: Arc<AtomicU64>,
    ) -> Self {
        Self {
            commands,
            frames_dropped,
        }
    }

    async fn call<T>(&self, command: impl FnOnce(Reply<T>) -> Command) -> Result<T, Error> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(command(reply))
            .await
            .map_err(|async_channel::SendError(_)| Error::Closed)?;
        answer.await.unwrap_or(Err(Error::Closed))
    }

    /// `initialize`: Clauseo's capabilities and protocol version 1. Any other version the agent
    /// answers closes the connection with [`Error::UnsupportedVersion`].
    pub async fn initialize(&self) -> Result<AgentInfo, Error> {
        self.call(Command::Initialize).await
    }

    /// `authenticate` with a method the agent runs itself; a terminal method gives
    /// [`Error::TerminalAuthMethod`] and sends nothing.
    pub async fn authenticate(&self, method_id: &str) -> Result<(), Error> {
        let method_id = method_id.to_owned();
        self.call(|reply| Command::Authenticate { method_id, reply })
            .await
    }

    /// `session/new`.
    pub async fn new_session(&self, request: NewSession) -> Result<SessionStart, Error> {
        self.call(|reply| Command::NewSession { request, reply })
            .await
    }

    /// `session/load`. The updates the agent replays before its response come with
    /// [`Origin::Replay`](crate::Origin::Replay).
    pub async fn load_session(&self, request: LoadSession) -> Result<SessionStart, Error> {
        self.call(|reply| Command::LoadSession { request, reply })
            .await
    }

    /// `session/prompt` for `turn`; its updates come with [`Origin::Turn`](crate::Origin::Turn).
    /// A prompt already in flight on the session gives [`Error::TurnRunning`] and sends nothing;
    /// so does one whose content the agent does not accept, with [`Error::UnsupportedContent`].
    ///
    /// Dropping the future after [`cancel`](Self::cancel) frees the session for the next
    /// prompt, for an agent that never answers a cancelled prompt.
    pub async fn prompt(
        &self,
        session: &AgentSessionId,
        turn: TurnId,
        content: Vec<ContentBlock>,
    ) -> Result<PromptEnd, Error> {
        let mut guard = PromptGuard {
            commands: &self.commands,
            session: Some(session.clone()),
            turn,
        };
        let session = session.clone();
        let answer = self
            .call(|reply| Command::Prompt {
                session,
                turn,
                content,
                reply,
            })
            .await;
        guard.session = None;
        answer
    }

    /// Steer a running turn. Not built yet: always [`Error::SteerUnsupported`], sending nothing.
    pub async fn steer(
        &self,
        session: &AgentSessionId,
        content: Vec<ContentBlock>,
    ) -> Result<SteerResult, Error> {
        drop((session, content));
        // Async for the signature steer keeps once it is built.
        futures::future::ready(Err(Error::SteerUnsupported)).await
    }

    /// `session/cancel`. Answers every pending permission request of the session as
    /// `cancelled` and returns their keys; later updates come with
    /// [`Origin::Late`](crate::Origin::Late) until the next prompt starts.
    pub async fn cancel(&self, session: &AgentSessionId) -> Result<Vec<PendingKey>, Error> {
        let session = session.clone();
        self.call(|reply| Command::Cancel { session, reply }).await
    }

    /// Answers a permission request with an option the agent offered.
    pub async fn answer_permission(
        &self,
        key: PendingKey,
        option_id: AgentChoiceId,
    ) -> Result<(), Error> {
        self.call(|reply| Command::AnswerPermission {
            key,
            option_id,
            reply,
        })
        .await
    }

    /// Answers an elicitation.
    pub async fn answer_elicitation(
        &self,
        key: PendingKey,
        response: ElicitationResponse,
    ) -> Result<(), Error> {
        self.call(|reply| Command::AnswerElicitation {
            key,
            response,
            reply,
        })
        .await
    }

    /// `session/set_config_option`; returns every option as the agent now has them.
    pub async fn set_config_option(
        &self,
        session: &AgentSessionId,
        option_id: AgentChoiceId,
        value: ConfigValue,
    ) -> Result<Vec<ConfigOption>, Error> {
        let session = session.clone();
        self.call(|reply| Command::SetConfigOption {
            session,
            option_id,
            value,
            reply,
        })
        .await
    }

    /// `session/set_mode`.
    pub async fn set_mode(
        &self,
        session: &AgentSessionId,
        mode_id: AgentChoiceId,
    ) -> Result<(), Error> {
        let session = session.clone();
        self.call(|reply| Command::SetMode {
            session,
            mode_id,
            reply,
        })
        .await
    }

    /// How many frames the frame tap dropped because it was full or closed.
    pub fn frames_dropped(&self) -> u64 {
        self.frames_dropped.load(Ordering::Relaxed)
    }
}

/// Tells the driver when a `prompt()` future is dropped before its answer.
struct PromptGuard<'a> {
    commands: &'a async_channel::Sender<Command>,
    /// `None` once the answer came.
    session: Option<AgentSessionId>,
    turn: TurnId,
}

impl Drop for PromptGuard<'_> {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            let turn = self.turn;
            // The channel is unbounded; an error means the driver has ended.
            match self
                .commands
                .try_send(Command::PromptDropped { session, turn })
            {
                Ok(()) | Err(_) => {}
            }
        }
    }
}
