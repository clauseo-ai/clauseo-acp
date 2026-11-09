//! The job port's notice, read from a fake port.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use super::{ACTIVE_PROCESS_LIMIT, TOO_MANY_PROGRAMS, hand_off, watch};

/// `JOB_OBJECT_MSG_NEW_PROCESS`, `JOB_OBJECT_MSG_EXIT_PROCESS` and
/// `JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO`: messages that say nothing to the lawyer.
const OTHERS: [u32; 3] = [6, 7, 4];

fn notices(messages: Vec<u32>) -> Vec<String> {
    let mut said = Vec::new();
    watch(messages, |line| said.push(line.to_owned()));
    said
}

#[test]
fn a_fork_bomb_at_the_cap_gives_one_notice_for_the_session() {
    let mut messages = OTHERS.to_vec();
    messages.extend([ACTIVE_PROCESS_LIMIT; 500]);
    messages.extend(OTHERS);
    messages.push(ACTIVE_PROCESS_LIMIT);
    assert_eq!(notices(messages), [TOO_MANY_PROGRAMS]);
}

#[test]
fn a_session_under_the_cap_gives_no_notice() {
    assert_eq!(notices(OTHERS.repeat(100)), Vec::<String>::new());
}

#[test]
fn the_notice_says_the_lawyer_s_words() {
    assert_eq!(
        TOO_MANY_PROGRAMS,
        "A task tried to start too many programs at once, so Clauseo stopped it. Ask the \
         assistant to try a smaller step."
    );
}

/// A stderr that blocks once the notice is written never stops the reader: every message after
/// the limit is still read while the notice's write waits.
#[test]
fn a_blocked_stderr_never_stops_the_port_draining() {
    let (wrote, written) = mpsc::channel();
    let (unblock, blocked) = mpsc::channel::<()>();
    let notify = hand_off(move |line: &str| {
        drop(wrote.send(line.to_owned()));
        // The fake stderr: its write never returns until the test ends.
        let _released = blocked.recv();
    })
    .unwrap();
    let read = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&read);
    let messages = std::iter::repeat_n(ACTIVE_PROCESS_LIMIT, 10_000)
        .chain(OTHERS)
        .inspect(move |_| {
            counted.fetch_add(1, Ordering::Relaxed);
        });
    let (done, drained) = mpsc::channel();
    std::thread::spawn(move || {
        watch(messages, notify);
        let _sent = done.send(());
    });
    // A reader that said the notice itself would still be blocked in its write here.
    drained
        .recv_timeout(Duration::from_secs(10))
        .expect("the reader stopped behind the blocked stderr");
    assert_eq!(read.load(Ordering::Relaxed), 10_003);
    let said = written.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(said, TOO_MANY_PROGRAMS);
    drop(unblock);
}
