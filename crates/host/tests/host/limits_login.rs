//! `HostHandle::limits_login_changed` (decision record 0048, P4d): desktop's word that an
//! agent's login changed is `limits`' `Input::LoginChanged`. It starts exactly one read at once,
//! past the age that would hold any other read back, and leaves the next scheduled read (the
//! one at the reset time) where it was.

use std::time::Duration;

use clauseo_protocol::AgentKind;

use crate::limits_host::{CLAUDE, Feed, Reads, at, start, usage};
use crate::support::{Dir, Scenario, wait};

/// A minute of the host clock.
const MINUTE: Duration = Duration::from_secs(60);

#[test]
fn a_login_change_reads_once_at_once_and_the_next_scheduled_read_is_unchanged() {
    let dir = Dir::new(
        "limits_login_changed",
        &[(CLAUDE, Scenario::Bundled("claude-prompt-stream"))],
    );
    let reads = Reads::new();
    let host = start(&dir, &reads);
    let mut feed = Feed::new(&host);
    feed.view("the current view");
    // The window resets 30 minutes in: the scheduled read is at that time.
    reads.answer(
        AgentKind::Claude,
        usage(AgentKind::Claude, 40.0, Some(at(1800))),
    );
    feed.changed("the start-up read");
    host.wait_armed(15 * MINUTE);

    // A minute later the last read is new: a lawyer's try would not read, a login change does.
    host.clock.advance(MINUTE);
    wait(
        "limits_login_changed",
        host.handle.limits_login_changed(AgentKind::Claude),
    )
    .unwrap();
    host.settle();
    assert_eq!(reads.count(AgentKind::Claude), 2, "one read at once");
    reads.answer(
        AgentKind::Claude,
        usage(AgentKind::Claude, 5.0, Some(at(1800))),
    );
    feed.changed("the login change's read");
    host.settle();
    assert_eq!(reads.count(AgentKind::Claude), 2, "exactly one read");

    // Up to a second before the reset nothing more reads; at the reset the scheduled read does.
    host.clock.advance(Duration::from_secs(29 * 60 - 1));
    host.settle();
    assert_eq!(
        reads.count(AgentKind::Claude),
        2,
        "no read before the reset"
    );
    host.wait_armed(Duration::from_secs(1));
    host.clock.advance(Duration::from_secs(1));
    reads.until_count(AgentKind::Claude, 3);
    host.settle();
    assert_eq!(reads.count(AgentKind::Claude), 3, "one read at the reset");
    host.shutdown();
}
