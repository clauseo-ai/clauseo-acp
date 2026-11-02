//! Each matter's `folder_state` for `ListMatters`, read off the actor. A `clauseo-stat` thread
//! stats the folders in order and sends each answer back. The actor's task gives the whole list
//! one deadline, [`LIST_WAIT`] on the host's clock: every folder not answered by then reads
//! `found`, and the thread stops after the stat it is in.
//!
//! A stat that hangs (a network share that does not answer) keeps its folder in [`Running`]
//! until it returns. A later list reads such a folder as `found` without statting it again, so
//! one hung folder strands at most one thread, however often the list is asked for.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_protocol::FolderState;
use futures::future::{Either, select};

use crate::clock::Clock;

/// How long one list may wait for its folders' stats; the rest read `found`.
pub(crate) const LIST_WAIT: Duration = Duration::from_secs(2);

/// The name of the threads that stat matter folders.
const THREAD_NAME: &str = "clauseo-stat";

/// The folders whose stat is running, shared by the actor and its stat threads.
pub(crate) type Running = Arc<Mutex<BTreeSet<PathBuf>>>;

/// A folder's stat: [`stat`], or a stand-in in tests.
type Stat = Arc<dyn Fn(&Path) -> FolderState + Send + Sync>;

/// The state of each folder of `folders`, in order.
pub(super) async fn folder_states(
    folders: Vec<PathBuf>,
    clock: Arc<dyn Clock>,
    running: Running,
) -> Vec<FolderState> {
    states_with(folders, clock, running, Arc::new(stat)).await
}

/// [`folder_states`] with `stat` doing each stat.
async fn states_with(
    folders: Vec<PathBuf>,
    clock: Arc<dyn Clock>,
    running: Running,
    stat: Stat,
) -> Vec<FolderState> {
    let mut states: Vec<Option<FolderState>> = {
        let busy = running.lock().unwrap_or_else(PoisonError::into_inner);
        folders
            .iter()
            .map(|folder| busy.contains(folder).then_some(FolderState::Found))
            .collect()
    };
    let wanted: Vec<(usize, PathBuf)> = folders
        .into_iter()
        .enumerate()
        .filter(|(index, _)| states.get(*index).is_some_and(Option::is_none))
        .collect();
    if !wanted.is_empty() {
        read(&mut states, wanted, &clock, running, stat).await;
    }
    states
        .into_iter()
        .map(|state| state.unwrap_or(FolderState::Found))
        .collect()
}

/// Stats `wanted` on a new thread into `states` until every one has answered or
/// [`LIST_WAIT`] has passed.
async fn read(
    states: &mut [Option<FolderState>],
    wanted: Vec<(usize, PathBuf)>,
    clock: &Arc<dyn Clock>,
    running: Running,
    stat: Stat,
) {
    let mut left = wanted.len();
    let (send, answers) = async_channel::bounded::<(usize, FolderState)>(left);
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            for (index, folder) in wanted {
                let mark = |busy: bool| {
                    let mut set = running.lock().unwrap_or_else(PoisonError::into_inner);
                    if busy {
                        set.insert(folder.clone());
                    } else {
                        set.remove(&folder);
                    }
                };
                mark(true);
                let state = stat(&folder);
                mark(false);
                // The list stopped waiting: no more stats.
                if send.send_blocking((index, state)).is_err() {
                    return;
                }
            }
        });
    if let Err(error) = started {
        tracing::warn!(%error, "could not start {THREAD_NAME}; folders read as found");
        return;
    }
    let mut deadline = clock.after(LIST_WAIT);
    while left > 0 {
        let answer = std::pin::pin!(answers.recv());
        match select(answer, &mut deadline).await {
            Either::Left((Ok((index, state)), _)) => {
                if let Some(slot) = states.get_mut(index) {
                    *slot = Some(state);
                }
                left -= 1;
            }
            // The thread ended early (it panicked): the rest read as found.
            Either::Left((Err(_closed), _)) => return,
            Either::Right(((), _)) => {
                tracing::warn!(
                    left,
                    "matter folders' stats did not answer in time; read as found"
                );
                return;
            }
        }
    }
}

/// Whether a folder is at `folder`: one `stat`, following a symlink as R8's `NoFolder` does.
/// Only a missing path, or one that is not a folder, is `Missing`; a stat that fails
/// otherwise (permissions, an offline share) is `Found`, and opening the matter gives the
/// reason.
fn stat(folder: &Path) -> FolderState {
    match std::fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => FolderState::Found,
        Ok(_) => FolderState::Missing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => FolderState::Missing,
        Err(_) => FolderState::Found,
    }
}

#[cfg(test)]
mod tests;
