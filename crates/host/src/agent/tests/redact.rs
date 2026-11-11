//! Iris's MED-1 on PR B: an agent that ends before its session is ready has its last stderr
//! line logged (`start_failure`), and that is when `session/new` held a lawyer's remote server.
//! The URL (a token in its path and its query) and every header value, from the keychain or
//! plain, reach the stderr redactor through `lawyer::redacted`.

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
const QUERY_TOKEN: &str = "q7QueryToken12";
const KEYCHAIN_HEADER: &str = "x5HeaderToken77";
const PLAIN_HEADER: &str = "firm-tenant-0042";

/// Settings with one remote server: a token in its URL's path and query, one header from the
/// keychain and one plain.
fn effective() -> Effective {
    let root = std::env::temp_dir().join(format!("clauseo-redact-mcp-{}", std::process::id()));
    crate::write::create_dir_all(&root).unwrap();
    let url = format!("https://mcp.example.com/v1/{PATH_TOKEN}/mcp?key={QUERY_TOKEN}&region=uk");
    let servers = json!({"docs": {"url": url, "enabled": true, "headers": {
        "X-Api-Key": {"keychain": true}, "X-Tenant": PLAIN_HEADER}}});
    let text = json!({"mcp": {"servers": servers}}).to_string();
    crate::write::write_file(&root.join("settings.json"), text.as_bytes()).unwrap();
    let effective = Effective::load(&Locations::under_root(&root), &SystemFiles, &[]);
    crate::write::remove_dir_all(&root).unwrap();
    assert_eq!(effective.warnings(), []);
    effective
}

#[test]
fn a_remote_servers_url_and_header_values_are_redacted_from_the_stderr_tail() {
    let secrets = McpSecrets::of(&[("docs/header/X-Api-Key", KEYCHAIN_HEADER)], &[]);
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
        QUERY_TOKEN,
        KEYCHAIN_HEADER,
        PLAIN_HEADER,
        "mcp.example.com",
    ] {
        assert!(!stderr_tail.contains(secret), "{secret} in {stderr_tail}");
    }
    assert!(stderr_tail.contains("[redacted]"), "{stderr_tail}");
    assert!(
        stderr_tail.contains("stderr-mcp-remote: last line"),
        "{stderr_tail}"
    );
}
