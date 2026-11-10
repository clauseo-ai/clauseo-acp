//! What a command did (`CommandOutcome`), split from `command.rs` for its length.

use serde::{Deserialize, Serialize};

use crate::{CheckpointId, MatterId, MessageId, ThreadId, TurnId};
#[cfg(doc)]
use crate::{HostCommand, RestoreRequest};

/// What a command did. The host's receipt stores it, so a repeated `command_id` returns the same
/// outcome. The queue commands, `RestartAgent`, `SetUiSettings`, `SetToolSettings`,
/// `SetPrivacySettings`, `FirstAct`, `UpdateMatter`, `OpenMatter`, `ForgetMatter`, `RefreshLimits`,
/// `MarkSeen`, `ScanFolder`, `CancelScan`, `SnoozeThread`, `MarkThreadDone`, `SweepDone`,
/// `InstallBrowser`, `CancelBrowserInstall`, `RecheckBrowserPolicy`, the four update commands, the
/// connector commands and the two allowed-site commands answer `Done`; the last two kinds store no
/// receipt, as the three browser commands do not.
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
    /// The tracked changes were accepted or rejected and the file written.
    RevisionsDecided {
        /// A checkpoint holding the file as it was just before: the `outside-edits` one taken
        /// first, or the last one when the folder had not changed. `RestoreVersion` of it, with
        /// [`RestoreRequest::Files`] naming the file, undoes the decision
        /// ([`HostCommand::RestoreVersion`]).
        before: CheckpointId,
        /// The `revisions` checkpoint taken after writing; missing when the checkpoint after the
        /// write could not be taken. The next checkpoint records the change.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkpoint: Option<CheckpointId>,
    },
}
