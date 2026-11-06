//! R24's wire-gaps follow-up against S28's fake connector: the error's kind in its real cases
//! (a key the connector refuses, a refused sign-in, refreshed tokens the keychain refuses), and
//! the last recharge's outcome (paid, expired by the connector or by the watch's deadline,
//! cancelled) with what clears it (the next recharge, sign-out, remove, the switch going off).

use std::time::Duration;

use clauseo_protocol::{
    ConnectorErrorKind, ConnectorSignInMethod, ConnectorStatus, ConnectorsState, HostCommand,
    RechargeOutcome, ToolsPatch,
};

use crate::connector_rig::{
    Rig, Start, add, add_credits, add_key, case_law, refresh, remove, sign_out, status,
};
use crate::support::Dir;

/// The error's kind, when the state shows one.
fn error_kind(state: &ConnectorsState) -> Option<ConnectorErrorKind> {
    if let Some(ConnectorStatus::Error { kind, .. }) = status(state) {
        *kind
    } else {
        None
    }
}

/// The last recharge's outcome of the listed connector.
fn last_recharge(state: &ConnectorsState) -> Option<RechargeOutcome> {
    case_law(state).and_then(|view| view.last_recharge)
}

/// `AddCredits` of the 200 pack: the state with its pay link, and its recharge.
fn open_recharge(rig: &mut Rig) -> (ConnectorsState, String) {
    rig.ok(add_credits("200"));
    let state = rig.state_until("the pay link", |state| {
        case_law(state).is_some_and(|view| view.pay_url.is_some())
    });
    (state, rig.fake.recharges().pop().unwrap())
}

/// Waits for the pay link to clear; returns that state.
fn pay_link_cleared(rig: &mut Rig) -> ConnectorsState {
    rig.state_until("the pay link cleared", |state| {
        case_law(state).is_some_and(|view| view.pay_url.is_none())
    })
}

