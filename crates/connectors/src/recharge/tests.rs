use super::*;
use crate::known::CASE_LAW;

const CREATED: &[u8] = include_bytes!("../../fixtures/recharge-created.json");
const PAID: &[u8] = include_bytes!("../../fixtures/recharge-paid.json");
const API_402: &[u8] = include_bytes!("../../fixtures/api-402.json");

#[test]
fn the_recharge_fixtures_parse() {
    let created = parse_recharge(CREATED, &CASE_LAW, &Origin::Production).unwrap();
    assert_eq!(created.recharge_id, "rch_example0001");
    assert_eq!(created.status, RechargeStatus::AwaitingPayment);
    assert_eq!(
        created.url.as_ref().map(PayUrl::as_str),
        Some("https://data.clauseo.chat/pay/rch_example0001")
    );
    assert_eq!(
        created.amount,
        Some(Amount {
            net_inr: "20.00".to_owned(),
            gst_inr: "3.60".to_owned(),
            gross_inr: "23.60".to_owned(),
        })
    );
    assert_eq!(created.credits, Some(200));
    assert_eq!(created.expires_at.as_deref(), Some("2026-11-02T12:30:00Z"));
    let paid = parse_recharge(PAID, &CASE_LAW, &Origin::Production).unwrap();
    assert_eq!(paid.status, RechargeStatus::Paid);
}

#[test]
fn a_recharge_under_a_payment_key_parses_the_same() {
    let wrapped = format!(
        r#"{{"payment":{},"extra":1}}"#,
        String::from_utf8_lossy(CREATED)
    );
    assert_eq!(
        parse_recharge(wrapped.as_bytes(), &CASE_LAW, &Origin::Production).unwrap(),
        parse_recharge(CREATED, &CASE_LAW, &Origin::Production).unwrap()
    );
}

#[test]
fn a_foreign_pay_link_is_refused_and_no_link_is_returned() {
    let body = String::from_utf8_lossy(CREATED)
        .replace("https://data.clauseo.chat", "http://data.clauseo.chat");
    assert_eq!(
        parse_recharge(body.as_bytes(), &CASE_LAW, &Origin::Production),
        Err(Error::NotOurPayLink)
    );
}

#[test]
fn an_unknown_status_and_a_missing_id() {
    let other = parse_recharge(
        br#"{"recharge_id":"rch_1","status":"expired","amount":{"net_inr":20,"x":1}}"#,
        &CASE_LAW,
        &Origin::Production,
    )
    .unwrap();
    assert_eq!(other.status, RechargeStatus::Other("expired".to_owned()));
    assert_eq!(other.url, None);
    assert_eq!(other.amount.unwrap().net_inr, "20");
    assert!(matches!(
        parse_recharge(br#"{"status":"paid"}"#, &CASE_LAW, &Origin::Production),
        Err(Error::Parse(_))
    ));
}

#[test]
fn the_402_body_gives_insufficient_credits_with_its_payment() {
    let err = insufficient_credits_402(API_402, &CASE_LAW, &Origin::Production);
    let Error::InsufficientCredits(payment) = err else {
        panic!("{err:?}");
    };
    assert_eq!(payment.recharge_id.as_deref(), Some("rch_example0001"));
    assert_eq!(
        payment.url.map(|url| url.as_str().to_owned()).as_deref(),
        Some("https://data.clauseo.chat/pay/rch_example0001")
    );
    assert!(is_insufficient_credits(
        &serde_json::from_slice(API_402).unwrap()
    ));
    assert_eq!(
        insufficient_credits_402(b"not json", &CASE_LAW, &Origin::Production),
        Error::InsufficientCredits(PaymentRef::default())
    );
}
