//! Commands: what a client asks the host to do, and what the host answers.
//!
//! Every command travels in a [`CommandEnvelope`] with a client-made [`CommandId`] and is
//! idempotent: a repeat returns the stored [`CommandOutcome`], and the same id with another body
//! is `command.conflict` (the host's check). Commands refuse fields they do not know
//! (`deny_unknown_fields` on every struct and enum here), and `HostCommand` has no catch-all: a
//! command this version does not know fails to decode. Commands never carry file-system paths:
//! folders and prompt files are named by opaque ids, and undo and restore by a matter-relative
//! [`MatterPath`] that `matters` resolves inside the matter. Typed fields ([`Title`],
//! [`AgentChoiceId`], [`MatterPath`], the ids) are checked when they decode;
//! [`CommandEnvelope::validate`] checks the rest of the shape (counts, sums, rules across
//! fields) and makes the [`ValidatedCommand`] the host takes; state checks (is a turn running,
//! was the option offered) are the host's.

use serde::{Deserialize, Serialize};

use crate::MatterPath;
use crate::{
    AgentChoiceId, AgentId, AgentKind, CheckpointId, ClientName, CommandId, Date, Delivery,
    ElicitationId, ElicitationResponse, FieldChange, FileDecision, FolderRefId, MatterId,
    MessageId, PermissionRequestId, PromptBlock, RestoreScope, ThreadId, Title, TurnId, UiPatch,
};
#[cfg(doc)]
use crate::{FileChoice, RawPayload};

mod validate;

pub use validate::{
    MAX_ELICITATION_VALUES_BYTES, MAX_FILE_LIST, MAX_MATTER_NAME_BYTES, MAX_PATH_LIST_BYTES,
    MAX_PROMPT_BLOCKS, MAX_PROMPT_FILES, MAX_PROMPT_TEXT_BYTES, ValidatedCommand,
};

/// Most turns queued in one thread. A `SendPrompt` that would queue one more is refused with
/// [`ErrorCode::ThreadQueueFull`](crate::ErrorCode::ThreadQueueFull) (the host's check, since the
/// queue is state).
pub const MAX_QUEUED_TURNS: usize = 10;

/// One command as a client sends it. It names no principal: the surface that takes it (the
/// desktop bridge, the transport, the cli) knows who sent it from its connection and passes the
/// principal to the host itself, so a client cannot claim to be someone else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CommandEnvelope {
    /// Made by the client; a repeat with the same id returns the stored outcome.
    pub command_id: CommandId,
    /// What to do.
    pub command: HostCommand,
}

/// Where a new matter's folder comes from. Folders are named by the ids the host issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum MatterSource {
    /// `folder` is the parent; the host creates `<parent>/<name>`. The name must then also work
    /// as a folder name on every system: at most [`MAX_MATTER_NAME_BYTES`], not `.` or `..`,
    /// no leading dot (Finder hides such a folder), none of `< > : " / \ | ? *`, no leading
    /// space, no trailing dot or space, and not a Windows device name
    /// ([`is_windows_reserved_name`](crate::is_windows_reserved_name)).
    NewFolder {
        /// The parent folder.
        folder: FolderRefId,
    },
    /// The matter is an existing folder, which keeps its own name: the matter's name is only a
    /// label and meets the title rules alone, so a folder called `Smith v Jones?` can keep it.
    /// Those rules refuse some folder names that exist: more than
    /// [`MAX_TITLE_CHARS`](crate::MAX_TITLE_CHARS) characters (a folder name may be 255 bytes), a
    /// control character (C0, DEL, C1), a line or paragraph separator (U+2028, U+2029), a bidi
    /// embedding, override or isolate (U+202A to U+202E, U+2066 to U+2069), and a name with no
    /// character that shows. The desktop app shortens or cleans such a name before it pre-fills the
    /// matter's name; the folder is not renamed.
    ExistingFolder {
        /// The folder.
        folder: FolderRefId,
    },
    /// The host creates `<default location>/<name>` itself, in the settings'
    /// `matters.default_location` (or, when that is not set, `Documents/Clauseo`), so no folder
    /// is picked. The name follows [`MatterSource::NewFolder`]'s folder-name rules.
    /// JSON: the plain string `"default_location"`.
    DefaultLocation,
}