/// A well-formed key the connector does not know is `sign_in_failed`; so is a sign-in the
/// connector refuses.
#[test]
fn a_refused_key_or_sign_in_is_sign_in_failed() {
    let dir = Dir::new("connector_outcomes_sign_in_failed", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    rig.ok(clauseo_protocol::ConnectorCommand::Check {});
    let unknown = format!("cdk_live_{}", "A".repeat(32));
    rig.keys.push(unknown.clone());
    assert!(rig.dispatch(add_key(&unknown)).is_err());
    let state = rig.state_until("the key's error", |state| error_kind(state).is_some());
    assert_eq!(
        error_kind(&state),
        Some(ConnectorErrorKind::SignInFailed),
        "{state:?}"
    );

    rig.fake.deny_authorize(Some("server_error"));
    rig.ok(add());
    let page = rig.pages.recv_timeout(Duration::from_secs(30)).unwrap();
    let _answer = rig.fake.drive_authorize(&url::Url::parse(&page).unwrap());
    let state = rig.state_until("the sign-in's error", |state| error_kind(state).is_some());
    assert_eq!(
        error_kind(&state),
        Some(ConnectorErrorKind::SignInFailed),
        "{state:?}"
    );
    rig.finish();
}

/// Signed in through the browser, then the keychain refuses the refreshed tokens twice: the
/// state shows `persist_failed` until the next command, then signed in again (`account`).
#[test]
fn refreshed_tokens_the_keychain_refuses_are_persist_failed() {
    let dir = Dir::new("connector_outcomes_persist_failed", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    rig.ok(clauseo_protocol::ConnectorCommand::Check {});
    rig.fake.set_access_ttl(Duration::from_mins(10));
    rig.ok(add());
    let page = rig.pages.recv_timeout(Duration::from_secs(30)).unwrap();
    let answer = rig
        .fake
        .drive_authorize(&url::Url::parse(&page).unwrap())
        .unwrap();
    assert_eq!(answer.status, 200);
    rig.state_until("signed in", |state| {
        matches!(status(state), Some(ConnectorStatus::SignedIn { .. }))
    });
    // Within the refresh margin of the access token's expiry, and past the read interval.
    rig.store.fail_next_writes(2);
    rig.host.clock.advance(Duration::from_mins(6));
    rig.ok(refresh());
    let state = rig.state_until("persist failed", |state| error_kind(state).is_some());
    assert_eq!(
        error_kind(&state),
        Some(ConnectorErrorKind::PersistFailed),
        "{state:?}"
    );
    assert!(rig.handle.persist_failed());
    rig.ok(refresh());
    let state = rig.state_until("signed in again", |state| {
        matches!(status(state), Some(ConnectorStatus::SignedIn { .. }))
    });
    assert!(
        matches!(
            status(&state),
            Some(ConnectorStatus::SignedIn {
                method: Some(ConnectorSignInMethod::Account),
                ..
            })
        ),
        "{state:?}"
    );
    rig.finish();
}

/// Paid after `mark_paid`, then sign-out clears it.
#[test]
fn a_paid_recharge_shows_paid_until_sign_out() {
    let dir = Dir::new("connector_outcomes_paid", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (state, recharge) = open_recharge(&mut rig);
    assert_eq!(last_recharge(&state), None);
    assert!(rig.fake.mark_paid(&recharge));
    let state = pay_link_cleared(&mut rig);
    assert_eq!(
        last_recharge(&state),
        Some(RechargeOutcome::Paid),
        "{state:?}"
    );
    rig.ok(sign_out());
    let state = rig.state_until("signed out", |state| {
        status(state) == Some(&ConnectorStatus::SignedOut)
    });
    assert_eq!(last_recharge(&state), None, "{state:?}");
    rig.finish();
}

/// Expired by the connector, then remove clears it.
#[test]
fn an_expired_recharge_shows_expired_until_remove() {
    let dir = Dir::new("connector_outcomes_expired", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (_, recharge) = open_recharge(&mut rig);
    assert!(rig.fake.expire_recharge(&recharge));
    let state = pay_link_cleared(&mut rig);
    assert_eq!(
        last_recharge(&state),
        Some(RechargeOutcome::Expired),
        "{state:?}"
    );
    rig.ok(remove());
    let state = rig.state_until("removed", |state| {
        case_law(state).is_none_or(|view| view.status == ConnectorStatus::NotAdded)
    });
    assert_eq!(last_recharge(&state), None, "{state:?}");
    rig.finish();
}

/// The watch's deadline passes first: expired; the next recharge's pay link clears it.
#[test]
fn the_watch_deadline_is_expired_until_the_next_recharge() {
    let dir = Dir::new("connector_outcomes_deadline", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    // Each poll answers at once, so the watch looks at its deadline every 2 s.
    rig.fake.set_wait_cap(Duration::ZERO);
    open_recharge(&mut rig);
    rig.host.clock.advance(Duration::from_mins(16));
    let state = pay_link_cleared(&mut rig);
    assert_eq!(
        last_recharge(&state),
        Some(RechargeOutcome::Expired),
        "{state:?}"
    );
    let (state, _) = open_recharge(&mut rig);
    assert_eq!(last_recharge(&state), None, "{state:?}");
    rig.finish();
}

/// Cancelled by the connector, then the switch going off clears it.
#[test]
fn a_cancelled_recharge_shows_cancelled_until_the_switch_goes_off() {
    let dir = Dir::new("connector_outcomes_cancelled", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (_, recharge) = open_recharge(&mut rig);
    assert!(rig.fake.cancel_recharge(&recharge));
    let state = pay_link_cleared(&mut rig);
    assert_eq!(
        last_recharge(&state),
        Some(RechargeOutcome::Cancelled),
        "{state:?}"
    );
    rig.host.ok(HostCommand::SetToolSettings {
        patch: ToolsPatch {
            connectors: Some(false),
            ..ToolsPatch::default()
        },
    });
    let state = rig.state_until("off", |state| !state.available);
    assert_eq!(last_recharge(&state), None, "{state:?}");
    rig.finish();
}
