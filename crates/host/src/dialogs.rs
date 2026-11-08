//! The native dialogs seam (R18): the questions only the lawyer can answer about widening a
//! matter's reach, drawn by the OS outside the webview, so no page can answer them. The desktop
//! implements [`NativeDialogs`] (`crates/desktop/src/dialogs/ask.rs`) and passes it with
//! `HostConfig::with_dialogs`; tests answer from a script ([`FakeDialogs`]).
//!
//! Contract:
//! - Every call blocks until the lawyer answers, so the host makes it off its actor.
//! - Escape, the close button, a dialog that fails or never shows: [`DialogAnswer::Refuse`],
//!   and a picker gives `None`. Nothing but a press of an allowing button allows. (macOS'
//!   alert can't tell Escape from Don't allow, so there Escape honours the Remember checkbox.)
//! - The dialog says only what the host passes; nothing the agent wrote reaches it.
//! - No path, host or matter name reaches a log: `Dialog`'s and `Pick`'s `Debug` print only
//!   their kind and flags.
//! - No host logic asks yet (R18 PR 3 does).

use std::path::PathBuf;

/// Draws the native dialogs. The desktop's draws them over the main window, in the app's
/// language.
pub trait NativeDialogs: Send + Sync {
    /// Asks the lawyer about one widening. Blocking; the host calls it off the actor.
    fn ask(&self, dialog: Dialog) -> DialogAnswer;
    /// A native picker opening at `pick.start`; `None` when the lawyer cancels.
    fn pick(&self, pick: Pick) -> Option<PathBuf>;
}

/// One question about widening a matter's reach. Its `Debug` leaves out every path, host and
/// name.
#[derive(Clone, PartialEq, Eq)]
pub enum Dialog {
    /// A refused path: a folder, or the exact file when it is another matter's.
    Folder {
        /// The name of the matter being worked on.
        matter_name: String,
        /// The folder or file asked for.
        path: PathBuf,
        /// Whether `path` is a file.
        is_file: bool,
        /// The name of the matter `path` belongs to, when it is another one's.
        other_matter: Option<String>,
    },
    /// A refused site.
    Site {
        /// The site's host name.
        host: String,
        /// Whether the site shares its server with others, so allowing it may reach them too.
        shared_hosting: bool,
    },
    /// Switching protection off for a matter.
    SwitchOff {
        /// The matter's name.
        matter_name: String,
    },
    /// Confirming a path the lawyer picked ([`NativeDialogs::pick`]) before it is allowed.
    ConfirmPick {
        /// The name of the matter being worked on.
        matter_name: String,
        /// The folder or file picked.
        path: PathBuf,
        /// Whether `path` is a file.
        is_file: bool,
        /// The name of the matter `path` belongs to, when it is another one's.
        other_matter: Option<String>,
    },
}

/// The lawyer's answer, one to one with the protocol's `EscalationAnswer` (R18 PR 1); the host
/// maps between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DialogAnswer {
    /// Allow this time only.
    AllowOnce,
    /// Allow, and remember it for the matter.
    AllowForMatter,
    /// Refuse this time; also Escape, the close button and a dialog that failed.
    Refuse,
    /// Refuse, and remember it for the matter.
    RefuseForMatter,
}

/// A native picker's request. Its `Debug` leaves out the path and the name.
#[derive(Clone, PartialEq, Eq)]
pub struct Pick {
    /// Where the picker opens.
    pub start: PathBuf,
    /// Whether the lawyer picks a file (`true`) or a folder (`false`).
    pub files_too: bool,
    /// The name of the matter the pick is for, which the picker's title names.
    pub matter_name: String,
}

/// The kind and flags only: no path, host or matter name.
impl std::fmt::Debug for Dialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Folder {
                is_file,
                other_matter,
                ..
            } => f
                .debug_struct("Folder")
                .field("is_file", is_file)
                .field("other_matter", &other_matter.is_some())
                .finish_non_exhaustive(),
            Self::Site { shared_hosting, .. } => f
                .debug_struct("Site")
                .field("shared_hosting", shared_hosting)
                .finish_non_exhaustive(),
            Self::SwitchOff { .. } => f.debug_struct("SwitchOff").finish_non_exhaustive(),
            Self::ConfirmPick {
                is_file,
                other_matter,
                ..
            } => f
                .debug_struct("ConfirmPick")
                .field("is_file", is_file)
                .field("other_matter", &other_matter.is_some())
                .finish_non_exhaustive(),
        }
    }
}

/// The flag only: no path or matter name.
impl std::fmt::Debug for Pick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pick")
            .field("files_too", &self.files_too)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;

#[cfg(feature = "test-support")]
mod fake;
#[cfg(feature = "test-support")]
pub use fake::FakeDialogs;
