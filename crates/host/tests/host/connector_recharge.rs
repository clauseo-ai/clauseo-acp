//! R24 PR5's recharge watch against S28's fake connector: a refused `AddCredits` leaves the
//! out-of-credit recharge watched, a relay out-of-credit while `AddCredits` opens its recharge
//! does not lose the command's pay link, and the switch going off ends the watch.

use std::time::Duration;

use clauseo_connectors::{PayUrl, PaymentRef};
use clauseo_host::SessionKey;
use clauseo_host::connectors::{RelayEvent, RelayEvents as _};
use clauseo_protocol::{
    CommandOutcome, ConnectorStatus, ErrorCode, HostCommand, HostEvent, MatterId, ThreadId,
    ToolsPatch,
};

use crate::connector_money::{agent, search};
use crate::connector_races::{HELD, dispatch_on_thread};
use crate::connector_rig::{Rig, Start, add_credits, case_law, signed_in, status};
use crate::support::{Dir, foreign_uuid, wait};

/// The fake's path that opens a recharge.
const RECHARGES: &str = "/v1/case-law/recharges";

/// Our pay link for `recharge`.
fn pay_link(recharge: &str) -> String {
    format!("https://data.clauseo.chat/pay/{recharge}")
}

/// The app state a new window gets first.
fn current(rig: &Rig) -> clauseo_protocol::ConnectorsState {
    let (_sub, first) = crate::settings::subscribe_app(&rig.host);
    first
        .into_iter()
        .find_map(|event| match event {
            HostEvent::ConnectorsChanged(state) => Some(state),
            _ => None,
        })
        .expect("no connector state in the first frames")
}

/// Out of credit through a relay (recharge R1 watched), then `AddCredits` of a pack not on
/// sale is refused: paying R1 still brings the balance back.
#[test]
fn a_refused_add_credits_keeps_the_out_of_credit_recharge_watched() {
    let dir = Dir::new("connector_recharge_refused", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    // Each poll answers at once, and the watch waits between polls on its cancel flag: a
    // cancelled watch has stopped before the payment, and a live one polls again within 2 s.
    rig.fake.set_wait_cap(Duration::ZERO);
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    rig.fake.set_balance(0);
    search(&mut reader, 1);
    rig.state_until("out of credit", |state| {
        matches!(status(state), Some(ConnectorStatus::OutOfCredit { .. }))
    });
    let recharge = rig.fake.recharges().pop().unwrap();
    let refused = rig.dispatch(add_credits("nope")).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{refused}");
    assert!(rig.fake.mark_paid(&recharge));
    rig.state_until("paid", |state| signed_in(state).is_some_and(|c| c > 0));
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}

/// A relay's out-of-credit (another recharge) while `AddCredits` is still opening its own:
/// the command answers `Done` with its own pay link in the state (D1).
#[test]
fn an_out_of_credit_while_add_credits_opens_keeps_its_pay_link() {
    let dir = Dir::new("connector_recharge_opening", &[]);
    let rig = Rig::start(&dir, Start { signed_in: true });
    rig.fake.delay_next(RECHARGES, HELD);
    let adding = dispatch_on_thread(&rig.host.handle, add_credits("200"));
    wait("the recharge request at the fake", async {
        while rig.fake.requests_to(RECHARGES).is_empty() {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
    rig.events.event(RelayEvent::OutOfCredit {
        session: SessionKey {
            matter: MatterId::new(foreign_uuid(1)).unwrap(),
            thread: ThreadId::new(foreign_uuid(2)).unwrap(),
            launch: 1,
        },
        payment: PaymentRef {
            recharge_id: Some("rch_from_the_relay".to_owned()),
            url: PayUrl::parse(
                &clauseo_connectors::CASE_LAW,
                "https://data.clauseo.chat/pay/rch_from_the_relay",
            ),
        },
    });
    assert_eq!(adding.join().unwrap(), Ok(CommandOutcome::Done));
    let recharge = rig.fake.recharges().pop().unwrap();
    let state = current(&rig);
    assert_eq!(
        case_law(&state).unwrap().pay_url,
        Some(pay_link(&recharge)),
        "{state:?}"
    );
    rig.finish();
}

/// `AddCredits`, then the switch goes off: the watch ends (no more polling for the recharge)
/// and its pay link goes with it.
#[test]
fn the_switch_going_off_ends_the_recharge_watch() {
    let dir = Dir::new("connector_recharge_switch_off", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    rig.ok(add_credits("200"));
    rig.state_until("the pay link", |state| {
        case_law(state).is_some_and(|view| view.pay_url.is_some())
    });
    rig.host.ok(HostCommand::SetToolSettings {
        patch: ToolsPatch {
            connectors: Some(false),
            ..ToolsPatch::default()
        },
    });
    let state = current(&rig);
    assert!(!state.available, "{state:?}");
    assert_eq!(
        case_law(&state).and_then(|view| view.pay_url.clone()),
        None,
        "{state:?}"
    );
    rig.finish();
}
