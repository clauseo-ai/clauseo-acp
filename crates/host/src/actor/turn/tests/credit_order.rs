//! The order the app hears a refusal in: the out-of-credit connectors' state goes out before
//! the thread's `CaseLawOutOfCredit` notice does. The app shows "Credits added. Ask again" while
//! the thread's latest notice is this one and the state does not read out of credit, so a
//! notice ahead of the state would flash it in between.

use std::sync::Arc;

use clauseo_connectors::fake::{FakeConnector, FakeOptions, ScratchDir};
use clauseo_connectors::{Connectors, MemoryStore, SecretStore};
use clauseo_protocol::{ConnectorStatus, DomainEvent, HostError, NoticeKind};

use super::credit_notice::{key, live_turn, refused};
use super::idle::{TWO_ANSWERS, start_idle, until_idle};
use crate::actor::Actor;
use crate::actor::connectors::{published, signed_in_for_test};
use crate::actor::input::Input;
use crate::config::IDLE_LIMIT;
use crate::connectors::{ConnectorEvents, ConnectorOpener, ConnectorsWiring};

/// Opens nothing: no page opens in this test.
struct NoPages;

impl ConnectorOpener for NoPages {
    fn open(&self, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn open_confirmed(&self, _: &str) -> Result<(), HostError> {
        Ok(())
    }
}

/// The status in the connectors' state last sent to the app.
fn sent_status(actor: &Actor) -> Option<ConnectorStatus> {
    let state = published(actor)?;
    Some(state.connectors.first()?.status.clone())
}

fn out_of_credit(status: Option<&ConnectorStatus>) -> bool {
    matches!(status, Some(ConnectorStatus::OutOfCredit { .. }))
}

/// The commit holding the out-of-credit notice landing: when the app's subscribers get it.
fn notice_lands(actor: &Actor, input: &Input) -> bool {
    matches!(input, Input::Committed { ticket, .. }
        if actor.pipeline.events(*ticket).iter().any(|envelope| matches!(&envelope.event,
            DomainEvent::Notice(notice) if notice.kind == Some(NoticeKind::CaseLawOutOfCredit))))
}

#[test]
fn the_out_of_credit_state_goes_out_before_the_notice() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let scratch = ScratchDir::new("credit-order").unwrap();
    let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
    let connectors = Connectors::new(fake.config(scratch.state_file()), store).unwrap();
    let handle = connectors.get("case-law").unwrap();
    handle.add_api_key(&fake.new_api_key()).unwrap();
    let wiring = ConnectorsWiring {
        connectors: Arc::new(connectors),
        events: ConnectorEvents::new(),
        opener: Arc::new(NoPages),
    };
    let mut rig = start_idle("credit-order", TWO_ANSWERS, "exit", |config| {
        config
            .with_idle_limit(IDLE_LIMIT)
            .with_live_agents(usize::MAX)
            .with_connectors(wiring)
    });
    let thread = rig.thread();
    live_turn(&mut rig, 10, thread);
    signed_in_for_test(&mut rig.actor);
    let before = sent_status(&rig.actor);
    assert!(
        matches!(before, Some(ConnectorStatus::SignedIn { .. })),
        "{before:?}"
    );

    rig.actor.on_input(refused(key(&rig, thread)));
    // In the refusal's own step, before the notice's commit can land.
    assert!(out_of_credit(sent_status(&rig.actor).as_ref()));
    rig.hold_next("the notice's commit", notice_lands);
    assert!(
        out_of_credit(sent_status(&rig.actor).as_ref()),
        "the notice lands while the app holds an in-credit state"
    );
    rig.release_held();
    until_idle(&mut rig, thread);
    rig.finish();
}
