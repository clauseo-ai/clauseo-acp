//! The public input and output types: what the host passes in and what it gets back.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_protocol::{
    CheckpointId, CheckpointKind, FileChange, ImportSummary, MatterId, MatterPath, SkippedFile,
    ThreadId, Timestamp, TurnId,
};

use crate::{Matter, PlaceholderProbe};

/// The per-file size cap: 512 MB. A larger file is skipped (`OverSizeCap`), never read.
pub const FILE_CAP_BYTES: u64 = 512_000_000;

/// The folder size above which an import warns: 20 GB. A warning only, never a refusal.
pub const IMPORT_WARN_BYTES: u64 = 20_000_000_000;

/// How many of the largest files a [`ScanReport`] names.
pub const LARGEST_FILES: usize = 10;

/// What a new or imported matter puts in its folder besides `.clauseo/matter.json`. Chosen per
/// create or import; the shadow repository is outside the folder either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// Nothing.
    None,
    /// An empty, non-bare git repository in the folder when it has no `.git`: no commits, no
    /// remote, unrelated to the shadow repository. Never a `gitdir:` file. Not built yet:
    /// [`Matter::create`] and [`Matter::import`] refuse it with `command.invalid`.
    EmptyRepository,
}

/// The marker the host uses unless it has a reason not to. `None` until the Codex-without-git
/// verdict says otherwise; that verdict changes this one constant.
pub const DEFAULT_MARKER: Marker = Marker::None;

/// How many versions a matter keeps. Passed at open, so a later policy is a new variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Retention {
    /// Every version, for ever: no pruning, squashing, gc or repack.
    KeepAll,
}

/// Size limits. The host passes [`Limits::DEFAULT`] unless a test needs smaller ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// A file larger than this is skipped and never read. Values of 4 GiB and above act as
    /// `u32::MAX` bytes: the stat cache stores sizes in 32 bits, as git's index does.
    pub file_cap_bytes: u64,
    /// An import of more bytes than this warns ([`ImportWarning::OverThreshold`]).
    pub import_warn_bytes: u64,
}

impl Limits {
    /// [`FILE_CAP_BYTES`] and [`IMPORT_WARN_BYTES`].
    pub const DEFAULT: Self = Self {
        file_cap_bytes: FILE_CAP_BYTES,
        import_warn_bytes: IMPORT_WARN_BYTES,
    };
}

/// What every matter operation needs from the host.
#[derive(Clone)]
pub struct Settings {
    /// The folder holding one folder per matter (`<root>/<matter id>/history.git`). On Windows
    /// it is under `%LOCALAPPDATA%`, never `Roaming`.
    pub matters_root: PathBuf,
    /// The size limits.
    pub limits: Limits,
    /// How many versions to keep.
    pub retention: Retention,
    /// Tells cloud-only placeholders apart; [`crate::OsProbe`] outside tests.
    pub probe: Arc<dyn PlaceholderProbe + Send + Sync>,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("matters_root", &self.matters_root)
            .field("limits", &self.limits)
            .field("retention", &self.retention)
            .finish_non_exhaustive()
    }
}

/// A matter to create or import. The host mints the ids and passes the time.
#[derive(Debug, Clone)]
pub struct NewMatter {
    /// The matter's id.
    pub id: MatterId,
    /// Its folder, absolute.
    pub folder: PathBuf,
    /// When it was made; written to `matter.json` and used as the root checkpoint's time.
    pub created_at: Timestamp,
    /// What to put in the folder besides `matter.json`.
    pub marker: Marker,
    /// The id of the root `import` checkpoint.
    pub checkpoint: CheckpointId,
}

/// The ids and time of a turn's checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnMeta {
    /// The id the checkpoint gets if one is created.
    pub checkpoint: CheckpointId,
    /// The thread the turn is in.
    pub thread: ThreadId,
    /// The turn.
    pub turn: TurnId,
    /// The commit time.
    pub at: Timestamp,
}

/// Whether a checkpoint made a commit or reused the last one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointOutcome {
    /// A new commit with this id.
    Created(CheckpointId),
    /// The folder equals the last checkpoint; this is its id.
    Reused(CheckpointId),
}

impl CheckpointOutcome {
    /// The checkpoint id, created or reused.
    pub const fn id(self) -> CheckpointId {
        match self {
            Self::Created(id) | Self::Reused(id) => id,
        }
    }
}

