use super::*;
use crate::known::CASE_LAW;
use crate::recharge::RechargeStatus;

const ACCOUNT: &[u8] = include_bytes!("../../fixtures/account.json");
const UNKNOWN: &[u8] = include_bytes!("../../fixtures/account-unknown-fields.json");
const OPEN: &[u8] = include_bytes!("../../fixtures/account-open-payment.json");

fn at() -> SystemTime {
    SystemTime::UNIX_EPOCH + std::time::Duration::from_hours(498_216)
}

#[test]
fn the_account_fixture_parses() {
    let account = parse_account(ACCOUNT, &CASE_LAW, &Origin::Production, at()).unwrap();
    assert_eq!(account.account_id, "acct_example0001");
    assert_eq!(account.state, "active");
    assert_eq!(account.balance_credits, 1000);
    assert_eq!(account.balance_inr, "100.00");
    assert_eq!(account.spent_today_credits, 46);
    assert_eq!(
        account.recent,
        [
            DaySpend {
                day: "2026-11-01".to_owned(),
                credits: 120
            },
            DaySpend {
                day: "2026-11-02".to_owned(),
                credits: 46
            },
        ]
    );
    assert_eq!(account.open_payment, None);
    assert!(account.claimed);
    assert_eq!(account.prices["search_page"], 20);
    assert_eq!(account.prices["read_per_4000_tokens"], 1);
    assert_eq!(account.prices.len(), 7);
    assert_eq!(account.fetched_at, at());
}

#[test]
fn unknown_fields_at_every_level_parse_to_the_same_values() {
    let plain = parse_account(ACCOUNT, &CASE_LAW, &Origin::Production, at()).unwrap();
    let extra = parse_account(UNKNOWN, &CASE_LAW, &Origin::Production, at()).unwrap();
    assert_eq!(plain, extra);
}

#[test]
fn an_open_payment_parses_with_its_pay_link() {
    let account = parse_account(OPEN, &CASE_LAW, &Origin::Production, at()).unwrap();
    let payment = account.open_payment.unwrap();
    assert_eq!(payment.recharge_id, "rch_example0001");
    assert_eq!(payment.status, RechargeStatus::AwaitingPayment);
    assert_eq!(
        payment.url.unwrap().as_str(),
        "https://data.clauseo.chat/pay/rch_example0001"
    );
    assert_eq!(payment.credits, Some(200));
    assert_eq!(payment.amount.unwrap().gross_inr, "23.60");
}

#[test]
fn an_open_payment_with_a_foreign_link_keeps_no_link() {
    let body = String::from_utf8(OPEN.to_vec())
        .unwrap()
        .replace("https://data.clauseo.chat/pay/", "https://pay.example/pay/");
    let account = parse_account(body.as_bytes(), &CASE_LAW, &Origin::Production, at()).unwrap();
    assert_eq!(account.open_payment.unwrap().url, None);
}

#[test]
fn not_an_object_is_a_parse_error() {
    for body in [b"[]".as_slice(), b"nope", b""] {
        assert!(matches!(
            parse_account(body, &CASE_LAW, &Origin::Production, at()),
            Err(Error::Parse(_))
        ));
    }
}
