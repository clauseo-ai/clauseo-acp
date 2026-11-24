//! Credits that came back where the app did not see it (an adjustment on the connector's side,
//! not a payment through the app): the first relayed paid call that succeeds clears the
//! out-of-credit state at once, with one `ConnectorsChanged`, and starts one account read for
//! the new balance, not waiting for the paced read.

use clauseo_protocol::{ConnectorStatus, HostEvent, Principal};
use futures::channel::oneshot;
use futures::{FutureExt as _, StreamExt as _};

use super::credit_notice::refused;
use super::credit_order::{out_of_credit, sent_status, wired_rig};
use super::rig::Rig;
use crate::AppSubscription;
use crate::actor::connectors::{
    ConnectorInput, read_in_flight, read_just_now_for_test, signed_in_for_test,
};
use crate::actor::input::{Input, Request};
use crate::config::SessionKey;
use crate::connectors::RelayEvent;

const ACCOUNT: &str = "/v1/case-law/account";

/// An app subscription on the rig's actor.
fn subscribe(rig: &mut Rig) -> AppSubscription {
    let (reply, mut answer) = oneshot::channel();
    let principal = Principal::LocalDesktop;
    rig.actor
        .on_request(Request::SubscribeApp { principal, reply });
    let mut got = None;
    rig.run_until("the app subscription", |_| {
        if let Ok(Some(result)) = answer.try_recv() {
            got = Some(result);
        }
        got.is_some()
    });
    got.unwrap().unwrap()
}

/// The statuses of the `ConnectorsChanged` events `app` holds now.
fn changes(app: &mut AppSubscription) -> Vec<Option<ConnectorStatus>> {
    let mut statuses = Vec::new();
    while let Some(Some(item)) = app.next().now_or_never() {
        if let HostEvent::ConnectorsChanged(state) = item.decode().unwrap() {
            statuses.push(state.connectors.first().map(|view| view.status.clone()));
        }
    }
    statuses
}

#[test]
fn a_paid_call_that_succeeds_clears_out_of_credit_at_once_and_reads_once() {
    let (mut rig, fake, _scratch) = wired_rig("credit-back");
    let thread = rig.thread();
    let session = SessionKey {
        matter: rig.matter(),
        thread,
        launch: 1,
    };
    signed_in_for_test(&mut rig.actor);
    rig.actor.on_input(refused(session));
    rig.settle_commits();
    assert!(out_of_credit(sent_status(&rig.actor).as_ref()));
    let mut app = subscribe(&mut rig);
    changes(&mut app);
    rig.run_until("no read in flight", |actor| !read_in_flight(actor));
    // A read just went out: the paced one is not due.
    read_just_now_for_test(&mut rig.actor);
    let reads = fake.requests_to(ACCOUNT).len();

    let paid = |session| {
        Input::Connector(Box::new(ConnectorInput::Relay(RelayEvent::PaidResult {
            session,
        })))
    };
    rig.actor.on_input(paid(session));
    let told = changes(&mut app);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(!out_of_credit(told[0].as_ref()), "{told:?}");
    assert!(read_in_flight(&rig.actor), "no read for the new balance");
    // Another success while that read is in flight starts none.
    rig.actor.on_input(paid(session));
    assert_eq!(changes(&mut app), []);
    rig.run_until("the read's end", |actor| !read_in_flight(actor));
    assert_eq!(fake.requests_to(ACCOUNT).len(), reads + 1);
    rig.finish();
}
