//! The owed forgets' two threads (R17 PR-E), never the actor's: the writer
//! (`clauseo-sandbox-owed`) keeps `<data_dir>/protection/owed_forgets.json`, and the walker
//! (`clauseo-sandbox-forget`) runs `SandboxSetup::forget_matter`, one matter at a time. A long
//! walk never holds up a write, so a forget made while another walks is still recorded at once.
//!
//! The file is `{"v":1,"matters":["<matter id>", ...]}`, written whole and atomically
//! (`write::replace_private_file`, as `blocklist/allowed.json` is). It names matter ids only. A
//! file that is missing, does not read, is over [`CAP`], or is of another version counts as no
//! forget owed (logged by its kind): the worst a damaged file does is leave a forget undone.
//!
//! Once the stop gave up on them (`abandoned`), neither thread writes or walks again: what is
//! still owed stays on disk for the next start's replay.

use std::collections::BTreeSet;
use std::io::{self, Read as _};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use clauseo_protocol::MatterId;
use clauseo_sandbox::{SandboxSetup, SetupError};
use serde::{Deserialize, Serialize};

/// The writer's thread.
pub(super) const WRITER: &str = "clauseo-sandbox-owed";
/// The walker's thread.
pub(super) const WALKER: &str = "clauseo-sandbox-forget";
/// The folder in the data folder.
pub(crate) const FOLDER: &str = "protection";
/// The file's name in it.
pub(crate) const FILE: &str = "owed_forgets.json";
/// The version this host writes.
const VERSION: u32 = 1;
/// The most bytes the file is read to: far past any real list.
const CAP: u64 = 1 << 20;

#[derive(Serialize, Deserialize)]
struct OnDisk {
    v: u32,
    #[serde(default)]
    matters: BTreeSet<MatterId>,
}

/// A change to the file.
#[derive(Debug, Clone, Copy)]
pub(super) enum Write {
    /// The matter's forget is owed.
    Owe(MatterId),
    /// It is not any more: forgotten, or the matter is back on the list.
    Drop(MatterId),
}

/// What the threads post back.
#[derive(Debug)]
pub(crate) enum Report {
    /// The writer's start-up read: the forgets owed from an earlier run.
    Read(Vec<MatterId>),
    /// An [`Write::Owe`] is done (saved or not, logged).
    Owed(MatterId),
    /// A [`Write::Drop`] is done.
    Dropped,
    /// A walk ended: whether the set-up forgot the matter.
    Walked { matter: MatterId, forgot: bool },
}

/// The writer: reads the file, posts it, then makes each change in order.
pub(super) fn write_owed(
    path: &Path,
    abandoned: &AtomicBool,
    writes: &mpsc::Receiver<Write>,
    post: &dyn Fn(Report),
) {
    let mut owed = read(path);
    post(Report::Read(owed.iter().copied().collect()));
    for write in writes {
        if abandoned.load(Ordering::SeqCst) {
            return;
        }
        let changed = match write {
            Write::Owe(matter) => owed.insert(matter),
            Write::Drop(matter) => owed.remove(&matter),
        };
        if changed && let Err(error) = save(path, &owed) {
            tracing::warn!(
                kind = ?error.kind(),
                "the sandbox's owed forgets were not saved"
            );
        }
        post(match write {
            Write::Owe(matter) => Report::Owed(matter),
            Write::Drop(_) => Report::Dropped,
        });
    }
}

/// The walker: forgets each matter in order, logged with its id and a failure's kind only.
pub(super) fn walk(
    setup: &dyn SandboxSetup,
    abandoned: &AtomicBool,
    matters: &mpsc::Receiver<MatterId>,
    post: &dyn Fn(Report),
) {
    for matter in matters {
        if abandoned.load(Ordering::SeqCst) {
            return;
        }
        let forgot = match setup.forget_matter(&matter) {
            Ok(()) => {
                tracing::info!(%matter, "the sandbox forgot a matter");
                true
            }
            Err(error) => {
                // Its text may name a path or an account (0047): its kind only.
                tracing::warn!(
                    %matter,
                    kind = kind(&error),
                    "the sandbox's forget of a matter failed; it stays owed"
                );
                false
            }
        };
        post(Report::Walked { matter, forgot });
    }
}

/// A set-up error's kind, for the log: never its text.
const fn kind(error: &SetupError) -> &'static str {
    match error {
        SetupError::Refused => "refused",
        SetupError::Failed(_) => "failed",
        SetupError::Blocked(_) => "blocked",
    }
}

/// The forgets owed on disk; none when the file is missing or unusable (logged).
fn read(path: &Path) -> BTreeSet<MatterId> {
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path)
        .and_then(|file| file.take(CAP.saturating_add(1)).read_to_end(&mut bytes));
    match read {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return BTreeSet::new(),
        Err(error) => {
            tracing::warn!(kind = ?error.kind(), "the sandbox's owed forgets do not read");
            return BTreeSet::new();
        }
    }
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > CAP {
        tracing::warn!(
            cap = CAP,
            "the sandbox's owed forgets are over their size cap"
        );
        return BTreeSet::new();
    }
    match serde_json::from_slice::<OnDisk>(&bytes) {
        Ok(disk) if disk.v == VERSION => disk.matters,
        Ok(disk) => {
            tracing::warn!(
                version = disk.v,
                "the sandbox's owed forgets are of another version"
            );
            BTreeSet::new()
        }
        Err(_unsaid) => {
            tracing::warn!("the sandbox's owed forgets do not parse");
            BTreeSet::new()
        }
    }
}

/// Writes `owed` to `path`, whole and atomically: the folder made first, an earlier crash's
/// temp files swept, the folder synced after.
fn save(path: &Path, owed: &BTreeSet<MatterId>) -> io::Result<()> {
    let disk = OnDisk {
        v: VERSION,
        matters: owed.clone(),
    };
    let bytes = serde_json::to_vec(&disk).map_err(io::Error::other)?;
    let folder = path.parent().unwrap_or(path);
    crate::write::create_dir_all(folder)?;
    crate::write::sweep_temp_files(path);
    crate::write::replace_private_file(path, &bytes)?;
    crate::write::sync_dir(folder)
}
