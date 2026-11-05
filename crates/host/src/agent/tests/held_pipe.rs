//! A helper that left the agent's process group holds its pipes open after the agent exits,
//! writing nothing: the owner posts `Ended` once the first bound passes, as before it checked
//! that the agent's output was all posted. Linux only: it needs `setsid`, which macOS lacks.

use std::collections::BTreeMap;

use clauseo_settings::AgentLaunch;
use futures::task::LocalSpawnExt;

use super::run_agent;
use crate::agent::{AgentInput, PUMP_END_BOUND, PUMP_END_ROUNDS};

/// The agent: it waits on its stdin, so its stdout reader has found the pipe empty and parked
/// before the exit; on the first line (`initialize`) it starts a helper in a session of its own
/// and exits 1. The helper keeps the agent's stdout and stderr open until the agent's stdin
/// closes (fd 3: a background job's own stdin is `/dev/null`).
const SCRIPT: &str = "exec 3<&0; read line; setsid sh -c 'read line' <&3 & exit 1";

/// Before the mark woke the parked reader, no read followed the mark, so the output never read
/// as posted and `Ended` came only after all [`PUMP_END_ROUNDS`] bounds.
#[test]
fn a_held_empty_stdout_ends_after_one_bound() {
    let agent = AgentLaunch {
        command: "/bin/sh".to_owned(),
        args: vec!["-c".to_owned(), SCRIPT.to_owned()],
        env: BTreeMap::new(),
    };
    let (mut pool, link, clock, mut rx) = run_agent(&agent, Vec::new());
    // The reader parks on the empty pipe.
    pool.run_until_stalled();
    let client = link.client.clone();
    pool.spawner()
        .spawn_local(async move { drop(client.initialize().await) })
        .unwrap();
    pool.run_until(async { while clock.next_armed().await != PUMP_END_BOUND {} });
    pool.run_until_stalled();
    let mut bounds = 0;
    let ended = loop {
        assert!(bounds < PUMP_END_ROUNDS, "no Ended after {bounds} bound(s)");
        clock.advance(PUMP_END_BOUND);
        bounds += 1;
        pool.run_until_stalled();
        if let Ok(AgentInput::Ended(ended)) = rx.try_recv() {
            break ended;
        }
    };
    assert_eq!(bounds, 1, "Ended after {bounds} bound(s): {ended:?}");
    // Dropping the connection closes the agent's stdin, which ends the helper.
    drop((link, pool));
}
