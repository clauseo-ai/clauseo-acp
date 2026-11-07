//! The refused list's entries, each canonicalised once per process with a time limit. An entry
//! on a network share that is down (`AppData\Roaming` redirected to a share off VPN) can hold a
//! canonicalise on SMB for 20 s or more, and every place check compares against every entry, so
//! each would wait that long. Here each entry not seen before is canonicalised on a thread of its
//! own, all given [`RESOLVE_WAIT`] together; one that fails or does not answer in time is taken
//! as written, as the check always took one that fails. The answer is kept for the process, so a
//! later check canonicalises no entry again and the first pays at most the wait once.

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

/// Each entry seen so far and the path it is compared by: canonical, or as written when its
/// canonicalise failed or ran out of time.
static RESOLVED: Mutex<Option<HashMap<PathBuf, PathBuf>>> = Mutex::new(None);

/// Held by the one check that resolves new entries, from finding them to keeping the answers,
/// so a check that comes meanwhile waits for it (at most the wait) and then finds them known,
/// and no entry is canonicalised twice. The canonicalise threads never take it.
static RESOLVING: Mutex<()> = Mutex::new(());

/// `refused` in order, each entry as [`RESOLVED`] keeps it; the entries not there yet are
/// canonicalised by `canonicalize` first, at most `wait` in all, under [`RESOLVING`]. A
/// canonicalise still running after the wait is left to finish on its thread, detached: it holds
/// no lock, only the sending end of a channel nobody reads. When any entry ran out of time, one
/// warning gives how many (never the paths).
pub(crate) fn resolved_entries(
    refused: &[PathBuf],
    canonicalize: &Canonicalize,
    wait: Duration,
) -> Vec<PathBuf> {
    let known = |entry: &PathBuf| {
        let cache = RESOLVED.lock().unwrap_or_else(PoisonError::into_inner);
        cache.as_ref().and_then(|map| map.get(entry).cloned())
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
            let (answers, timed_out) = resolve_all(&new, canonicalize, wait);
            if timed_out > 0 {
                tracing::warn!(
                    timed_out,
                    "refused folders did not answer in time; compared by their path as written"
                );
            }
            let mut cache = RESOLVED.lock().unwrap_or_else(PoisonError::into_inner);
            cache
                .get_or_insert_with(HashMap::new)
                .extend(new.into_iter().zip(answers));
        }
    }
    refused
        .iter()
        .map(|entry| known(entry).unwrap_or_else(|| entry.clone()))
        .collect()
}

/// `entries` canonicalised, each on a thread of its own, at most `wait` in all; an entry whose
/// canonicalise fails, does not answer within `wait`, or gets no thread, as written. With them,
/// how many did not answer within `wait`. The wait is
/// one more thread's `recv_timeout` on a channel nobody sends on, so no clock is read.
fn resolve_all(
    entries: &[PathBuf],
    canonicalize: &Canonicalize,
    wait: Duration,
) -> (Vec<PathBuf>, usize) {
    let mut answers: Vec<PathBuf> = entries.to_vec();
    let (sender, receiver) = mpsc::channel::<Option<(usize, PathBuf)>>();
    let mut pending = 0_usize;
    for (index, entry) in entries.iter().enumerate() {
        let (sender, canonicalize, entry) =
            (sender.clone(), Arc::clone(canonicalize), entry.clone());
        let spawned = std::thread::Builder::new()
            .name("refused-folder".to_owned())
            .spawn(move || {
                if let Ok(real) = canonicalize(&entry) {
                    drop(sender.send(Some((index, real))));
                } else {
                    drop(sender.send(None));
                }
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
                drop(timer.send(Some((usize::MAX, PathBuf::new()))));
            }
        });
    if timed.is_err() {
        return (answers, 0);
    }
    while pending > 0 {
        match receiver.recv() {
            Ok(Some((usize::MAX, _))) | Err(_) => break,
            Ok(Some((index, real))) => {
                if let Some(answer) = answers.get_mut(index) {
                    *answer = real;
                }
                pending -= 1;
            }
            Ok(None) => pending -= 1,
        }
    }
    drop(stop);
    (answers, pending)
}
