//! What a command did (`CommandOutcome`), split from `command.rs` for its length.

use serde::{Deserialize, Serialize};

use crate::{CheckpointId, MatterId, MessageId, ThreadId, TurnId};

/// What a command did. The host's receipt stores it, so a repeated `command_id` returns the same
/// outcome. The queue commands, `RestartAgent`, `SetUiSettings`, `SetToolSettings`,
/// `SetPrivacySettings`, `FirstAct`, `UpdateMatter`, `OpenMatter`, `ForgetMatter`, `RefreshLimits`,
/// `MarkSeen`, `ScanFolder`, `CancelScan`, `SnoozeThread`, `MarkThreadDone`, `SweepDone`,
/// `InstallBrowser`, `CancelBrowserInstall`, the four update commands, the connector commands and
/// the two allowed-site commands answer `Done`; the last two kinds store no receipt, as the two
/// browser commands do not. A jailed Codex's `SetMode` or mode `SetConfigOption` that a later
/// one replaced answers `Replaced`.
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
    /// Nothing it asked was applied: a later command of the same kind replaced it before it took
    /// effect (a jailed Codex's mode switch the next one replaced, R17 PR-D2c). Not success: the
    /// app must not read it as the mode being set. A receipt an older build cannot decode counts
    /// as missing (`lib.rs`, Depends on), so this closed enum may gain it.
    Replaced,
}

#[cfg(test)]
mod tests {
    use super::CommandOutcome;

    /// `Replaced` is a plain string on the wire, never `done`.
    #[test]
    fn replaced_is_its_own_plain_string() {
        let text = serde_json::to_string(&CommandOutcome::Replaced).unwrap();
        assert_eq!(text, r#""replaced""#);
        let back: CommandOutcome = serde_json::from_str(&text).unwrap();
        assert_eq!(back, CommandOutcome::Replaced);
        assert_ne!(back, CommandOutcome::Done);
    }
}
