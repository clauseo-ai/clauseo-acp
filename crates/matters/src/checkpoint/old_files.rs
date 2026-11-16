//! A checkpoint as a build from before the dot-folder rule would have taken it (tests only,
//! behind `test-support`): `crate::test_support::checkpoint_with_old_files`.

use std::path::Path;

use clauseo_protocol::{CheckpointKind, MatterPath};

use super::{Core, Spec};
use crate::Error;
use crate::history::HistoryStore as _;
use crate::no_download::NoDownload;
use crate::stat::{CacheEntry, EntryState, Stat};
use crate::types::{Checkpoint, Settings, TimelineEntry, TurnMeta};

/// The walk of `folder`, plus each of `old` read and versioned, committed into `core` as the
/// `before-turn` checkpoint `meta`.
pub(crate) fn commit_with_old(
    core: &mut Core,
    folder: &Path,
    settings: &Settings,
    meta: &TurnMeta,
    old: &[&str],
) -> Result<Checkpoint, Error> {
    let mut walked = core
        .scan_folder(folder, settings, None, &mut |_| {})?
        .ok_or_else(|| Error::history("walk", "cancelled without a cancel flag"))?;
    let guard = NoDownload::begin(settings.probe.as_ref());
    for path in old {
        let abs = folder.join(path);
        let metadata =
            std::fs::symlink_metadata(&abs).map_err(|err| Error::io("stat", &abs, err))?;
        let (blob, _) = core
            .history
            .store_file(&abs, metadata.len(), u64::MAX, &guard)?;
        let path = MatterPath::new(path).map_err(|err| Error::history("path", err))?;
        walked.entries.push(CacheEntry {
            path,
            stat: Stat::from_metadata(&metadata),
            state: EntryState::Read(blob),
        });
    }
    drop(guard);
    walked
        .entries
        .sort_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
    let spec = Spec {
        entry: TimelineEntry {
            id: meta.checkpoint,
            kind: CheckpointKind::BeforeTurn,
            thread: Some(meta.thread),
            turn: Some(meta.turn),
        },
        at: meta.at,
        tool_paths: None,
    };
    core.commit(walked, &spec)
}
