//! What the client does with the agent's stdout as bytes: the 8 MiB line cap, and reads that fail.

use std::io;

use super::*;

/// The longest line the agent may send, newline excluded: 8 MiB.
const EIGHT_MIB: usize = 8 * 1024 * 1024;

/// Answers the `initialize` the client wrote with a line of exactly `bytes` bytes before its
/// newline: the response padded with spaces, which JSON allows.
async fn answer_initialize_padded(agent: &mut Agent, bytes: usize) {
    let init = agent.read().await;
    assert_eq!(init["method"], "initialize");
    let response = json!({"jsonrpc": "2.0", "id": init["id"], "result": {"protocolVersion": 1}});
    let mut line = serde_json::to_vec(&response).unwrap();
    line.resize(bytes, b' ');
    line.push(b'\n');
    agent.stdout.unbounded_send(Ok(line)).unwrap();
}

/// A line of exactly 8 MiB is read like any other.
#[test]
fn line_of_exactly_8_mib_is_read() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        client.initialize().await.unwrap();
    };
    let script = async {
        answer_initialize_padded(&mut agent, EIGHT_MIB).await;
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

/// A line one byte over 8 MiB ends the connection with a protocol error.
#[test]
fn line_one_byte_over_8_mib_ends_the_connection() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move { client.initialize().await };
    let script = answer_initialize_padded(&mut agent, EIGHT_MIB + 1);
    let (driver, init, ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert!(matches!(driver, Err(Error::Protocol(_))), "{driver:?}");
    assert_eq!(init, Err(Error::Closed));
}

/// A stdout read interrupted by a signal (`ErrorKind::Interrupted`) is retried: the line after
/// it is read.
#[test]
fn interrupted_stdout_read_is_retried() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        client.initialize().await.unwrap();
    };
    let script = async {
        let init = agent.read().await;
        let interrupted = io::Error::from(io::ErrorKind::Interrupted);
        agent.stdout.unbounded_send(Err(interrupted)).unwrap();
        agent.respond(&init, &json!({"protocolVersion": 1}));
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

/// A stdout read that fails for any other reason ends the connection as a closed stdout does
/// (`Error::Closed`): nothing after it is read.
#[test]
fn failed_stdout_read_ends_the_connection() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move { client.initialize().await };
    let script = async {
        let init = agent.read().await;
        let broken = io::Error::new(io::ErrorKind::BrokenPipe, "the pipe broke");
        agent.stdout.unbounded_send(Err(broken)).unwrap();
        agent.respond(&init, &json!({"protocolVersion": 1}));
    };
    let (driver, init, ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Err(Error::Closed));
    assert_eq!(init, Err(Error::Closed));
}
