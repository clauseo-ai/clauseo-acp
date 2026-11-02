//! `classify_tool_result` on the MCP fixtures.

use clauseo_connectors::fake::fixture;
use clauseo_connectors::{
    CASE_LAW, Origin, ToolOutcome, classify_tool_result, classify_tool_result_for,
};
use serde_json::{Value, json};

fn load(name: &str) -> Value {
    serde_json::from_str(fixture(name).unwrap()).unwrap()
}

#[test]
fn insufficient_credits_is_out_of_credit_with_the_recharge_and_the_pay_link() {
    let result = load("mcp/insufficient-credits-result.json");
    let ToolOutcome::OutOfCredit(payment) = classify_tool_result(&CASE_LAW, &result) else {
        panic!("not out of credit");
    };
    assert_eq!(payment.recharge_id.as_deref(), Some("rch_example0001"));
    assert_eq!(
        payment.url.unwrap().as_str(),
        "https://data.clauseo.chat/pay/rch_example0001"
    );
    // A whole JSON-RPC response is read through its `result`.
    let response = json!({"jsonrpc": "2.0", "id": 3, "result": result});
    assert!(matches!(
        classify_tool_result(&CASE_LAW, &response),
        ToolOutcome::OutOfCredit(_)
    ));
}

#[test]
fn a_pay_link_on_another_host_gives_no_url() {
    let text = fixture("mcp/insufficient-credits-result.json")
        .unwrap()
        .replace(
            "https://data.clauseo.chat/pay/",
            "https://data.clauseo.chat.evil.example/pay/",
        );
    let result: Value = serde_json::from_str(&text).unwrap();
    let ToolOutcome::OutOfCredit(payment) = classify_tool_result(&CASE_LAW, &result) else {
        panic!("not out of credit");
    };
    assert_eq!(payment.recharge_id.as_deref(), Some("rch_example0001"));
    assert_eq!(payment.url, None);
}

#[test]
fn ordinary_results_are_ok_and_other_errors_are_other() {
    assert_eq!(
        classify_tool_result(&CASE_LAW, &load("mcp/search-law-result.json")),
        ToolOutcome::Ok
    );
    assert_eq!(
        classify_tool_result(&CASE_LAW, &load("mcp/read-document-result.json")),
        ToolOutcome::Ok
    );
    let others = [
        json!({"isError": true, "content": []}),
        json!({"isError": true, "structuredContent": {"error": {"code": "rate_limited"}}}),
        json!({"isError": true, "structuredContent": "insufficient_credits"}),
    ];
    for result in others {
        assert_eq!(
            classify_tool_result(&CASE_LAW, &result),
            ToolOutcome::OtherError,
            "{result}"
        );
    }
    let mut not_error = load("mcp/insufficient-credits-result.json");
    not_error["isError"] = json!(false);
    assert_eq!(classify_tool_result(&CASE_LAW, &not_error), ToolOutcome::Ok);
    assert_eq!(
        classify_tool_result(&CASE_LAW, &json!(null)),
        ToolOutcome::Ok
    );
}

#[test]
fn under_staging_only_the_staging_pay_link_is_kept() {
    let result = load("mcp/insufficient-credits-result.json");
    let ToolOutcome::OutOfCredit(payment) =
        classify_tool_result_for(&CASE_LAW, &Origin::Staging, &result)
    else {
        panic!("not out of credit");
    };
    assert_eq!(
        payment.url, None,
        "a production link under staging is not ours"
    );
    let staged: Value = serde_json::from_str(
        &fixture("mcp/insufficient-credits-result.json")
            .unwrap()
            .replace(
                "https://data.clauseo.chat",
                "https://data-staging.clauseo.chat",
            ),
    )
    .unwrap();
    let ToolOutcome::OutOfCredit(payment) =
        classify_tool_result_for(&CASE_LAW, &Origin::Staging, &staged)
    else {
        panic!("not out of credit");
    };
    assert_eq!(
        payment.url.unwrap().as_str(),
        "https://data-staging.clauseo.chat/pay/rch_example0001"
    );
}
