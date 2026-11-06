//! The recharge watch's drop guard.

use super::{Post, WatchEnded};
use crate::actor::connectors::ConnectorInput;
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
