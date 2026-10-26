//! A matter: create, import and open, and the operations on an open one. The work is in
//! `checkpoint.rs` and `undo.rs`; this file checks inputs and keeps the folder and history in
//! step.

mod json;

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use clauseo_protocol::{CheckpointId, CheckpointKind, ImportSummary, MatterId, MatterPath};

use crate::checkpoint::{Core, Spec};
use crate::history::{GixStore, HistoryStore};
use crate::stat::EntryState;
use crate::types::{
    Checkpoint, Imported, Marker, NewMatter, Progress, Settings, TimelineEntry, TurnMeta,
    UndoRequest, Undone,
};
use crate::{Error, refuse, scan, undo, write};

/// The git dir's name inside `<matters root>/<matter id>/`.
const GIT_DIR: &str = "history.git";

/// An open matter: its folder and its history. Writes take `&mut self`; the host keeps one
/// `Matter` per matter and runs it off its own thread.
#[derive(Debug)]
pub struct Matter {
    id: MatterId,
    folder: PathBuf,
    settings: Settings,
    core: Core,
}

impl Matter {
    /// Creates matter `new`: its folder (which must not exist; its parents are made), its
    /// `matter.json`, its git dir and a root `import` checkpoint of the empty folder.
    pub fn create(settings: Settings, new: NewMatter) -> Result<Self, Error> {
        refuse_marker(new.marker)?;
        let home = matter_home(&settings, new.id);
        if exists(&home)? {
            return Err(Error::HistoryExists { git_dir: home });
        }
        if exists(&new.folder)? {
            return Err(Error::FolderExists { folder: new.folder });
        }
        if let Some(parent) = new.folder.parent() {
            write::create_dir_all(parent).map_err(|err| Error::io("create", parent, err))?;
        }
        write::create_dir(&new.folder).map_err(|err| Error::io("create", &new.folder, err))?;
        let folder = new.folder.clone();
        let made = Self::make(settings, new, None, &mut |_| {});
        match made {
            Ok(Made::Done(matter, _)) => Ok(*matter),
            Ok(Made::Cancelled) => Err(Error::history("create", "cancelled without a cancel flag")),
            Err(err) => {
                undo_make(&home, &folder);
                if let Err(cleanup) = write::remove_dir(&folder) {
                    tracing::warn!(error = %cleanup, "the new matter folder was left behind");
                }
                Err(err)
            }
        }
    }

    /// Imports the existing folder `new.folder` as a matter: refuses a drive root or a listed
    /// folder (or an ancestor of one), writes `matter.json` and the git dir, and takes the root
    /// `import` checkpoint. `progress` gets files and bytes read; when `cancel` is set the git
    /// dir and `matter.json` are removed and the result is [`Imported::Cancelled`]. A failed
    /// import removes them too.
    pub fn import(
        settings: Settings,
        new: NewMatter,
        refused: &[PathBuf],
        progress: &mut dyn FnMut(Progress),
        cancel: &AtomicBool,
    ) -> Result<Imported, Error> {
        refuse_marker(new.marker)?;
        refuse::check_folder(&new.folder, refused)?;
        let home = matter_home(&settings, new.id);
        if exists(&home)? {
            return Err(Error::HistoryExists { git_dir: home });
        }
        let folder = new.folder.clone();
        match Self::make(settings, new, Some(cancel), progress) {
            Ok(Made::Done(matter, summary)) => {
                let warnings =
                    scan::warnings(&matter.folder, summary.bytes.get(), &matter.settings.limits);
                Ok(Imported::Done {
                    matter,
                    summary,
                    warnings,
                })
            }
            Ok(Made::Cancelled) => {
                undo_make(&home, &folder);
                Ok(Imported::Cancelled)
            }
            Err(err) => {
                undo_make(&home, &folder);
                Err(err)
            }
        }
    }

