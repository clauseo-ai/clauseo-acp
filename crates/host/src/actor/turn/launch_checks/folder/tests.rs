//! The launch's folder checks wait for the host's open as long as macOS's prompt may be up
//! ([`PREFLIGHT_WAIT`]): a slow open that succeeds lets the launch through, one that never
//! answers is `matter.folder_permission`, and only then is a gone folder `matter.folder_missing`.

use std::io;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use clauseo_protocol::{ErrorCode, MatterId, Timestamp};
use uuid::Uuid;

use super::*;
use crate::ManualClock;

fn clock() -> ManualClock {
    ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap())
}

fn matter() -> MatterId {
    MatterId::new(Uuid::now_v7()).unwrap()
}

/// A folder that is there, without a marker (let through), removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let path = std::env::temp_dir().join(format!(
            "clauseo-launch-folder-{name}-{}",
            std::process::id()
        ));
        crate::write::remove_dir_all(&path).unwrap();
        crate::write::create_dir_all(&path).unwrap();
        Folder(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _removed = crate::write::remove_dir_all(&self.0);
    }
}

/// An open that tells `entered` it began, then blocks until `release` sends (or is dropped)
/// and answers `answer`.
fn held_open(
    answer: io::Result<()>,
) -> (
    impl FnOnce(&Path) -> io::Result<()> + Send + 'static,
    async_channel::Receiver<()>,
    mpsc::Sender<()>,
) {
    let (entered, started) = async_channel::unbounded::<()>();
    let (release, blocked) = mpsc::channel::<()>();
    let blocked = Mutex::new(blocked);
    let open = move |_: &Path| {
        let _told = entered.send_blocking(());
        let _released = blocked.lock().map(|blocked| blocked.recv());
        answer
    };
    (open, started, release)
}

/// The lawyer takes 30 s, then a minute and a half, to press Allow: well past the old 10 s
/// probe wait, the launch goes ahead.
#[test]
fn a_slow_open_that_succeeds_lets_the_launch_through() {
    let folder = Folder::new("slow");
    let clock = clock();
    let (open, started, release) = held_open(Ok(()));
    let checked = check_with(Arc::new(clock.clone()), matter(), folder.0.clone(), open);
    let lawyer = async move {
        started.recv().await.unwrap();
        assert_eq!(clock.next_armed().await, PREFLIGHT_WAIT);
        clock.advance(Duration::from_secs(30));
        clock.advance(Duration::from_secs(60));
        release.send(()).unwrap();
    };
    let (result, ()) = futures::executor::block_on(futures::future::join(checked, lawyer));
    result.unwrap();
}

/// The prompt left up past [`PREFLIGHT_WAIT`] (or a volume that hangs): the launch is refused
/// as the privacy settings' refusal, never as a missing folder.
#[test]
fn an_open_unanswered_past_the_wait_is_folder_permission() {
    let folder = Folder::new("hung");
    let clock = clock();
    let (open, started, release) = held_open(Ok(()));
    let checked = check_with(Arc::new(clock.clone()), matter(), folder.0.clone(), open);
    let move_on = async move {
        started.recv().await.unwrap();
        assert_eq!(clock.next_armed().await, PREFLIGHT_WAIT);
        clock.advance(PREFLIGHT_WAIT);
    };
    let (result, ()) = futures::executor::block_on(futures::future::join(checked, move_on));
    assert_eq!(
        result.unwrap_err().code(),
        ErrorCode::MatterFolderPermission
    );
    drop(release);
}

#[test]
fn a_refused_open_is_folder_permission() {
    let folder = Folder::new("refused");
    let open = |_: &Path| Err(io::Error::from(io::ErrorKind::PermissionDenied));
    let checked = check_with(Arc::new(clock()), matter(), folder.0.clone(), open);
    assert_eq!(
        futures::executor::block_on(checked).unwrap_err().code(),
        ErrorCode::MatterFolderPermission
    );
}

/// A folder that is gone fails the open with `NotFound`, which is let through to the next
/// check: `matter.folder_missing`.
#[test]
fn a_gone_folder_is_still_folder_missing() {
    let gone =
        std::env::temp_dir().join(format!("clauseo-launch-folder-gone-{}", std::process::id()));
    crate::write::remove_dir_all(&gone).unwrap();
    let checked = check_with(Arc::new(clock()), matter(), gone, |folder: &Path| {
        std::fs::read_dir(folder).map(drop)
    });
    assert_eq!(
        futures::executor::block_on(checked).unwrap_err().code(),
        ErrorCode::MatterFolderMissing
    );
}
