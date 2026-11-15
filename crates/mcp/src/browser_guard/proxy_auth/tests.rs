use std::io;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::super::devtools::seam::{CdpLink, GuardClient, is_guard_id};
use super::{Password, ProxyAuth};

/// An obviously fake password.
const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ORIGIN: &str = "http://127.0.0.1:61200";

/// A link that keeps what is sent.
#[derive(Default)]
struct Fake {
    sent: Mutex<Vec<Value>>,
}

impl CdpLink for Fake {
    fn send(&self, message: &Value) -> io::Result<()> {
        self.sent.lock().unwrap().push(message.clone());
        Ok(())
    }
}

impl Fake {
    fn take(&self) -> Vec<Value> {
        std::mem::take(&mut *self.sent.lock().unwrap())
    }
}

/// A responder and the fake link it answers through.
fn auth() -> (ProxyAuth, Arc<Fake>) {
    let link = Arc::new(Fake::default());
    let as_link: Arc<dyn CdpLink> = Arc::<Fake>::clone(&link);
    let auth = ProxyAuth::new(as_link, ORIGIN.to_owned(), Password::parse(SECRET).unwrap());
    (auth, link)
}

/// What was sent, each with its id checked as a guard id and then taken out.
fn sent_without_ids(link: &Fake) -> Vec<Value> {
    link.take()
        .into_iter()
        .map(|mut message| {
            let id = message["id"].as_i64().unwrap();
            assert!(is_guard_id(id), "{id}");
            message.as_object_mut().unwrap().remove("id");
            message
        })
        .collect()
}

fn challenge(source: &str, origin: &str) -> Value {
    json!({
        "method": "Fetch.authRequired",
        "params": {
            "requestId": "interception-7",
            "request": {"url": "https://example.com/"},
            "frameId": "F",
            "resourceType": "Document",
            "authChallenge": {"source": source, "origin": origin, "scheme": "basic", "realm": "r"},
        },
    })
}

/// The relay sends the start command itself and waits for its reply; the responder sends
/// nothing of its own.
#[test]
fn it_takes_the_root_fetch_events_and_enables_fetch_with_auth_requests() {
    let (auth, link) = auth();
    assert_eq!(auth.events(), ["Fetch.requestPaused", "Fetch.authRequired"]);
    assert_eq!(
        auth.start_commands(),
        [(
            "Fetch.enable".to_owned(),
            json!({"patterns": [{"urlPattern": "*"}], "handleAuthRequests": true}),
        )]
    );
    assert_eq!(link.take(), Vec::<Value>::new());
}

/// Error replies to continues are only noted; only the first is, so a page that makes Chrome
/// refuse many cannot flood stderr.
#[test]
fn only_the_first_continue_refusal_is_noted() {
    let (auth, link) = auth();
    let id = 1 << 30;
    auth.on_reply(id, &json!({"id": id, "result": {}}));
    assert!(!auth.refusal_noted.load(Ordering::SeqCst));
    let refused = |id: i64| json!({"id": id, "error": {"code": -32602, "message": "Invalid id"}});
    auth.on_reply(id + 1, &refused(id + 1));
    assert!(auth.refusal_noted.load(Ordering::SeqCst));
    auth.on_reply(id + 2, &refused(id + 2));
    assert_eq!(
        link.take(),
        Vec::<Value>::new(),
        "a reply is never answered"
    );
}

#[test]
fn a_paused_request_is_continued() {
    let (auth, link) = auth();
    let paused = json!({
        "method": "Fetch.requestPaused",
        "params": {"requestId": "interception-3", "request": {"url": "https://example.com/"}},
    });
    auth.on_event(&paused);
    assert_eq!(
        sent_without_ids(&link),
        [json!({
            "method": "Fetch.continueRequest",
            "params": {"requestId": "interception-3"},
        })]
    );
}

