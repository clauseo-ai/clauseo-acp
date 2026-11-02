use std::sync::{Arc, Mutex, mpsc};

use clauseo_protocol::{ErrorCode, Timestamp};

use super::{PREPARE_WAIT, off_actor};
use crate::ManualClock;

fn clock() -> ManualClock {
    ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap())
}

#[test]
fn the_answer_comes_back_from_its_thread() {
    let name = off_actor(Arc::new(clock()), || {
        Ok(std::thread::current().name().map(str::to_owned))
    });
    let name = futures::executor::block_on(name).unwrap();
    assert_eq!(name.as_deref(), Some("clauseo-codex-fs"));
}

#[test]
fn an_error_comes_back_as_it_is() {
    let failed = off_actor::<()>(Arc::new(clock()), || {
        Err(crate::error::Error::refused(
            ErrorCode::MatterCodexFolder,
            "x",
        ))
    });
    let error = futures::executor::block_on(failed).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterCodexFolder);
}

#[test]
fn a_hung_check_fails_closed_at_the_deadline() {
    let clock = clock();
    let (entered, started) = async_channel::unbounded::<()>();
    let (release, blocked) = mpsc::channel::<()>();
    let blocked = Mutex::new(blocked);
    let hung = off_actor(Arc::new(clock.clone()), move || {
        let _told = entered.send_blocking(());
        let _released = blocked.lock().map(|blocked| blocked.recv());
        Ok(())
    });
    let move_on = async move {
        started.recv().await.unwrap();
        assert_eq!(clock.next_armed().await, PREPARE_WAIT);
        clock.advance(PREPARE_WAIT);
    };
    let (result, ()) = futures::executor::block_on(futures::future::join(hung, move_on));
    assert_eq!(result.unwrap_err().code(), ErrorCode::MatterFolderMissing);
    drop(release);
}

/// The re-check resolves the stored folder again: a matter stored as a link that now points at
/// a folder holding `.codex` is refused.
#[cfg(unix)]
#[test]
fn the_recheck_resolves_a_linked_matter_again() {
    let root = std::env::temp_dir().join(format!("clauseo-recheck-{}", std::process::id()));
    crate::write::remove_dir_all(&root).unwrap();
    crate::write::create_dir_all(&root.join("clean")).unwrap();
    crate::write::create_dir_all(&root.join("other/.codex")).unwrap();
    let link = root.join("link");
    crate::write::symlink(&root.join("clean"), &link).unwrap();
    let check =
        || futures::executor::block_on(super::recheck(Arc::new(clock()), Some(link.clone())));
    assert!(check().is_ok());
    crate::write::remove_file(&link).unwrap();
    crate::write::symlink(&root.join("other"), &link).unwrap();
    assert_eq!(check().unwrap_err().code(), ErrorCode::MatterCodexFolder);
    crate::write::remove_dir_all(&root).unwrap();
}
