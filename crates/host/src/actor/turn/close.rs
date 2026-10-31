//! The commit that closes a turn. Every path that ends a turn (step 7, a turn that never
//! started, a dequeued turn, the retry of a failed end, start-up repair) builds one
//! [`TurnClose`] and pushes it with [`TurnClose::push_into`], which keeps the order rules:
//! expiries, cancelled tool calls, the matter's checkpoint events, `FilesChanged`,
//! `TurnFailed`, `TurnEnded`, `TurnStoppedAtLimit`, and the detach last, so R6 leaves the
//! thread in the state the detach gives.

use clauseo_protocol::{
    AgentSessionDetached, CheckpointId, DetachReason, DomainEvent, ElicitationExpired,
    ElicitationId, ErrorCode, ExpiryReason, FilesChanged, MatterId, PermissionExpired,
    PermissionRequestId, StopReason, StreamId, ThreadId, TurnEnded, TurnFailed, TurnId,
    TurnStoppedAtLimit,
};

use super::super::Actor;
use super::out::Out;
use super::{LimitMark, cancel};
use crate::error::Error;

/// The requests a close expires.
#[derive(Debug, Clone)]
pub(super) enum Expiries {
    /// None.
    None,
    /// Every request the thread's sent view holds pending, with this reason.
    Pending(ExpiryReason),
    /// These requests, read from the whole log at start-up, with `HostRestarted`.
    Listed {
        permissions: Vec<PermissionRequestId>,
        elicitations: Vec<ElicitationId>,
    },
}

impl Expiries {
    /// `PermissionExpired` and `ElicitationExpired` for each request.
    pub(super) fn push_into(&self, actor: &mut Actor, out: &mut Out) -> Result<(), Error> {
        let (reason, permissions, elicitations) = match self {
            Self::None => return Ok(()),
            Self::Pending(reason) => {
                let (permissions, elicitations) = actor
                    .loaded(StreamId::Thread(out.thread()))
                    .map(|loaded| {
                        let permissions = loaded.sent.permissions().map(|(id, _)| id).collect();
                        let elicitations = loaded.sent.elicitations().map(|(id, _)| id).collect();
                        (permissions, elicitations)
                    })
                    .unwrap_or_default();
                (*reason, permissions, elicitations)
            }
            Self::Listed {
                permissions,
                elicitations,
            } => (
                ExpiryReason::HostRestarted,
                permissions.clone(),
                elicitations.clone(),
            ),
        };
        for request in permissions {
            out.push(actor, PermissionExpired { request, reason })?;
        }
        for elicitation in elicitations {
            let expired = ElicitationExpired {
                elicitation,
                reason,
            };
            out.push(actor, expired)?;
        }
        Ok(())
    }

    /// The reason the expiries give, for the retry of a failed end.
    fn reason(&self) -> ExpiryReason {
        match self {
            Self::None => ExpiryReason::TurnEnded,
            Self::Pending(reason) => *reason,
            Self::Listed { .. } => ExpiryReason::HostRestarted,
        }
    }
}

/// Everything the commit closing a turn holds besides the thread's held text.
#[derive(Debug, Clone)]
pub(super) struct TurnClose {
    pub(super) turn: TurnId,
    pub(super) stop: StopReason,
    /// The turn's first failure: logged as `TurnFailed`.
    pub(super) failure: Option<(ErrorCode, Option<String>)>,
    pub(super) expiries: Expiries,
    /// `Cancelled` for each tool call of the turn not finished, as sent.
    pub(super) cancel_tools: bool,
    /// Events of the matter's stream: checkpoint records and outside edits.
    pub(super) matter: Option<(MatterId, Vec<DomainEvent>)>,
    pub(super) files: Option<FilesChanged>,
    pub(super) checkpoint_after: Option<CheckpointId>,
    /// The turn's limit signal; `TurnStoppedAtLimit` when it stops `AtLimit`.
    pub(super) limit: Option<LimitMark>,
    /// The agent exited during the turn: `AgentSessionDetached`, last.
    pub(super) detach: Option<DetachReason>,
}

impl TurnClose {
    /// A close of `turn` with `stop` and nothing else.
    pub(super) fn new(turn: TurnId, stop: StopReason) -> Self {
        Self {
            turn,
            stop,
            failure: None,
            expiries: Expiries::None,
            cancel_tools: false,
            matter: None,
            files: None,
            checkpoint_after: None,
            limit: None,
            detach: None,
        }
    }

    /// Adds an event to the matter's stream part of the close.
    pub(super) fn push_matter(&mut self, matter: MatterId, event: impl Into<DomainEvent>) {
        match &mut self.matter {
            Some((_, events)) => events.push(event.into()),
            None => self.matter = Some((matter, vec![event.into()])),
        }
    }

    /// Pushes the close into `out`, in the order of the module doc.
    pub(super) fn push_into(&self, actor: &mut Actor, out: &mut Out) -> Result<(), Error> {
        let turn = self.turn;
        self.expiries.push_into(actor, out)?;
        if self.cancel_tools {
            cancel::push_tool_cancels(actor, out.thread(), turn, out)?;
        }
        if let Some((matter, events)) = &self.matter {
            for event in events {
                out.push_matter(actor, *matter, event.clone())?;
            }
        }
        if let Some(files) = &self.files {
            out.push(actor, files.clone())?;
        }
        if let Some((code, detail)) = &self.failure {
            let detail = detail.clone();
            out.push(
                actor,
                TurnFailed {
                    turn,
                    code: *code,
                    detail,
                },
            )?;
        }
        let ended = TurnEnded {
            turn,
            stop: self.stop,
            usage: None,
            checkpoint_after: self.checkpoint_after,
        };
        out.push(actor, ended)?;
        if let (Some(limit), StopReason::AtLimit) = (&self.limit, self.stop) {
            let stopped = TurnStoppedAtLimit {
                turn,
                window: limit.window.clone(),
                resets_at: limit.resets_at,
                reason: limit.reason,
            };
            out.push(actor, stopped)?;
        }
        if let Some(reason) = self.detach.clone() {
            out.push(actor, AgentSessionDetached { reason })?;
        }
        Ok(())
    }

    /// One commit holding the close, tagged `then`.
    pub(super) fn send(
        &self,
        actor: &mut Actor,
        thread: ThreadId,
        then: super::Then,
    ) -> Result<(), Error> {
        let mut out = Out::new(actor, thread, None)?;
        self.push_into(actor, &mut out)?;
        out.send(actor, then)
    }

    /// The reason the close's expiries give.
    pub(super) fn expiry(&self) -> ExpiryReason {
        self.expiries.reason()
    }
}
