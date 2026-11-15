//! The seam for the guard's own CDP traffic, end to end through the relay: a client's start
//! command, its events and its replies, none of which reach agent-browser.

use std::io::Write;
use std::sync::mpsc;

use serde_json::{Value, json};

use super::seam::{CdpLink, GuardClient, next_guard_id};
use super::tests::{client_frame, read_frame, relay, right_path, upgrade};

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

#[test]
fn agent_browsers_root_commands_in_a_taken_domain_are_refused_and_its_page_ones_are_not() {
    let (devtools, mut browser_reads, mut browser_writes) = relay();
    let client = std::sync::Arc::new(FakeClient {
        link: devtools.link(),
        events: std::sync::Mutex::default(),
        replies: std::sync::Mutex::default(),
    });
    let browser = std::thread::spawn(move || {
        let start: Value = serde_json::from_slice(&browser_reads.next().unwrap().unwrap()).unwrap();
        let reply = format!("{{\"id\":{},\"result\":{{}}}}\0", start["id"]);
        browser_writes.write_all(reply.as_bytes()).unwrap();
        // The next message is the first that got through: the page-session enable.
        let next: Value = serde_json::from_slice(&browser_reads.next().unwrap().unwrap()).unwrap();
        (next, browser_reads, browser_writes)
    });
    devtools.attach(&[client]).unwrap();
    let (_, mut agent) = upgrade(&devtools, &right_path(&devtools));
    // No `sessionId`, an empty one or a null one: the root session's, each refused.
    let root_sessions = [
        json!({"id": 5, "method": "Fetch.enable"}),
        json!({"id": 6, "method": "Fetch.continueWithAuth"}),
        json!({"id": 8, "method": "Fetch.enable", "sessionId": ""}),
        json!({"id": 9, "method": "Fetch.disable", "sessionId": null}),
    ];
    for command in root_sessions {
        agent
            .write_all(&client_frame(true, 0x1, command.to_string().as_bytes()))
            .unwrap();
        let reply: Value = serde_json::from_slice(&read_frame(&mut agent)).unwrap();
        assert_eq!(reply["id"], command["id"], "{reply}");
        assert_eq!(reply["error"]["code"], -32600, "{command}: {reply}");
    }
    let page = json!({"id": 7, "method": "Fetch.enable", "sessionId": "S1", "params": {}});
    agent
        .write_all(&client_frame(true, 0x1, page.to_string().as_bytes()))
        .unwrap();
    let (forwarded, _reads, _writes) = browser.join().unwrap();
    assert_eq!(
        forwarded, page,
        "the page session's enable reaches the browser"
    );
}

#[test]
fn with_no_client_the_seam_refuses_nothing() {
    let (devtools, mut browser_reads, _browser_writes) = relay();
    // W1's hook: no client, so no domain is the guard's.
    devtools
        .start_guard(&std::collections::BTreeMap::new())
        .unwrap();
    let (_, mut agent) = upgrade(&devtools, &right_path(&devtools));
    let root = json!({"id": 5, "method": "Fetch.enable", "params": {}});
    agent
        .write_all(&client_frame(true, 0x1, root.to_string().as_bytes()))
        .unwrap();
    let forwarded: Value = serde_json::from_slice(&browser_reads.next().unwrap().unwrap()).unwrap();
    assert_eq!(forwarded, root);
}
