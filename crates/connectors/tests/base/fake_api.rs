//! The fake's MCP endpoint, JSON API, pricing and knobs.

use std::time::Duration;

use clauseo_connectors::fake::{FakeConnector, FakeOptions, Fixture};
use clauseo_connectors::{CASE_LAW, Origin, ToolOutcome, classify_tool_result_for};
use serde_json::{Value, json};

use crate::support::*;

fn call(name: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": name, "arguments": {}}})
}

#[test]
fn initialize_and_tools_list_need_no_token_and_tools_call_does() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let init = post_json(
        &fake,
        "/mcp/case-law",
        None,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
    );
    assert_eq!(init.status, 200);
    assert_eq!(init.header("mcp-session-id"), Some("fake-session-0001"));
    let body: Value = serde_json::from_slice(&init.body).unwrap();
    assert_eq!(body["id"], 1);
    assert_eq!(body["result"]["serverInfo"]["name"], "clauseo-case-law");

    let list = post_json(
        &fake,
        "/mcp/case-law",
        None,
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    );
    let body: Value = serde_json::from_slice(&list.body).unwrap();
    let names: Vec<&str> = body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "search_law",
            "read_document",
            "citator",
            "check_citations",
            "account",
            "recharge"
        ]
    );

    let notification = post_json(
        &fake,
        "/mcp/case-law",
        None,
        &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    );
    assert_eq!((notification.status, notification.body.len()), (202, 0));

    assert_eq!(
        post_json(&fake, "/mcp/case-law", None, &call("search_law")).status,
        401
    );
    let (access, _) = sign_in(&fake);
    let reply = post_json(&fake, "/mcp/case-law", Some(&access), &call("search_law"));
    let body: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(
        body["result"]["structuredContent"]["results"][0]["url"],
        "https://indiankanoon.org/doc/257876/"
    );
    assert_eq!(fake.balance(), 980, "search_page costs 20");
}

#[test]
fn sse_pushed_results_and_injected_failures() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let (access, _) = sign_in(&fake);
    fake.respond_as_sse(true);
    let reply = post_json(
        &fake,
        "/mcp/case-law",
        Some(&access),
        &call("read_document"),
    );
    assert_eq!(reply.header("content-type"), Some("text/event-stream"));
    let text = reply.text();
    let data = text
        .strip_prefix("event: message\ndata: ")
        .unwrap()
        .trim_end();
    let body: Value = serde_json::from_str(data).unwrap();
    assert_eq!(
        body["result"]["structuredContent"]["documents"][0]["title"],
        "Minerva Mills Ltd. v. Union of India (1980)"
    );
    fake.respond_as_sse(false);

    fake.push_tool_result(
        "citator",
        Fixture::Json(json!({"content": [], "isError": true})),
    );
    let reply = post_json(&fake, "/mcp/case-law", Some(&access), &call("citator"));
    let body: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(body["result"]["isError"], true);

    fake.fail_next("/v1/case-law/account", 500);
    assert_eq!(
        get(&fake, "/v1/case-law/account", Some(&access)).status,
        500
    );
    assert_eq!(
        get(&fake, "/v1/case-law/account", Some(&access)).status,
        200
    );
    fake.delay_next("/pricing.json", Duration::from_millis(50));
    assert_eq!(get(&fake, "/pricing.json", None).status, 200);
}

#[test]
fn every_request_is_recorded() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let _ = get(&fake, "/pricing.json?x=1", Some("tok"));
    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/pricing.json");
    assert_eq!(requests[0].query, "x=1");
    assert_eq!(requests[0].header("Authorization"), Some("Bearer tok"));
    fake.clear_requests();
    assert_eq!(fake.requests().len(), 0);
}

#[test]
fn pricing_answers_the_fixture_with_the_fake_s_origin_and_the_knob() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let body: Value = serde_json::from_slice(&get(&fake, "/pricing.json", None).body).unwrap();
    assert_eq!(body["status"], "live");
    assert_eq!(
        body["endpoints"]["mcp"],
        format!("{}/mcp/case-law", fake.base())
    );
    fake.set_pricing(Fixture::File("pricing-preview.json"));
    let body: Value = serde_json::from_slice(&get(&fake, "/pricing.json", None).body).unwrap();
    assert_eq!(body["status"], "preview");
    fake.set_pricing(Fixture::Raw {
        status: 200,
        content_type: "text/html".to_owned(),
        body: b"<html>".to_vec(),
    });
    assert_eq!(get(&fake, "/pricing.json", None).text(), "<html>");
}