/// A checkpoint's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// The checkpoint, created or reused.
    pub outcome: CheckpointOutcome,
    /// The files changed against the previous checkpoint, sorted by path; empty when reused.
    pub changes: Vec<FileChange>,
    /// Files not versioned that are new or whose size or modification time changed since the
    /// last checkpoint, sorted by path.
    pub skipped: Vec<SkippedFile>,
}

/// An undo of one step: the changes between two checkpoints, file by file.
#[derive(Debug, Clone, Copy)]
pub struct UndoRequest<'a> {
    /// The checkpoint before the step.
    pub before: CheckpointId,
    /// The checkpoint after the step. An `undo` checkpoint here undoes that undo.
    pub after: CheckpointId,
    /// One decision for every path the step changed (a renamed file by its new path), and no
    /// other path; otherwise the undo fails with `command.invalid` before writing anything.
    pub decisions: &'a [clauseo_protocol::FileDecision],
    /// The thread the step was in, for the undo checkpoint's trailers.
    pub thread: Option<ThreadId>,
    /// The step's turn, for the undo checkpoint's trailers.
    pub turn: Option<TurnId>,
    /// The id of the `outside-edits` checkpoint, used only if the folder changed since the last
    /// checkpoint.
    pub outside_edits: CheckpointId,
    /// The id of the `undo` checkpoint.
    pub checkpoint: CheckpointId,
    /// The commit time of both.
    pub at: Timestamp,
}

/// What an undo did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undone {
    /// The checkpoint of unrecorded edits taken before writing, if the folder had changed.
    pub outside_edits: Option<Checkpoint>,
    /// The `undo` checkpoint (reused if the folder came out as the last checkpoint).
    pub checkpoint: Checkpoint,
    /// The files put back as they were before the step (written or deleted), sorted.
    pub restored: Vec<MatterPath>,
    /// The files left as they are, sorted: those decided `Keep`, and those that are now
    /// cloud-only, over the cap or behind a symlink.
    pub kept: Vec<MatterPath>,
    /// The undone files that had been edited since the step ("Undo anyway"), sorted. Their
    /// edits are in the `outside_edits` checkpoint or an earlier one.
    pub edited_since: Vec<MatterPath>,
}

/// One checkpoint on the timeline, from its commit's trailers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineEntry {
    /// The checkpoint's id.
    pub id: CheckpointId,
    /// Why it was taken; `Unknown` for a kind from a newer Clauseo.
    pub kind: CheckpointKind,
    /// The thread, for a checkpoint taken for a turn or an undo.
    pub thread: Option<ThreadId>,
    /// The turn, likewise.
    pub turn: Option<TurnId>,
}

/// How far a scan or an import has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Files seen so far.
    pub files: u64,
    /// Bytes seen (scan) or read (import) so far.
    pub bytes: u64,
}

/// One of the largest files of a scanned folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LargeFile {
    /// The file, relative to the folder.
    pub path: MatterPath,
    /// Its size in bytes.
    pub bytes: u64,
}

/// Something worth telling the lawyer before an import; never a refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportWarning {
    /// The folder is on a network share (a UNC path), so checkpoints read over the network.
    UncShare,
    /// The files to version add up to more than [`Limits::import_warn_bytes`].
    OverThreshold {
        /// Their total size.
        bytes: u64,
    },
}

/// What importing a folder would take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanReport {
    /// Files that would be versioned.
    pub files: u64,
    /// Their total size in bytes.
    pub bytes: u64,
    /// Cloud-only placeholders, which would be skipped and never read.
    pub cloud_only: u64,
    /// Files over the size cap, which would be skipped.
    pub over_cap: u64,
    /// The largest files that are not cloud-only (over-cap ones included), largest first, at
    /// most [`LARGEST_FILES`]; ties sorted by path.
    pub largest: Vec<LargeFile>,
    /// Warnings, in the order of [`ImportWarning`]'s variants.
    pub warnings: Vec<ImportWarning>,
}

/// A scan's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scanned {
    /// It finished.
    Done(ScanReport),
    /// The cancel flag was set.
    Cancelled,
}

/// An import's result.
#[derive(Debug)]
pub enum Imported {
    /// The matter exists, with its root `import` checkpoint.
    Done {
        /// The matter.
        matter: Box<Matter>,
        /// What the root checkpoint versioned.
        summary: ImportSummary,
        /// The same warnings a scan gives.
        warnings: Vec<ImportWarning>,
    },
    /// The cancel flag was set; the git dir and `matter.json` were removed.
    Cancelled,
}
