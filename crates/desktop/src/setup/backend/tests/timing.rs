//! The leave's timer and the composer's clock, driven by hand: [`FakeTimer`] hands each wait
//! the leave asks for to a [`Script`], which fires them one by one, moving the [`Stepping`]
//! clock to each wait's end. A wait nobody fires never ends, so time stands still by default.

use std::future::poll_fn;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clauseo_agents::Clock;
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures::channel::oneshot;

use crate::bridge::lock;
use crate::dialogs::BoxFuture;
use crate::setup::leave::{LeaveTimer, Wait};

/// The time every rig starts at.
pub(super) fn start() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

/// A clock that moves only when a [`Script`] fires a wait.
pub(super) struct Stepping(Mutex<SystemTime>);

impl Default for Stepping {
    fn default() -> Self {
        Self(Mutex::new(start()))
    }
}

impl Clock for Stepping {
    fn now(&self) -> SystemTime {
        *lock(&self.0)
    }
    fn monotonic(&self) -> Duration {
        self.now().duration_since(UNIX_EPOCH).unwrap_or_default()
    }
}

/// One wait the leave asked for: which, when it ends, and how to end it.
struct Ask {
    wait: Wait,
    at: SystemTime,
    fire: oneshot::Sender<()>,
}

/// The leave's timer: each wait goes to the [`Script`].
pub(super) struct FakeTimer {
    clock: Arc<Stepping>,
    asks: UnboundedSender<Ask>,
    taken: Mutex<Option<UnboundedReceiver<Ask>>>,
}

impl FakeTimer {
    pub(super) fn new(clock: Arc<Stepping>) -> Self {
        let (asks, taken) = unbounded();
        Self {
            clock,
            asks,
            taken: Mutex::new(Some(taken)),
        }
    }

    /// The script that fires the waits, once.
    pub(super) fn script(&self) -> Script {
        Script {
            asks: lock(&self.taken).take().expect("one script per rig"),
            open: Vec::new(),
            clock: Arc::clone(&self.clock),
        }
    }
}

impl LeaveTimer for FakeTimer {
    fn after(&self, wait: Wait) -> BoxFuture<'static, ()> {
        let (fire, fired) = oneshot::channel();
        let at = self.clock.now() + wait.duration();
        drop(self.asks.unbounded_send(Ask { wait, at, fire }));
        Box::pin(async move {
            if fired.await.is_err() {
                std::future::pending::<()>().await;
            }
        })
    }
}

/// Fires the leave's waits by hand.
pub(super) struct Script {
    asks: UnboundedReceiver<Ask>,
    open: Vec<Ask>,
    clock: Arc<Stepping>,
}

impl Script {
    /// Waits until the leave asks for `wait`, leaving it open.
    pub(super) async fn asked(&mut self, wait: Wait) {
        self.find(wait).await;
    }

    /// Fires `wait` once it is asked for: the clock moves to its end.
    pub(super) async fn fire(&mut self, wait: Wait) {
        let index = self.find(wait).await;
        let ask = self.open.remove(index);
        *lock(&self.clock.0) = ask.at;
        let _fired = ask.fire.send(());
        yield_now().await;
    }

    /// How long since the rig started.
    pub(super) fn elapsed(&self) -> Duration {
        self.clock.now().duration_since(start()).unwrap_or_default()
    }

    async fn find(&mut self, wait: Wait) -> usize {
        loop {
            if let Some(index) = self.open.iter().position(|ask| ask.wait == wait) {
                return index;
            }
            let ask = self
                .asks
                .next()
                .await
                .expect("the timer outlives the script");
            self.open.push(ask);
        }
    }
}

/// Lets the futures joined beside this one run once.
pub(super) async fn yield_now() {
    let mut yielded = false;
    poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await;
}

/// Yields until `done` holds; fails naming `what` after 1000 yields.
pub(super) async fn until(what: &str, done: impl Fn() -> bool) {
    for _ in 0..1000 {
        if done() {
            return;
        }
        yield_now().await;
    }
    panic!("never happened: {what}");
}
