//! `Origin::Local` (feature `local`) against the fake on the local test server's own address,
//! `http://127.0.0.1:8793`: discovery, the issuer and `iss`, the endpoints, the requests'
//! `Host` and `Origin`, and the pay link. It needs the port free: stop the real local server
//! (the Mac mini's) before running it.

use std::sync::Arc;

use clauseo_connectors::fake::{FakeConnector, FakeHandle, FakeOptions, ScratchDir};
use clauseo_connectors::{
    CASE_LAW, ConnectorHandle, Connectors, LOCAL_ORIGIN, McpRequestHeaders, MemoryStore, Origin,
    PayUrl, SignInError,
};

use super::support::complete;

/// The fake on 8793 and a client on `Origin::Local`.
fn local_rig() -> (FakeHandle, Connectors, ConnectorHandle, ScratchDir) {
    let fake = FakeConnector::start_on(8793, FakeOptions::default())
        .expect("127.0.0.1:8793 is taken: stop the local server to run this test");
    assert_eq!(fake.base(), LOCAL_ORIGIN);
    let scratch = ScratchDir::new("local").unwrap();
    let mut config = fake.config(scratch.state_file());
    config.origin = Origin::Local;
    let store = Arc::new(MemoryStore::new());
    let connectors =
        Connectors::new(config, store as Arc<dyn clauseo_connectors::SecretStore>).unwrap();
    let handle = connectors.get("case-law").unwrap();
    (fake, connectors, handle, scratch)
}

/// One test, so the fixed port is bound once at a time.
#[test]
fn local_signs_in_on_exactly_its_own_http_origin() {
    let (fake, _connectors, handle, _scratch) = local_rig();

    // An endpoint on another http host is refused at discovery (before
    // any discovery passes: its metadata is then kept).
    let good = fake.as_metadata();
    let mut bad = good.clone();
    bad["token_endpoint"] = "http://127.0.0.2:8793/token".into();
    fake.set_as_metadata(bad);
    assert!(handle.begin_sign_in(None).is_err());
    let mut bad = good.clone();
    bad["issuer"] = "http://localhost:8793".into();
    fake.set_as_metadata(bad);
    assert!(handle.begin_sign_in(None).is_err());
    fake.set_as_metadata(good);

    // A different http host in `iss` is refused.
    fake.wrong_iss("http://127.0.0.2:8793");
    let pending = handle.begin_sign_in(None).unwrap();
    let (_, result) = complete(&fake, pending);
    assert_eq!(result, Err(SignInError::IssuerMismatch));

    // The issuer and `iss` exactly `http://127.0.0.1:8793`: the sign-in completes.
    fake.wrong_iss(LOCAL_ORIGIN);
    let pending = handle.begin_sign_in(None).unwrap();
    let authorize = pending.authorize_url().clone();
    assert_eq!(authorize.origin().ascii_serialization(), LOCAL_ORIGIN);
    assert_eq!(authorize.path(), "/authorize");
    let client_id = authorize
        .query_pairs()
        .find(|(key, _)| key == "client_id")
        .map(|(_, value)| value.into_owned());
    assert_eq!(
        client_id.as_deref(),
        Some("http://127.0.0.1:8793/oauth/clients/clauseo-desktop.json")
    );
    let (page, result) = complete(&fake, pending);
    assert_eq!(page.status, 200);
    result.unwrap();

    // The pay link: exactly `http://127.0.0.1:8793/pay/<id>`, kept as is.
    let recharge = handle.create_recharge("200").unwrap();
    let url = recharge.url.unwrap();
    assert_eq!(
        url.as_str(),
        format!("{LOCAL_ORIGIN}/pay/{}", recharge.recharge_id)
    );
    assert!(url.is_for(&CASE_LAW, &Origin::Local));
    assert!(!url.is_for(&CASE_LAW, &Origin::Production));

    // An MCP call, as the relay makes them.
    let reply = handle
        .mcp_post(
            br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            &McpRequestHeaders::default(),
        )
        .unwrap();
    assert_eq!(reply.status, 200);
    assert_eq!(fake.requests_to("/mcp/case-law").len(), 1);

    // Every request went to `127.0.0.1:8793` by name, with no `Origin` header.
    let requests = fake.requests();
    assert!(requests.len() > 5, "{} requests", requests.len());
    for request in &requests {
        assert_eq!(
            request.header("host"),
            Some("127.0.0.1:8793"),
            "{}",
            request.path
        );
        assert_eq!(request.header("origin"), None, "{}", request.path);
    }
    assert!(PayUrl::parse_for(&CASE_LAW, &Origin::Local, url.as_str()).is_some());
}
