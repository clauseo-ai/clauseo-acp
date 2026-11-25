//! The shared attempt's bookkeeping: a message that wakes late, and an attempt that ends late.

use super::*;

fn owed() -> Owed {
    Owed {
        id: json!(1),
        body: Arc::from(b"{}".to_vec()),
        version: "2025-06-18",
    }
}

fn running(attempt: u64) -> State {
    State {
        stage: Stage::Running {
            owed: owed(),
            attempt,
        },
        attempt,
    }
}

#[test]
fn a_late_waker_leaves_a_newer_attempt_alone_and_it_still_succeeds() {
    // Attempt 1 failed; a new message started attempt 2 before attempt 1's waiter woke.
    let mut state = running(2);
    assert!(matches!(
        settle(&mut state, 1),
        Err(WarmFail::Call(Error::Network(_)))
    ));
    assert!(matches!(state.stage, Stage::Running { attempt: 2, .. }));
    // Attempt 2 succeeds: the initialize is done, and its waiters get it.
    assert_eq!(finish(&mut state, 2, Ok(None)), None);
    assert!(matches!(state.stage, Stage::Done));
    assert_eq!(settle(&mut state, 2), Ok(()));
}

#[test]
fn a_waiter_past_the_deadline_fails_its_own_attempt_for_all() {
    let mut state = running(1);
    assert!(settle(&mut state, 1).is_err());
    assert!(matches!(state.stage, Stage::Failed { attempt: 1, .. }));
    // Its late success still marks the initialize done.
    finish(&mut state, 1, Ok(None));
    assert!(matches!(state.stage, Stage::Done));
}

#[test]
fn an_old_attempt_that_ends_late_changes_nothing_of_a_newer_one() {
    let mut state = running(2);
    assert!(!current(&state, 1));
    assert!(current(&state, 2));
    finish(&mut state, 1, Ok(None));
    assert!(matches!(state.stage, Stage::Running { attempt: 2, .. }));
    finish(&mut state, 1, Err(WarmFail::Refused));
    assert!(matches!(state.stage, Stage::Running { attempt: 2, .. }));
}
