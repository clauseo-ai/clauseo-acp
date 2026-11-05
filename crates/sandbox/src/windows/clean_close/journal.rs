//! The volume's change journal and the trees' file IDs, as a seam: [`Journal`]. On Windows it
//! is `NtfsJournal` (`ntfs.rs`); the tests use a fake, so the decision is tested on every OS.

use std::io;
use std::path::{Path, PathBuf};

/// An update sequence number: a record's place in the journal.
pub type Usn = i64;

/// What a launch reads about an own root, opened by its path as itself (never through a link).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootFacts {
    /// The volume's serial number.
    pub volume: u64,
    /// The root's 64-bit file reference, the journal's form; `None` when its 128-bit file ID
    /// doesn't fit in 64 bits (`ReFS`), which has no record this module reads.
    pub id: Option<u64>,
    /// Whether the volume's file system is NTFS.
    pub ntfs: bool,
    /// Whether the volume is on a removable drive (`DRIVE_REMOVABLE`, or a USB, 1394, SD or MMC
    /// bus): another computer can change it while it is away, unjournaled.
    pub removable: bool,
    /// The root's final path, in the form [`Journal::path_of`] gives.
    pub path: PathBuf,
}

/// A journal's identity and extent (`FSCTL_QUERY_USN_JOURNAL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalState {
    /// `UsnJournalID`: a new journal (deleted and made again) has a new one.
    pub id: u64,
    /// The lowest USN that can still be read: the larger of `FirstUsn` and `LowestValidUsn`.
    /// A cursor below it lost records (the journal wrapped).
    pub first: Usn,
    /// `NextUsn`: the next record's USN. A cursor above it means the volume went back.
    pub next: Usn,
}

/// One journal record, as the unprivileged read gives it (no name).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// Its USN.
    pub usn: Usn,
    /// The file or folder it is about (`FileReferenceNumber`).
    pub file: u64,
    /// The folder the name it is about was in (`ParentFileReferenceNumber`).
    pub parent: u64,
    /// `USN_REASON_*` bits.
    pub reason: u32,
    /// The item's attributes at the record (`FILE_ATTRIBUTE_*`).
    pub attributes: u32,
}

/// `FILE_ATTRIBUTE_DIRECTORY`.
pub const ATTRIBUTE_DIRECTORY: u32 = 0x10;
/// `FILE_ATTRIBUTE_REPARSE_POINT`.
pub const ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

impl Change {
    /// Whether the record is about a folder that isn't a reparse point (a junction, say).
    pub fn plain_folder(&self) -> bool {
        self.attributes & ATTRIBUTE_DIRECTORY != 0 && self.attributes & ATTRIBUTE_REPARSE_POINT == 0
    }
}

/// Why the journal couldn't be used. Each one means the full walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalError {
    /// The volume isn't NTFS.
    NotNtfs,
    /// The volume has no active journal (`ERROR_JOURNAL_NOT_ACTIVE`, or one being deleted).
    NoJournal,
    /// The call isn't supported here (an older Windows, another file system, another record
    /// version).
    Unsupported,
    /// Records past the cursor are gone (`ERROR_JOURNAL_ENTRY_DELETED`): it wrapped meanwhile.
    Lost,
    /// Another failure, of this kind.
    Failed(io::ErrorKind),
}

/// Whether a read goes on after a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Read on.
    More,
    /// Stop: enough is known.
    Stop,
}

/// One name below an own root, found at session end: its file reference, its folder's, and
/// whether the collection went into it (a folder that isn't a link).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listed {
    /// Its 64-bit file reference.
    pub id: u64,
    /// Its folder's.
    pub parent: u64,
    /// Whether it is a folder the collection listed (never a link).
    pub folder: bool,
}

/// A folder's current place, found by its file ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Its final path, in the form [`RootFacts::path`] has.
    pub path: PathBuf,
    /// Whether it is a junction, symbolic link or other name surrogate now.
    pub link: bool,
}

/// The volume calls the clean-close record needs, from the host at the lawyer's Medium token.
pub trait Journal {
    /// Opens the own root `path` as itself (never following a link) and reads its facts.
    fn root(&self, path: &Path) -> io::Result<RootFacts>;

    /// The journal of the volume holding `root`, through a handle on `root`.
    fn query(&self, root: &Path) -> Result<JournalState, JournalError>;

    /// Reads journal `journal`'s records of the volume holding `root` from `from` (a record
    /// boundary: a stored `NextUsn`) up to at least `to`, in order, handing each to `each`
    /// until it says [`Flow::Stop`]. `FSCTL_READ_UNPRIVILEGED_USN_JOURNAL` on Windows.
    fn read(
        &self,
        root: &Path,
        journal: u64,
        from: Usn,
        to: Usn,
        each: &mut dyn FnMut(&Change) -> Flow,
    ) -> Result<(), JournalError>;

    /// Where the folder with file reference `id` on volume `volume` (the one holding `root`)
    /// is now, opened by its ID; `None` when no file has that ID any more.
    fn path_of(&self, root: &Path, volume: u64, id: u64) -> io::Result<Option<Found>>;

    /// Every name below `root`, each folder opened relative to its parent's handle and never
    /// through a link, with its file reference and its folder's. Any folder that can't be
    /// opened or listed is an error: an incomplete set must never be recorded.
    fn tree(&self, root: &Path) -> io::Result<Vec<Listed>>;
}
