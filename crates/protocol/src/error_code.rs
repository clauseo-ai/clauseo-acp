//! `ErrorCode`, the frozen dotted codes a host call fails with, and `HostError`, the failure.
//!
//! Codes are append-only and a code's meaning never changes. The UI picks its copy by code, never
//! from `message`. `docs/decisions/0003-error-codes.md` holds one row per code; a test checks it
//! against [`ErrorCode::ALL`].

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

use crate::serde_util::BorrowedStr;
use crate::{Error, MatterId, MatterPath};

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
    MatterCodexFolder = "matter.codex_folder", retryable: false, meaning: "The matter folder \
        holds a `.codex` folder, whose settings Codex would load.";
    MatterFileLocked = "matter.file_locked", retryable: true, meaning: "A file the operation \
        needs is locked by another program, such as an open document.";
    MatterTurnRunning = "matter.turn_running", retryable: true, meaning: "A turn is running in \
        the matter, so undo or restore waits.";
    MatterFolderIsDrive = "matter.folder_is_drive", retryable: false, meaning: "The folder may \
        not be a matter: it is the root of a drive or a mounted volume.";
    MatterFolderIsHome = "matter.folder_is_home", retryable: false, meaning: "The folder may not \
        be a matter: it is the home folder, or holds it.";
    MatterFolderIsSystem = "matter.folder_is_system", retryable: false, meaning: "The folder may \
        not be a matter: it is a system folder or a folder of app data or keys, or holds one.";
    MatterFolderUnreadable = "matter.folder_unreadable", retryable: false, meaning: "Clauseo may \
        not read the folder, or its `.clauseo/matter.json` does not read, so it may belong to a \
        matter.";
    MatterFolderIsMatter = "matter.folder_is_matter", retryable: false, meaning: "The folder is \
        a matter's folder, or sits inside one or holds one; the error's `matter` names that \
        matter when it is on Clauseo's list.";
    MatterFolderInMattersRoot = "matter.folder_in_matters_root", retryable: false, meaning: "The \
        folder is or holds Clauseo's data folder, or is, holds or sits inside the matters' \
        history folder in it.";
    MatterFolderNetwork = "matter.folder_network", retryable: false, meaning: "The matter \
        folder is on a network drive, where Codex's sandbox can neither read nor write, so Codex \
        is not started there.";
    MatterFolderNoPermissions = "matter.folder_no_permissions", retryable: false,
        meaning: "The matter folder is on a drive that keeps no file permissions (FAT, exFAT), \
        which Codex's sandbox needs, so Codex is not started there.";
    TransportUnauthorized = "transport.unauthorized", retryable: false, meaning: "The client is \
        not allowed to make this call.";
    TransportTooLarge = "transport.too_large", retryable: false, meaning: "The message is larger \
        than the transport accepts.";
    SettingsLocked = "settings.locked", retryable: false, meaning: "The change names a setting \
        that the firm's policy sets, so the lawyer cannot change it; nothing was changed.";
    AgentInstallOffline = "agent.install_offline", retryable: true, meaning: "The agent's install \
        cannot reach its hosts: name resolution or connect failed with no route. The install \
        waits and tries again by itself.";
    AgentInstallBlocked = "agent.install_blocked", retryable: false, meaning: "A proxy or \
        firewall blocked the agent's download: a 403 or 407 from a proxy, a TLS failure, or a web \
        page where a file should be.";
    AgentInstallDiskFull = "agent.install_disk_full", retryable: true, meaning: "Too little free \
        space for the agent's install; a retry succeeds once space is freed.";
    AgentInstallFailed = "agent.install_failed", retryable: true, meaning: "The agent's install \
        failed for another reason, after three tries.";
    SigninCancelled = "signin.cancelled", retryable: false, meaning: "The lawyer cancelled the \
        sign-in, in Clauseo or on the vendor's page.";
    SigninLinkExpired = "signin.link_expired", retryable: true, meaning: "The sign-in link or \
        device code expired before the sign-in finished; a new sign-in gets a new one.";
    SigninNoPlan = "signin.no_plan", retryable: false, meaning: "The account has no plan the \
        agent accepts.";
    SigninDeviceCodeOff = "signin.device_code_off", retryable: false, meaning: "Device-code \
        sign-in is turned off for the account.";
    SigninPortBusy = "signin.port_busy", retryable: true, meaning: "Another program holds the \
        port the agent's sign-in listens on (Codex's 1455).";
    SigninFailed = "signin.failed", retryable: true, meaning: "The sign-in ended before signing \
        in, for any other reason.";
    SigninPolicyUnreadable = "signin.policy_unreadable", retryable: false, meaning: "The firm's \
        policy cannot be read and once set this agent's firm plan, so sign-in and launch are \
        refused rather than sent to a plan login.";
    AgentGuardMissing = "agent.guard_missing", retryable: false, meaning: "A Claude launch that \
        must ask before every change could not set up the guard that makes it ask, so it was \
        not started.";
    UpdateFailed = "update.failed", retryable: true, meaning: "The update's check, download, \
        signature check or install failed; the version running keeps running, and a later check \
        tries again.";
    SettingsRefused = "settings.refused", retryable: false, meaning: "The change breaks a \
        settings rule, such as an agent entry that is not Clauseo's own install, a secret-shaped \
        variable or an address that is not https; the message names the key, never the value, \
        and nothing was changed.";
    SettingsKeyStored = "settings.key_stored", retryable: false, meaning: "A key is already \
        stored for this agent, so this window may not replace it; it is changed or removed in \
        Settings. No key text is in the message.";
    SigninLoginShared = "signin.login_shared", retryable: false, meaning: "A Codex login, logout \
        or authenticate was refused while Clauseo's Codex home shares the lawyer's ChatGPT login \
        file; the remedy is in the app, \"Stop using it here\".";
    SigninFirmManaged = "signin.firm_managed", retryable: false, meaning: "Sign-in is \
        refused: a firm endpoint is set by policy.";
    SigninKeyInUse = "signin.key_in_use", retryable: false, meaning: "Sign-in is refused: an \
        API key is stored.";
    SigninEndpointInUse = "signin.endpoint_in_use", retryable: false, meaning: "Sign-in is \
        refused: an endpoint is set in Settings.";
    MatterFolderTooDeep = "matter.folder_too_deep", retryable: false, meaning: "The matter \
        folder's path is too long for Windows to start programs in it: move the folder closer \
        to the top of its drive.";
    SigninAgentsStillRunning = "signin.agents_still_running", retryable: true, meaning: "An \
        agent was still running after Clauseo waited for it to stop, so the login was not \
        changed and nothing was removed; trying again once it has stopped can succeed.";
    AgentKeysLoading = "agent.keys_loading", retryable: true, meaning: "Clauseo has not \
        finished loading the stored API keys; no agent was started; trying again in a moment can \
        succeed.";
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
    /// The matter the failure is about, where the code says so: for
    /// `matter.folder_is_matter`, the matter on Clauseo's list that has the folder. Missing
    /// otherwise, and when that matter is not on the list (forgotten, or from another install).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    matter: Option<MatterId>,
    /// The matter's files the failure is about: for `matter.file_locked`, the files held open,
    /// sorted. Missing when it names none. For `matter.file_locked` the files are here, not in
    /// `message`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    files: Vec<MatterPath>,
}

