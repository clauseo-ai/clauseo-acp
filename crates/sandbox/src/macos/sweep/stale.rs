//! A crashed host's leftovers: [`clean_stale`], which the next launcher runs.

use std::path::Path;
use std::time::Duration;

use super::end::end_with;
use super::{Checker, SessionMark, Survivors, api, now, start_of};
use crate::macos::profile::TAG_PREFIX;

/// How long the end of one crashed host's leftovers may take.
const LEFTOVER_DEADLINE: Duration = Duration::from_secs(5);

/// Ends what an earlier host left in `dir` (canonical), the marks folder, and removes its marks:
/// those of launchers of a host that runs no longer, by the pid and start its tag names
/// ([`host_of`]). For each, a fresh sentinel starts under a minimal profile whose one write is
/// that mark ([`SessionMark::control_args`]); every process the mark finds is ended
/// ([`end_session_processes`]'s rounds), and only once a vouched scan finds none is the mark
/// removed. A scan that stays unknown keeps the mark (logged, `kind` `stale_mark_kept`), so a
/// later launcher tries again. A host that still runs, this one included, is never touched.
/// Answers how many marks went.
pub(crate) fn clean_stale(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let Ok(check) = api() else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let Some((host, start)) = entry.file_name().to_str().and_then(host_of) else {
            continue;
        };
        if host_alive(host, start) {
            continue;
        }
        if let Ok(mark) = SessionMark::open(&entry.path())
            && end_leftovers(check, &mark)
        {
            removed += 1;
        }
    }
    removed
}

/// Ends every process of `mark`, an earlier host's, under a control of its own, and removes it
/// once a vouched scan finds none; answers whether it went.
fn end_leftovers(check: &dyn Checker, mark: &SessionMark) -> bool {
    let ended = mark
        .start_sentinel(check, &mark.control_args())
        .map_err(|why| Survivors {
            left: Vec::new(),
            why,
        })
        .and_then(|_| end_with(check, &[mark], now() + LEFTOVER_DEADLINE));
    mark.end_sentinel();
    match ended {
        Ok(gone) => {
            if gone.ended > 0 {
                tracing::warn!(
                    kind = "stale_processes_ended",
                    tag = mark.tag(),
                    ended = gone.ended,
                    "processes an earlier host left running were ended"
                );
            }
            match mark.remove() {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(tag = mark.tag(), %error, "a stale session mark could not be removed");
                    false
                }
            }
        }
        Err(survivors) => {
            tracing::warn!(
                kind = "stale_mark_kept",
                tag = mark.tag(),
                left = survivors.left.len(),
                why = %survivors.why,
                "an earlier host's session mark is kept: its scan could not show its processes gone"
            );
            false
        }
    }
}

/// The host a launcher's tag names, `clauseo-<pid>-<start>-<n>-<nonce>`: its pid and its start
/// (microseconds since the epoch).
pub(super) fn host_of(tag: &str) -> Option<(u32, u64)> {
    let mut parts = tag.strip_prefix(TAG_PREFIX)?.split('-');
    let pid = parts.next()?.parse().ok()?;
    let start = parts.next()?.parse().ok()?;
    Some((pid, start))
}

/// Whether the host `pid` that started at `start` still runs (it may still hold its marks).
/// A process with that pid and another start is not it: the pid was reused.
fn host_alive(pid: u32, start: u64) -> bool {
    start_of(pid) == Some(start)
}
