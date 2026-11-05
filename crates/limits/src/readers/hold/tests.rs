use std::sync::Arc;

use futures::FutureExt as _;

use super::Gate;

#[test]
fn an_open_gate_lets_reads_in_and_a_hold_waits_for_them_to_end() {
    let gate = Arc::new(Gate::default());
    let first = Arc::clone(&gate).enter().now_or_never().expect("open");
    let second = Arc::clone(&gate).enter().now_or_never().expect("open");
    let mut hold = Box::pin(gate.hold());
    assert!((&mut hold).now_or_never().is_none(), "two reads run");
    drop(first);
    assert!((&mut hold).now_or_never().is_none(), "one read runs");
    drop(second);
    assert!((&mut hold).now_or_never().is_some());
}

#[test]
fn dropping_the_guard_resumes_the_reads() {
    let gate = Arc::new(Gate::default());
    let guard = gate.hold().now_or_never().expect("no read ran");
    let mut held = Box::pin(Arc::clone(&gate).enter());
    assert!((&mut held).now_or_never().is_none());
    drop(guard);
    let running = (&mut held).now_or_never().expect("resumed");
    // A new hold counts that read now.
    let mut hold = Box::pin(gate.hold());
    assert!((&mut hold).now_or_never().is_none());
    drop(running);
    let guard = hold.now_or_never().expect("the read ended");
    drop(guard);
    assert!(Arc::clone(&gate).enter().now_or_never().is_some());
}

#[test]
fn two_holds_need_two_drops() {
    let gate = Arc::new(Gate::default());
    let first = gate.hold().now_or_never().expect("no read ran");
    // A hold taken while held still resolves at once: nothing runs.
    let second = gate.hold().now_or_never().expect("no read ran");
    let mut held = Box::pin(Arc::clone(&gate).enter());
    assert!((&mut held).now_or_never().is_none());
    drop(first);
    assert!((&mut held).now_or_never().is_none(), "one hold is left");
    drop(second);
    assert!(held.now_or_never().is_some());
}

#[test]
fn the_hold_starts_at_the_call_not_at_the_first_poll() {
    let gate = Arc::new(Gate::default());
    let hold = gate.hold();
    assert!(Arc::clone(&gate).enter().now_or_never().is_none());
    // Dropping the unresolved hold drops its guard with it.
    drop(hold);
    assert!(Arc::clone(&gate).enter().now_or_never().is_some());
}
