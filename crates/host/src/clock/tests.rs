use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use clauseo_protocol::Timestamp;

use super::{Clock, ManualClock, SystemClock};

/// Records its id when woken.
struct Recorder {
    id: u32,
    log: Arc<Mutex<Vec<u32>>>,
}

impl Wake for Recorder {
    fn wake(self: Arc<Self>) {
        self.log.lock().unwrap().push(self.id);
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn manual_clock_fires_in_deadline_order() {
    let clock = ManualClock::new(Timestamp::new(1_000).unwrap());
    let log = Arc::new(Mutex::new(Vec::new()));
    // Armed in this order: 30 ms, 10 ms, 20 ms, then another 10 ms (a tie with the second).
    let mut timers: Vec<_> = [(1, 30), (2, 10), (3, 20), (4, 10), (5, 50)]
        .into_iter()
        .map(|(id, delay)| (id, clock.after(ms(delay))))
        .collect();
    assert_eq!(clock.armed(), 5);
    for (id, timer) in &mut timers {
        let waker = Waker::from(Arc::new(Recorder {
            id: *id,
            log: Arc::clone(&log),
        }));
        let mut cx = Context::from_waker(&waker);
        assert!(timer.as_mut().poll(&mut cx).is_pending());
    }

    clock.advance(ms(30));
    assert_eq!(*log.lock().unwrap(), vec![2, 4, 3, 1]);
    assert_eq!(clock.armed(), 1);
    assert_eq!(clock.now().wall, Timestamp::new(1_030).unwrap());
    assert_eq!(clock.now().mono, ms(30));

    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    for (id, timer) in &mut timers {
        let ready = timer.as_mut().poll(&mut cx).is_ready();
        assert_eq!(ready, *id != 5, "timer {id}");
    }
    // A dropped timer is disarmed.
    timers.clear();
    assert_eq!(clock.armed(), 0);

    // next_armed returns each armed delay once, in arming order.
    futures::executor::block_on(async {
        assert_eq!(clock.next_armed().await, ms(30));
        assert_eq!(clock.next_armed().await, ms(10));
    });
}

#[test]
fn manual_clock_wakes_another_thread() {
    let clock = ManualClock::new(Timestamp::ZERO);
    let waiter = {
        let clock = clock.clone();
        std::thread::spawn(move || {
            futures::executor::block_on(async {
                clock.after(ms(250)).await;
                clock.now().mono
            })
        })
    };
    futures::executor::block_on(async {
        assert_eq!(clock.next_armed().await, ms(250));
    });
    clock.advance(ms(100));
    assert_eq!(clock.armed(), 1);
    clock.advance(ms(150));
    assert_eq!(waiter.join().unwrap(), ms(250));
}

#[test]
fn zero_delay_is_ready_at_once() {
    let clock = ManualClock::new(Timestamp::ZERO);
    let mut timer = clock.after(Duration::ZERO);
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(timer.as_mut().poll(&mut cx), Poll::Ready(()));
}

#[test]
fn system_clock_mono_does_not_go_back() {
    let first = SystemClock.now();
    let second = SystemClock.now();
    assert!(second.mono >= first.mono);
    assert!(first.wall.as_millis() > 0);
}
