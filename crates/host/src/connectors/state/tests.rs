use std::collections::BTreeMap;
use std::time::SystemTime;

use clauseo_connectors::{
    Account, CASE_LAW, CredentialKind, CredentialStatus, Liveness, Pack, Pricing, PricingStatus,
};
use clauseo_protocol::{Balance, ConnectorStatus, PackView};

use super::{CaseLaw, paise};

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

/// Signed in: the balance in paise, low under ten search pages (200 credits at 20 a page);
/// out of credit wins over the balance; a credential that ends takes the account with it.
#[test]
fn the_view_shows_the_balance_low_and_out_of_credit() {
    let mut state = CaseLaw {
        liveness: Some(live()),
        ..CaseLaw::default()
    };
    state.set_credential(CredentialStatus::SignedIn(CredentialKind::ApiKey));
    state.set_account(account(199, "19.90"));
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
                paise: Some(1_990)
            }),
            low: true,
        }
    );
    state.set_account(account(200, "₹20"));
    assert_eq!(
        status(&state),
        ConnectorStatus::SignedIn {
            account_label: None,
            balance: Some(Balance {
                credits: 200,
                paise: None
            }),
            low: false,
        }
    );
    state.out_of_credit = Some(super::OutOfCredit { pay_url: None });
    assert_eq!(
        status(&state),
        ConnectorStatus::OutOfCredit { pay_url: None }
    );
    state.set_account(account(0, "0.00"));
    assert_eq!(
        status(&state),
        ConnectorStatus::OutOfCredit { pay_url: None }
    );
    state.set_account(account(10, "1.00"));
    assert!(matches!(status(&state), ConnectorStatus::SignedIn { .. }));
    state.set_credential(CredentialStatus::KeyRevoked);
    assert_eq!(status(&state), ConnectorStatus::KeyRevoked);
    assert!(state.account.is_none());
}
