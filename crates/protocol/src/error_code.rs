//! `ErrorCode`, the frozen dotted codes a host call fails with, and `HostError`, the failure.
//!
//! Codes are append-only and a code's meaning never changes. The UI picks its copy by code, never
//! from `message`. `docs/decisions/0003-error-codes.md` holds one row per code; a test checks it
//! against [`ErrorCode::ALL`].

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

use crate::Error;
use crate::open_enum::BorrowedStr;

/// Writes `ErrorCode` and its methods from one table: variant, dotted string, retryable.
macro_rules! error_codes {
    ($($(#[doc = $doc:literal])* $variant:ident = $code:literal, retryable: $retryable:literal;)+) => {
        /// Why a host call failed: a stable dotted string such as `store.disk_full`.
        ///
        /// A string this version does not know decodes to [`ErrorCode::Unknown`] (and
        /// [`HostError`] keeps the original string in its `message`).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
        #[cfg_attr(feature = "specta", derive(specta::Type))]
        #[non_exhaustive]
        pub enum ErrorCode {
            $($(#[doc = $doc])* #[serde(rename = $code)] $variant,)+
        }

        impl ErrorCode {
            /// Every code, in table order.
            pub const ALL: [Self; $crate::open_enum::count_idents!($($variant)+)] =
                [$(Self::$variant),+];

            /// The Rust name of the variant, such as `"StoreDiskFull"`.
            pub const fn variant_name(&self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant),)+
                }
            }

            /// The dotted string, such as `"store.disk_full"`.
            pub const fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)+
                }
            }

            /// Whether the same call may succeed if made again later, with nothing else changed.
            pub const fn is_retryable(&self) -> bool {
                match self {
                    $(Self::$variant => $retryable,)+
                }
            }

            /// The code for `code`, or `None` when this version does not know it.
            fn lookup(code: &str) -> Option<Self> {
                match code {
                    $($code => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

error_codes! {
    /// A bug or an unexpected failure in the host. Tauri's own string rejections map here too.
    Internal = "internal", retryable: false;
    /// The command failed shape validation: a length, count or format.
    CommandInvalid = "command.invalid", retryable: false;
    /// The command conflicts with what the host already holds, such as a reused `command_id`
    /// with a different body.
    CommandConflict = "command.conflict", retryable: false;
    /// The command names a matter the host does not know.
    CommandUnknownMatter = "command.unknown_matter", retryable: false;
    /// The command names a thread the host does not know.
    CommandUnknownThread = "command.unknown_thread", retryable: false;
    /// A turn is running, so the thread refuses this command (archive, restart) for now.
    ThreadTurnRunning = "thread.turn_running", retryable: true;
    /// The host stopped during the turn; the turn ended without its answer.
    ThreadInterrupted = "thread.interrupted", retryable: false;
    /// The queued turn an edit, remove or send-now names has already started or ended.
    ThreadTurnNotQueued = "thread.turn_not_queued", retryable: false;
    /// The thread already has 10 queued turns.
    ThreadQueueFull = "thread.queue_full", retryable: true;
    /// The permission request was already answered, expired or never existed.
    PermissionNotPending = "permission.not_pending", retryable: false;
    /// The answer names an option the permission request did not offer.
    PermissionUnknownOption = "permission.unknown_option", retryable: false;
    /// The elicitation was already answered, expired or never existed.
    ElicitationNotPending = "elicitation.not_pending", retryable: false;
    /// The values do not match the elicitation's schema.
    ElicitationInvalidValues = "elicitation.invalid_values", retryable: false;
    /// The agent's process did not start or did not finish the ACP handshake.
    AgentLaunchFailed = "agent.launch_failed", retryable: true;
    /// The agent's process exited; a new launch may work.
    AgentExited = "agent.exited", retryable: true;
    /// The agent sent something that breaks ACP.
    AgentProtocolError = "agent.protocol_error", retryable: false;
    /// The agent needs the lawyer to sign in first.
    AgentAuthRequired = "agent.auth_required", retryable: false;
    /// The agent speaks an ACP version Clauseo does not support.
    AgentUnsupportedVersion = "agent.unsupported_version", retryable: false;
    /// The disk is full; space must be freed first.
    StoreDiskFull = "store.disk_full", retryable: false;
    /// The data folder holds data that does not read back.
    StoreCorrupt = "store.corrupt", retryable: false;
    /// A read or write in the data folder failed, possibly for a passing reason.
    StoreIo = "store.io", retryable: true;
    /// Another Clauseo process holds the data folder.
    StoreLocked = "store.locked", retryable: false;
    /// The data came from a newer Clauseo.
    StoreNewerVersion = "store.newer_version", retryable: false;
    /// Taking a checkpoint of the matter's files failed.
    MatterCheckpointFailed = "matter.checkpoint_failed", retryable: true;
    /// The matter's folder is gone (moved, deleted, or its drive is not mounted).
    MatterFolderMissing = "matter.folder_missing", retryable: false;
    /// The folder may not be a matter: a drive root, the home folder or a system folder.
    MatterFolderRefused = "matter.folder_refused", retryable: false;
    /// A file the operation needs is locked by another program (such as an open document).
    MatterFileLocked = "matter.file_locked", retryable: true;
    /// A turn is running in the matter, so undo or restore waits.
    MatterTurnRunning = "matter.turn_running", retryable: true;
    /// The client is not allowed to make this call.
    TransportUnauthorized = "transport.unauthorized", retryable: false;
    /// The message is larger than the transport accepts.
    TransportTooLarge = "transport.too_large", retryable: false;
    /// A code from a newer Clauseo that this version does not know. Never produced here.
    Unknown = "unknown", retryable: false;
}

impl ErrorCode {
    /// The code for `code`, or [`ErrorCode::Unknown`] when this version does not know it.
    pub fn from_str_lossy(code: &str) -> Self {
        Self::lookup(code).unwrap_or(Self::Unknown)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ErrorCode {
    type Err = Error;

    /// Parses a dotted code; a string this version does not know is an error here (use
    /// [`ErrorCode::from_str_lossy`] or serde to get [`ErrorCode::Unknown`] instead).
    fn from_str(code: &str) -> Result<Self, Error> {
        Self::lookup(code).ok_or_else(|| Error::UnknownErrorCode {
            code: code.to_owned(),
        })
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let code = BorrowedStr::deserialize(deserializer)?;
        Ok(Self::from_str_lossy(code.as_str()))
    }
}

/// What every host call returns on failure.
///
/// `message` is for logs; the UI shows copy chosen by `code`. Decoding a code this version does
/// not know gives `code: Unknown` and moves the original code into `message` as
/// `"<code>: <message>"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[error("{code}: {message}")]
pub struct HostError {
    /// Why the call failed.
    pub code: ErrorCode,
    /// A detail for logs, never shown to the lawyer.
    pub message: String,
    /// Whether the same call may succeed later.
    pub retryable: bool,
}

impl HostError {
    /// A failure with `code`, its `retryable` taken from [`ErrorCode::is_retryable`].
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            retryable: code.is_retryable(),
            message: message.into(),
        }
    }
}

impl From<Error> for HostError {
    fn from(error: Error) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

/// `HostError` as read, before its code is looked up.
#[derive(Deserialize)]
struct HostErrorWire<'a> {
    #[serde(borrow)]
    code: BorrowedStr<'a>,
    message: String,
    retryable: bool,
}

impl<'de> Deserialize<'de> for HostError {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = HostErrorWire::deserialize(deserializer)?;
        let raw = wire.code.as_str();
        let (code, message) = match ErrorCode::lookup(raw) {
            Some(code) => (code, wire.message),
            None => (ErrorCode::Unknown, format!("{raw}: {}", wire.message)),
        };
        Ok(Self {
            code,
            message,
            retryable: wire.retryable,
        })
    }
}
