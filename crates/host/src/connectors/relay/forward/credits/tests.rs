//! Reading `_meta["chat.clauseo/credits"]`: present, absent, malformed, negative, a float, and on
//! error results.

use serde_json::value::RawValue;
use serde_json::{Value, json};

use super::{CREDITS_KEY, Credits, credits};

fn read(value: &Value) -> Option<Credits> {
    credits(&RawValue::from_string(value.to_string()).unwrap())
}

fn with(credits: &Value) -> Value {
    json!({"content": [{"type": "text", "text": "2 results"}], "isError": false,
        "_meta": {CREDITS_KEY: credits}})
}

fn both(charged: u32, balance: i64) -> Credits {
    Credits {
        charged: Some(charged),
        balance: Some(balance),
    }
}

#[test]
fn present_credits_are_read() {
    let value = with(&json!({"charged_credits": 40, "balance_credits": 1960}));
    assert_eq!(read(&value), Some(both(40, 1960)));
    // A free call: charged 0 and the balance.
    let free = with(&json!({"charged_credits": 0, "balance_credits": 1960}));
    assert_eq!(read(&free), Some(both(0, 1960)));
    // Other `_meta` keys and other members of the credits object are passed over.
    let more = json!({"isError": false, "_meta": {"other/key": [1, 2],
        CREDITS_KEY: {"charged_credits": 1, "balance_credits": 5, "note": "x"}}});
    assert_eq!(read(&more), Some(both(1, 5)));
}

#[test]
fn absent_credits_are_none() {
    for value in [
        json!({"content": [], "isError": false}),
        json!({"content": [], "_meta": {}}),
        json!({"content": [], "_meta": {"other/key": 1}}),
        json!({"content": [], "_meta": null}),
        json!({"content": [], "_meta": {CREDITS_KEY: null}}),
        json!({"content": [], "_meta": {CREDITS_KEY: [40, 1960]}}),
        json!({"content": [], "structuredContent": {"_meta": {CREDITS_KEY: {"charged_credits": 4}}}}),
        json!([1, 2]),
    ] {
        assert_eq!(read(&value), None, "{value}");
    }
    // The text never counts: only the result object's own `_meta`.
    let text = json!({"content": [{"type": "text",
        "text": "{\"_meta\": {\"chat.clauseo/credits\": {\"charged_credits\": 4}}}"}]});
    assert_eq!(read(&text), None);
}

#[test]
fn malformed_numbers_count_as_absent() {
    let none = Some(Credits::default());
    for bad in [
        json!(1.0),
        json!(1.5),
        json!("40"),
        json!(null),
        json!(true),
        json!({"n": 4}),
        json!(-1),
        json!(4_294_967_296_u64),
    ] {
        let value = with(&json!({"charged_credits": bad, "balance_credits": bad}));
        let read = read(&value).unwrap();
        assert_eq!(read.charged, None, "{value}");
        if bad != json!(-1) && bad != json!(4_294_967_296_u64) {
            assert_eq!(read.balance, None, "{value}");
        }
    }
    // An exponent is no whole number, though it names one.
    let raw =
        r#"{"_meta": {"chat.clauseo/credits": {"charged_credits": 1e2, "balance_credits": 2E3}}}"#;
    assert_eq!(
        credits(&RawValue::from_string(raw.to_owned()).unwrap()),
        none
    );
    // A duplicate key does not read.
    let dup =
        r#"{"_meta": {"chat.clauseo/credits": {"charged_credits": 1, "charged_credits": 2}}}"#;
    assert_eq!(
        credits(&RawValue::from_string(dup.to_owned()).unwrap()),
        None
    );
}

#[test]
fn a_negative_charge_is_absent_and_a_negative_balance_is_read() {
    let value = with(&json!({"charged_credits": -5, "balance_credits": -20}));
    assert_eq!(
        read(&value),
        Some(Credits {
            charged: None,
            balance: Some(-20),
        })
    );
}

#[test]
fn each_number_is_read_on_its_own() {
    let charge_only = with(&json!({"charged_credits": 0}));
    assert_eq!(
        read(&charge_only),
        Some(Credits {
            charged: Some(0),
            balance: None,
        })
    );
    let balance_only = with(&json!({"charged_credits": 2.5, "balance_credits": 7}));
    assert_eq!(
        read(&balance_only),
        Some(Credits {
            charged: None,
            balance: Some(7),
        })
    );
}

#[test]
fn error_results_carry_credits_too() {
    // Out of credits: refused, nothing charged, and the balance.
    let out = json!({"isError": true, "content": [{"type": "text", "text": "Out of credits"}],
        "structuredContent": {"error": "insufficient_credits"},
        "_meta": {CREDITS_KEY: {"charged_credits": 0, "balance_credits": 3}}});
    assert_eq!(read(&out), Some(both(0, 3)));
    // Upstream failure: refused, nothing charged, no balance.
    let failed = json!({"isError": true, "content": [],
        "_meta": {CREDITS_KEY: {"charged_credits": 0}}});
    assert_eq!(
        read(&failed),
        Some(Credits {
            charged: Some(0),
            balance: None,
        })
    );
}
