//! The session key (Windows only): the guard's part of the key the patched agent-browser's
//! daemon checks on every connection (`env.rs` `SESSION_KEY`).

use std::path::Path;

use super::super::{
    CDP, Os, SESSION_KEY, WindowsDirs, agent_env, build, chrome_env, new_session_key,
};
use super::{get, source_for};
use crate::browser_guard::relay::redact::{MARK, Redactor, Secrets};

/// Drawn fresh from the OS's random source, given only to the agent-browser connected to the
/// browser (which the guard creates in its job), never to the one-shot or Chrome, never
/// taken from the host, and taken out of agent-browser's output.
#[test]
fn the_session_key_goes_only_to_the_connected_agent_browser_and_never_out() {
    let key = new_session_key().unwrap();
    assert_eq!(key.len(), 64);
    assert!(
        key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "{key}"
    );
    assert_ne!(key, new_session_key().unwrap());

    let cwd = Path::new(r"C:\state\browser");
    let given = [(SESSION_KEY, "from the host")];
    let base = build(
        &source_for(Os::Windows, &given, ""),
        Os::Windows,
        cwd,
        &WindowsDirs::default(),
    )
    .unwrap();
    assert!(
        !base.contains_key(SESSION_KEY),
        "the host's value must not pass"
    );

    let endpoint = "ws://127.0.0.1:1024/devtools/browser/0-a";
    let agent = agent_env(&base, endpoint, Some(&key));
    assert_eq!(get(&agent, SESSION_KEY), key);
    assert_eq!(get(&agent, CDP), endpoint);
    assert!(!chrome_env(&agent).contains_key(SESSION_KEY));
    assert!(!agent_env(&base, endpoint, None).contains_key(SESSION_KEY));

    let mut redactor = Redactor::new(Secrets::of(&agent).unwrap());
    let mut out = redactor.push(format!("daemon said {key}\n").as_bytes());
    out.extend(redactor.finish());
    let mark = String::from_utf8(MARK.to_vec()).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        format!("daemon said {mark}\n")
    );
}
