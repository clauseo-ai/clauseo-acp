//! `HostError`, the failure every host call returns: split from `error_code.rs` for its size.

use serde::{Deserialize, Deserializer, Serialize};

use super::ErrorCode;
use crate::serde_util::BorrowedStr;
use crate::{Error, MatterId, MatterPath, PolicySetting};

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
    /// `matter.folder_is_matter`, `matter.folder_inside_matter` and
    /// `matter.folder_holds_matter`, the matter on Clauseo's list that has the folder. Missing
    /// otherwise, and when that matter is not on the list (forgotten, or from another install).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    matter: Option<MatterId>,
    /// The matter's files the failure is about: for `matter.file_locked`, the files held open,
    /// sorted. Missing when it names none. For `matter.file_locked` the files are here, not in
    /// `message`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    files: Vec<MatterPath>,
    /// The policy setting that refused a Claude launch, for `agent.user_policy` and
    /// `agent.firm_policy_weakens` when the host knows it; missing otherwise. The turn's
    /// `TurnFailed` carries it on.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    setting: Option<PolicySetting>,
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
            setting: None,
        }
    }

    /// The same failure, naming `matter` (see [`HostError::matter`]).
    #[must_use]
    pub const fn with_matter(mut self, matter: MatterId) -> Self {
        self.matter = Some(matter);
        self
    }

    /// The matter the failure is about, for the codes that name one
    /// (`matter.folder_is_matter`, `matter.folder_inside_matter`, `matter.folder_holds_matter`).
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

    /// The same failure, naming the policy `setting` that refused the launch (see
    /// [`HostError::setting`]).
    #[must_use]
    pub const fn with_setting(mut self, setting: PolicySetting) -> Self {
        self.setting = Some(setting);
        self
    }

    /// The policy setting that refused a Claude launch, when the failure names one.
    pub const fn setting(&self) -> Option<PolicySetting> {
        self.setting
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
    /// not parse or a limit `validate` checks. A prompt's text over its limit
    /// ([`Measure::PromptText`](crate::Measure::PromptText)) is `prompt.too_long` instead, read
    /// from the typed measure, never from the message. Only for a command's input; a value read
    /// back from the store that does not parse is `store.corrupt`, which the store builds with
    /// [`HostError::new`].
    #[expect(
        clippy::needless_pass_by_value,
        reason = "taken by value so a caller writes `.map_err(HostError::invalid_command)`"
    )]
    pub fn invalid_command(error: Error) -> Self {
        Self::new(Self::invalid_command_code(&error), error.to_string())
    }

    /// The code [`HostError::invalid_command`] gives `error`: `prompt.too_long` for a prompt's
    /// text over its limit (`Error::TooLarge` of
    /// [`Measure::PromptText`](crate::Measure::PromptText)), `command.invalid` for anything else.
    /// The one place that choice is made, so a caller that logs the code logs what the wire says.
    pub const fn invalid_command_code(error: &Error) -> ErrorCode {
        if matches!(
            error,
            Error::TooLarge {
                what: crate::Measure::PromptText,
                ..
            }
        ) {
            ErrorCode::PromptTooLong
        } else {
            ErrorCode::CommandInvalid
        }
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
    #[serde(default)]
    setting: Option<PolicySetting>,
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
                setting: wire.setting,
            },
            None => Self {
                code: ErrorCode::Unknown,
                message: format!("{raw}: {}", wire.message),
                retryable: wire.retryable,
                matter: wire.matter,
                files: wire.files,
                setting: wire.setting,
            },
        })
    }
}
