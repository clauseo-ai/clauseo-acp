//! Each line read, to its launch: a refusal as `Denial::Path`, `dropped` as `Denial::Dropped`,
//! the breaker's pause as `Denial::ReporterPaused` to every launch once, until it resumes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Capped, LINE_CAP, Shared, Watch};
use crate::windows::setup::watch::consumer::Access as WireAccess;
use crate::windows::setup::watch::protocol::{Line, access_named};
use crate::windows::sids::Sid;
use crate::{Access, Denial, display_safe_path};

/// Reports the line `text` to its launch, if it is a line of the protocol's and its launch
/// is registered.
pub(super) fn deliver<W: Watch>(shared: &Shared<W>, text: &str) {
    match Line::parse(text) {
        Some(Line::Denied {
            sid, path, access, ..
        }) => {
            // Local only when the line says `"net":false` itself and the path is `X:\...`:
            // a missing `net` is a network path, whatever `Line`'s default says.
            let local = sent_local(text) && is_local_drive_path(Path::new(&path));
            refused(shared, &sid, path, &access, !local);
        }
        Some(Line::Dropped { dropped: count, .. }) => dropped(shared, count),
        Some(Line::Paused { paused, .. }) => pause(shared, paused.secs),
        Some(Line::Resumed { .. }) => shared.lock().paused = false,
        None => {}
    }
}

/// The watcher's breaker opened for `secs`: every registered launch hears it once, until a
/// `resumed` line (or a new connection) clears it. The host-side source still reports.
fn pause<W: Watch>(shared: &Shared<W>, secs: u64) {
    let sinks: Vec<Arc<dyn Capped>> = {
        let mut state = shared.lock();
        if std::mem::replace(&mut state.paused, true) {
            return;
        }
        state
            .launches
            .values()
            .map(|entry| Arc::clone(&entry.sink))
            .collect()
    };
    for sink in sinks {
        sink.denied(Denial::ReporterPaused { secs });
    }
}

/// Whether the line `text` carries `"net":false` itself (not by a default).
fn sent_local(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|line| line.get("net").and_then(serde_json::Value::as_bool))
        == Some(false)
}

/// A refusal of `path` to the pool user `sid`; a `net` path is only shown, never opened.
fn refused<W: Watch>(shared: &Shared<W>, sid: &str, path: String, access: &str, net: bool) {
    let (Ok(sid), Some(wire)) = (Sid::parse(sid), access_named(access)) else {
        return;
    };
    let found = {
        let mut state = shared.lock();
        let found = state
            .launches
            .iter()
            .rev()
            .find(|(_, entry)| entry.pool_user == sid)
            .map(|(id, entry)| (*id, Arc::clone(&entry.policy), Arc::clone(&entry.sink)));
        if let Some((id, _, _)) = &found {
            state.last_routed = Some(*id);
        }
        found
    };
    let Some((_, policy, sink)) = found else {
        return;
    };
    let path = PathBuf::from(path);
    let access = match wire {
        WireAccess::Read => Access::Read,
        WireAccess::Write => Access::Write,
        WireAccess::Unknown if net => Access::Read,
        WireAccess::Unknown => resolve(shared, &sid, &path),
    };
    // `classify` compares strings only: it opens nothing, a network path included.
    let class = policy.classify(&path, access);
    sink.denied(Denial::Path {
        path: display_safe_path(&path),
        access,
        class,
    });
}

/// A plain open's access: a write when the pool user may read the path, else a read. Only a
/// local drive-letter path is checked, and only by a line that isn't `net`.
fn resolve<W: Watch>(shared: &Shared<W>, sid: &Sid, path: &Path) -> Access {
    if !is_local_drive_path(path) {
        return Access::Read;
    }
    match shared.watch.would_deny(sid, path, Access::Read) {
        Some(false) => Access::Write,
        Some(true) | None => Access::Read,
    }
}

/// `dropped` refusals the watcher held back: to the last refusal's launch, or to every
/// launch when none is known.
fn dropped<W: Watch>(shared: &Shared<W>, dropped: u64) {
    let count = u32::try_from(dropped).unwrap_or(u32::MAX);
    if count == 0 {
        return;
    }
    let sinks: Vec<Arc<dyn Capped>> = {
        let state = shared.lock();
        match state.last_routed.and_then(|id| state.launches.get(&id)) {
            Some(entry) => vec![Arc::clone(&entry.sink)],
            None => state
                .launches
                .values()
                .map(|entry| Arc::clone(&entry.sink))
                .collect(),
        }
    };
    for sink in sinks {
        sink.denied(Denial::Dropped { count });
    }
}
/// Whether `path` is a local drive-letter path, `X:\...` with backslashes only, the only kind
/// the Authz check opens: not UNC, `\\?\`, `\\.\` or any device path, not drive-relative
/// (`X:name`), with no `/`, and with no `.` or
/// `..` part and no empty part but a trailing one, so nothing it names can be rewritten into
/// another kind. A drive letter mapped to a share is refused by the Windows check after this.
pub fn is_local_drive_path(path: &Path) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    let mut chars = text.chars();
    let drive = matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(letter), Some(':'), Some('\\')) if letter.is_ascii_alphabetic()
    );
    if !drive || text.contains(['\0', '/']) {
        return false;
    }
    let parts: Vec<&str> = text.get(3..).unwrap_or_default().split('\\').collect();
    let last = parts.len().saturating_sub(1);
    parts.iter().enumerate().all(|(at, part)| match *part {
        "." | ".." => false,
        "" => at == last,
        _ => true,
    })
}

/// The bytes read so far split into lines, each at most [`LINE_CAP`] long.
#[derive(Default)]
pub(super) struct Lines {
    line: Vec<u8>,
    /// The line went past the cap: skipped to its `\n`.
    skipping: bool,
}

impl Lines {
    /// Adds `chunk`, giving each line it ends, without its `\n`.
    pub(super) fn push(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        let mut done = Vec::new();
        let mut rest = chunk;
        while let Some(at) = rest.iter().position(|byte| *byte == b'\n') {
            let (head, tail) = rest.split_at(at);
            self.extend(head);
            if !std::mem::take(&mut self.skipping) {
                done.push(std::mem::take(&mut self.line));
            }
            self.line.clear();
            rest = tail.get(1..).unwrap_or_default();
        }
        self.extend(rest);
        done
    }

    fn extend(&mut self, part: &[u8]) {
        if self.skipping {
            return;
        }
        if self.line.len() + part.len() > LINE_CAP {
            self.skipping = true;
            self.line.clear();
        } else {
            self.line.extend_from_slice(part);
        }
    }
}
