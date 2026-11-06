//! The jobs' drop guards: the recharge watch's, and a `Check` that ends without a result.

use clauseo_protocol::{MatterId, ThreadId, TurnId};
use uuid::{Builder, Uuid};

use super::{Change, Finished, Job, Post, Unfinished, WatchEnded};
use crate::actor::connectors::ConnectorInput;
use crate::actor::connectors::hint::HintState;
use crate::actor::input::Input;

/// A watch's thread that panics after its pay link was posted still tells the actor the watch
/// ended, so no stale watch (or pay link) stays behind.
#[test]
fn a_watch_that_panics_still_says_it_ended() {
    let (send, receive) = async_channel::unbounded();
    let thread = std::thread::spawn(move || {
        let _watch = WatchEnded {
            post: Post(send),
            watch: 7,
        };
        panic!("the watch failed after its pay link was posted");
    });
    assert!(thread.join().is_err());
    let input = receive.try_recv().expect("no word from the watch");
    assert!(
        matches!(&input, Input::Connector(connector)
            if matches!(**connector, ConnectorInput::WatchEnded { watch: 7 })),
        "{input:?}"
    );
}

fn uuid(n: u8) -> Uuid {
    Builder::from_unix_timestamp_millis(1_700_000_000_000, &[n; 10]).into_uuid()
}

/// A `Check` job's thread that panics posts `Change::NoLiveness` from its drop guard, and that
/// change ends the case-law hint's wait (`hint.rs`), as `on_finished` applies it.
#[test]
fn a_check_that_panics_ends_the_hints_wait() {
    let (send, receive) = async_channel::unbounded();
    let thread = std::thread::spawn(move || {
        let job = Job::Check { slot: false };
        let _unfinished = Unfinished {
            post: Some(Post(send)),
            command: None,
            slot: false,
            change: job.on_panic(),
            generation: 0,
        };
        panic!("the liveness check failed");
    });
    assert!(thread.join().is_err());
    let input = receive.try_recv().expect("no word from the check");
    let Input::Connector(connector) = &input else {
        panic!("{input:?}");
    };
    let ConnectorInput::Finished(finished) = &**connector else {
        panic!("{connector:?}");
    };
    let finished: &Finished = finished;
    assert_eq!(finished.change, Change::NoLiveness);
    assert!(finished.liveness.is_none());
    let mut hint = HintState::default();
    hint.wait(
        MatterId::new(uuid(1)).unwrap(),
        ThreadId::new(uuid(2)).unwrap(),
        TurnId::new(uuid(3)).unwrap(),
    );
    assert!(hint.waits());
    hint.no_liveness();
    assert!(!hint.waits(), "the matter still waits on liveness");
}
