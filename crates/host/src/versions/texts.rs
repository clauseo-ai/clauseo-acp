//! What the agent reads: the tools' descriptions in `tools/list`, and each refusal, one plain
//! sentence it can repeat to the lawyer. None of it reaches the lawyer's screen from the host.

/// `list_versions`' description.
pub(super) const LIST_DESCRIPTION: &str = "Read-only. Lists the versions Clauseo kept of this \
matter's folder, newest first, as text: a line with how many, a next_before line, then one block \
per version with its id, at (when it was taken), kind (import, before_turn, after_turn, \
outside_edits, undo, restore, revisions), by (agent, lawyer, outside_clauseo), \
this_conversation (yes or no), restored_from for a restore, files_total, and at most 25 of the \
files it changed, one per line as change: path (a rename adds a from: line). Covers only this \
matter. Files that were deleted are listed with the version that deleted them. 20 versions by \
default, 50 at most; pass next_before as before for older ones. Changes nothing.";

/// `read_version`'s description.
pub(super) const READ_DESCRIPTION: &str = "Read-only. Returns one file as an earlier version of \
this matter held it, as text: a header line (version, at, format, offset, next_offset or end, \
total_bytes, path), a blank line, then the file's text, 64 KiB a page from offset; pass \
next_offset as offset for the next page. Text files saved as UTF-8 (up to 16 MiB) come back as \
they are; Word (.docx) files (up to 25 MiB) as the text Clauseo's Word tool shows. PDFs and \
other files are not read. Covers only this matter. Earlier versions of a file the matter has held \
ever since can be read, including text the lawyer later removed from it. A file that was deleted \
is listed by list_versions, but its earlier versions cannot be read, even when a file with its \
name is there again, unless the lawyer brought it back with Undo or from Versions. Changes \
nothing.";

/// Why a call was refused; [`Refusal::text`] is what the agent reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    /// The version is not on this matter's timeline (made up, or another matter's).
    UnknownVersion,
    /// The path is not in that version's file list.
    NotInVersion,
    /// The file left the matter after that version (or is not in the folder now), and the
    /// lawyer did not bring it back with Undo or Restore.
    Deleted,
    /// Neither text nor a Word file.
    Unsupported,
    /// The turn's calls are spent.
    TooManyCalls,
    /// The turn's stetdoc runs are spent.
    TooManyDocx,
    /// The matter's thread did not take the read or did not answer in time.
    Unavailable,
    /// The matter is not open on its thread yet; a read never opens it.
    NotOpen,
    /// A text file over [`super::MAX_TEXT_BYTES`].
    TooLarge,
    /// A Word file over [`super::MAX_DOCX_BYTES`].
    WordTooLarge,
    /// No stetdoc for a Word file.
    NoWordTool,
    /// The check that the file is in the matter now failed.
    NotChecked,
    /// The history did not read.
    NotRead,
    /// stetdoc, or the Word file's copy for it, failed.
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
    pub(super) const ALL: [Self; 20] = [
        Self::UnknownVersion,
        Self::NotInVersion,
        Self::Deleted,
        Self::Unsupported,
        Self::TooManyCalls,
        Self::TooManyDocx,
        Self::Unavailable,
        Self::NotOpen,
        Self::TooLarge,
        Self::WordTooLarge,
        Self::NoWordTool,
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
                "That file was deleted from this matter, so I can't read its earlier versions, \
                 even if a file with its name is there now. The lawyer can bring it back from \
                 Versions."
            }
            Self::Unsupported => {
                "I can only read text files saved as UTF-8, and Word (.docx) files, from earlier \
                 versions; that file is neither."
            }
            Self::TooManyCalls => {
                "The version history has been looked at 100 times since the lawyer's last \
                 message, which is the limit. It can be looked at again after their next message."
            }
            Self::TooManyDocx => {
                "Word files from earlier versions have been read 20 times since the lawyer's last \
                 message, which is the limit. More can be read after their next message."
            }
            Self::Unavailable => {
                "The version history is not available right now. Try again in a moment."
            }
            Self::NotOpen => {
                "The version history can't be read until Clauseo has opened this matter. Try \
                 again in the next turn."
            }
            Self::TooLarge => {
                "That file is over 16 MiB, too large to read from an earlier version."
            }
            Self::WordTooLarge => {
                "That Word file is over 25 MiB, too large to read from an earlier version."
            }
            Self::NoWordTool => {
                "Word files can't be read from earlier versions here, because Clauseo's Word tool \
                 is missing."
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
            Self::NotOpen => "not_open",
            Self::TooLarge => "too_large",
            Self::NoWordTool => "no_word_tool",
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
