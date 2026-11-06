use std::collections::BTreeMap;
use std::time::SystemTime;

use clauseo_connectors::{
    Account, CASE_LAW, CredentialKind, CredentialStatus, DaySpend, Liveness, Origin, Pack, PayUrl,
    Pricing, PricingStatus,
};
use clauseo_protocol::{
    Balance, ConnectorErrorKind, ConnectorSignInMethod, ConnectorStatus, PackView, RechargeOutcome,
    Timestamp,
};

use super::{CaseLaw, Failure, GrossRate, paise};

/// The host's wall time `millis`.
fn at(millis: i64) -> Timestamp {
    Timestamp::new(millis).unwrap()
}

#[test]
fn paise_reads_rupees_and_at_most_two_paise_digits() {
    for (text, want) in [
        ("23.60", Some(2_360)),
        ("236", Some(23_600)),
        ("0.1", Some(10)),
        ("0.10", Some(10)),
        ("1180.00", Some(118_000)),
        ("42949672.95", Some(u32::MAX)),
        ("42949672.96", None),
        ("0.105", None),
        ("1.", None),
        (".5", None),
        ("", None),
        ("-1.00", None),
        ("+1", None),
        ("1,180.00", None),
        ("₹236", None),
        (" 1", None),
    ] {
        assert_eq!(paise(text), want, "{text:?}");
    }
}

fn pack(name: &str, credits: i64, gross: &str) -> Pack {
    Pack {
        pack: name.to_owned(),
        credits,
        gross_inr: gross.to_owned(),
        ..Pack::default()
    }
}

fn live() -> Liveness {
    Liveness {
        live: true,
        pricing: Some(Pricing {
            status: PricingStatus::Live,
            credit_inr: "0.10".to_owned(),
            gst_rate: "0.18".to_owned(),
            prices: BTreeMap::from([("search_page".to_owned(), 20)]),
            packs: vec![
                pack("500", 500, "59.00"),
                pack("200", 200, "23.60"),
                pack("bad", 9, "₹1"),
            ],
            ..Pricing::default()
        }),
        fetched_at: Some(SystemTime::UNIX_EPOCH),
    }
}

fn account(credits: i64, inr: &str) -> Account {
    Account {
        account_id: "acct_1".to_owned(),
        state: "active".to_owned(),
        balance_credits: credits,
        balance_inr: inr.to_owned(),
        spent_today_credits: 0,
        recent: Vec::new(),
        open_payment: None,
        claimed: true,
        prices: BTreeMap::new(),
        fetched_at: SystemTime::UNIX_EPOCH,
    }
}

/// Live and not added: listed with the packs in paise (the unreadable one left out) and the
/// cheapest as the price. Not live and not added: nothing listed.
#[test]
fn the_view_lists_a_live_connector_with_its_prices_in_paise() {
    let mut state = CaseLaw {
        credential: Some(CredentialStatus::NotAdded),
        liveness: Some(live()),
        ..CaseLaw::default()
    };
    let view = state.view(&CASE_LAW, true, false);
    let connector = &view.connectors[0];
    assert_eq!(connector.price_paise, Some(2_360));
    assert_eq!(
        connector.packs,
        [
            PackView {
                pack: "500".to_owned(),
                credits: 500,
                paise: 5_900
            },
            PackView {
                pack: "200".to_owned(),
                credits: 200,
                paise: 2_360
            },
        ]
    );
    assert_eq!(connector.status, ConnectorStatus::NotAdded);
    state.liveness = Some(Liveness {
        live: false,
        pricing: None,
        fetched_at: None,
    });
    assert_eq!(state.view(&CASE_LAW, true, false).connectors, Vec::new());
}

