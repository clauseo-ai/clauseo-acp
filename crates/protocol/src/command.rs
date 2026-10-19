//! Commands: what a client asks the host to do, and what the host answers.
//!
//! Every command travels in a [`CommandEnvelope`] with a client-made [`CommandId`] and is
//! idempotent: a repeat returns the stored [`CommandOutcome`], and the same id with another body
//! is `command.conflict` (the host's check). Commands refuse fields they do not know
//! (`deny_unknown_fields` on every struct and enum here), name files and folders by opaque ids,
//! never paths, and `HostCommand` has no catch-all: a command this version does not know fails to
//! decode. [`HostCommand::validate`] checks shape only (lengths, counts, formats; see
//! `command/validate.rs`); state checks (is a turn running, was the option offered) are the
//! host's.

use serde::{Deserialize, Serialize};

use crate::{
    AgentId, CheckpointId, CommandId, CompactString, Delivery, ElicitationId, ElicitationResponse,
    FileDecision, FolderRefId, MatterId, MessageId, PermissionRequestId, Principal, PromptBlock,
    RestoreScope, ThreadId, TurnId,
};

mod validate;

pub use validate::{
    MAX_ELICITATION_VALUES_BYTES, MAX_FILE_LIST, MAX_ID_TEXT_BYTES, MAX_PROMPT_FILES,
    MAX_PROMPT_TEXT_BYTES, MAX_TITLE_CHARS,
};

/// One command as a client sends it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CommandEnvelope {
    /// Made by the client; a repeat with the same id returns the stored outcome.
    pub command_id: CommandId,
    /// Who sent it.
    pub principal: Principal,
    /// What to do.
    pub command: HostCommand,
}

/// Where a new matter's folder comes from. Folders are named by the ids the host issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum MatterSource {
    /// `folder` is the parent; the host creates `<parent>/<name>`.
    NewFolder {
        /// The parent folder.
        folder: FolderRefId,
    },
    /// The matter is an existing folder.
    ExistingFolder {
        /// The folder.
        folder: FolderRefId,
    },
}

/// A config option's new value, matching the option's kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ConfigValue {
    /// The value of one of a select option's choices.
    Select(#[cfg_attr(feature = "specta", specta(type = String))] CompactString),
    /// A boolean option's value.
    Boolean(bool),
}

/// What a client asks the host to do. JSON: `{"send_prompt": {...}}`; unknown fields and unknown
/// commands are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum HostCommand {
    /// Make a matter from a folder.
    CreateMatter {
        /// The matter's name; for `NewFolder` also the new folder's name.
        name: String,
        /// Where its folder comes from.
        source: MatterSource,
    },
    /// Start a thread in a matter with an agent.
    CreateThread {
        /// The matter.
        matter: MatterId,
        /// The agent's registry id.
        agent: AgentId,
    },
    /// Send a prompt. Never refused because a turn is running: it is queued, sent now or steered.
    SendPrompt {
        /// The thread.
        thread: ThreadId,
        /// The prompt.
        content: Vec<PromptBlock>,
        /// How to deliver it while a turn runs; missing means [`Delivery::Queue`].
        #[serde(skip_serializing_if = "Option::is_none", default)]
        delivery: Option<Delivery>,
    },
    /// Replace a queued prompt's content; the turn keeps its id and its place.
    EditQueuedPrompt {
        /// The thread.
        thread: ThreadId,
        /// The queued turn.
        turn: TurnId,
        /// The new content.
        content: Vec<PromptBlock>,
    },
    /// Remove a queued prompt; its turn ends cancelled without starting.
    RemoveQueuedPrompt {
        /// The thread.
        thread: ThreadId,
        /// The queued turn.
        turn: TurnId,
    },
    /// Cancel the running turn and start this queued turn next.
    SendQueuedNow {
        /// The thread.
        thread: ThreadId,
        /// The queued turn.
        turn: TurnId,
    },
    /// Start the head of a held queue.
    ResumeQueue {
        /// The thread.
        thread: ThreadId,
    },
    /// Stop the thread's current turn; its queued turns stay, held.
    Cancel {
        /// The thread.
        thread: ThreadId,
    },
    /// Start the thread's agent again; refused while a turn runs.
    RestartAgent {
        /// The thread.
        thread: ThreadId,
    },
    /// Answer a permission request; the first valid answer wins.
    AnswerPermission {
        /// The thread.
        thread: ThreadId,
        /// The request.
        request: PermissionRequestId,
        /// One of the offered options' ids.
        #[cfg_attr(feature = "specta", specta(type = String))]
        option_id: CompactString,
    },
    /// Answer an elicitation.
    AnswerElicitation {
        /// The thread.
        thread: ThreadId,
        /// The elicitation.
        elicitation: ElicitationId,
        /// The answer.
        response: ElicitationResponse,
    },
    /// Set one of the agent's config options.
    SetConfigOption {
        /// The thread.
        thread: ThreadId,
        /// The option's id.
        #[cfg_attr(feature = "specta", specta(type = String))]
        option_id: CompactString,
        /// The new value.
        value: ConfigValue,
    },
    /// Set the agent's mode, when it has no mode config option.
    SetMode {
        /// The thread.
        thread: ThreadId,
        /// The mode's id.
        #[cfg_attr(feature = "specta", specta(type = String))]
        mode_id: CompactString,
    },
    /// Rename a thread.
    RenameThread {
        /// The thread.
        thread: ThreadId,
        /// The new title.
        #[cfg_attr(feature = "specta", specta(type = String))]
        title: CompactString,
    },
    /// Archive a thread; refused while a turn runs.
    ArchiveThread {
        /// The thread.
        thread: ThreadId,
    },
    /// Undo a step, file by file; refused while a turn runs in the matter.
    UndoStep {
        /// The thread.
        thread: ThreadId,
        /// The step's turn.
        turn: TurnId,
        /// Each file's choice.
        files: Vec<FileDecision>,
    },
    /// Restore an earlier version; refused while a turn runs in the matter.
    RestoreVersion {
        /// The matter.
        matter: MatterId,
        /// The version.
        checkpoint: CheckpointId,
        /// What to put back.
        scope: RestoreScope,
    },
}

/// What a command did. The host's receipt stores it, so a repeated `command_id` returns the same
/// outcome. The queue commands and `RestartAgent` answer `Done`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum CommandOutcome {
    /// Done; nothing new to name.
    Done,
    /// The matter was made.
    MatterCreated {
        /// The new matter.
        matter: MatterId,
    },
    /// The thread was made.
    ThreadCreated {
        /// The new thread.
        thread: ThreadId,
    },
    /// The prompt was taken (queued, sent now or steered).
    PromptAccepted {
        /// The turn that answers it (for a steer, until a `SteerResolved` says otherwise).
        turn: TurnId,
        /// The prompt's message.
        message: MessageId,
    },
    /// The step was undone.
    StepUndone {
        /// The checkpoint the undo wrote.
        checkpoint: CheckpointId,
    },
    /// The version was restored.
    VersionRestored {
        /// The checkpoint the restore wrote.
        checkpoint: CheckpointId,
    },
}
