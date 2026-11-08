use super::*;
use crate::known::CASE_LAW;

const CREATED: &[u8] = include_bytes!("../../fixtures/recharge-created.json");
const PAID: &[u8] = include_bytes!("../../fixtures/recharge-paid.json");
const API_402: &[u8] = include_bytes!("../../fixtures/api-402.json");
const GET_AWAITING: &[u8] = include_bytes!("../../fixtures/recharge-get-awaiting.json");
const GET_PAID: &[u8] = include_bytes!("../../fixtures/recharge-get-paid.json");
const GET_EXPIRED: &[u8] = include_bytes!("../../fixtures/recharge-get-expired.json");
const GET_HELD: &[u8] = include_bytes!("../../fixtures/recharge-get-held.json");
/// The same four after the connector's change: a top-level `recharge_id` in every state.
const WITH_ID: [(&[u8], &[u8], &str); 4] = [
    (
        GET_AWAITING,
        include_bytes!("../../fixtures/recharge-get-awaiting-with-id.json"),
        "rch_8GGFDJKZ",
    ),
    (
        GET_PAID,
        include_bytes!("../../fixtures/recharge-get-paid-with-id.json"),
        "rch_8GGFDJKZ",
    ),
    (
        GET_EXPIRED,
        include_bytes!("../../fixtures/recharge-get-expired-with-id.json"),
        "rch_EFM1CF50",
    ),
    (
        GET_HELD,
        include_bytes!("../../fixtures/recharge-get-held-with-id.json"),
        "rch_H0LD0001",
    ),
];

fn poll(bytes: &[u8], asked: &str) -> Result<Recharge, Error> {
    parse_recharge_poll(bytes, &CASE_LAW, &Origin::Production, asked)
}

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

/// The connector's `GET /recharges/{id}` answers of 2026-11-06, as the poll reads them.
#[test]
fn the_poll_reads_the_connector_s_four_shapes() {
    let awaiting = poll(GET_AWAITING, "rch_8GGFDJKZ").unwrap();
    assert_eq!(awaiting.status, RechargeStatus::AwaitingPayment);
    assert!(!awaiting.status.settled());
    assert_eq!(
        awaiting.url.as_ref().map(PayUrl::as_str),
        Some("https://data.clauseo.chat/pay/rch_8GGFDJKZ")
    );
    assert_eq!(awaiting.credits, Some(2000));
    // Paid: no `recharge_id`, no `payment`; the id asked about, and the new balance.
    let paid = poll(GET_PAID, "rch_8GGFDJKZ").unwrap();
    assert_eq!(paid.recharge_id, "rch_8GGFDJKZ");
    assert_eq!(paid.status, RechargeStatus::Paid);
    assert!(paid.status.settled());
    assert_eq!(paid.balance_credits, Some(4935));
    // Expired: the top-level status wins over the fresh link's `awaiting_payment`, the fresh
    // link's other id is not refused, and its link is not taken as the asked one's.
    let expired = poll(GET_EXPIRED, "rch_EFM1CF50").unwrap();
    assert_eq!(expired.recharge_id, "rch_EFM1CF50");
    assert_eq!(expired.status, RechargeStatus::Other("expired".to_owned()));
    assert!(expired.status.settled());
    assert_eq!(expired.url, None);
    assert_eq!(expired.credits, None);
    let held = poll(GET_HELD, "rch_1").unwrap();
    assert_eq!(
        held.status,
        RechargeStatus::Other("amount_mismatch_held".to_owned())
    );
    assert!(held.status.settled());
}

/// The coming shape: a top-level `recharge_id` in every state. Another one is refused; with
/// no top-level status, the payment's counts; a foreign pay link is dropped, not refused.
#[test]
fn the_poll_checks_only_a_top_level_id_and_drops_a_foreign_link() {
    let ours = poll(
        br#"{"recharge_id":"rch_1","status":"paid","balance_credits":7}"#,
        "rch_1",
    );
    assert_eq!(ours.unwrap().balance_credits, Some(7));
    assert!(matches!(
        poll(br#"{"recharge_id":"rch_2","status":"paid"}"#, "rch_1"),
        Err(Error::Parse(_))
    ));
    let nested = poll(
        br#"{"payment":{"recharge_id":"rch_1","status":"expired","url":"http://evil.example/pay/rch_1"}}"#,
        "rch_1",
    )
    .unwrap();
    assert_eq!(nested.status, RechargeStatus::Other("expired".to_owned()));
    assert_eq!(nested.url, None);
    assert!(matches!(poll(b"[1]", "rch_1"), Err(Error::Parse(_))));
    // No status at all is not settled: polled again.
    assert!(!poll(b"{}", "rch_1").unwrap().status.settled());
}

/// The connector's shapes with a top-level `recharge_id` read as today's do, for the id asked
/// about; asked about another, each is refused.
#[test]
fn the_shapes_with_a_top_level_id_read_the_same_and_refuse_another_id() {
    for (today, with_id, asked) in WITH_ID {
        assert_eq!(
            poll(with_id, asked).unwrap(),
            poll(today, asked).unwrap(),
            "{asked}"
        );
        assert!(
            matches!(poll(with_id, "rch_other"), Err(Error::Parse(_))),
            "{asked}"
        );
    }
}
