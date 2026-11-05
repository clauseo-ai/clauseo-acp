//! [`ReadProgress`]: whether what the agent wrote to stdout before a mark has all been read and
//! sent on the event channel.

use futures::executor::LocalPool;
use futures::task::LocalSpawnExt;

use super::*;

/// One `agent_message_chunk` for a session the client never started.
fn chunk(agent: &Agent) {
    agent.update(&json!({
        "sessionId": "s-9",
        "update": {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "The last words."},
        },
    }));
}

/// A line the reader has not read yet keeps the progress from saying drained, however long the
/// reader waited before the mark; once the reader has read it (and found the pipe empty again)
/// and its event is in the channel, it says drained. A reader that parked on the empty pipe
/// before the mark is woken by it, so a pipe that stays empty reads as drained with nothing
/// else waking the reader (a helper holding the agent's stdout open, writing nothing).
#[test]
fn a_line_written_before_the_mark_is_read_and_sent_before_it_is_drained() {
    let (client, events, driver, agent) = pipes();
    let progress = client.read_progress();
    let mut pool = LocalPool::new();
    pool.spawner()
        .spawn_local(async move { drop(driver.await) })
        .unwrap();
    pool.run_until_stalled();
    // The reader parked on the empty pipe before the mark: no read since it, until the
    // mark's wake runs.
    let empty = progress.mark();
    assert!(!progress.drained_since(empty), "no read since the mark");
    pool.run_until_stalled();
    assert!(
        progress.drained_since(empty),
        "the mark did not wake the parked reader"
    );

    // The agent writes its last chunk; the host has not run the reader yet.
    chunk(&agent);
    let mark = progress.mark();
    assert!(
        !progress.drained_since(mark),
        "the unread chunk counted as drained"
    );
    assert!(events.is_empty());

    pool.run_until_stalled();
    assert!(
        progress.drained_since(mark),
        "read and sent, then the pipe empty"
    );
    assert!(matches!(
        events.try_recv(),
        Ok(AcpEvent::Update {
            update: AgentUpdate::MessageChunk { .. },
            ..
        })
    ));

    // The stdout's end says drained too.
    let mark = progress.mark();
    drop(agent);
    pool.run_until_stalled();
    assert!(progress.drained_since(mark), "the stdout ended");
    drop(client);
    pool.run_until_stalled();
}
