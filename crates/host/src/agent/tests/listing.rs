//! The stop's listing waits.

use super::{ended, fake_agent, run_agent, scenario};
use crate::agent::{AgentInput, Ended};

/// A dropped `AgentLink` stops its agent with the quit's short listing wait,
/// [`super::leftovers::SNAPSHOT_WAIT`] (1 s), not a stop's `STOP_LIST_WAIT`: with the listing
/// held, nothing ends until 1 s passes on the clock, and then the ladder runs. Unix: Windows lists
/// nothing before its ladder.
#[cfg(unix)]
#[test]
fn a_dropped_link_waits_one_second_for_its_listing() {
    use crate::agent::leftovers::{self, SNAPSHOT_WAIT};

    let (mut pool, link, clock, mut rx) = run_agent(&fake_agent(scenario("idle")), Vec::new());
    let held = leftovers::hold_next_listing();
    drop(link);
    pool.run_until(held.taken.recv()).unwrap();
    pool.run_until_stalled();
    while let Ok(input) = rx.try_recv() {
        assert!(
            !matches!(input, AgentInput::Ended(_)),
            "the agent ended before its listing's wait"
        );
    }
    clock.advance(SNAPSHOT_WAIT);
    let within = futures::future::select(
        Box::pin(ended(&mut rx)),
        async_io::Timer::after(std::time::Duration::from_secs(10)),
    );
    let futures::future::Either::Left((ended, _)) = pool.run_until(within) else {
        panic!("the agent did not end within 10 s of the listing's 1 s wait");
    };
    assert!(matches!(ended, Ended::Stopped { .. }), "{ended:?}");
    drop(held);
}
