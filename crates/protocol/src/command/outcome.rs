//! What a command did (`CommandOutcome`), split from `command.rs` for its length.

use serde::{Deserialize, Serialize};

use crate::{CheckpointId, MatterId, MessageId, ThreadId, TurnId};

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
}
