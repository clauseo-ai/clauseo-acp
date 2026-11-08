//! Shutdowns of a [`Running`] host that move the manual clock while they wait: past a turn's
//! cancel fallback, or through an agent's stop ladder.

use std::time::Duration;

use futures::future::Either;

use super::{Running, wait};

/// Shuts the host down while moving the clock past the 5 s cancel fallback when it is
/// armed, for a running turn whose agent never answers its cancel.
pub(crate) fn shutdown_past_cancel(mut host: Running) {
    let clock = host.clock.clone();
    let mover = async move {
        while clock.next_armed().await != Duration::from_secs(5) {}
        clock.advance(Duration::from_secs(5));
    };
    let result = wait("shutdown", async {
        let shutdown = std::pin::pin!(host.handle.shutdown());
        let mover = std::pin::pin!(mover);
        match futures::future::select(shutdown, mover).await {
            Either::Left((result, _mover)) => result,
            Either::Right(((), shutdown)) => shutdown.await,
        }
    });
    result.unwrap_or_else(|e| panic!("shutdown: {e}"));
    if let Some(join) = host.join.take() {
        join.join().unwrap_or_else(|e| panic!("join: {e}"));
    }
}

/// Shuts the host down with its agents' stop ladders on the manual clock: once a stop's 2 s
/// stdin grace is armed (the agent is being stopped and is not gone yet), `between` runs on a
/// thread of its own and its answer is returned; then every timer armed is moved past until the
/// shutdown ends. `between` runs off the waiting thread, so when it hangs the watchdog still
/// fails the test, and the unwind drops the host, which kills the agent's tree.
pub(crate) fn shutdown_stopping<T: Send + 'static>(
    mut host: Running,
    between: impl FnOnce() -> T + Send + 'static,
) -> T {
    let clock = host.clock.clone();
    let (answer, answered) = async_channel::bounded(1);
    let mover = async move {
        while clock.next_armed().await != Duration::from_secs(2) {}
        std::thread::spawn(move || answer.send_blocking(between()));
        let value = answered.recv().await.expect("between panicked");
        clock.advance(Duration::from_secs(2));
        (value, clock)
    };
    let (result, value) = wait("shutdown", async {
        let shutdown = std::pin::pin!(host.handle.shutdown());
        let mover = std::pin::pin!(mover);
        match futures::future::select(shutdown, mover).await {
            Either::Left((result, _mover)) => (result, None),
            Either::Right(((value, clock), shutdown)) => {
                // The rest of the ladder: move each timer past as it is armed.
                let shutdown = std::pin::pin!(shutdown);
                let rest = std::pin::pin!(async move {
                    loop {
                        let next = clock.next_armed().await;
                        clock.advance(next);
                    }
                });
                match futures::future::select(shutdown, rest).await {
                    Either::Left((result, _rest)) => (result, Some(value)),
                    Either::Right(((), _)) => unreachable!("the mover never ends"),
                }
            }
        }
    });
    result.unwrap_or_else(|e| panic!("shutdown: {e}"));
    if let Some(join) = host.join.take() {
        join.join().unwrap_or_else(|e| panic!("join: {e}"));
    }
    value.expect("the shutdown ended before the agent's stop was seen")
}
