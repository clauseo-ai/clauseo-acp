//! `ErrorCode`, the frozen dotted codes a host call fails with, and `HostError`, the failure.
//!
//! Codes are append-only and a code's meaning never changes. The UI picks its copy by code, never
//! from `message`. `docs/decisions/0003-error-codes.md` holds one row per code; a test checks it
//! against [`ErrorCode::ALL`].

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

use crate::Error;
use crate::serde_util::BorrowedStr;

/// Writes `ErrorCode` and its methods from one table: variant, dotted string, retryable, and the
/// meaning, which is both the variant's doc comment and what `meaning()` returns. The record in
/// `docs/decisions/0003-error-codes.md` has the same text in its meaning column.
macro_rules! error_codes {
    ($($variant:ident = $code:literal, retryable: $retryable:literal, meaning: $meaning:literal;)+) => {
        /// Why a host call failed: a stable dotted string such as `store.disk_full`.
        ///
        /// A string this version does not know decodes to [`ErrorCode::Unknown`] (and
        /// [`HostError`] keeps the original string in its `message`). Not `non_exhaustive`:
        /// `Unknown` covers newer codes on the wire, so a match lists every code and a new code
        /// flags each match that should handle it. `Unknown` stays last; a test checks it.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
        #[cfg_attr(feature = "specta", derive(specta::Type))]
        pub enum ErrorCode {
            $(#[doc = $meaning] #[serde(rename = $code)] $variant,)+
        }

        impl ErrorCode {
            /// Every code, in table order.
            pub const ALL: [Self; $crate::open_enum::count_idents!($($variant)+)] =
                [$(Self::$variant),+];

            /// The Rust name of the variant, such as `"StoreDiskFull"`, for tests.
            #[doc(hidden)]
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

            /// What the code means: the same sentence as the variant's doc comment and its row in
            /// the record.
            pub const fn meaning(&self) -> &'static str {
                match self {
                    $(Self::$variant => $meaning,)+
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
    Internal = "internal", retryable: false, meaning: "A bug or an unexpected failure in the \
        host; Tauri's own string rejections map here too.";
    CommandInvalid = "command.invalid", retryable: false, meaning: "The command failed shape \
        validation: a length, count or format. A query that fails to decode (a `PageLimit` or \
        `ReadLen` out of range) gets this code too.";
    CommandConflict = "command.conflict", retryable: false, meaning: "The command conflicts with \
        what the host holds, such as a reused `command_id` with a different body.";
    CommandUnknownMatter = "command.unknown_matter", retryable: false, meaning: "The command \
        names a matter the host does not know.";
    CommandUnknownThread = "command.unknown_thread", retryable: false, meaning: "The command \
        names a thread the host does not know.";
    ThreadTurnRunning = "thread.turn_running", retryable: true, meaning: "A turn is running, so \
        the thread refuses this command (archive, restart) for now.";
    ThreadInterrupted = "thread.interrupted", retryable: false, meaning: "The host stopped during \
        the turn; the turn ended without its answer.";
    ThreadTurnNotQueued = "thread.turn_not_queued", retryable: false, meaning: "The queued turn \
        an edit, remove or send-now names has already started or ended.";
    ThreadQueueFull = "thread.queue_full", retryable: true, meaning: "The thread already has \
        `MAX_QUEUED_TURNS` queued turns.";
    PermissionNotPending = "permission.not_pending", retryable: false, meaning: "The permission \
        request was already answered, expired or never existed.";
    PermissionUnknownOption = "permission.unknown_option", retryable: false, meaning: "The answer \
        names an option the permission request did not offer.";
    ElicitationNotPending = "elicitation.not_pending", retryable: false, meaning: "The \
        elicitation was already answered, expired or never existed.";
    ElicitationInvalidValues = "elicitation.invalid_values", retryable: false, meaning: "The \
        values do not match the elicitation's schema.";
    AgentLaunchFailed = "agent.launch_failed", retryable: true, meaning: "The agent's process did \
        not start or did not finish the ACP handshake.";
    AgentExited = "agent.exited", retryable: true, meaning: "The agent's process exited; a new \
        launch may work.";
    AgentProtocolError = "agent.protocol_error", retryable: false, meaning: "The agent sent \
        something that breaks ACP.";
    AgentAuthRequired = "agent.auth_required", retryable: false, meaning: "The agent needs the \
        lawyer to sign in first.";
    AgentUnsupportedVersion = "agent.unsupported_version", retryable: false, meaning: "The agent \
        speaks an ACP version Clauseo does not support.";
    StoreDiskFull = "store.disk_full", retryable: false, meaning: "The disk is full; space must \
        be freed first.";
    StoreCorrupt = "store.corrupt", retryable: false, meaning: "The data folder holds data that \
        does not read back, a stored value that does not parse included.";
    StoreIo = "store.io", retryable: true, meaning: "A read or write in the data folder failed, \
        possibly for a passing reason.";
    StoreLocked = "store.locked", retryable: false, meaning: "Another Clauseo process holds the \
        data folder.";
    StoreNewerVersion = "store.newer_version", retryable: false, meaning: "The data came from a \
        newer Clauseo.";
    MatterCheckpointFailed = "matter.checkpoint_failed", retryable: true, meaning: "Taking a \
        checkpoint of the matter's files failed.";
    MatterFolderMissing = "matter.folder_missing", retryable: false, meaning: "The matter's \
        folder is gone: moved, deleted, or its drive is not mounted.";
    MatterFolderRefused = "matter.folder_refused", retryable: false, meaning: "The folder may not \
        be a matter: a drive root, the home folder or a system folder.";
    MatterFileLocked = "matter.file_locked", retryable: true, meaning: "A file the operation \
        needs is locked by another program, such as an open document.";
    MatterTurnRunning = "matter.turn_running", retryable: true, meaning: "A turn is running in \
        the matter, so undo or restore waits.";
    TransportUnauthorized = "transport.unauthorized", retryable: false, meaning: "The client is \
        not allowed to make this call.";
    TransportTooLarge = "transport.too_large", retryable: false, meaning: "The message is larger \
        than the transport accepts.";
    Unknown = "unknown", retryable: false, meaning: "A code from a newer Clauseo that this \
        version does not know. Never produced here.";
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
///
/// The fields are private so `code` and `retryable` cannot disagree: [`HostError::new`] takes
/// `retryable` from the code, and so does decoding a code this version knows (a code's meaning
/// is frozen), whatever `retryable` the peer wrote. Only for [`ErrorCode::Unknown`] is the
/// peer's `retryable` kept, since a code this version does not know has none of its own here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[error("{code}: {message}")]
pub struct HostError {
    /// Why the call failed.
    code: ErrorCode,
    /// A detail for logs, never shown to the lawyer.
    message: String,
    /// Whether the same call may succeed later.
    retryable: bool,
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

    /// Why the call failed.
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    /// A detail for logs, never shown to the lawyer.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Whether the same call may succeed later.
    pub const fn retryable(&self) -> bool {
        self.retryable
    }

    /// A `command.invalid` failure for a command whose own input broke a rule: a value that did
    /// not parse or a limit `validate` checks. Only for a command's input; a value read back from
    /// the store that does not parse is `store.corrupt`, which the store builds with
    /// [`HostError::new`].
    #[expect(
        clippy::needless_pass_by_value,
        reason = "taken by value so a caller writes `.map_err(HostError::invalid_command)`"
    )]
    pub fn invalid_command(error: Error) -> Self {
        Self::new(ErrorCode::CommandInvalid, error.to_string())
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
        Ok(match ErrorCode::lookup(raw) {
            // `lookup` finds `Unknown` for the string "unknown", which has no `retryable` of its
            // own: the peer's value stays, as it does for a code this version does not know.
            Some(code) => Self {
                code,
                message: wire.message,
                retryable: if code == ErrorCode::Unknown {
                    wire.retryable
                } else {
                    code.is_retryable()
                },
            },
            None => Self {
                code: ErrorCode::Unknown,
                message: format!("{raw}: {}", wire.message),
                retryable: wire.retryable,
            },
        })
    }
}