/// What a restore puts back, as a command asks for it: the strict, closed twin of the logged
/// [`RestoreScope`], with the same wire form. Unknown fields and kinds are refused, as in every
/// command; the host turns it into the scope it logs with `From`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum RestoreRequest {
    /// The whole matter.
    Matter,
    /// These files only. JSON: `{"files": {"paths": [..]}}`.
    Files {
        /// The files, relative to the matter folder.
        paths: Vec<MatterPath>,
    },
}

impl From<RestoreRequest> for RestoreScope {
    fn from(request: RestoreRequest) -> Self {
        match request {
            RestoreRequest::Matter => Self::Matter,
            RestoreRequest::Files { paths } => Self::Files { paths },
        }
    }
}

/// How to deliver a prompt while a turn runs, as a command asks for it: the strict, closed twin
/// of the logged [`Delivery`], with the same wire form, so a mode from a newer client is refused
/// when the command decodes, never run as another mode. The host turns it into the delivery it
/// logs with `From`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum DeliveryRequest {
    /// Wait in the thread's queue; also what a missing field means.
    #[default]
    Queue,
    /// Cancel the running turn, then send this prompt next.
    SendNow,
    /// Add it to the running turn where the agent can, else send it now.
    Steer,
}

impl From<DeliveryRequest> for Delivery {
    fn from(request: DeliveryRequest) -> Self {
        match request {
            DeliveryRequest::Queue => Self::Queue,
            DeliveryRequest::SendNow => Self::SendNow,
            DeliveryRequest::Steer => Self::Steer,
        }
    }
}

/// Why account limits are refreshed, as `RefreshLimits` asks for it: a window got focus, or a
/// meter or the settings page opened (`Shown`), or the lawyer pressed "Try again" (`TryAgain`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum RefreshWhy {
    /// A window got focus, or a meter or the settings page opened. Reads only data older than
    /// 5 minutes.
    Shown,
    /// The lawyer asked again. Reads now, unless a read is in flight.
    TryAgain,
}

