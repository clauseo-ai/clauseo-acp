//! The refused list's entries, each canonicalised once per process with a time limit. An entry
//! on a network share that is down (`AppData\Roaming` redirected to a share off VPN) can hold a
//! canonicalise on SMB for 20 s or more, and every place check compares against every entry, so
//! each would wait that long. Here each entry not seen before is canonicalised on a thread of its
//! own, all given [`RESOLVE_WAIT`] together; one that fails or does not answer in time is taken
//! as written, as the check always took one that fails. The answer is kept for the process, so a
//! later check canonicalises no entry again and the first pays at most the wait once. A late
//! success replaces the as-written fallback of an entry that timed out (see [`keep`]).

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::Duration;

/// How long a check waits, in all, for the entries it has not seen before.
pub(crate) const RESOLVE_WAIT: Duration = Duration::from_secs(2);

/// The canonicalise the entries go through: `dunce::canonicalize` outside tests.
pub(crate) type Canonicalize = Arc<dyn Fn(&Path) -> io::Result<PathBuf> + Send + Sync>;

/// An entry's answer: canonical, or as written. `timed_out` marks the as-written fallback of an
/// entry that ran out of time, the one a late success replaces.
struct Resolved {
    path: PathBuf,
    timed_out: bool,
}

/// Each entry seen so far and its answer.
static RESOLVED: Mutex<Option<HashMap<PathBuf, Resolved>>> = Mutex::new(None);

/// Held by the one check that resolves new entries, from finding them to keeping the answers,
/// so a check that comes meanwhile waits for it (at most the wait) and then finds them known,
/// and no entry is canonicalised twice. The canonicalise threads never take it.
static RESOLVING: Mutex<()> = Mutex::new(());

/// `refused` in order, each entry as [`RESOLVED`] keeps it; the entries not there yet are
/// canonicalised by `canonicalize` first, at most `wait` in all, under [`RESOLVING`]. A
/// canonicalise still running after the wait is left to finish on its thread, detached: it holds
/// no lock while it runs, and takes [`RESOLVED`] only if it succeeds (see [`keep`]). When any
/// entry ran out of time, one warning gives how many (never the paths).
pub(crate) fn resolved_entries(
    refused: &[PathBuf],
    canonicalize: &Canonicalize,
    wait: Duration,
) -> Vec<PathBuf> {
    let known = |entry: &PathBuf| {
        let cache = RESOLVED.lock().unwrap_or_else(PoisonError::into_inner);
        cache
            .as_ref()
            .and_then(|map| map.get(entry).map(|held| held.path.clone()))
    };
    let unknown = || -> Vec<PathBuf> {
        refused
            .iter()
            .filter(|entry| known(entry).is_none())
            .cloned()
            .collect()
    };
    if !unknown().is_empty() {
        let _resolving = RESOLVING.lock().unwrap_or_else(PoisonError::into_inner);
        let new = unknown();
        if !new.is_empty() {
            let answers = resolve_all(&new, canonicalize, wait);
            let timed_out = answers.iter().filter(|answer| answer.timed_out).count();
            if timed_out > 0 {
                tracing::warn!(
                    timed_out,
                    "refused folders did not answer in time; compared by their path as written"
                );
            }
            for (entry, answer) in new.iter().zip(answers) {
                keep(entry, answer.path, answer.timed_out);
            }
        }
    }
    refused
        .iter()
        .map(|entry| known(entry).unwrap_or_else(|| entry.clone()))
        .collect()
}

/// Stores `path` for `entry` unless that would replace a canonical answer. A canonical answer
/// replaces an absent entry or a timed-out fallback; a fallback fills only an absent entry. So the
/// order of the two writers does not matter: a late success that lands before the first check's
/// insert is kept by that insert, and one that lands after replaces the fallback it left. Takes
/// [`RESOLVED`] only, never [`RESOLVING`].
fn keep(entry: &PathBuf, path: PathBuf, timed_out: bool) {
    let mut cache = RESOLVED.lock().unwrap_or_else(PoisonError::into_inner);
    let map = cache.get_or_insert_with(HashMap::new);
    let replace = map
        .get(entry)
        .is_none_or(|held| held.timed_out && !timed_out);
    if replace {
        map.insert(entry.clone(), Resolved { path, timed_out });
    }
}

/// `entries` canonicalised, each on a thread of its own, at most `wait` in all. An entry whose
/// canonicalise fails, or does not answer within `wait`, is as written; one that did not answer
/// in time is marked `timed_out`. A thread that succeeds calls [`keep`] before it reports, so an
/// answer that comes after the wait still lands in [`RESOLVED`]. The wait is one more thread's
/// `recv_timeout` on a channel nobody sends on, so no clock is read.
fn resolve_all(entries: &[PathBuf], canonicalize: &Canonicalize, wait: Duration) -> Vec<Resolved> {
    let mut answers: Vec<Resolved> = entries
        .iter()
        .map(|entry| Resolved {
            path: entry.clone(),
            timed_out: true,
        })
        .collect();
    let (sender, receiver) = mpsc::channel::<(usize, Option<PathBuf>)>();
    let mut pending = 0_usize;
    for (index, entry) in entries.iter().enumerate() {
        let (sender, canonicalize, entry) =
            (sender.clone(), Arc::clone(canonicalize), entry.clone());
        let spawned = std::thread::Builder::new()
            .name("refused-folder".to_owned())
            .spawn(move || {
                let real = canonicalize(&entry).ok();
                if let Some(real) = &real {
                    keep(&entry, real.clone(), false);
                }
                drop(sender.send((index, real)));
            });
        pending += usize::from(spawned.is_ok());
    }
    // The timer: its channel's sender stays here, so it waits the whole `wait` unless this
    // returns first, when the sender drops and it ends at once.
    let (stop, stopped) = mpsc::channel::<()>();
    let timer = sender;
    let timed = std::thread::Builder::new()
        .name("refused-folder-wait".to_owned())
        .spawn(move || {
            if let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(wait) {
                drop(timer.send((usize::MAX, None)));
            }
        });
    if timed.is_err() {
        return answers;
    }
    while pending > 0 {
        match receiver.recv() {
            Ok((usize::MAX, _)) | Err(_) => break,
            Ok((index, real)) => {
                if let Some(answer) = answers.get_mut(index) {
                    answer.timed_out = false;
                    if let Some(real) = real {
                        answer.path = real;
                    }
                }
                pending -= 1;
            }
        }
    }
    drop(stop);
    answers
}