impl HostError {
    /// A failure with `code`, its `retryable` taken from [`ErrorCode::is_retryable`].
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            retryable: code.is_retryable(),
            message: message.into(),
            matter: None,
            files: Vec::new(),
        }
    }

    /// The same failure, naming `matter` (see [`HostError::matter`]).
    #[must_use]
    pub const fn with_matter(mut self, matter: MatterId) -> Self {
        self.matter = Some(matter);
        self
    }

    /// The matter the failure is about, for the codes that name one
    /// (`matter.folder_is_matter`).
    pub const fn matter(&self) -> Option<MatterId> {
        self.matter
    }

    /// The same failure, naming the matter's `files` it is about (see [`HostError::files`]).
    #[must_use]
    pub fn with_files(mut self, files: Vec<MatterPath>) -> Self {
        self.files = files;
        self
    }

    /// The matter's files the failure is about: for `matter.file_locked`, the files held open.
    /// Empty when it names none.
    pub fn files(&self) -> &[MatterPath] {
        &self.files
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
    #[serde(default)]
    matter: Option<MatterId>,
    #[serde(default)]
    files: Vec<MatterPath>,
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
                matter: wire.matter,
                files: wire.files,
            },
            None => Self {
                code: ErrorCode::Unknown,
                message: format!("{raw}: {}", wire.message),
                retryable: wire.retryable,
                matter: wire.matter,
                files: wire.files,
            },
        })
    }
}
