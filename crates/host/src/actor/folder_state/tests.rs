use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use clauseo_protocol::{FolderState, Timestamp};

use super::{LIST_WAIT, Running, folder_states, stat, states_with};
use crate::ManualClock;
use crate::pipeline::tests::support::TempDir;

fn clock() -> ManualClock {
    ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap())
}

#[test]
fn folders_read_found_or_missing_in_order() {
    let dir = TempDir::new("folder-states");
    let here = dir.path().to_path_buf();
    let file = dir.path().join("a-file");
    crate::write::write_file(&file, b"x").unwrap();
    let folders: Vec<PathBuf> = vec![here.clone(), dir.path().join("gone"), file, here];
    let running = Running::default();
    let list = folder_states(folders, Arc::new(clock()), Arc::clone(&running));
    let states = futures::executor::block_on(list);
    assert_eq!(
        states,
        [
            FolderState::Found,
            FolderState::Missing,
            FolderState::Missing,
            FolderState::Found
        ]
    );
    assert!(running.lock().unwrap().is_empty(), "a stat stayed running");
}

#[test]
fn no_folders_is_no_states() {
    let list = folder_states(Vec::new(), Arc::new(clock()), Running::default());
    assert_eq!(futures::executor::block_on(list), []);
}

/// A stand-in stat whose `share` and `share 2` say they started, then block until the test
/// drops the returned sender, and which counts the stats of `share`.
fn hanging(
    dir: &Path,
) -> (
    super::Stat,
    async_channel::Receiver<()>,
    mpsc::Sender<()>,
    Arc<AtomicUsize>,
) {
    let (share, second) = (dir.join("share"), dir.join("share 2"));
    let (entered, started) = async_channel::unbounded::<()>();
    let (release, blocked) = mpsc::channel::<()>();
    let blocked = Arc::new(Mutex::new(blocked));
    let count = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&count);
    let stand_in = move |folder: &Path| {
        if folder == share || folder == second {
            if folder == share {
                counted.fetch_add(1, Ordering::SeqCst);
            }
            let _told = entered.send_blocking(());
            let _released = blocked.lock().map(|blocked| blocked.recv());
            return FolderState::Missing;
        }
        stat(folder)
    };
    (Arc::new(stand_in), started, release, count)
}

#[test]
fn a_hung_stat_reads_found_once_the_list_deadline_passes_and_is_not_statted_again() {
    let dir = TempDir::new("folder-states-hang");
    let (stand_in, started, release, count) = hanging(dir.path());
    let share = dir.path().join("share");
    let gone = dir.path().join("gone");
    let folders = vec![dir.path().to_path_buf(), gone.clone(), share.clone()];
    let clock = clock();
    let running = Running::default();
    let mover = clock.clone();
    let move_on = async move {
        started.recv().await.unwrap();
        assert_eq!(mover.next_armed().await, LIST_WAIT);
        mover.advance(LIST_WAIT);
    };
    let list = states_with(
        folders,
        Arc::new(clock.clone()),
        Arc::clone(&running),
        Arc::clone(&stand_in),
    );
    let (states, ()) = futures::executor::block_on(futures::future::join(list, move_on));
    assert_eq!(
        states,
        [FolderState::Found, FolderState::Missing, FolderState::Found]
    );
    assert!(running.lock().unwrap().contains(&share));

    // Asked again while the share's stat still hangs: it reads found at once, with no new
    // stat of it and no wait on the clock.
    let list = states_with(
        vec![share, gone],
        Arc::new(clock),
        Arc::clone(&running),
        stand_in,
    );
    let states = futures::executor::block_on(list);
    assert_eq!(states, [FolderState::Found, FolderState::Missing]);
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "the hung folder was statted again"
    );
    drop(release);
}

#[test]
fn the_whole_list_has_one_deadline() {
    let dir = TempDir::new("folder-states-deadline");
    let (stand_in, started, release, _count) = hanging(dir.path());
    // Two folders that would each hang: one advance of the list's deadline answers both.
    let folders = vec![dir.path().join("share 2"), dir.path().join("share")];
    let clock = clock();
    let mover = clock.clone();
    let move_on = async move {
        started.recv().await.unwrap();
        assert_eq!(mover.next_armed().await, LIST_WAIT);
        mover.advance(LIST_WAIT);
    };
    let list = states_with(
        folders,
        Arc::new(clock.clone()),
        Running::default(),
        stand_in,
    );
    let (states, ()) = futures::executor::block_on(futures::future::join(list, move_on));
    assert_eq!(states, [FolderState::Found, FolderState::Found]);
    assert_eq!(clock.armed(), 0, "a second timer was armed");
    drop(release);
}
