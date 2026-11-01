//! The client's calls, notifications and failures against the scripted app-server.

use clauseo_codex_appserver::{
    Account, ClientInfo, Error, GetAccountRateLimitsResponse, GetAccountResponse,
    InitializeResponse, Notification, PlanType, RateLimitReachedType, RateLimitResetCreditStatus,
    RateLimitResetType,
};
use clauseo_protocol::ErrorCode;
use futures::StreamExt;
use serde_json::json;

use crate::peer::{MAX_LINE_BYTES, debug_lines, drain, fixture, pipes, reader_params};

#[test]
fn the_reader_handshake_and_reads_decode_the_schema_fixtures() {
    let (client, _notifications, driver, mut server) = pipes();
    let calls = async move {
        let init = client.initialize(ClientInfo::clauseo()).await.unwrap();
        let account = client.account().await.unwrap();
        let limits = client.rate_limits(reader_params()).await.unwrap();
        (init, account, limits)
    };
    let script = async move {
        let init = server.request("initialize").await;
        assert_eq!(
            init["params"],
            json!({"clientInfo": {"name": "clauseo", "version": env!("CARGO_PKG_VERSION")},
                   "capabilities": {}})
        );
        server.respond(&init, &fixture("initialize_response.json"));
        assert_eq!(server.read().await, json!({"method": "initialized"}));
        let account = server.request("account/read").await;
        assert_eq!(account["params"], json!({}));
        server.respond(&account, &fixture("account_response_chatgpt.json"));
        let limits = server.request("account/rateLimits/read").await;
        assert_eq!(limits["params"], json!({"excludeResetCreditDetails": true}));
        server.respond(&limits, &fixture("rate_limits_response.json"));
        server.read_end().await;
    };
    let (ended, (init, account, limits), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(
        ended,
        Ok(()),
        "the driver ends cleanly once the client is dropped"
    );
    let expected: InitializeResponse =
        serde_json::from_value(fixture("initialize_response.json")).unwrap();
    assert_eq!(init, expected);
    let Some(Account::Chatgpt(chatgpt)) = account.account else {
        panic!("expected a ChatGPT account, got {:?}", account.account);
    };
    assert_eq!(chatgpt.plan_type, PlanType::Pro);
    assert!(account.requires_openai_auth);
    let codex = &limits.rate_limits_by_limit_id.as_ref().unwrap()["codex"];
    assert_eq!(
        codex.primary.as_ref().unwrap().window_duration_mins,
        Some(300)
    );
    assert_eq!(limits.account_id.as_deref(), Some("account-1"));
    let expected: GetAccountRateLimitsResponse =
        serde_json::from_value(fixture("rate_limits_response.json")).unwrap();
    assert_eq!(limits, expected);
}

#[test]
fn responses_out_of_order_match_by_id_and_an_update_between_them_is_streamed() {
    let (client, mut notifications, driver, mut server) = pipes();
    let calls = async move {
        let (account, limits) =
            futures::join!(client.account(), client.rate_limits(reader_params()));
        (account.unwrap(), limits.unwrap())
    };
    let script = async move {
        let first = server.read().await;
        let second = server.read().await;
        let (account, limits) = if first["method"] == "account/read" {
            (first, second)
        } else {
            (second, first)
        };
        // The second request is answered first, and this answer carries `jsonrpc`.
        server.send(&json!({"jsonrpc": "2.0", "id": limits["id"],
                            "result": fixture("rate_limits_response.json")}));
        server.send(&json!({"method": "account/rateLimits/updated",
                            "params": fixture("rate_limits_updated.json")}));
        server.respond(&account, &fixture("account_response_chatgpt.json"));
        server.read_end().await;
    };
    let (ended, (account, limits), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(ended, Ok(()));
    let expected: GetAccountResponse =
        serde_json::from_value(fixture("account_response_chatgpt.json")).unwrap();
    assert_eq!(account, expected);
    assert_eq!(limits.rate_limits.limit_id.as_deref(), Some("codex"));
    let update = serde_json::from_value(fixture("rate_limits_updated.json")).unwrap();
    assert_eq!(
        drain(&mut notifications),
        [Notification::RateLimitsUpdated(Box::new(update))]
    );
}

#[test]
fn an_unknown_notification_arrives_as_other_with_a_debug_line() {
    let (client, mut notifications, driver, mut server) = pipes();
    let calls = async move { client.account().await.unwrap() };
    let script = async move {
        let account = server.request("account/read").await;
        server.send(&json!({"method": "thread/started", "params": {"thread": {"id": "t-1"}}}));
        server.respond(&account, &fixture("account_response_chatgpt.json"));
        server.read_end().await;
    };
    let lines = debug_lines(|| {
        let (ended, _, ()) =
            futures::executor::block_on(async { futures::join!(driver, calls, script) });
        assert_eq!(ended, Ok(()));
    });
    assert_eq!(
        drain(&mut notifications),
        [Notification::Other {
            method: "thread/started".to_owned()
        }]
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("method=\"thread/started\"")),
        "{lines:?}"
    );
}

#[test]
fn a_request_from_the_server_gets_method_not_found_and_the_connection_goes_on() {
    let (client, _notifications, driver, mut server) = pipes();
    let calls = async move { client.account().await.unwrap() };
    let script = async move {
        let account = server.request("account/read").await;
        server.send(
            &json!({"id": "server-1", "method": "item/tool/requestUserInput",
                            "params": {}}),
        );
        let refusal = server.read().await;
        assert_eq!(refusal["id"], "server-1");
        assert_eq!(refusal["error"]["code"], -32601);
        assert!(refusal.get("result").is_none());
        server.respond(&account, &fixture("account_response_chatgpt.json"));
        server.read_end().await;
    };
    let (ended, _, ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(ended, Ok(()));
}

#[test]
fn unknown_fields_are_ignored_and_unknown_enum_values_keep_their_string() {
    let (client, _notifications, driver, mut server) = pipes();
    let calls = async move {
        let account = client.account().await.unwrap();
        let limits = client.rate_limits(reader_params()).await.unwrap();
        (account, limits)
    };
    let script = async move {
        let account = server.request("account/read").await;
        server.respond(
            &account,
            &json!({"account": {"type": "enterpriseSso", "tenant": "x"},
                    "requiresOpenaiAuth": false, "newField": [1, 2]}),
        );
        let limits = server.request("account/rateLimits/read").await;
        server.respond(
            &limits,
            &json!({
                "rateLimits": {"planType": "pro_ultra", "rateLimitReachedType": "planet_depleted",
                               "primary": {"usedPercent": 1, "burst": true}},
                "rateLimitResetCredits": {"availableCount": 1, "credits": [{
                    "grantedAt": 1, "id": "c-1", "resetType": "weeklyReset",
                    "status": "expired", "shiny": "yes"}]},
                "extra": {"nested": true}
            }),
        );
        server.read_end().await;
    };
    let (ended, (account, limits), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(ended, Ok(()));
    assert_eq!(
        account.account,
        Some(Account::Unknown("enterpriseSso".to_owned()))
    );
    let snapshot = &limits.rate_limits;
    assert_eq!(
        snapshot.plan_type,
        Some(PlanType::Unknown("pro_ultra".to_owned()))
    );
    assert_eq!(
        snapshot.rate_limit_reached_type,
        Some(RateLimitReachedType::Unknown("planet_depleted".to_owned()))
    );
    assert_eq!(snapshot.primary.as_ref().unwrap().used_percent, 1);
    let credit = &limits.rate_limit_reset_credits.unwrap().credits.unwrap()[0];
    assert_eq!(
        credit.reset_type,
        RateLimitResetType::Unknown("weeklyReset".to_owned())
    );
    assert_eq!(
        credit.status,
        RateLimitResetCreditStatus::Unknown("expired".to_owned())
    );
    // The schema's own "unknown" is a named value, not the catch-all.
    assert_eq!(PlanType::from_wire("unknown"), PlanType::CodexUnknown);
}

#[test]
fn an_error_response_becomes_rpc_and_a_bad_result_fails_only_its_call() {
    let (client, _notifications, driver, mut server) = pipes();
    let calls = async move {
        let refused = client.account().await;
        let malformed = client.account().await;
        let fine = client.account().await;
        (refused, malformed, fine)
    };
    let script = async move {
        let first = server.request("account/read").await;
        server.send(&json!({"id": first["id"],
                            "error": {"code": -32600, "message": "not signed in", "data": 1}}));
        let second = server.request("account/read").await;
        server.respond(&second, &json!({"account": null}));
        let third = server.request("account/read").await;
        server.respond(&third, &fixture("account_response_chatgpt.json"));
        server.read_end().await;
    };
    let (ended, (refused, malformed, fine), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(ended, Ok(()));
    let refused = refused.unwrap_err();
    assert_eq!(
        refused,
        Error::Rpc {
            code: -32600,
            message: "not signed in".to_owned()
        }
    );
    assert_eq!(refused.code(), ErrorCode::AgentProtocolError);
    assert!(
        matches!(malformed, Err(Error::Protocol(_))),
        "{malformed:?}"
    );
    assert!(fine.is_ok(), "{fine:?}");
}

/// Two calls are pending when the server sends `bad`; both fail with `Error::Protocol`, as does
/// the driver.
fn pending_calls_fail_with_protocol(bad: Vec<u8>) {
    let (client, _notifications, driver, mut server) = pipes();
    let calls =
        async move { futures::join!(client.account(), client.rate_limits(reader_params())) };
    let script = async move {
        server.read().await;
        server.read().await;
        server.send_raw(&bad);
        server
    };
    let (ended, (account, limits), _server) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    let Err(Error::Protocol(message)) = ended else {
        panic!("expected the driver to end with Error::Protocol, got {ended:?}");
    };
    assert_eq!(account, Err(Error::Protocol(message.clone())));
    assert_eq!(limits, Err(Error::Protocol(message)));
}

#[test]
fn a_line_that_is_not_json_fails_every_pending_call_and_the_driver() {
    pending_calls_fail_with_protocol(b"codex: starting up\n".to_vec());
}

#[test]
fn json_that_is_not_a_message_fails_every_pending_call_and_the_driver() {
    pending_calls_fail_with_protocol(b"[1, 2]\n".to_vec());
}

#[test]
fn a_line_over_8_mib_fails_every_pending_call_and_the_driver() {
    let mut line = vec![b' '; MAX_LINE_BYTES + 1];
    line.push(b'\n');
    pending_calls_fail_with_protocol(line);
}

#[test]
fn a_line_of_exactly_8_mib_is_read() {
    let (client, _notifications, driver, mut server) = pipes();
    let calls = async move { client.account().await };
    let script = async move {
        let request = server.request("account/read").await;
        let mut line = serde_json::to_vec(
            &json!({"id": request["id"], "result": fixture("account_response_chatgpt.json")}),
        )
        .unwrap();
        line.resize(MAX_LINE_BYTES, b' ');
        line.push(b'\n');
        server.send_raw(&line);
        server.read_end().await;
    };
    let (ended, account, ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(ended, Ok(()));
    assert!(account.is_ok(), "{account:?}");
}

#[test]
fn stdout_closing_fails_every_pending_call_with_closed() {
    let (client, mut notifications, driver, mut server) = pipes();
    let late = client.clone();
    let calls =
        async move { futures::join!(client.account(), client.rate_limits(reader_params())) };
    let script = async move {
        server.read().await;
        server.read().await;
        server.exit();
    };
    let (ended, (account, limits), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(ended, Err(Error::Closed));
    assert_eq!(account, Err(Error::Closed));
    assert_eq!(limits, Err(Error::Closed));
    assert_eq!(Error::Closed.code(), ErrorCode::AgentExited);
    // A call after the driver ended fails at once, and the stream ends.
    assert_eq!(
        futures::executor::block_on(late.account()),
        Err(Error::Closed)
    );
    assert_eq!(futures::executor::block_on(notifications.next()), None);
}

#[test]
fn a_dropped_notification_stream_discards_updates_without_stopping_the_connection() {
    let (client, notifications, driver, mut server) = pipes();
    drop(notifications);
    let calls = async move { client.account().await };
    let script = async move {
        let account = server.request("account/read").await;
        server.send(&json!({"method": "account/rateLimits/updated",
                            "params": fixture("rate_limits_updated.json")}));
        server.respond(&account, &fixture("account_response_chatgpt.json"));
        server.read_end().await;
    };
    let (ended, account, ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(ended, Ok(()));
    assert!(account.is_ok(), "{account:?}");
}
