//! What the chooser's commands take and answer, as the bindings show them to the page.

use serde::{Deserialize, Serialize};

use crate::picked::PickedFolder;

/// What `pick_folder_or_choose` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum PickAnswer {
    /// The native dialog picked a folder.
    Picked { folder: PickedFolder },
    /// The native dialog was cancelled.
    Cancelled,
    /// The native dialog would stall: choose in the page, in this session.
    Choose { start: ChooserStart },
}

/// A chooser session, open at once; where it may start comes from `chooser_starts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct ChooserStart {
    /// The session, for `chooser_starts`, `chooser_list`, `chooser_choose` and
    /// `chooser_cancel`.
    pub(crate) chooser: u32,
}

/// A place to start from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct StartPoint {
    /// What `Go::Start` names it by.
    pub(crate) key: u8,
    /// What it is.
    pub(crate) kind: StartKind,
    /// How it shows: `C:` for a drive, the `picked::display` rule otherwise, and empty for home.
    pub(crate) display: String,
}

/// What a start point is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum StartKind {
    /// A fixed or removable drive.
    Drive,
    /// The home folder.
    Home,
    /// The Desktop.
    Desktop,
    /// The folder of the last pick.
    LastPick,
}

/// Where `chooser_list` goes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Go {
    /// A start point, by its key.
    Start { key: u8 },
    /// A folder of the last listing, by its index.
    Into { index: u32 },
    /// The folder above, stopping at a drive or share root.
    Up,
    /// A path the lawyer typed (`place.rs` says which it takes).
    Typed { path: String },
    /// The folder shown, read again.
    Again,
}

/// A folder as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct Listing {
    /// The folder shown, by the `picked::display` rule; empty for home.
    pub(crate) display: String,
    /// Its parts from the root (`C:`, `\\server\share`), or from home (shown as an empty one).
    pub(crate) crumbs: Vec<String>,
    /// What reading it gave.
    pub(crate) state: ListState,
}

/// What reading a folder gave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ListState {
    /// Its folders, and whether there were more than shown.
    Folders { items: Vec<Item>, more: bool },
    /// It did not answer in time.
    NoAnswer,
    /// It is not there.
    NotFound,
    /// The chooser does not open it; the folder shown stays.
    NotAllowed,
}

/// A folder in a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct Item {
    /// Its name.
    pub(crate) name: String,
    /// Whether Clauseo refuses it (`refused_folders`); it does not open.
    pub(crate) refused: bool,
}
