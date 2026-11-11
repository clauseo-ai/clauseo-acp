//! Iris's MED-1 on PR B: an agent that ends before its session is ready has its last stderr
//! line logged (`start_failure`), and that is when `session/new` held a lawyer's remote server.
//! The URL (whole, its host, its path and query tokens, a key-like path segment, a query value
//! decoded) and every header value, from the keychain or plain (as it is, JSON-escaped,
//! percent-encoded, a `Basic` credential decoded), reach the stderr redactor through
//! `lawyer::redacted`.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{DetachReason, Timestamp};
use clauseo_settings::{Effective, Locations, SystemFiles};
use futures::channel::mpsc;
use futures::executor::LocalPool;
use serde_json::json;

use super::{ended, fake_agent, scenario};
use crate::agent::loads::lawyer::{self, McpSecrets};
use crate::agent::{AgentInput, Ended, launch_spec, start};
use crate::clock::ManualClock;

const PATH_TOKEN: &str = "p4thToken77";
const KEY_SEGMENT: &str = "k3yLikeSegment0123";
const QUERY_TOKEN: &str = "q7%2BQuery%2FToken";
const QUERY_DECODED: &str = "q7+Query/Token";
/// `user1:password1`, as a `Basic` header from the keychain.
const BASIC: &str = "Basic dXNlcjE6cGFzc3dvcmQx";
const PLAIN_HEADER: &str = "firm\"tenant/0042";

/// Settings with one remote server: a token in its URL's path and query and a key-like path
/// segment, a `Basic` header from the keychain and a plain one holding a quote and a slash.
fn effective() -> Effective {
    let root = std::env::temp_dir().join(format!("clauseo-redact-mcp-{}", std::process::id()));
    crate::write::create_dir_all(&root).unwrap();
    let url = format!(
        "https://mcp.example.com/v1/{PATH_TOKEN}/{KEY_SEGMENT}/mcp?key={QUERY_TOKEN}&region=uk"
    );
    let servers = json!({"docs": {"url": url, "enabled": true, "headers": {
        "Authorization": {"keychain": true}, "X-Tenant": PLAIN_HEADER}}});
    let text = json!({"mcp": {"servers": servers}}).to_string();
    crate::write::write_file(&root.join("settings.json"), text.as_bytes()).unwrap();
    let effective = Effective::load(&Locations::under_root(&root), &SystemFiles, &[]);
    crate::write::remove_dir_all(&root).unwrap();
    assert_eq!(effective.warnings(), []);
    effective
}

#[test]
fn a_remote_servers_url_and_header_values_are_redacted_from_the_stderr_tail() {
    let secrets = McpSecrets::of(&[("docs/header/Authorization", BASIC)], &[]);
    let redacted = lawyer::redacted(&effective(), &secrets);
    let spec = launch_spec(
        &fake_agent(scenario("stderr-mcp-remote")),
        BTreeMap::new(),
        BTreeMap::new(),
        &[],
        std::iter::empty(),
        std::env::temp_dir(),
        &[],
    )
    .unwrap()
    .with_redacted(redacted);
    let mut pool = LocalPool::new();
    let (tx, mut rx) = mpsc::unbounded();
    let post: Rc<dyn Fn(AgentInput)> = Rc::new(move |input| tx.unbounded_send(input).unwrap());
    let _link = start(
        &pool.spawner(),
        &PassthroughLauncher::new(SpawnOptions::new()),
        &spec,
        Arc::new(ManualClock::new(Timestamp::ZERO)),
        post,
        &mut None,
    )
    .unwrap();
    let Ended::Exited(DetachReason::Exited { code, stderr_tail }) = pool.run_until(ended(&mut rx))
    else {
        panic!("the agent did not exit on its own")
    };
    assert_eq!(code, Some(1));
    for secret in [
        PATH_TOKEN,
        KEY_SEGMENT,
        QUERY_TOKEN,
        QUERY_DECODED,
        "mcp.example.com",
        "dXNlcjE6cGFzc3dvcmQx",
        "user1:password1",
        "password1",
        PLAIN_HEADER,
        "firm\\\"tenant/0042",
        "firm%22tenant%2F0042",
    ] {
        assert!(!stderr_tail.contains(secret), "{secret} in {stderr_tail}");
    }
    assert!(stderr_tail.contains("[redacted]"), "{stderr_tail}");
    assert!(
        stderr_tail.contains("stderr-mcp-remote: last line"),
        "{stderr_tail}"
    );
}
