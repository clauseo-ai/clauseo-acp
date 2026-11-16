//! What the agent reads: the tools' descriptions in `tools/list`, and each refusal, one plain
//! sentence it can repeat to the lawyer. None of it reaches the lawyer's screen from the host.

/// `list_versions`' description.
pub(super) const LIST_DESCRIPTION: &str = "Read-only. Lists the versions Clauseo kept of this \
matter's folder, newest first: each version's id, when it was taken, what took it (import, \
before_turn, after_turn, outside_edits, undo, restore, revisions), who (agent, lawyer, \
outside_clauseo), whether this conversation took it, the version it restored from for a \
restore, and the files it changed (at most 25 names, with files_total). Covers only this matter. \
Files that were deleted are listed with the version that deleted them. 20 versions by default, \
50 at most; pass next_before as before for older ones. Changes nothing.";

/// `read_version`'s description.
pub(super) const READ_DESCRIPTION: &str = "Read-only. Returns one file as an earlier version of \
this matter held it, as text: 64 KiB a page from offset, with next_offset while more is left \
and total_bytes. Text files come back as they are; Word (.docx) files as the text Clauseo's Word \
tool shows. PDFs and other files are not read. Covers only this matter. Earlier versions of \
files the matter still holds can be read, including text the lawyer later removed from them; \
files the matter no longer holds are listed by list_versions but cannot be read. Changes \
nothing.";

/// Why a call was refused; [`Refusal::text`] is what the agent reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    /// The version is not on this matter's timeline (made up, or another matter's).
    UnknownVersion,
    /// The path is not in that version's file list.
    NotInVersion,
    /// The file is not in the matter now, and no exact rename leads to one that is.
    Deleted,
    /// Neither text nor a Word file.
    Unsupported,
    /// The turn's calls are spent.
    TooManyCalls,
    /// The turn's stetdoc runs are spent.
    TooManyDocx,
    /// The matter's thread did not answer, or the matter did not open.
    Unavailable,
    /// The check that the file is in the matter now failed.
    NotChecked,
    /// The history did not read.
    NotRead,
    /// No stetdoc, or it failed.
    WordFailed,
    /// `version` is missing or not a string.
    BadVersion,
    /// `path` is missing or not a string.
    BadPath,
    /// `offset` is not a whole number.
    BadOffset,
    /// `offset` is past the file's end.
    PastEnd,
    /// `limit` is not a whole number from 1 to 50.
    BadLimit,
    /// `before` is not a string.
    BadBefore,
}

impl Refusal {
    /// Every refusal (tests).
    #[cfg(test)]
    pub(super) const ALL: [Self; 16] = [
        Self::UnknownVersion,
        Self::NotInVersion,
        Self::Deleted,
        Self::Unsupported,
        Self::TooManyCalls,
        Self::TooManyDocx,
        Self::Unavailable,
        Self::NotChecked,
        Self::NotRead,
        Self::WordFailed,
        Self::BadVersion,
        Self::BadPath,
        Self::BadOffset,
        Self::PastEnd,
        Self::BadLimit,
        Self::BadBefore,
    ];

    /// The sentence the agent reads.
    pub(super) const fn text(self) -> &'static str {
        match self {
            Self::UnknownVersion => {
                "There is no version with that id in this matter's history. Use list_versions to \
                 see its versions."
            }
            Self::NotInVersion => {
                "That version has no file at that path. Give the path as list_versions shows it, \
                 relative to the matter's folder."
            }
            Self::Deleted => {
                "That file is no longer in this matter, so I can't read its earlier versions. The \
                 lawyer can bring it back from Versions."
            }
            Self::Unsupported => {
                "I can only read text files and Word (.docx) files from earlier versions, and \
                 that file is neither."
            }
            Self::TooManyCalls => {
                "The version history has been looked at 100 times in this turn, which is the \
                 limit. It can be looked at again in the next turn."
            }
            Self::TooManyDocx => {
                "Word files from earlier versions have been read 20 times in this turn, which is \
                 the limit. More can be read in the next turn."
            }
            Self::Unavailable => {
                "The version history is not available right now. Try again in a moment."
            }
            Self::NotChecked => {
                "I couldn't check whether that file is still in this matter, so I can't read its \
                 earlier versions right now."
            }
            Self::NotRead => "That version of the file could not be read from the history.",
            Self::WordFailed => "That Word file could not be turned into text.",
            Self::BadVersion => "Give version as a version id from list_versions.",
            Self::BadPath => {
                "Give path as a file path from list_versions, relative to the matter's folder."
            }
            Self::BadOffset => {
                "Give offset as a whole number of bytes, such as the next_offset of the page \
                 before."
            }
            Self::PastEnd => "That offset is past the end of the file.",
            Self::BadLimit => "Give limit as a whole number from 1 to 50.",
            Self::BadBefore => "Give before as a version id from list_versions.",
        }
    }

    /// Its name for the log.
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::UnknownVersion => "unknown_version",
            Self::NotInVersion => "not_in_version",
            Self::Deleted => "deleted",
            Self::Unsupported => "unsupported",
            Self::TooManyCalls => "too_many_calls",
            Self::TooManyDocx => "too_many_docx",
            Self::Unavailable => "unavailable",
            Self::NotChecked => "not_checked",
            Self::NotRead => "not_read",
            Self::WordFailed => "word_failed",
            Self::BadVersion => "bad_version",
            Self::BadPath => "bad_path",
            Self::BadOffset => "bad_offset",
            Self::PastEnd => "past_end",
            Self::BadLimit => "bad_limit",
            Self::BadBefore => "bad_before",
        }
    }
}
