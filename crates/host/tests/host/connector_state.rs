//! R24 PR5: the connector state and commands at the host, against S28's fake connector. The
//! done-when's scenario (check, add by signing in, add credits and pay, sign out, add a key,
//! remove, a window that reconnects), the price list's other answers, the switch, a reused
//! `command_id`, and no token or key in anything the app sees.

use std::time::Duration;

use clauseo_connectors::fake::Fixture;
use clauseo_protocol::{
    Balance, CommandOutcome, ConnectorCommand, ConnectorStatus, ErrorCode, HostCommand, HostEvent,
    MatterId, PackView, ToolsPatch,
};

use crate::connector_rig::{
    Rig, Start, add, add_credits, add_key, case_law, refresh, remove, sign_out, signed_in, status,
};
use crate::support::{Dir, command_id, foreign_uuid};

/// The done-when's scenario: each command publishes the state, the pay link reaches the state
/// before `AddCredits` answers, paying it brings the balance, and a window that subscribes again
/// gets the current state first. No event or reply holds a token or key the fake issued.
#[test]
fn the_commands_publish_the_state_and_no_token_reaches_the_app() {
    let dir = Dir::new("connector_state_scenario", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    rig.ok(ConnectorCommand::Check {});
    let state = rig.state_until("the live price list", |state| case_law(state).is_some());
    let view = case_law(&state).unwrap();
    assert_eq!(view.status, ConnectorStatus::NotAdded);
    assert_eq!(view.price_paise, Some(2_360));
    assert_eq!(
        view.packs.first(),
        Some(&PackView {
            pack: "200".to_owned(),
            credits: 200,
            paise: 2_360
        })
    );
    assert!(state.available && !state.locked);

    // Add: the browser shows the sign-in page, which the test then signs in on.
    rig.ok(add());
    let state = rig.next_state("signing in");
    assert_eq!(status(&state), Some(&ConnectorStatus::SigningIn));
    let page = rig.pages.recv_timeout(Duration::from_secs(30)).unwrap();
    let answer = rig
        .fake
        .drive_authorize(&url::Url::parse(&page).unwrap())
        .unwrap();
    assert_eq!(answer.status, 200);
    let state = rig.state_until("signed in", |state| signed_in(state).is_some());
    let ConnectorStatus::SignedIn {
        account_label,
        balance: Some(Balance { credits, paise }),
        low,
    } = status(&state).unwrap()
    else {
        panic!("{state:?}");
    };
    assert_eq!((account_label, *credits, *low), (&None, 1_000, false));
    // At the fake's price list, ₹0.10 a credit plus 18% GST: 1,000 credits are ₹118.
    assert_eq!(*paise, Some(11_800), "{state:?}");
    // The spend, priced the same way: 46 credits are 542.8 paise, shown as 543.
    let spend = case_law(&state).unwrap().recent_spend.clone().unwrap();
    assert_eq!(
        (spend.today_credits, spend.today_paise),
        (46, Some(543)),
        "{state:?}"
    );
    assert_eq!(spend.days[0].paise, Some(1_416), "{state:?}");

    // Add credits: the reply follows the pay link into the state; paying brings the balance.
    rig.ok(add_credits("200"));
    let state = rig.next_state("the pay link");
    let pay_url = case_law(&state).unwrap().pay_url.clone().unwrap();
    let recharge = rig.fake.recharges().pop().unwrap();
    assert_eq!(pay_url, format!("https://data.clauseo.chat/pay/{recharge}"));
    assert!(rig.fake.mark_paid(&recharge));
    let state = rig.state_until("the paid balance", |state| signed_in(state) == Some(1_200));
    assert_eq!(case_law(&state).unwrap().pay_url, None);

    // Sign out: a revoke at the fake, then signed out.
    rig.ok(sign_out());
    let state = rig.state_until("signed out", |state| {
        status(state) == Some(&ConnectorStatus::SignedOut)
    });
    assert!(case_law(&state).unwrap().recent_spend.is_none());
    assert_eq!(rig.fake.requests_to("/revoke").len(), 1);

    // A firm's key instead, then removed.
    let key = rig.fake.new_api_key();
    rig.keys.push(key.clone());
    rig.ok(add_key(&key));
    rig.state_until("signed in with the key", |state| signed_in(state).is_some());
    rig.ok(remove());
    let state = rig.state_until("removed", |state| {
        status(state) == Some(&ConnectorStatus::NotAdded)
    });

    // A window that comes back gets the current state in its first frames.
    let (_again, first) = crate::settings::subscribe_app(&rig.host);
    assert!(
        first.contains(&HostEvent::ConnectorsChanged(state)),
        "{first:?}"
    );
    rig.finish();
    assert!(!format!("{:?}", add_key(&key)).contains(&key));
}

/// With `pricing-preview.json`, the connector is not listed, and `Add` is refused.
#[test]
fn a_preview_price_list_lists_nothing() {
    let dir = Dir::new("connector_state_preview", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    rig.fake.set_pricing(Fixture::File("pricing-preview.json"));
    rig.ok(ConnectorCommand::Check {});
    // The host publishes only a changed state, and nothing changed: the first frames' state
    // (nothing listed) is the last one. A refresh reads the same.
    let refused = rig.dispatch(add()).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{refused}");
    let (_sub, first) = crate::settings::subscribe_app(&rig.host);
    let states: Vec<_> = first
        .iter()
        .filter_map(|event| match event {
            HostEvent::ConnectorsChanged(state) => Some(state),
            _ => None,
        })
        .collect();
    assert_eq!(states.len(), 1, "{first:?}");
    assert!(states[0].connectors.is_empty(), "{states:?}");
    rig.host.shutdown();
}

/// A price list that fails to load lists nothing either.
#[test]
fn a_failed_price_list_lists_nothing() {
    let dir = Dir::new("connector_state_failed_fetch", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    rig.fake.fail_next("/pricing.json", 500);
    rig.ok(ConnectorCommand::Check {});
    let (_sub, first) = crate::settings::subscribe_app(&rig.host);
    let listed = first.iter().any(|event| {
        matches!(event, HostEvent::ConnectorsChanged(state) if !state.connectors.is_empty())
    });
    assert!(!listed, "{first:?}");
    assert_eq!(rig.fake.requests_to("/pricing.json").len(), 1);
    rig.host.shutdown();
}

/// Every connector command, for the switch's table.
fn every_command() -> Vec<ConnectorCommand> {
    vec![
        ConnectorCommand::Check {},
        add(),
        add_key("pasted-key-for-the-window-test"),
        add_credits("200"),
        refresh(),
        ConnectorCommand::OpenPayLink {
            id: "case-law".to_owned(),
        },
        ConnectorCommand::DismissCaseLawHint {
            matter: MatterId::new(foreign_uuid(7)).unwrap(),
        },
        sign_out(),
        remove(),
    ]
}

/// With `tools.connectors` set `false` by policy: the state says off and locked, every command
/// but `SignOut` and `Remove` is refused with `settings.locked` (and reaches nothing at the
/// fake), and those two tidy up.
#[test]
fn the_switch_off_refuses_every_command_but_sign_out_and_remove() {
    let dir = Dir::new("connector_state_switch_off", &[]);
    dir.set_policy(r#"{"tools": {"connectors": false}}"#);
    let mut rig = Rig::start(&dir, Start::default());
    let key = rig.fake.new_api_key();
    rig.handle.add_api_key(&key).unwrap();
    rig.keys.push(key);
    let before = rig.fake.requests().len();
    for command in every_command() {
        let what = format!("{command:?}");
        let tidy = matches!(
            command,
            ConnectorCommand::SignOut { .. } | ConnectorCommand::Remove { .. }
        );
        let reply = rig.dispatch(command);
        if tidy {
            assert_eq!(reply, Ok(CommandOutcome::Done), "{what}");
        } else {
            let refused = reply.unwrap_err();
            assert_eq!(
                refused.code(),
                ErrorCode::SettingsLocked,
                "{what}: {refused}"
            );
        }
    }
    assert_eq!(
        rig.fake.requests().len(),
        before,
        "a refused command reached the fake"
    );
    let state = rig.state_until("removed", |state| {
        status(state) == Some(&ConnectorStatus::NotAdded) || case_law(state).is_none()
    });
    assert!(!state.available && state.locked, "{state:?}");
    rig.finish();
}

/// The lawyer's own off (`SetToolSettings` carries `tools.connectors`): the state says off and
/// not locked, and commands are refused with `command.invalid`; on again, they run.
#[test]
fn the_lawyers_own_off_is_set_from_settings() {
    let dir = Dir::new("connector_state_own_off", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    let set = |connectors| HostCommand::SetToolSettings {
        patch: ToolsPatch {
            connectors: Some(connectors),
            ..ToolsPatch::default()
        },
    };
    rig.host.ok(set(false));
    let state = rig.state_until("off", |state| !state.available);
    assert!(!state.locked);
    let refused = rig.dispatch(ConnectorCommand::Check {}).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{refused}");
    rig.host.ok(set(true));
    rig.state_until("on", |state| state.available);
    rig.ok(ConnectorCommand::Check {});
    rig.host.shutdown();
}

/// A reused `command_id` is `command.conflict`, never a second run: for `AddApiKey` with the
/// same key (whose body hashes as any other key's), with another key, and for another command;
/// and an id a connector command used refuses any command. Only the first reached the fake.
#[test]
fn a_reused_command_id_is_a_conflict() {
    let dir = Dir::new("connector_state_reused_id", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    let key = rig.fake.new_api_key();
    let other = rig.fake.new_api_key();
    rig.keys.extend([key.clone(), other.clone()]);
    let id = command_id();
    assert_eq!(rig.dispatch_as(id, add_key(&key)), Ok(CommandOutcome::Done));
    let reads = rig.account_reads();
    for command in [add_key(&key), add_key(&other), sign_out()] {
        let what = format!("{command:?}");
        let refused = rig.dispatch_as(id, command).unwrap_err();
        assert_eq!(
            refused.code(),
            ErrorCode::CommandConflict,
            "{what}: {refused}"
        );
    }
    let refused = rig
        .host
        .dispatch_as(
            id,
            HostCommand::SetToolSettings {
                patch: ToolsPatch::default(),
            },
        )
        .unwrap_err();
    assert_eq!(refused.code(), ErrorCode::CommandConflict, "{refused}");
    assert_eq!(rig.account_reads(), reads, "a conflict checked a key");
    // Two at once under one new id: one runs, the other is a conflict.
    let twin = command_id();
    let host = &rig.host.handle;
    let envelope = |key: &str| clauseo_protocol::CommandEnvelope {
        command_id: twin,
        command: HostCommand::Connector(add_key(key)),
    };
    let (first, second) = futures::executor::block_on(futures::future::join(
        host.dispatch(envelope(&key)),
        host.dispatch(envelope(&key)),
    ));
    let codes =
        [&first, &second].map(|reply| reply.as_ref().err().map(clauseo_protocol::HostError::code));
    assert!(
        codes.contains(&None) && codes.contains(&Some(ErrorCode::CommandConflict)),
        "{first:?} {second:?}"
    );
    rig.finish();
}
