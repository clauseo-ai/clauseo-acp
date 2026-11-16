//! R24 PR5's races between a credential's end and the work in flight, against S28's fake
//! connector: an `AddApiKey` whose key check is still at the connector when `Remove` or the
//! `tools.connectors` switch going off comes in stores nothing, and an account read still at the
//! connector when `SignOut` comes in does not bring the old account's spending back.

use std::thread::JoinHandle;
use std::time::Duration;

use clauseo_connectors::CredentialStatus;
use clauseo_host::HostHandle;
use clauseo_protocol::{
    CommandEnvelope, CommandOutcome, ConnectorCommand, ConnectorStatus, ErrorCode, HostCommand,
    HostError, HostEvent, ToolsPatch,
};

use crate::connector_rig::{Rig, Start, add_key, case_law, refresh, remove, sign_out, status};
use crate::support::{Dir, command_id, wait};

/// The fake's account path, which the key check and every balance read take.
const ACCOUNT: &str = "/v1/case-law/account";

/// How long the fake holds the request in flight: real time, so the other command can run
/// while it waits.
pub(crate) const HELD: Duration = Duration::from_millis(1_500);

/// Dispatches `command` from a thread of its own.
pub(crate) fn dispatch_on_thread(
    handle: &HostHandle,
    command: ConnectorCommand,
) -> JoinHandle<Result<CommandOutcome, HostError>> {
    let handle = handle.clone();
    std::thread::spawn(move || {
        let envelope = CommandEnvelope {
            command_id: command_id(),
            command: HostCommand::Connector(command),
        };
        wait("dispatch", handle.dispatch(envelope))
    })
}

/// Waits, in real time up to the watchdog, until the fake has had `n` account requests.
pub(crate) fn until_account_reads(rig: &Rig, n: usize) {
    wait("the account request at the fake", async {
        while rig.account_reads() < n {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
}

/// `AddApiKey` with its key check held at the fake, then `ending` (which must answer `Done`)
/// while the check is in flight: the key is refused with `command.conflict` and the keychain
/// holds nothing.
fn a_key_in_flight_loses_to(name: &str, ending: impl FnOnce(&mut Rig)) {
    let dir = Dir::new(name, &[]);
    let mut rig = Rig::start(&dir, Start::default());
    let key = rig.fake.new_api_key();
    rig.keys.push(key.clone());
    rig.fake.delay_next(ACCOUNT, HELD);
    let base = rig.account_reads();
    let adding = dispatch_on_thread(&rig.host.handle, add_key(&key));
    until_account_reads(&rig, base + 1);
    ending(&mut rig);
    let added = adding.join().unwrap();
    let refused = added.as_ref().expect_err("the key was added after its end");
    assert_eq!(refused.code(), ErrorCode::CommandConflict, "{refused}");
    rig.seen.push(serde_json::to_string(refused).unwrap());
    assert_eq!(
        rig.handle.status(),
        CredentialStatus::NotAdded,
        "the key is stored"
    );
    rig.finish();
}

/// `Remove` while the key's check is at the connector: the remove wins.
#[test]
fn a_remove_while_a_key_is_checked_stores_no_key() {
    a_key_in_flight_loses_to("connector_races_remove", |rig| {
        assert_eq!(rig.dispatch(remove()), Ok(CommandOutcome::Done));
    });
}

/// The lawyer's switch going off while the key's check is at the connector: the switch wins.
#[test]
fn the_switch_going_off_while_a_key_is_checked_stores_no_key() {
    a_key_in_flight_loses_to("connector_races_switch_off", |rig| {
        rig.host.ok(HostCommand::SetToolSettings {
            patch: ToolsPatch {
                connectors: Some(false),
                ..ToolsPatch::default()
            },
        });
    });
}

/// A balance read held at the fake while `SignOut` runs: once the read comes back, the state is
/// still signed out with no spending, and so is a new window's first state.
#[test]
fn a_late_account_read_does_not_bring_the_spending_back_after_sign_out() {
    let dir = Dir::new("connector_races_late_read", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    rig.host.clock.advance(Duration::from_secs(61));
    rig.fake.delay_next(ACCOUNT, HELD);
    let base = rig.account_reads();
    let reading = dispatch_on_thread(&rig.host.handle, refresh());
    until_account_reads(&rig, base + 1);
    rig.ok(sign_out());
    rig.state_until("signed out", |state| {
        status(state) == Some(&ConnectorStatus::SignedOut)
    });
    let read = reading.join().unwrap();
    assert_eq!(read, Ok(CommandOutcome::Done));
    // The read's reply follows whatever it published; a new window gets the state as it is now.
    let (_sub, first) = crate::settings::subscribe_app(&rig.host);
    let state = first
        .iter()
        .find_map(|event| match event {
            HostEvent::ConnectorsChanged(state) => Some(state),
            _ => None,
        })
        .expect("no connector state in the first frames");
    let view = case_law(state).unwrap();
    assert_eq!(view.status, ConnectorStatus::SignedOut, "{state:?}");
    assert_eq!(
        view.recent_spend, None,
        "the old account's spending came back"
    );
    rig.finish();
}

/// A balance read held at the fake while a relay reports the key revoked: once the read comes
/// back, its balance (read under the key that ended) is not shown.
#[test]
fn a_late_account_read_is_not_applied_after_the_credential_ended() {
    use clauseo_host::SessionKey;
    use clauseo_host::connectors::{CredentialEnd, RelayEvent, RelayEvents as _};
    use clauseo_protocol::{MatterId, ThreadId};

    let dir = Dir::new("connector_races_ended_read", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    rig.host.clock.advance(Duration::from_secs(61));
    rig.fake.set_balance(777);
    rig.fake.delay_next(ACCOUNT, HELD);
    let base = rig.account_reads();
    let reading = dispatch_on_thread(&rig.host.handle, refresh());
    until_account_reads(&rig, base + 1);
    rig.events.event(RelayEvent::CredentialEnded {
        session: SessionKey {
            matter: MatterId::new(crate::support::foreign_uuid(1)).unwrap(),
            thread: ThreadId::new(crate::support::foreign_uuid(2)).unwrap(),
            launch: 1,
        },
        ended: CredentialEnd::KeyRevoked,
    });
    rig.state_until("revoked", |state| {
        status(state) == Some(&ConnectorStatus::KeyRevoked)
    });
    assert_eq!(reading.join().unwrap(), Ok(CommandOutcome::Done));
    let (_sub, first) = crate::settings::subscribe_app(&rig.host);
    let state = first
        .iter()
        .find_map(|event| match event {
            HostEvent::ConnectorsChanged(state) => Some(state),
            _ => None,
        })
        .expect("no connector state in the first frames");
    assert_ne!(
        crate::connector_rig::signed_in(state),
        Some(777),
        "the read from before the revoke was applied: {state:?}"
    );
    rig.finish();
}
