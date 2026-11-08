use std::thread;

use clauseo_protocol::AgentId;
use futures::executor::block_on;

use super::{RecvError, TryRecvError, channel};
use crate::agent::login_link::LoginSplit;

fn split(agent: &AgentId, lawyers_file_gone: bool) -> LoginSplit {
    LoginSplit {
        agent: agent.clone(),
        lawyers_file_gone,
    }
}

#[test]
fn a_slow_reader_still_sees_the_latest_split_of_each_agent() {
    let (sender, splits) = channel();
    let codex: AgentId = "codex-acp".parse().unwrap();
    let other: AgentId = "codex-acp-2".parse().unwrap();
    // Far more than the old bound of 16, with nobody reading.
    for n in 0..1_000 {
        sender.send(split(&codex, n % 2 == 1));
        if n == 10 {
            sender.send(split(&other, true));
        }
    }
    sender.send(split(&codex, false));
    // The codex agent's first unread split went in first, so it comes out first, as its latest.
    assert_eq!(splits.try_recv(), Ok(split(&codex, false)));
    assert_eq!(splits.try_recv(), Ok(split(&other, true)));
    assert_eq!(splits.try_recv(), Err(TryRecvError::Empty));
}

#[test]
fn a_waiting_reader_wakes_and_a_closed_channel_ends_once_read() {
    let (sender, splits) = channel();
    let codex: AgentId = "codex-acp".parse().unwrap();
    let reader = {
        let splits = splits.clone();
        thread::spawn(move || block_on(splits.recv()))
    };
    // Whether the reader waits already or not, the split reaches it.
    sender.send(split(&codex, true));
    assert_eq!(reader.join().unwrap(), Ok(split(&codex, true)));

    sender.send(split(&codex, false));
    drop(sender);
    assert_eq!(block_on(splits.recv()), Ok(split(&codex, false)));
    assert_eq!(block_on(splits.recv()), Err(RecvError));
    assert_eq!(splits.try_recv(), Err(TryRecvError::Closed));
}
