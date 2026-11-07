//! Refresh-on-check's plan and its one forced refresh at a time, without a backend.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clauseo_agents::{AccountFuture, AccountHash, AccountRead};
use clauseo_protocol::AgentId;
use futures::channel::oneshot;
use futures::executor::block_on;
use futures::future::join;

use super::{GUARD, Plan, Refresh};
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

#[test]
fn the_first_check_forces_and_a_confirmed_stamp_does_not() {
    let files = Files::new("refresh-the_first_check_forces_and_a_c");
    let refresh = Refresh::default();
    let old = Some(wall() - Duration::from_secs(120));
    let plan = refresh.plan(&codex(), files.stamp(1), old, wall(), Duration::ZERO);
    assert_eq!(plan, Plan::Forced { retry: true }, "first since start");
    refresh.forced_ended(&codex(), true, files.stamp(2));
    let plan = refresh.plan(&codex(), files.stamp(2), old, wall(), Duration::ZERO);
    assert_eq!(
        plan,
        Plan::Plain,
        "the post-refresh stamp is the confirmed one"
    );
    let plan = refresh.plan(&codex(), files.stamp(3), old, wall(), Duration::ZERO);
    assert_eq!(plan, Plan::Forced { retry: true }, "a changed stamp");
    refresh.forced_ended(&codex(), false, files.stamp(3));
    let plan = refresh.plan(&codex(), files.stamp(3), old, wall(), Duration::ZERO);
    assert_eq!(
        plan,
        Plan::Forced { retry: true },
        "nothing confirmed after a refusal"
    );
}

#[test]
fn a_login_under_a_minute_old_reads_plain_and_one_forced_recheck_is_due_at_sixty_seconds() {
    let files = Files::new("refresh-a_login_under_a_minute_old_rea");
    let refresh = Refresh::default();
    let ten_ago = Some(wall() - Duration::from_secs(10));
    let start = Duration::from_secs(1000);
    let plan = refresh.plan(&codex(), files.stamp(1), ten_ago, wall(), start);
    assert_eq!(plan, Plan::Plain);
    // Another plain check meanwhile makes no second one due.
    let later = start + Duration::from_secs(20);
    assert_eq!(
        refresh.plan(&codex(), files.stamp(1), ten_ago, wall(), later),
        Plan::Plain
    );
    assert!(!refresh.fire_if_due(
        &codex(),
        start + GUARD.saturating_sub(Duration::from_secs(11))
    ));
    assert!(refresh.fire_if_due(
        &codex(),
        start + GUARD.saturating_sub(Duration::from_secs(10))
    ));
    let due = start + GUARD;
    assert_eq!(
        refresh.plan(&codex(), files.stamp(1), ten_ago, wall(), due),
        Plan::Forced { retry: true }
    );
    assert!(
        !refresh.fire_if_due(&codex(), due + GUARD),
        "one re-check only"
    );
}

#[test]
fn a_login_changed_again_before_its_recheck_is_read_plain_and_nothing_more_is_due() {
    let files = Files::new("refresh-a_login_changed_again_before_i");
    let refresh = Refresh::default();
    let ten_ago = Some(wall() - Duration::from_secs(10));
    assert_eq!(
        refresh.plan(&codex(), files.stamp(1), ten_ago, wall(), Duration::ZERO),
        Plan::Plain
    );
    assert!(refresh.fire_if_due(&codex(), GUARD));
    assert_eq!(
        refresh.plan(&codex(), files.stamp(2), ten_ago, wall(), GUARD),
        Plan::Plain
    );
    assert!(!refresh.fire_if_due(&codex(), GUARD * 10));
}

#[test]
fn a_retry_after_no_answer_is_due_once_and_not_retried_again() {
    let files = Files::new("refresh-a_retry_after_no_answer_is_due");
    let refresh = Refresh::default();
    refresh.retry_at(&codex(), files.stamp(1), Duration::from_secs(60));
    assert!(refresh.fire_if_due(&codex(), Duration::from_secs(60)));
    let old = Some(wall() - Duration::from_secs(120));
    assert_eq!(
        refresh.plan(
            &codex(),
            files.stamp(1),
            old,
            wall(),
            Duration::from_secs(60)
        ),
        Plan::Forced { retry: false }
    );
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
        refresh.forced(&codex(), move || read_held(&running, opened))
    };
    let second = refresh.forced(&codex(), || -> AccountFuture {
        panic!("a second refresh of the same home started")
    });
    let ((), (a, b)) = block_on(join(async { open.send(()).unwrap() }, join(first, second)));
    assert_eq!((a, b), (account(), account()));
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
        refresh.forced(&codex(), move || read_held(&running, opened_a))
    };
    let second = {
        let running = Arc::clone(&running);
        refresh.forced(&other, move || read_held(&running, opened_b))
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
    assert_eq!((a, b), (account(), account()));
    assert_eq!(
        *lock(&running),
        (0, 1, 2),
        "two refreshes, never two at once"
    );
}