#[test]
fn at_zero_credit_tools_and_the_api_point_to_a_payment() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let (access, _) = sign_in(&fake);
    fake.set_balance(0);
    let reply = post_json(&fake, "/mcp/case-law", Some(&access), &call("search_law"));
    let body: Value = serde_json::from_slice(&reply.body).unwrap();
    let origin = Origin::Fake(fake.base_url());
    let ToolOutcome::OutOfCredit(payment) =
        classify_tool_result_for(&CASE_LAW, &origin, &body["result"])
    else {
        panic!("{body}");
    };
    let id = payment.recharge_id.unwrap();
    assert_eq!(
        payment.url.unwrap().as_str(),
        format!("https://data.clauseo.chat/pay/{id}"),
        "the fake's pay link comes back in the production form"
    );
    assert_eq!(
        post_json(&fake, "/mcp/case-law", Some(&access), &call("account")).status,
        200,
        "free tools still answer"
    );

    let account = get(&fake, "/v1/case-law/account", Some(&access));
    assert_eq!(account.status, 402);
    let body: Value = serde_json::from_slice(&account.body).unwrap();
    assert_eq!(body["error"]["code"], "insufficient_credits");
    assert_eq!(
        body["payment"]["recharge_id"],
        id.as_str(),
        "the open recharge is reused"
    );
    assert_eq!(
        post_json(
            &fake,
            "/v1/case-law/recharges",
            Some(&access),
            &json!({"pack": "200"})
        )
        .status,
        402
    );

    assert!(fake.mark_paid(&id));
    assert_eq!(fake.balance(), 200);
    let account: Value =
        serde_json::from_slice(&get(&fake, "/v1/case-law/account", Some(&access)).body).unwrap();
    assert_eq!(account["balance_credits"], 200);
    assert_eq!(account["balance_inr"], "20.00");
    assert_eq!(account["open_payment"], Value::Null);
}

#[test]
fn a_recharge_is_created_and_a_wait_poll_returns_when_paid() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let (access, _) = sign_in(&fake);
    assert_eq!(
        post_json(
            &fake,
            "/v1/case-law/recharges",
            Some(&access),
            &json!({"pack": "300"})
        )
        .status,
        400
    );
    let created = post_json(
        &fake,
        "/v1/case-law/recharges",
        Some(&access),
        &json!({"pack": "500"}),
    );
    assert_eq!(created.status, 200);
    let body: Value = serde_json::from_slice(&created.body).unwrap();
    let id = body["recharge_id"].as_str().unwrap().to_owned();
    assert_eq!(body["status"], "awaiting_payment");
    assert_eq!(body["url"], format!("{}/pay/{id}", fake.base()));
    assert_eq!(body["amount"]["gross_inr"], "59.00");
    assert_eq!(fake.recharges(), std::slice::from_ref(&id));

    let path = format!("/v1/case-law/recharges/{id}");
    fake.set_wait_cap(Duration::from_millis(100));
    let pending: Value =
        serde_json::from_slice(&get(&fake, &format!("{path}?wait=40"), Some(&access)).body)
            .unwrap();
    assert_eq!(
        pending["status"], "awaiting_payment",
        "held for the cap, then answered"
    );

    fake.set_wait_cap(Duration::from_secs(10));
    std::thread::scope(|scope| {
        let poll = scope.spawn(|| get(&fake, &format!("{path}?wait=40"), Some(&access)));
        while fake.requests_to(&path).len() < 2 {
            std::thread::yield_now();
        }
        assert!(fake.mark_paid(&id));
        let paid: Value = serde_json::from_slice(&poll.join().unwrap().body).unwrap();
        assert_eq!(paid["status"], "paid");
    });
    assert_eq!(
        get(&fake, "/v1/case-law/recharges/rch_nope", Some(&access)).status,
        404
    );
}