/// Signed in: the balance in paise with GST, from its credits (the account's own net figure,
/// readable or not, is not shown), low under ten search pages (200 credits at 20 a page);
/// out of credit wins over the balance; a credential that ends takes the account with it.
#[test]
fn the_view_shows_the_balance_low_and_out_of_credit() {
    let mut state = CaseLaw {
        liveness: Some(live()),
        ..CaseLaw::default()
    };
    state.set_credential(CredentialStatus::SignedIn(CredentialKind::ApiKey));
    state.set_account(account(199, "19.90"), at(1_000));
    let status = |state: &CaseLaw| {
        state.view(&CASE_LAW, true, false).connectors[0]
            .status
            .clone()
    };
    assert_eq!(
        status(&state),
        ConnectorStatus::SignedIn {
            account_label: None,
            balance: Some(Balance {
                credits: 199,
                paise: Some(2_348),
                as_of: Some(at(1_000)),
            }),
            low: true,
            method: Some(ConnectorSignInMethod::ApiKey),
        }
    );
    state.set_account(account(200, "₹20"), at(2_000));
    assert_eq!(
        status(&state),
        ConnectorStatus::SignedIn {
            account_label: None,
            balance: Some(Balance {
                credits: 200,
                paise: Some(2_360),
                as_of: Some(at(2_000)),
            }),
            low: false,
            method: Some(ConnectorSignInMethod::ApiKey),
        }
    );
    state.out_of_credit = Some(super::OutOfCredit { pay_url: None });
    assert_eq!(
        status(&state),
        ConnectorStatus::OutOfCredit { pay_url: None }
    );
    state.set_account(account(0, "0.00"), at(3_000));
    assert_eq!(
        status(&state),
        ConnectorStatus::OutOfCredit { pay_url: None }
    );
    state.set_account(account(10, "1.00"), at(4_000));
    assert!(matches!(status(&state), ConnectorStatus::SignedIn { .. }));
    state.set_credential(CredentialStatus::KeyRevoked);
    assert_eq!(status(&state), ConnectorStatus::KeyRevoked);
    assert!(state.account.is_none() && state.account_at.is_none());
}

/// The fixture's price list: `credit_inr` "0.10", `gst_rate` "0.18", five packs.
fn fixture_pricing() -> Pricing {
    serde_json::from_str(include_str!(
        "../../../../connectors/fixtures/pricing-live.json"
    ))
    .unwrap()
}

/// The gross rate is the price list's own: each pack's credits price to its `gross_inr`.
#[test]
fn the_gross_rate_prices_each_pack_at_its_gross_amount() {
    let pricing = fixture_pricing();
    let rate = GrossRate::from_pricing(&pricing).unwrap();
    assert_eq!(pricing.packs.len(), 5);
    for pack in &pricing.packs {
        assert_eq!(
            rate.paise(pack.credits),
            paise(&pack.gross_inr),
            "{}",
            pack.pack
        );
    }
}

/// Credits at ₹0.10 plus 18% (11.8 paise each), rounded half-up once per amount.
#[test]
fn credits_price_in_gross_paise_rounded_half_up() {
    let rate = GrossRate::from_pricing(&fixture_pricing()).unwrap();
    for (credits, want) in [
        (200, Some(2_360)),
        (1_000, Some(11_800)),
        (1_180, Some(13_924)),
        (0, Some(0)),
        (1, Some(12)),
        (3, Some(35)),
        (-1, None),
        (i64::MAX, None),
    ] {
        assert_eq!(rate.paise(credits), want, "{credits}");
    }
    // 5.5 paise a credit: half a paisa rounds up.
    let half = Pricing {
        credit_inr: "0.05".to_owned(),
        gst_rate: "0.1".to_owned(),
        ..Pricing::default()
    };
    let rate = GrossRate::from_pricing(&half).unwrap();
    assert_eq!((rate.paise(1), rate.paise(3)), (Some(6), Some(17)));
}

