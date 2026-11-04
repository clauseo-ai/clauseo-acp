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
    assert_eq!((&mut hold).now_or_never(), Some(()));
}

#[test]
fn a_held_gate_keeps_new_reads_out_until_resumed() {
    let gate = Arc::new(Gate::default());
    assert_eq!(gate.hold().now_or_never(), Some(()), "no read ran");
    let mut held = Box::pin(Arc::clone(&gate).enter());
    assert!((&mut held).now_or_never().is_none());
    // A hold taken again, while held, still resolves at once: nothing runs.
    assert_eq!(gate.hold().now_or_never(), Some(()));
    gate.resume();
    let running = (&mut held).now_or_never().expect("resumed");
    // The hold counts that read now.
    let mut hold = Box::pin(gate.hold());
    assert!((&mut hold).now_or_never().is_none());
    drop(running);
    assert_eq!(hold.now_or_never(), Some(()));
    gate.resume();
    assert!(Arc::clone(&gate).enter().now_or_never().is_some());
}

#[test]
fn the_hold_starts_at_the_call_not_at_the_first_poll() {
    let gate = Arc::new(Gate::default());
    let hold = gate.hold();
    assert!(Arc::clone(&gate).enter().now_or_never().is_none());
    drop(hold);
    gate.resume();
    assert!(Arc::clone(&gate).enter().now_or_never().is_some());
}