    /// Writes `matter.json` and the git dir, then the root checkpoint.
    fn make(
        settings: Settings,
        new: NewMatter,
        cancel: Option<&AtomicBool>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Made, Error> {
        let git_dir = matter_home(&settings, new.id).join(GIT_DIR);
        let history = GixStore::create(&git_dir, &new.folder)?;
        json::write(&new.folder, new.id, new.created_at)?;
        let mut core = Core::load(history)?;
        let Some(snapshot) = core.snapshot(&new.folder, &settings, cancel, progress)? else {
            return Ok(Made::Cancelled);
        };
        let spec = Spec {
            entry: TimelineEntry {
                id: new.checkpoint,
                kind: CheckpointKind::Import,
                thread: None,
                turn: None,
            },
            at: new.created_at,
            tool_paths: None,
        };
        let root = core.commit(snapshot, &spec)?;
        let summary = summary(&core, &root);
        let matter = Self {
            id: new.id,
            folder: new.folder,
            settings,
            core,
        };
        Ok(Made::Done(Box::new(matter), summary))
    }

    /// Opens matter `id` in `folder`: `matter.folder_missing` if the folder is gone or its
    /// `matter.json` is missing or names another matter. Rebuilds the timeline from the
    /// trailers and loads the stat cache (rebuilt if it does not match).
    pub fn open(settings: Settings, id: MatterId, folder: &Path) -> Result<Self, Error> {
        json::check(folder, id)?;
        let git_dir = matter_home(&settings, id).join(GIT_DIR);
        let history = GixStore::open(&git_dir, folder)?;
        let core = Core::load(history)?;
        if core.timeline.head().is_none() {
            return Err(Error::history(
                "open",
                "the matter's timeline has no checkpoint",
            ));
        }
        Ok(Self {
            id,
            folder: folder.to_owned(),
            settings,
            core,
        })
    }

    /// The matter's id.
    pub const fn id(&self) -> MatterId {
        self.id
    }

    /// The matter's folder.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// Every checkpoint, oldest first.
    pub fn timeline(&self) -> &[TimelineEntry] {
        &self.core.timeline.entries
    }

    /// The last checkpoint's id.
    pub fn last_checkpoint(&self) -> Option<CheckpointId> {
        self.core.timeline.head().map(|(id, _, _)| id)
    }

    /// The checkpoint before a turn: created only if the folder differs from the last
    /// checkpoint (then its changes are outside edits, which the host logs), otherwise the last
    /// checkpoint's id is reused.
    pub fn before_turn(&mut self, meta: &TurnMeta) -> Result<Checkpoint, Error> {
        self.turn_checkpoint(meta, CheckpointKind::BeforeTurn, None)
    }

    /// The checkpoint after a turn. `tool_paths` are the paths the turn's agent tool calls
    /// named (locations and diff paths); a changed file outside them is flagged
    /// `may_include_lawyer_edits`. With no change, the `before-turn` checkpoint is reused.
    pub fn after_turn(
        &mut self,
        meta: &TurnMeta,
        tool_paths: &BTreeSet<MatterPath>,
    ) -> Result<Checkpoint, Error> {
        self.turn_checkpoint(meta, CheckpointKind::AfterTurn, Some(tool_paths))
    }

    fn turn_checkpoint(
        &mut self,
        meta: &TurnMeta,
        kind: CheckpointKind,
        tool_paths: Option<&BTreeSet<MatterPath>>,
    ) -> Result<Checkpoint, Error> {
        json::check(&self.folder, self.id)?;
        let spec = Spec {
            entry: TimelineEntry {
                id: meta.checkpoint,
                kind,
                thread: Some(meta.thread),
                turn: Some(meta.turn),
            },
            at: meta.at,
            tool_paths,
        };
        self.core.take(&self.folder, &self.settings, &spec)
    }

    /// The files the step from `before` to `after` changed whose content now differs from
    /// `after` (edited since), sorted: the list for "Undo anyway" or "Keep current".
    pub fn edited_since(
        &mut self,
        before: CheckpointId,
        after: CheckpointId,
    ) -> Result<Vec<MatterPath>, Error> {
        json::check(&self.folder, self.id)?;
        undo::edited_since(&mut self.core, &self.folder, &self.settings, before, after)
    }

    /// Undoes the step `request` names, file by file. Checks every decision and every lock
    /// before writing anything; takes an `outside-edits` checkpoint first if the folder changed
    /// since the last checkpoint; then writes the before-step contents (temp file and rename),
    /// deletes files the step created, and takes an `undo` checkpoint.
    pub fn undo(&mut self, request: &UndoRequest<'_>) -> Result<Undone, Error> {
        json::check(&self.folder, self.id)?;
        undo::undo(&mut self.core, &self.folder, &self.settings, request)
    }
}

/// [`Matter::make`]'s result.
enum Made {
    Done(Box<Matter>, ImportSummary),
    Cancelled,
}

/// What the root checkpoint `root` versioned, from the cache it wrote.
fn summary(core: &Core, root: &Checkpoint) -> ImportSummary {
    let mut summary = ImportSummary::default();
    let mut bytes: u64 = 0;
    for entry in &core.cache.entries {
        match entry.state {
            EntryState::Read(_) => {
                summary.files = summary.files.saturating_add(1);
                bytes = bytes.saturating_add(u64::from(entry.stat.size));
            }
            EntryState::Unread(_) | EntryState::Skipped => {}
        }
    }
    summary.cloud_only = root
        .skipped
        .iter()
        .filter(|file| file.reason == clauseo_protocol::SkippedFileReason::CloudOnly)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    summary.bytes = clauseo_protocol::Count::clamped(bytes);
    summary
}

fn refuse_marker(marker: Marker) -> Result<(), Error> {
    match marker {
        Marker::None => Ok(()),
        Marker::EmptyRepository => Err(Error::MarkerNotBuilt),
    }
}

/// `<matters root>/<matter id>`.
fn matter_home(settings: &Settings, id: MatterId) -> PathBuf {
    settings.matters_root.join(id.to_string())
}

fn exists(path: &Path) -> Result<bool, Error> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(Error::io("stat", path, err)),
    }
}

/// Removes what [`Matter::make`] wrote: the matter's folder under the matters root and
/// `matter.json`. Failures are logged; the caller's result stands.
fn undo_make(home: &Path, folder: &Path) {
    if let Err(err) = write::remove_dir_all(home)
        && err.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(path = %home.display(), error = %err, "history folder left behind");
    }
    json::remove(folder);
}