/// The largest rates the server could send, with the largest counts: `None`, never a panic
/// (debug) or a wrapped amount (release).
#[test]
fn an_overflowing_amount_is_none() {
    let rate = GrossRate::from_pricing(&Pricing {
        credit_inr: "9999999999.9999".to_owned(),
        gst_rate: "9999999999".to_owned(),
        ..Pricing::default()
    })
    .unwrap();
    for credits in [1, 1_000_000, i64::MAX - 1, i64::MAX] {
        assert_eq!(rate.paise(credits), None, "{credits}");
    }
    let fixture = GrossRate::from_pricing(&fixture_pricing()).unwrap();
    assert_eq!(fixture.paise(i64::MAX), None);
    let gst_only = GrossRate::from_pricing(&Pricing {
        credit_inr: "0.10".to_owned(),
        gst_rate: "9999999999".to_owned(),
        ..Pricing::default()
    })
    .unwrap();
    assert_eq!(gst_only.paise(i64::MAX), None);
}

/// A missing or unreadable `credit_inr` or `gst_rate` gives no rate.
#[test]
fn a_rate_that_does_not_read_is_none() {
    let rate_of = |credit_inr: &str, gst_rate: &str| {
        GrossRate::from_pricing(&Pricing {
            credit_inr: credit_inr.to_owned(),
            gst_rate: gst_rate.to_owned(),
            ..Pricing::default()
        })
    };
    assert!(rate_of("0.10", "0").is_some());
    assert!(rate_of("0.1234", "0.1234").is_some());
    for (credit, gst) in [
        ("0.10", ""),
        ("", "0.18"),
        ("0", "0.18"),
        ("0.00", "0.18"),
        ("0.12345", "0.18"),
        ("0.10", "0.18000"),
        ("0.10", "-0.18"),
        ("-0.10", "0.18"),
        ("+0.10", "0.18"),
        ("1,000.00", "0.18"),
        ("₹0.10", "0.18"),
        ("0.10", "18%"),
        ("0.10", " 0.18"),
        ("12345678901", "0.18"),
    ] {
        assert_eq!(rate_of(credit, gst), None, "{credit:?} {gst:?}");
    }
}

/// Signed in: the spend and the balance in gross paise. With no readable GST rate, or a price
/// list that is not live, every rupee amount is `None` and the credits stay.
#[test]
fn the_view_prices_the_spend_and_balance_with_gst() {
    let mut state = CaseLaw {
        liveness: Some(live()),
        ..CaseLaw::default()
    };
    state.set_credential(CredentialStatus::SignedIn(CredentialKind::ApiKey));
    state.set_account(
        Account {
            spent_today_credits: 200,
            recent: vec![DaySpend {
                day: "2026-11-02".to_owned(),
                credits: 1_000,
            }],
            ..account(1_180, "118.00")
        },
        at(5_000),
    );
    let view = |state: &CaseLaw| state.view(&CASE_LAW, true, false).connectors[0].clone();
    let connector = view(&state);
    let spend = connector.recent_spend.unwrap();
    assert_eq!((spend.today_credits, spend.today_paise), (200, Some(2_360)));
    assert_eq!(
        (spend.days[0].credits, spend.days[0].paise),
        (1_000, Some(11_800))
    );
    let ConnectorStatus::SignedIn {
        balance: Some(balance),
        ..
    } = connector.status
    else {
        panic!("{:?}", connector.status);
    };
    assert_eq!(
        balance,
        Balance {
            credits: 1_180,
            paise: Some(13_924),
            as_of: Some(at(5_000)),
        }
    );
    let unpriced = |state: &CaseLaw| {
        let connector = view(state);
        let spend = connector.recent_spend.unwrap();
        let ConnectorStatus::SignedIn {
            balance: Some(balance),
            ..
        } = connector.status
        else {
            panic!("{:?}", connector.status);
        };
        assert_eq!((spend.today_credits, balance.credits), (200, 1_180));
        spend.today_paise.is_none() && spend.days[0].paise.is_none() && balance.paise.is_none()
    };
    let pricing = state.liveness.as_mut().unwrap().pricing.as_mut().unwrap();
    pricing.gst_rate = "eighteen".to_owned();
    assert!(unpriced(&state));
    let pricing = state.liveness.as_mut().unwrap().pricing.as_mut().unwrap();
    pricing.gst_rate = String::new();
    assert!(unpriced(&state));
    state.liveness = Some(live());
    state.liveness.as_mut().unwrap().live = false;
    assert!(unpriced(&state));
}

