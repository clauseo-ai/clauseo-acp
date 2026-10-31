//! The sent view of one thread stream: what the host has sent to the store, not only what came
//! back durable.
//!
//! A `tool_call_update` can arrive while its `ToolCallStarted` commit is still in flight, so the
//! checks that decide what to log ("unknown tool call", replay dedupe, "not pending", the inline
//! recount after a patch) read what was sent. The view keeps two layers folded by the same
//! [`Layer::observe`]: `sent`, moved when a commit is sent, and `committed`, moved when a commit
//! returns `Ok`. Lookups read `sent`. After a failed commit, once the stream's in-flight commits
//! have all returned, [`SentView::roll_back`] sets `sent` back to `committed`, which is what the
//! log holds and what the reducer folded.

use std::collections::BTreeMap;

use clauseo_protocol::{
    AgentSessionId, CompactString, DomainEvent, ElicitationId, ElicitationRequest, Envelope,
    MessageId, PermissionOption, PermissionRequestId, StreamId, ToolCallId, ToolCallStatus, TurnId,
};

use crate::build::ToolFields;

/// One tool call as last sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SentTool {
    /// ACP's `toolCallId`, unique within one agent session.
    pub(crate) acp_id: CompactString,
    /// The turn that started it.
    pub(crate) turn: TurnId,
    /// Its inline costs as last sent, for the recount after a patch.
    pub(crate) fields: ToolFields,
    /// Whether its status is final: completed, failed or cancelled.
    pub(crate) finished: bool,
}

/// A permission request with no answer logged yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingPermission {
    /// The tool call it asks about, if any.
    pub(crate) tool_call: Option<ToolCallId>,
    /// The options the agent offered; an answer must name one of them.
    pub(crate) options: Vec<PermissionOption>,
}

/// One layer of the view: the fold of a run of envelopes of the stream.
#[derive(Debug, Clone, Default, PartialEq)]
struct Layer {
    /// The attached agent session, from `AgentSessionAttached` until `AgentSessionDetached`.
    session: Option<AgentSessionId>,
    /// Tool calls by id.
    tools: BTreeMap<ToolCallId, SentTool>,
    /// Tool call ids by session, then ACP id. Nested so a lookup borrows the `&str`.
    tool_keys: BTreeMap<AgentSessionId, BTreeMap<CompactString, ToolCallId>>,
    /// Message ids by session, then ACP message id.
    messages: BTreeMap<AgentSessionId, BTreeMap<CompactString, MessageId>>,
    /// Permission requests not resolved or expired.
    permissions: BTreeMap<PermissionRequestId, PendingPermission>,
    /// Elicitations not resolved or expired.
    elicitations: BTreeMap<ElicitationId, ElicitationRequest>,
}

