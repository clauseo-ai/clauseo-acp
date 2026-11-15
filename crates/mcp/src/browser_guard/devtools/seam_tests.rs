//! The seam for the guard's own CDP traffic, end to end through the relay: a client's start
//! command, its events and its replies, none of which reach agent-browser.

use std::io::Write;
use std::sync::mpsc;

use serde_json::{Value, json};

use super::seam::{CdpLink, GuardClient, next_guard_id};
use super::tests::{read_frame, relay, right_path, upgrade};

/// A fake guard client: records what it gets, answers each root event through its link, and
/// has one start command.
struct FakeClient {
    link: std::sync::Arc<dyn CdpLink>,
    events: std::sync::Mutex<Vec<Value>>,
    replies: std::sync::Mutex<Vec<i64>>,
}

impl GuardClient for FakeClient {
    fn events(&self) -> &[&str] {
        &["Fetch.authRequired"]
    }

    fn start_commands(&self) -> Vec<(String, Value)> {
        vec![(
            "Fetch.enable".to_owned(),
            json!({"handleAuthRequests": true}),
        )]
    }

    fn on_reply(&self, id: i64, _message: &Value) {
        self.replies.lock().unwrap().push(id);
    }

    fn on_event(&self, message: &Value) {
        self.events.lock().unwrap().push(message.clone());
        let answer =
            json!({"id": next_guard_id(), "method": "Fetch.continueWithAuth", "params": {}});
        self.link.send(&answer).unwrap();
    }
}

#[test]
fn a_guard_client_gets_its_start_reply_events_and_replies_and_agent_browser_none_of_them() {
    let (devtools, mut browser_reads, mut browser_writes) = relay();
    let client = std::sync::Arc::new(FakeClient {
        link: devtools.link(),
        events: std::sync::Mutex::default(),
        replies: std::sync::Mutex::default(),
    });
    // The browser: answers the start command, then sends a root auth event, and answers the
    // client's own command, then one of agent-browser's.
    let (connected, wait_for_agent) = mpsc::channel::<()>();
    let browser = std::thread::spawn(move || {
        let start: Value = serde_json::from_slice(&browser_reads.next().unwrap().unwrap()).unwrap();
        assert_eq!(start["method"], "Fetch.enable");
        assert!(start.get("sessionId").is_none(), "the root session");
        let reply = format!("{{\"id\":{},\"result\":{{}}}}\0", start["id"]);
        browser_writes.write_all(reply.as_bytes()).unwrap();
        browser_writes
            .write_all(b"{\"method\":\"Fetch.authRequired\",\"params\":{}}\0")
            .unwrap();
        let answer: Value =
            serde_json::from_slice(&browser_reads.next().unwrap().unwrap()).unwrap();
        assert_eq!(answer["method"], "Fetch.continueWithAuth");
        let reply = format!("{{\"id\":{},\"result\":{{}}}}\0", answer["id"]);
        browser_writes.write_all(reply.as_bytes()).unwrap();
        wait_for_agent.recv().unwrap();
        browser_writes
            .write_all(b"{\"id\":4,\"result\":{}}\0")
            .unwrap();
        (browser_reads, browser_writes)
    });
    let as_client: std::sync::Arc<dyn GuardClient> = std::sync::Arc::<FakeClient>::clone(&client);
    devtools.attach(&[as_client]).unwrap();
    let (_, mut agent) = upgrade(&devtools, &right_path(&devtools));
    connected.send(()).unwrap();
    assert_eq!(
        read_frame(&mut agent),
        br#"{"id":4,"result":{}}"#,
        "only agent-browser's own"
    );
    let _browser = browser.join().unwrap();
    assert_eq!(client.events.lock().unwrap().len(), 1);
    assert_eq!(
        client.replies.lock().unwrap().len(),
        1,
        "the reply to its own command"
    );
}

#[test]
fn a_start_command_without_an_answer_fails_the_start() {
    let (devtools, _browser_reads, browser_writes) = relay();
    drop(browser_writes);
    let client = std::sync::Arc::new(FakeClient {
        link: devtools.link(),
        events: std::sync::Mutex::default(),
        replies: std::sync::Mutex::default(),
    });
    assert!(devtools.attach(&[client]).is_err());
}