/// `SignedIn` says how: the browser sign-in is `account`, a pasted key `api_key`.
#[test]
fn signed_in_says_how() {
    let mut state = CaseLaw::default();
    let method = |state: &CaseLaw| {
        let view = state.view(&CASE_LAW, true, false);
        let ConnectorStatus::SignedIn { method, .. } = view.connectors[0].status else {
            panic!("{view:?}");
        };
        method
    };
    state.set_credential(CredentialStatus::SignedIn(CredentialKind::OAuth));
    assert_eq!(method(&state), Some(ConnectorSignInMethod::Account));
    state.set_credential(CredentialStatus::SignedIn(CredentialKind::ApiKey));
    assert_eq!(method(&state), Some(ConnectorSignInMethod::ApiKey));
}

/// The error's kind: a sign-in failure, and `persist_failed` once the crate holds the
/// credential in memory only while signed in (not again while it stays so, nor signed out).
#[test]
fn the_error_says_which_kind() {
    let mut state = CaseLaw::default();
    state.set_credential(CredentialStatus::SignedOut);
    let status = |state: &CaseLaw| {
        state.view(&CASE_LAW, true, false).connectors[0]
            .status
            .clone()
    };
    state.error = Some(Failure::sign_in("the sign-in did not finish"));
    assert_eq!(
        status(&state),
        ConnectorStatus::Error {
            message: "the sign-in did not finish".to_owned(),
            kind: Some(ConnectorErrorKind::SignInFailed),
        }
    );
    state.error = None;
    state.note_persist(true);
    assert_eq!(state.error, None, "signed out: nothing to save");
    state.note_persist(false);
    state.set_credential(CredentialStatus::SignedIn(CredentialKind::OAuth));
    state.note_persist(true);
    assert!(matches!(
        status(&state),
        ConnectorStatus::Error {
            kind: Some(ConnectorErrorKind::PersistFailed),
            ..
        }
    ));
    // The next command clears it; it does not come back while the crate stays so.
    state.error = None;
    state.note_persist(true);
    assert!(matches!(status(&state), ConnectorStatus::SignedIn { .. }));
}

/// The last recharge's outcome is in the view, and a credential that ends takes it.
#[test]
fn the_last_recharge_goes_with_the_credential() {
    let mut state = CaseLaw::default();
    state.set_credential(CredentialStatus::SignedIn(CredentialKind::ApiKey));
    state.last_recharge = Some(RechargeOutcome::Expired);
    let last = |state: &CaseLaw| state.view(&CASE_LAW, true, false).connectors[0].last_recharge;
    assert_eq!(last(&state), Some(RechargeOutcome::Expired));
    state.set_credential(CredentialStatus::SignedOut);
    assert_eq!(last(&state), None);
}

/// `OpenPayLink`'s check (`CaseLaw::pay_link`) takes the origin it is given. The staging
/// origin's cases are `PayUrl::is_for`'s (clauseo-connectors, `--features staging`) and
/// desktop's `staging-connector` tests: the host's tests build without the staging host.
#[test]
fn the_pay_link_is_checked_against_the_origin_given() {
    let production = PayUrl::parse(&CASE_LAW, "https://data.clauseo.chat/pay/rch_1").unwrap();
    let held = |url: &PayUrl| CaseLaw {
        pay_url: Some(url.clone()),
        ..CaseLaw::default()
    };
    assert_eq!(
        held(&production).pay_link(&Origin::Production),
        Some(&production)
    );
    // The fake's link comes back in the production form and is checked as production.
    let fake = Origin::Fake(url::Url::parse("http://127.0.0.1:4321").unwrap());
    let from_fake = PayUrl::parse_for(&CASE_LAW, &fake, "http://127.0.0.1:4321/pay/rch_1").unwrap();
    assert_eq!(held(&from_fake).pay_link(&fake), Some(&from_fake));
    // No link held, nothing to open.
    assert_eq!(CaseLaw::default().pay_link(&Origin::Production), None);
}
