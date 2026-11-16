//! Examples for the connector goldens (R24), and the snapshots that prove the API key never
//! encodes. A child of `snapshots.rs`, so its snapshots sit in `snapshots/`.

use super::*;
use clauseo_protocol::{
    Balance, ConnectorCommand, ConnectorErrorKind, ConnectorSignInMethod, ConnectorStatus,
    ConnectorView, ConnectorsState, DaySpendView, PackView, RecentSpend, RechargeOutcome,
    RedactedString, Timestamp,
};
use zeroize::Zeroizing;

/// A key that must never show in a snapshot.
const KEY: &str = "ik_live_9b2e44c0a1f3d5e6c7b8a9d0e1f2a3b4";

/// The connectors' state with one connector signed in.
pub(super) fn connectors_state() -> ConnectorsState {
    ConnectorsState {
        available: true,
        locked: false,
        connectors: vec![ConnectorView {
            id: "case-law".to_owned(),
            name: "Indian case law".to_owned(),
            price_paise: Some(2_360),
            packs: vec![PackView {
                pack: "pack_200".to_owned(),
                credits: 200,
                paise: 2_360,
            }],
            recent_spend: Some(RecentSpend {
                today_credits: 40,
                today_paise: Some(472),
                days: vec![DaySpendView {
                    day: "2026-11-04".to_owned(),
                    credits: 120,
                    paise: Some(1_416),
                }],
            }),
            status: ConnectorStatus::SignedIn {
                account_label: None,
                balance: Some(Balance {
                    credits: 1_180,
                    paise: Some(13_924),
                    as_of: Some(Timestamp::new(1_762_400_000_000).unwrap()),
                }),
                low: false,
                method: Some(ConnectorSignInMethod::Account),
            },
            pay_url: Some("https://data.clauseo.chat/pay/rch_01".to_owned()),
            last_recharge: Some(RechargeOutcome::Paid),
        }],
    }
}

/// `AddApiKey` holding `key`.
fn add_api_key(key: &str) -> ConnectorCommand {
    ConnectorCommand::AddApiKey {
        id: "case-law".to_owned(),
        key: RedactedString::new(Zeroizing::new(key.to_owned())).unwrap(),
    }
}

/// `AddCredits` of the case-law connector's first pack.
pub(super) fn add_credits() -> ConnectorCommand {
    ConnectorCommand::AddCredits {
        id: "case-law".to_owned(),
        pack: "pack_200".to_owned(),
    }
}

// The `AddApiKey` golden holds the placeholder, since a real key cannot round-trip: it encodes
// as `[redacted]`. `add_api_key_with_a_real_key` snapshots one with a real key.
mod connector_command {
    use super::*;

    golden! { ConnectorCommand::name_of {
    Check => check: ConnectorCommand::Check {};
    Add => add: ConnectorCommand::Add { id: "case-law".to_owned() };
    AddApiKey => add_api_key_placeholder: super::add_api_key(clauseo_protocol::REDACTED_TEXT);
    AddCredits => add_credits: super::add_credits();
    SignOut => sign_out: ConnectorCommand::SignOut { id: "case-law".to_owned() };
    Remove => remove: ConnectorCommand::Remove { id: "case-law".to_owned() };
    RefreshBalance => refresh_balance: ConnectorCommand::RefreshBalance {
        id: "case-law".to_owned(),
    };
    OpenPayLink => open_pay_link: ConnectorCommand::OpenPayLink { id: "case-law".to_owned() };
    DismissCaseLawHint => dismiss_case_law_hint: ConnectorCommand::DismissCaseLawHint {
        matter: matter_id(1),
    };
    } }
}

/// A command holding a real key snapshots as the placeholder one does: `[redacted]`, never the
/// key, in the snapshot, the JSON and `Debug`.
#[test]
fn add_api_key_with_a_real_key() {
    let command = HostCommand::Connector(add_api_key(KEY));
    snapshot!("add_api_key_with_a_real_key", command);
    let json = serde_json::to_string(&command).unwrap();
    assert!(!json.contains(KEY), "{json}");
    assert!(!format!("{command:?}").contains(KEY));
    let snap = include_str!("snapshots__connectors__add_api_key_with_a_real_key.snap");
    assert!(snap.contains("[redacted]") && !snap.contains(KEY), "{snap}");
}

/// Each status but `SignedIn` (the golden's), in order.
#[test]
fn connector_statuses() {
    let statuses = [
        ConnectorStatus::NotAdded,
        ConnectorStatus::SigningIn,
        ConnectorStatus::OutOfCredit {
            pay_url: Some("https://data.clauseo.chat/pay/rch_02".to_owned()),
        },
        ConnectorStatus::OutOfCredit { pay_url: None },
        ConnectorStatus::SignedOut,
        ConnectorStatus::KeyRevoked,
        ConnectorStatus::Error {
            message: "the sign-in did not finish".to_owned(),
            kind: Some(ConnectorErrorKind::SignInFailed),
        },
        ConnectorStatus::Error {
            message: "the sign-in could not be saved".to_owned(),
            kind: Some(ConnectorErrorKind::PersistFailed),
        },
        // As an older host wrote it: no kind.
        ConnectorStatus::Error {
            message: "the sign-in could not be saved".to_owned(),
            kind: None,
        },
    ];
    snapshot!("connector_statuses", statuses);
    round_trips(&statuses);
}