impl Layer {
    /// Folds one envelope of the stream.
    fn observe(&mut self, envelope: &Envelope) {
        match &envelope.event {
            DomainEvent::AgentSessionAttached(attached) => {
                self.session = Some(attached.session.clone());
            }
            DomainEvent::AgentSessionDetached(_) => self.session = None,
            DomainEvent::UserMessage(message) => {
                self.message(message.acp_message_id.as_ref(), message.message);
            }
            DomainEvent::TextAppended(text) => {
                self.message(text.acp_message_id.as_ref(), text.message);
            }
            DomainEvent::ContentAppended(content) => {
                self.message(content.acp_message_id.as_ref(), content.message);
            }
            DomainEvent::ToolCallStarted(started) => {
                if let Some(session) = &self.session {
                    self.tool_keys
                        .entry(session.clone())
                        .or_default()
                        .insert(started.acp_id.clone(), started.tool_call);
                }
                self.tools.insert(
                    started.tool_call,
                    SentTool {
                        acp_id: started.acp_id.clone(),
                        turn: started.turn,
                        fields: ToolFields::of_started(started),
                        finished: is_final(started.status),
                    },
                );
            }
            DomainEvent::ToolCallUpdated(updated) => {
                if let Some(tool) = self.tools.get_mut(&updated.tool_call) {
                    tool.fields = tool.fields.patched(&updated.patch);
                    if let Some(status) = updated.patch.status {
                        tool.finished = is_final(status);
                    }
                }
            }
            DomainEvent::PermissionRequested(requested) => {
                self.permissions.insert(
                    requested.request,
                    PendingPermission {
                        tool_call: requested.tool_call,
                        options: requested.options.clone(),
                    },
                );
            }
            DomainEvent::PermissionResolved(resolved) => {
                self.permissions.remove(&resolved.request);
            }
            DomainEvent::PermissionExpired(expired) => {
                self.permissions.remove(&expired.request);
            }
            DomainEvent::ElicitationRequested(requested) => {
                self.elicitations
                    .insert(requested.elicitation, requested.request.clone());
            }
            DomainEvent::ElicitationResolved(resolved) => {
                self.elicitations.remove(&resolved.elicitation);
            }
            DomainEvent::ElicitationExpired(expired) => {
                self.elicitations.remove(&expired.elicitation);
            }
            DomainEvent::ThreadCreated(_)
            | DomainEvent::ThreadRenamed(_)
            | DomainEvent::ThreadArchived(_)
            | DomainEvent::TurnQueued(_)
            | DomainEvent::QueuedPromptEdited(_)
            | DomainEvent::SteerResolved(_)
            | DomainEvent::TurnStarted(_)
            | DomainEvent::PlanReplaced(_)
            | DomainEvent::ModeChanged(_)
            | DomainEvent::ConfigOptionsReplaced(_)
            | DomainEvent::CommandsAvailable(_)
            | DomainEvent::UsageUpdated(_)
            | DomainEvent::Notice(_)
            | DomainEvent::CompactionChanged(_)
            | DomainEvent::FilesChanged(_)
            | DomainEvent::TurnFailed(_)
            | DomainEvent::TurnStoppedAtLimit(_)
            | DomainEvent::TurnEnded(_)
            | DomainEvent::StepUndone(_)
            | DomainEvent::MatterCreated(_)
            | DomainEvent::CheckpointRecorded(_)
            | DomainEvent::OutsideEdits(_)
            | DomainEvent::VersionRestored(_)
            | DomainEvent::Unknown { .. } => {}
        }
    }

    /// Records a message's ACP id under the attached session. A message is keyed once: later
    /// text of the same message finds it already there and allocates nothing.
    fn message(&mut self, acp_message_id: Option<&CompactString>, message: MessageId) {
        let (Some(session), Some(acp)) = (&self.session, acp_message_id) else {
            return;
        };
        if let Some(known) = self.messages.get(session)
            && known.contains_key(acp.as_str())
        {
            return;
        }
        self.messages
            .entry(session.clone())
            .or_default()
            .insert(acp.clone(), message);
    }
}

/// Whether a tool call status is final.
const fn is_final(status: ToolCallStatus) -> bool {
    match status {
        ToolCallStatus::Completed | ToolCallStatus::Failed | ToolCallStatus::Cancelled => true,
        ToolCallStatus::Pending | ToolCallStatus::InProgress | ToolCallStatus::Unknown => false,
    }
}

/// The sent view of one thread stream.
#[derive(Debug, Clone)]
pub(crate) struct SentView {
    /// The stream it follows; envelopes of other streams are skipped.
    stream: StreamId,
    /// Moved when a commit is sent; every lookup reads it.
    sent: Layer,
    /// Moved when a commit returns `Ok`.
    committed: Layer,
}

impl SentView {
    /// An empty view of `stream`, for a stream with no events.
    pub(crate) fn new(stream: StreamId) -> Self {
        Self {
            stream,
            sent: Layer::default(),
            committed: Layer::default(),
        }
    }

