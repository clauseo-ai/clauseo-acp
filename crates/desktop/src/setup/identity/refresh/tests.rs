//! Refresh-on-check's plan and its one forced refresh at a time, without a backend.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clauseo_agents::{AccountFuture, AccountHash, AccountRead};
use clauseo_protocol::AgentId;
use futures::channel::oneshot;
use futures::executor::block_on;
use futures::future::join;

use super::{Ended, GUARD, Plan, Refresh, backoff, confirms, forced_share};
use crate::bridge::lock;
use crate::setup::identity::Stamp;
use crate::test_support::Scratch;

fn wall() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

/// Login files told apart by their stamps: each a file of its own in a scratch folder.
struct Files(Scratch);

impl Files {
    fn new(test: &str) -> Self {
        Self(Scratch::folder(test, &[]))
    }

    /// The stamp of file `n` (made on first use).
    fn stamp(&self, n: u64) -> Stamp {
        let path = self.0.path().join(format!("auth-{n}.json"));
        if !path.exists() {
            self.0.file(&format!("auth-{n}.json"), b"{}");
        }
        Stamp::File(clauseo_agents::file_stamp(&path).unwrap().unwrap())
    }
}

/// Wakes itself and yields once: the other futures of a `join` get a turn.
async fn yield_now() {
    let mut yielded = false;
    std::future::poll_fn(|cx| {
        if yielded {
            std::task::Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    })
    .await;
}

fn account() -> AccountRead {
    AccountRead::Account(AccountHash::of("lawyer@firm.example").unwrap())
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn the_first_check_forces_and_a_confirmed_stamp_does_not() {
    let files = Files::new("refresh-the_first_check_forces_and_a_c");
    let refresh = Refresh::default();
    assert_eq!(
        refresh.plan(&codex(), files.stamp(1), secs(1000)),
        Plan::Forced,
        "first since start"
    );
    refresh.forced_ended(&codex(), Ended::Confirmed(files.stamp(2)), secs(1000));
    assert_eq!(
        refresh.plan(&codex(), files.stamp(2), secs(1001)),
        Plan::Plain,
        "the rewritten stamp is the confirmed one"
    );
    assert!(
        !refresh.fire_if_due(&codex(), secs(5000)),
        "nothing pending"
    );
    // No login file: nothing to refresh, nothing pending.
    assert_eq!(
        refresh.plan(&codex(), Stamp::Missing, secs(5000)),
        Plan::Plain
    );
    assert!(!refresh.fire_if_due(&codex(), secs(9000)));
}

#[test]
fn a_stamp_change_ten_seconds_after_our_last_force_is_forced_at_sixty() {
    let files = Files::new("refresh-a_stamp_change_ten_seconds_aft");
    let refresh = Refresh::default();
    let start = secs(1000);
    assert_eq!(refresh.plan(&codex(), files.stamp(1), start), Plan::Forced);
    refresh.forced_ended(&codex(), Ended::Confirmed(files.stamp(2)), start);
    assert_eq!(
        refresh.plan(&codex(), files.stamp(3), start + secs(10)),
        Plan::Plain,
        "pending, but forced under a minute ago: read plain"
    );
    assert!(!refresh.fire_if_due(&codex(), start + secs(59)));
    assert!(refresh.fire_if_due(&codex(), start + GUARD));
    assert_eq!(
        refresh.plan(&codex(), files.stamp(3), start + GUARD),
        Plan::Forced
    );
}

#[test]
fn a_stamp_change_ninety_seconds_after_our_last_force_is_forced_at_once() {
    let files = Files::new("refresh-a_stamp_change_ninety_seconds_");
    let refresh = Refresh::default();
    assert_eq!(
        refresh.plan(&codex(), files.stamp(1), secs(0)),
        Plan::Forced
    );
    refresh.forced_ended(&codex(), Ended::Confirmed(files.stamp(2)), secs(0));
    assert_eq!(
        refresh.plan(&codex(), files.stamp(3), secs(90)),
        Plan::Forced
    );
}

#[test]
fn writes_every_ten_seconds_are_still_forced_every_sixty_while_pending() {
    let files = Files::new("refresh-writes_every_ten_seconds_are_s");
    let refresh = Refresh::default();
    let mut forced = Vec::new();
    for n in 0..=30 {
        let now = secs(n * 10);
        // A new write every 10 s; a look between them finds the forced check due when it is.
        let stamp = files.stamp(n);
        let fired = refresh.fire_if_due(&codex(), now);
        if refresh.plan(&codex(), stamp, now) == Plan::Forced {
            assert!(n == 0 || fired, "a look asked for it at {now:?}");
            forced.push(n * 10);
            refresh.forced_ended(&codex(), Ended::Confirmed(stamp), now);
        }
    }
    assert_eq!(forced, [0, 60, 120, 180, 240, 300]);
}

#[test]
fn forced_reads_that_confirm_nothing_go_on_with_the_backoff() {
    assert_eq!(
        [1, 2, 3, 4, 5, 9].map(backoff),
        [
            secs(30),
            secs(60),
            secs(120),
            secs(300),
            secs(300),
            secs(300)
        ]
    );
    let files = Files::new("refresh-forced_reads_that_confirm_none");
    let refresh = Refresh::default();
    let login = files.stamp(1);
    let mut forced = Vec::new();
    let mut now = secs(0);
    while now <= secs(1200) {
        if refresh.fire_if_due(&codex(), now) || forced.is_empty() {
            assert_eq!(refresh.plan(&codex(), login, now), Plan::Forced);
            forced.push(now.as_secs());
            // No answer within its 25 s share.
            let ended = now + secs(25);
            refresh.forced_ended(&codex(), Ended::Missed, ended);
        } else {
            assert_eq!(refresh.plan(&codex(), login, now), Plan::Plain);
        }
        now += secs(5);
    }
    // Each try: the later of 60 s after the last and the backoff after its miss.
    assert_eq!(forced, [0, 60, 145, 290, 615, 940]);
}

#[test]
fn a_rewrite_with_an_unconfirmed_read_waits_only_for_the_rate() {
    let files = Files::new("refresh-a_rewrite_with_an_unconfirmed_");
    let refresh = Refresh::default();
    assert_eq!(
        refresh.plan(&codex(), files.stamp(1), secs(0)),
        Plan::Forced
    );
    refresh.forced_ended(&codex(), Ended::Missed, secs(0));
    assert_eq!(
        refresh.plan(&codex(), files.stamp(1), secs(60)),
        Plan::Forced
    );
    refresh.forced_ended(&codex(), Ended::Unconfirmed, secs(60));
    assert!(
        !refresh.fire_if_due(&codex(), secs(119)),
        "60 s after the last force, the misses not counted up"
    );
    assert!(refresh.fire_if_due(&codex(), secs(120)));
}

#[test]
fn only_a_stamp_no_later_than_the_reply_can_be_its_rewrite() {
    let files = Files::new("refresh-only_a_stamp_no_later_than_the");
    let written = files.stamp(1);
    let long_after = wall() + secs(100 * 365 * 24 * 3600);
    assert!(confirms(written, long_after));
    // The file was written (and its status changed) after this reply.
    assert!(!confirms(written, wall()));
    assert!(!confirms(Stamp::Missing, long_after));
    assert_eq!(forced_share(secs(40)), secs(25));
}

/// A login file that is not there: these tests look at the lock, not the stamps.
fn nowhere() -> std::path::PathBuf {
    std::path::PathBuf::from("/nowhere/auth.json")
}

/// A forced read that records how many run at once, finishing when `open` is used.
fn read_held(
    running: &Arc<Mutex<(usize, usize, usize)>>,
    open: oneshot::Receiver<()>,
) -> AccountFuture {
    let running = Arc::clone(running);
    {
        let mut now = lock(&running);
        now.0 += 1;
        now.1 = now.1.max(now.0);
        now.2 += 1;
    }
    Box::pin(async move {
        let _open = open.await;
        lock(&running).0 -= 1;
        account()
    })
}

#[test]
fn two_checks_of_one_home_share_one_forced_refresh() {
    let refresh = Refresh::default();
    let running = Arc::new(Mutex::new((0, 0, 0)));
    let (open, opened) = oneshot::channel();
    let first = {
        let running = Arc::clone(&running);
        refresh.forced(&codex(), nowhere(), move || read_held(&running, opened))
    };
    let second = refresh.forced(&codex(), nowhere(), || -> AccountFuture {
        panic!("a second refresh of the same home started")
    });
    let ((), (a, b)) = block_on(join(async { open.send(()).unwrap() }, join(first, second)));
    assert_eq!((a.read, b.read), (account(), account()));
    assert_eq!(
        *lock(&running),
        (0, 1, 1),
        "one refresh, its answer used twice"
    );
}

#[test]
fn checks_of_two_homes_never_refresh_at_once() {
    let refresh = Refresh::default();
    let running = Arc::new(Mutex::new((0, 0, 0)));
    let other = AgentId::new("codex-acp-2").unwrap();
    let (open_a, opened_a) = oneshot::channel();
    let (open_b, opened_b) = oneshot::channel();
    let first = {
        let running = Arc::clone(&running);
        refresh.forced(&codex(), nowhere(), move || read_held(&running, opened_a))
    };
    let second = {
        let running = Arc::clone(&running);
        refresh.forced(&other, nowhere(), move || read_held(&running, opened_b))
    };
    let opens = async {
        // Each refresh is let go only after the test has had a turn: the second may start
        // only once the first ended.
        yield_now().await;
        assert_eq!(*lock(&running), (1, 1, 1), "the second waits for the first");
        open_a.send(()).unwrap();
        yield_now().await;
        yield_now().await;
        open_b.send(()).unwrap();
    };
    let ((), (a, b)) = block_on(join(opens, join(first, second)));
    assert_eq!((a.read, b.read), (account(), account()));
    assert_eq!(
        *lock(&running),
        (0, 1, 2),
        "two refreshes, never two at once"
    );
}
