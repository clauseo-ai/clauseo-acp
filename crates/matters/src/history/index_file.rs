//! The stat cache as `history.git/index`, a git index file written and read with gix-index:
//! one entry per [`CacheEntry`], mode `100644`, and the [`EntryState`] as git's flags.

use std::io::{self, Write};
use std::path::Path;

use clauseo_protocol::MatterPath;
use gix::hash::{Kind as HashKind, ObjectId};
use gix::index::entry::{self, Flags, Mode};
use gix::index::{State, decode};

use super::gix_store::{modified, to_gix, to_oid};
use crate::stat::{CacheEntry, EntryState, FileTime, Stat, StatCache};
use crate::{Error, write};

/// The flags that say an entry is [`EntryState::Unread`]: git's skip-worktree, which tells git
/// not to look at the file.
const UNREAD: Flags = Flags::EXTENDED.union(Flags::SKIP_WORKTREE);
/// The flags that say an entry is [`EntryState::Skipped`]: git's intent-to-add (tracked but
/// not in the tree, as `git add -N` writes), with the empty blob.
const SKIPPED: Flags = Flags::EXTENDED.union(Flags::INTENT_TO_ADD);

fn index_time(time: FileTime) -> entry::stat::Time {
    entry::stat::Time {
        secs: time.secs,
        nsecs: time.nanos,
    }
}

fn file_time(time: entry::stat::Time) -> FileTime {
    FileTime {
        secs: time.secs,
        nanos: time.nsecs,
    }
}

/// The index state of `cache`.
fn state_of(cache: &StatCache) -> State {
    let mut state = State::new(HashKind::Sha1);
    for entry in &cache.entries {
        let (id, flags) = match entry.state {
            EntryState::Read(oid) => (to_gix(oid), Flags::empty()),
            EntryState::Unread(oid) => (to_gix(oid), UNREAD),
            EntryState::Skipped => (ObjectId::empty_blob(HashKind::Sha1), SKIPPED),
        };
        let stat = entry::Stat {
            mtime: index_time(entry.stat.mtime),
            ctime: index_time(entry.stat.ctime),
            dev: entry.stat.dev,
            ino: entry.stat.ino,
            uid: 0,
            gid: 0,
            size: entry.stat.size,
        };
        let path = gix::bstr::BStr::new(entry.path.as_str());
        state.dangerously_push_entry(stat, id, flags, Mode::FILE, path);
    }
    state
}

/// The cache entries of `state`, or why it is not a cache this module wrote.
fn entries_of(state: &State) -> Result<Vec<CacheEntry>, String> {
    let mut entries = Vec::with_capacity(state.entries().len());
    let mut previous: Option<&[u8]> = None;
    for entry in state.entries() {
        let path: &[u8] = entry.path(state);
        if previous.is_some_and(|before| before >= path) {
            return Err(format!("{path:?} is out of order"));
        }
        previous = Some(path);
        let path = std::str::from_utf8(path)
            .ok()
            .and_then(|text| MatterPath::new(text).ok())
            .ok_or_else(|| format!("{path:?} is not a matter path"))?;
        if entry.mode != Mode::FILE {
            return Err(format!(
                "{} has mode {:o}",
                path.as_str(),
                entry.mode.bits()
            ));
        }
        let unread = entry.flags.contains(Flags::SKIP_WORKTREE);
        let skipped = entry.flags.contains(Flags::INTENT_TO_ADD);
        let empty = entry.id == ObjectId::empty_blob(HashKind::Sha1);
        let oid = to_oid(&entry.id).map_err(|err| err.to_string())?;
        let state = match (unread, skipped) {
            _ if entry.flags.stage_raw() != 0 => None,
            (false, false) => Some(EntryState::Read(oid)),
            (true, false) => Some(EntryState::Unread(oid)),
            (false, true) => empty.then_some(EntryState::Skipped),
            (true, true) => None,
        };
        let state =
            state.ok_or_else(|| format!("{} has flags {:?}", path.as_str(), entry.flags))?;
        let stat = Stat {
            mtime: file_time(entry.stat.mtime),
            ctime: file_time(entry.stat.ctime),
            dev: entry.stat.dev,
            ino: entry.stat.ino,
            size: entry.stat.size,
        };
        entries.push(CacheEntry { path, stat, state });
    }
    Ok(entries)
}

/// The cache in the index file `path`: `None` when it is missing, or does not read as a cache
/// this module wrote (logged). `written_at` is the file's modification time.
pub(super) fn load(path: &Path) -> Result<Option<StatCache>, Error> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::io("open", path, err)),
    };
    let written_at = modified(&file, path)?;
    let options = decode::Options {
        thread_limit: Some(1),
        ..decode::Options::default()
    };
    let index = match gix::index::File::at(path, HashKind::Sha1, false, options) {
        Ok(index) => index,
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "the stat cache does not read");
            return Ok(None);
        }
    };
    match entries_of(&index) {
        Ok(entries) => Ok(Some(StatCache {
            entries,
            written_at: Some(written_at),
        })),
        Err(reason) => {
            tracing::warn!(path = %path.display(), %reason, "the stat cache does not read");
            Ok(None)
        }
    }
}

/// Step 4 of the sync order: writes `cache` to the index file `path` by a synced temp file
/// renamed over it, then syncs its folder. No extensions; gix picks version 3 when an entry
/// has an extended flag, else 2.
pub(super) fn save(path: &Path, cache: &StatCache) -> Result<(), Error> {
    let index = gix::index::File::from_state(state_of(cache), path);
    let options = gix::index::write::Options {
        extensions: gix::index::write::Extensions::None,
        skip_hash: false,
    };
    write::replace(path, &mut |file| {
        let mut out = io::BufWriter::new(&mut *file);
        index
            .write_to(&mut out, options)
            .map_err(|err| Error::history("write stat cache", err))?;
        out.flush().map_err(|err| Error::io("write", path, err))
    })
}