    /// The view of a stream loaded from its log: both layers folded from `envelopes`.
    pub(crate) fn rebuild<'a>(
        stream: StreamId,
        envelopes: impl IntoIterator<Item = &'a Envelope>,
    ) -> Self {
        let mut view = Self::new(stream);
        for envelope in envelopes {
            if envelope.stream == stream {
                view.committed.observe(envelope);
            }
        }
        view.sent = view.committed.clone();
        view
    }

    /// A commit holding `envelopes` was sent; envelopes of other streams are skipped.
    pub(crate) fn on_sent(&mut self, envelopes: &[Envelope]) {
        for envelope in envelopes {
            if envelope.stream == self.stream {
                self.sent.observe(envelope);
            }
        }
    }

    /// A commit holding `envelopes` returned `Ok`; envelopes of other streams are skipped.
    pub(crate) fn on_committed(&mut self, envelopes: &[Envelope]) {
        for envelope in envelopes {
            if envelope.stream == self.stream {
                self.committed.observe(envelope);
            }
        }
    }

    /// The stream's in-flight commits have all returned after a failure: what was sent and
    /// failed is forgotten, and the view is the log's again.
    pub(crate) fn roll_back(&mut self) {
        self.sent.clone_from(&self.committed);
    }

    /// The agent session attached as sent, if any.
    pub(crate) fn session(&self) -> Option<&AgentSessionId> {
        self.sent.session.as_ref()
    }

    /// The tool call ACP knows as `acp_id` in `session`.
    pub(crate) fn tool_call(
        &self,
        session: &AgentSessionId,
        acp_id: &str,
    ) -> Option<(ToolCallId, &SentTool)> {
        let id = *self.sent.tool_keys.get(session)?.get(acp_id)?;
        self.sent.tools.get(&id).map(|tool| (id, tool))
    }

    /// The tool call with this id.
    pub(crate) fn tool(&self, id: ToolCallId) -> Option<&SentTool> {
        self.sent.tools.get(&id)
    }

    /// The tool calls of `turn` whose status is not final, by id.
    pub(crate) fn unfinished_tools(&self, turn: TurnId) -> impl Iterator<Item = ToolCallId> + '_ {
        self.sent
            .tools
            .iter()
            .filter(move |(_, tool)| tool.turn == turn && !tool.finished)
            .map(|(id, _)| *id)
    }

    /// The message ACP knows as `acp_message_id` in `session`.
    pub(crate) fn message(
        &self,
        session: &AgentSessionId,
        acp_message_id: &str,
    ) -> Option<MessageId> {
        self.sent
            .messages
            .get(session)?
            .get(acp_message_id)
            .copied()
    }

    /// The pending permission request with this id.
    pub(crate) fn permission(&self, id: PermissionRequestId) -> Option<&PendingPermission> {
        self.sent.permissions.get(&id)
    }

    /// Every pending permission request, by id.
    pub(crate) fn permissions(
        &self,
    ) -> impl Iterator<Item = (PermissionRequestId, &PendingPermission)> {
        self.sent
            .permissions
            .iter()
            .map(|(id, pending)| (*id, pending))
    }

    /// The pending elicitation with this id.
    pub(crate) fn elicitation(&self, id: ElicitationId) -> Option<&ElicitationRequest> {
        self.sent.elicitations.get(&id)
    }

    /// Every pending elicitation, by id.
    pub(crate) fn elicitations(
        &self,
    ) -> impl Iterator<Item = (ElicitationId, &ElicitationRequest)> {
        self.sent
            .elicitations
            .iter()
            .map(|(id, request)| (*id, request))
    }

    /// Whether the sent layer equals the committed one: nothing is in flight, or all of it
    /// returned and was folded.
    #[cfg(test)]
    pub(crate) fn settled(&self) -> bool {
        self.sent == self.committed
    }

    /// Whether two views hold the same sent layer.
    #[cfg(test)]
    pub(crate) fn same_as(&self, other: &Self) -> bool {
        self.stream == other.stream && self.sent == other.sent
    }
}

#[cfg(test)]
mod tests;
