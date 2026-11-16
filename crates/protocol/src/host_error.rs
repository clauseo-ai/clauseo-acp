//! `HostError`, what every host call returns on failure, and the refusal of a file it can name.

use serde::{Deserialize, Serialize};

use crate::open_enum::open_unit_enum;
use crate::{Error, ErrorCode, MatterId, MatterPath};

mod wire;

open_unit_enum! {
    /// Why the folder refused a change to a file (`matter.file_refused`); the file is as it was.
    pub enum RefusedBecause {
        /// The file's permissions refuse the change.
        Permissions,
        /// The file has a second name (a hard link), which changing it in place would change too.
        SecondName,
        /// The file could only be changed in place, and Clauseo could not first confirm its saved
        /// copy of the file, so it left the file alone.
        SavedCopyUnconfirmed,
    }
    test mod refused_because;
}

/// The most files a [`HostError`] names ([`HostError::with_files`] keeps the first ones): the UI
/// names the first, and lists no more than a dialog can show.
pub const MAX_ERROR_FILES: usize = 50;

/// For `matter.file_refused`: why the files were refused, how many were, and how many files the
/// action wrote, deleted or meant to, so the UI can tell an action on that file alone (`of` is
/// 1) from one that did everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FileRefusal {
    /// Why every one of the refused files was refused; missing when they were refused for
    /// different reasons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub because: Option<RefusedBecause>,
    /// How many files were refused: [`HostError::files`] names at most [`MAX_ERROR_FILES`].
    pub refused: u32,
    /// How many files the action wrote, deleted or meant to, the refused ones included.
    pub of: u32,
}

/// For `matter.file_write_failed`: whether the file was put back as it was, and how many files
/// the action wrote, deleted or meant to (1: nothing else changed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct WriteFailure {
    /// The version from before the write was written back whole: the file is as it was.
    /// Otherwise it may be damaged.
    pub put_back: bool,
    /// How many files the action wrote, deleted or meant to, this one included.
    pub of: u32,
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
    /// `matter.folder_is_matter`, `matter.folder_inside_matter` and
    /// `matter.folder_holds_matter`, the matter on Clauseo's list that has the folder. Missing
    /// otherwise, and when that matter is not on the list (forgotten, or from another install).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    matter: Option<MatterId>,
    /// The matter's files the failure is about: for `matter.file_locked`, the files held open,
    /// sorted; for `matter.file_folder_clash`, the entry in the way (the file's own path when a
    /// folder now has it, else its folder that is now a file), then the file being restored; for
    /// `matter.file_refused`, the refused files, sorted; for `matter.file_write_failed`, the
    /// file. At most [`MAX_ERROR_FILES`]. Missing when it names none. For each the files are
    /// here, not in `message`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    files: Vec<MatterPath>,
    /// For `matter.file_refused`: why the files were refused, how many were, and how many files
    /// the action wrote, deleted or meant to. Missing otherwise.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    refusal: Option<FileRefusal>,
    /// For `matter.file_write_failed`: whether the file was put back as it was, and how many
    /// files the action wrote, deleted or meant to. Missing otherwise.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    write_failure: Option<WriteFailure>,
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
            refusal: None,
            write_failure: None,
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

    /// The same failure, naming the matter's `files` it is about (see [`HostError::files`]): the
    /// first [`MAX_ERROR_FILES`] of them.
    #[must_use]
    pub fn with_files(mut self, mut files: Vec<MatterPath>) -> Self {
        files.truncate(MAX_ERROR_FILES);
        self.files = files;
        self
    }

    /// The matter's files the failure is about: for `matter.file_locked`, the files held open;
    /// for `matter.file_folder_clash`, the entry in the way, then the file being restored; for
    /// `matter.file_refused`, the refused files; for `matter.file_write_failed`, the file. Empty
    /// when it names none.
    pub fn files(&self) -> &[MatterPath] {
        &self.files
    }

    /// The same failure, with why its files were refused (see [`HostError::refusal`]).
    #[must_use]
    pub const fn with_refusal(mut self, refusal: FileRefusal) -> Self {
        self.refusal = Some(refusal);
        self
    }

    /// For `matter.file_refused`: why the files were refused, how many were, and how many files
    /// the action wrote, deleted or meant to.
    pub const fn refusal(&self) -> Option<FileRefusal> {
        self.refusal
    }

    /// The same failure, with how the file's write failed (see [`HostError::write_failure`]).
    #[must_use]
    pub const fn with_write_failure(mut self, failure: WriteFailure) -> Self {
        self.write_failure = Some(failure);
        self
    }

    /// For `matter.file_write_failed`: whether the file was put back as it was, and how many
    /// files the action wrote, deleted or meant to.
    pub const fn write_failure(&self) -> Option<WriteFailure> {
        self.write_failure
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
