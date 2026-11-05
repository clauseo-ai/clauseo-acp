use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use clauseo_protocol::{ErrorCode, FolderSupportKind, Timestamp};

use super::picked;
use crate::ManualClock;
use crate::agent::codex::prepare::PREPARE_WAIT;
use crate::agent::folder_support::Answer;
use crate::error::Error;

fn clock() -> ManualClock {
    ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap())
}

/// A place create takes.
fn here() -> PathBuf {
    std::env::temp_dir()
}

/// The query answers what a matter's row would show: the kind, `check_failed` for a failed
/// check, nothing for a fine folder.
#[test]
fn a_picked_folder_answers_as_the_row_shows() {
    for (answer, shown) in [
        (
            Answer::Known(Some(FolderSupportKind::Network)),
            Some(FolderSupportKind::Network),
        ),
        (Answer::Failed, Some(FolderSupportKind::CheckFailed)),
        (Answer::Known(None), None),
    ] {
        let got =
            futures::executor::block_on(picked(Arc::new(clock()), || Ok(here()), move |_| answer));
        assert_eq!(got.unwrap(), shown, "{answer:?}");
    }
}

/// A place create refuses is the answer's error, and the check never runs.
#[test]
fn a_refused_place_is_the_error_and_is_never_checked() {
    let checked = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&checked);
    let refused = || Err(Error::refused(ErrorCode::MatterFolderIsHome, "home"));
    let got = futures::executor::block_on(picked(Arc::new(clock()), refused, move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Answer::Known(Some(FolderSupportKind::Network))
    }));
    assert_eq!(got.unwrap_err().code(), ErrorCode::MatterFolderIsHome);
    assert_eq!(checked.load(Ordering::SeqCst), 0);
}

/// A check that hangs (a share that does not answer) answers `check_failed` at
/// [`PREPARE_WAIT`]: an unknown folder, never an error, and never the hung check's answer.
#[test]
fn a_hung_check_of_a_picked_folder_answers_check_failed() {
    let clock = clock();
    let (entered, started) = async_channel::unbounded::<()>();
    let (release, blocked) = mpsc::channel::<()>();
    let blocked = Mutex::new(blocked);
    let hung = picked(
        Arc::new(clock.clone()),
        || Ok(here()),
        move |_| {
            let _told = entered.send_blocking(());
            let _released = blocked.lock().map(|blocked| blocked.recv());
            Answer::Known(Some(FolderSupportKind::Network))
        },
    );
    let move_on = async move {
        started.recv().await.unwrap();
        assert_eq!(clock.next_armed().await, PREPARE_WAIT);
        clock.advance(PREPARE_WAIT);
    };
    let (result, ()) = futures::executor::block_on(futures::future::join(hung, move_on));
    assert_eq!(result.unwrap(), Some(FolderSupportKind::CheckFailed));
    drop(release);
}
