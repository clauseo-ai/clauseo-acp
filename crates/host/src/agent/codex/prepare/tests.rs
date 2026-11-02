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
    assert_eq!(result.unwrap_err().code(), ErrorCode::AgentLaunchFailed);
    drop(release);
}
