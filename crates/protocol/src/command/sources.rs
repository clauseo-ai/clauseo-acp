//! What a command names by value and not by id: where a new matter's folder comes from
//! (`MatterSource`), why account limits are refreshed (`RefreshWhy`) and a config option's new
//! value (`ConfigValue`). Split from `command.rs` for its length; `command` re-exports them.

use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::MAX_MATTER_NAME_BYTES;
use crate::{AgentChoiceId, FolderRefId};

/// Where a new matter's folder comes from. Folders are named by the ids the host issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum MatterSource {
    /// `folder` is the parent; the host creates `<parent>/<name>`. The name must then also work
    /// as a folder name on every system: at most [`MAX_MATTER_NAME_BYTES`], not `.` or `..`,
    /// no leading dot (Finder hides such a folder), none of `< > : " / \ | ? *`, no leading
    /// space, no trailing dot or space, and not a Windows device name
    /// ([`is_windows_reserved_name`](crate::is_windows_reserved_name)).
    NewFolder {
        /// The parent folder.
        folder: FolderRefId,
    },
    /// The matter is an existing folder, which keeps its own name: the matter's name is only a
    /// label and meets the title rules alone, so a folder called `Smith v Jones?` can keep it.
    /// Those rules refuse some folder names that exist: more than
    /// [`MAX_TITLE_CHARS`](crate::MAX_TITLE_CHARS) characters (a folder name may be 255 bytes), a
    /// control character (C0, DEL, C1), a line or paragraph separator (U+2028, U+2029), a bidi
    /// embedding, override or isolate (U+202A to U+202E, U+2066 to U+2069), and a name with no
    /// character that shows. The desktop app shortens or cleans such a name before it pre-fills the
    /// matter's name; the folder is not renamed.
    ExistingFolder {
        /// The folder.
        folder: FolderRefId,
    },
    /// The host creates `<default location>/<name>` itself, in the settings'
    /// `matters.default_location` (or, when that is not set, `Documents/Clauseo`), so no folder
    /// is picked. The name follows [`MatterSource::NewFolder`]'s folder-name rules.
    /// JSON: the plain string `"default_location"`.
    DefaultLocation,
}

/// Why account limits are refreshed, as `RefreshLimits` asks for it: a window got focus, or a
/// meter or the settings page opened (`Shown`), or the lawyer pressed "Try again" (`TryAgain`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum RefreshWhy {
    /// A window got focus, or a meter or the settings page opened. Reads only data older than
    /// 5 minutes.
    Shown,
    /// The lawyer asked again. Reads now, unless a read is in flight.
    TryAgain,
}

/// A config option's new value, matching the option's kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ConfigValue {
    /// The value of one of a select option's choices.
    Select(AgentChoiceId),
    /// A boolean option's value.
    Boolean(bool),
}