#[test]
fn the_proxy_challenge_gets_the_credential() {
    let (auth, link) = auth();
    auth.on_event(&challenge("Proxy", ORIGIN));
    assert_eq!(
        sent_without_ids(&link),
        [json!({
            "method": "Fetch.continueWithAuth",
            "params": {
                "requestId": "interception-7",
                "authChallengeResponse": {
                    "response": "ProvideCredentials",
                    "username": "clauseo",
                    "password": SECRET,
                },
            },
        })]
    );
}

#[test]
fn any_other_challenge_is_cancelled() {
    let (auth, link) = auth();
    for (source, origin) in [
        ("Server", ORIGIN),
        ("Server", "https://example.com"),
        ("Proxy", "http://127.0.0.1:61201"),
        ("Proxy", "http://127.0.0.1:612000"),
        ("Proxy", "http://localhost:61200"),
        ("Proxy", "http://127.0.0.1:61200/"),
        ("proxy", ORIGIN),
    ] {
        auth.on_event(&challenge(source, origin));
        let sent = sent_without_ids(&link);
        let params = &sent[0]["params"];
        assert_eq!(sent.len(), 1, "{source} {origin}");
        assert_eq!(params["requestId"], "interception-7");
        assert_eq!(
            params["authChallengeResponse"],
            json!({"response": "CancelAuth"}),
            "{source} {origin}"
        );
        assert!(!sent[0].to_string().contains(SECRET), "{source} {origin}");
    }
    // A challenge with no authChallenge at all is cancelled too.
    auth.on_event(&json!({"method": "Fetch.authRequired", "params": {"requestId": "r"}}));
    assert_eq!(
        link.take()[0]["params"]["authChallengeResponse"]["response"],
        "CancelAuth"
    );
}

#[test]
fn session_events_and_others_are_ignored() {
    let (auth, link) = auth();
    let mut scoped = challenge("Proxy", ORIGIN);
    scoped["sessionId"] = json!("S1");
    auth.on_event(&scoped);
    let mut paused = json!({"method": "Fetch.requestPaused", "params": {"requestId": "1"}});
    paused["sessionId"] = json!("S1");
    auth.on_event(&paused);
    auth.on_event(&json!({"method": "Page.loadEventFired", "params": {}}));
    auth.on_event(&json!({"method": "Fetch.requestPaused", "params": {}}));
    assert_eq!(link.take(), Vec::<Value>::new());
}

/// An empty or `null` `sessionId` is the root session's, as Chromium and the relay read it.
#[test]
fn an_empty_or_null_session_is_the_roots() {
    let (auth, link) = auth();
    for session in [json!(""), Value::Null] {
        let mut paused = json!({"method": "Fetch.requestPaused", "params": {"requestId": "4"}});
        paused["sessionId"] = session.clone();
        auth.on_event(&paused);
        let sent = sent_without_ids(&link);
        assert_eq!(sent.len(), 1, "{session}");
        assert_eq!(sent[0]["method"], "Fetch.continueRequest", "{session}");
    }
}

/// Each answer takes a fresh id from the seam's one counter.
#[test]
fn each_answer_has_a_guard_id_of_its_own() {
    let (auth, link) = auth();
    let paused = json!({"method": "Fetch.requestPaused", "params": {"requestId": "1"}});
    auth.on_event(&paused);
    auth.on_event(&paused);
    let ids: Vec<i64> = link
        .take()
        .iter()
        .map(|sent| sent["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().all(|id| is_guard_id(*id)), "{ids:?}");
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn debug_hides_the_password() {
    let (auth, _link) = auth();
    let text = format!("{auth:?}");
    assert!(!text.contains(SECRET), "{text}");
    assert!(text.contains("Password(<hidden>)"), "{text}");
    assert!(Password::parse(&SECRET[1..]).is_none());
    assert!(Password::parse(&SECRET.to_ascii_uppercase()).is_none());
}

/// The `Proxy-Authorization` value Chrome sends: base64 of `clauseo:<password>`.
#[test]
fn the_basic_credential_is_the_base64_of_the_user_and_password() {
    let password = Password::parse(SECRET).unwrap();
    let basic = password.basic();
    assert_eq!(basic.len(), 96);
    assert!(basic.starts_with("Y2xhdXNlbzow"), "clauseo:0 in base64");
}