/// A config option's new value, matching the option's kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ConfigValue {
    /// The value of one of a select option's choices.
    Select(AgentChoiceId),
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
        /// The matter's name; for [`MatterSource::NewFolder`] and
        /// [`MatterSource::DefaultLocation`] also the new folder's name.
        name: Title,
        /// Where its folder comes from.
        source: MatterSource,
        /// The client it is for; missing means none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client: Option<ClientName>,
        /// Its next date, such as a hearing; missing means none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_date: Option<Date>,
    },
    /// Change a matter's details. Each field: missing leaves it, `null` clears it (`client`,
    /// `next_date`), a value sets it. A command that changes nothing answers `Done` too.
    UpdateMatter {
        /// The matter.
        matter: MatterId,
        /// The client it is for.
        #[serde(default, skip_serializing_if = "FieldChange::is_keep")]
        #[cfg_attr(feature = "specta", specta(type = Option<Option<ClientName>>))]
        client: FieldChange<ClientName>,
        /// Its next date.
        #[serde(default, skip_serializing_if = "FieldChange::is_keep")]
        #[cfg_attr(feature = "specta", specta(type = Option<Option<Date>>))]
        next_date: FieldChange<Date>,
        /// Whether it is pinned to the top of the rail.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pinned: Option<bool>,
    },
    /// The lawyer opened a matter: its `last_opened` becomes now, which orders "Recent" and
    /// clears a `ready` or `stopped` status from turns that ended before.
    OpenMatter {
        /// The matter.
        matter: MatterId,
    },
    /// Take a matter off Clauseo's list and stop its threads' agents; refused with
    /// `matter.turn_running` while a turn, undo or restore runs in it or turns wait. The
    /// folder, its `.clauseo` and the matter's history are left as they are: importing the
    /// folder again brings the matter back with its threads and versions.
    ForgetMatter {
        /// The matter.
        matter: MatterId,
    },
    /// "Locate folder": the matter's folder moved, and the lawyer picked where it is now. The
    /// host accepts the folder only when its `.clauseo/matter.json` names this matter, then
    /// stores it as the matter's folder and logs `matter_relocated`; it answers `Done`.
    /// Refused while a turn or an undo holds the matter's writer (`matter.turn_running`); a
    /// folder with no marker, an unreadable one or one naming another matter is
    /// `matter.folder_missing`; a folder import would refuse, or one that is, holds or sits
    /// inside another matter's folder, is `matter.folder_refused`. A refusal changes nothing.
    RelocateMatter {
        /// The matter.
        matter: MatterId,
        /// The folder the lawyer picked.
        folder: FolderRefId,
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
        /// The prompt: 1 to [`MAX_PROMPT_BLOCKS`] blocks, no empty `Text` block, at least one
        /// character that shows (as [`check_display_title`](crate::check_display_title) counts
        /// it) or one `File` block, at most [`MAX_PROMPT_TEXT_BYTES`] of text and
        /// [`MAX_PROMPT_FILES`] `File` blocks.
        content: Vec<PromptBlock>,
        /// How to deliver it while a turn runs; missing means queue, and queue is written as a
        /// missing field. A mode this version does not know is refused when the command decodes.
        #[serde(default, skip_serializing_if = "Delivery::is_queue")]
        delivery: DeliveryRequest,
    },
    /// Replace a queued prompt's content; the turn keeps its id and its place.
    EditQueuedPrompt {
        /// The thread.
        thread: ThreadId,
        /// The queued turn.
        turn: TurnId,
        /// The new content, under the rules of `SendPrompt`'s `content`.
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
        option_id: AgentChoiceId,
    },
    /// Answer an elicitation.
    AnswerElicitation {
        /// The thread.
        thread: ThreadId,
        /// The elicitation.
        elicitation: ElicitationId,
        /// The answer. An `Accept`'s values are a JSON object ([`RawPayload::Map`] with string
        /// keys) of at most [`MAX_ELICITATION_VALUES_BYTES`].
        response: ElicitationResponse,
    },
    /// Set one of the agent's config options.
    SetConfigOption {
        /// The thread.
        thread: ThreadId,
        /// The option's id.
        option_id: AgentChoiceId,
        /// The new value.
        value: ConfigValue,
    },
    /// Set the agent's mode, when it has no mode config option.
    SetMode {
        /// The thread.
        thread: ThreadId,
        /// The mode's id.
        mode_id: AgentChoiceId,
    },
    /// Rename a thread.
    RenameThread {
        /// The thread.
        thread: ThreadId,
        /// The new title.
        title: Title,
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
        /// Each named file's choice ([`FileChoice::Undo`] or [`FileChoice::Keep`]); files the
        /// list leaves out are kept as they are. The list needs at least one `Undo`: an empty
        /// list, or one of only `Keep`, undoes nothing and is refused. The host refuses a path
        /// that is not in the step's `FilesChanged`.
        files: Vec<FileDecision>,
    },
    /// Restore an earlier version; refused while a turn runs in the matter.
    RestoreVersion {
        /// The matter.
        matter: MatterId,
        /// The version.
        checkpoint: CheckpointId,
        /// What to put back. A `Files` scope names at least one file; a kind or field this
        /// version does not know is refused when the command decodes.
        scope: RestoreRequest,
    },
    /// Change the display settings; answers `Done`, and every app subscriber gets
    /// `HostEvent::SettingsChanged`. A patch naming a key the firm's policy sets is refused
    /// whole with `settings.locked`, and a font size out of range with `command.invalid`;
    /// either way nothing changes.
    SetUiSettings {
        /// The fields to change.
        patch: UiPatch,
    },
    /// Refresh account limits. Answers `Done` and logs nothing. `Shown` reads only data older
    /// than 5 minutes; `TryAgain` reads now unless a read is in flight. No read starts while the
    /// agent has a permission or an elicitation pending; it starts once that is answered.
    /// `agent: None` means every agent. JSON: `{"refresh_limits": {"why": "shown"}}`, with
    /// `"agent": "claude"` (or `"codex"`) when one agent is named.
    RefreshLimits {
        /// The agent, or `None` for every agent; left out of the JSON when `None`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<AgentKind>,
        /// What asked for the refresh.
        why: RefreshWhy,
    },
}

/// What a command did. The host's receipt stores it, so a repeated `command_id` returns the same
/// outcome. The queue commands, `RestartAgent`, `SetUiSettings`, `UpdateMatter`, `OpenMatter`,
/// `ForgetMatter` and `RefreshLimits` answer `Done`.
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
