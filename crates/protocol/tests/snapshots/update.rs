//! Golden snapshots of the updater's wire types (decision record 0037): one per `UpdateState`
//! variant, the values of its unit enums, and the view `HostEvent::UpdateChanged` and
//! `QueryResult::Update` carry. A child of `snapshots.rs`, so its snapshots sit in `snapshots/`
//! with the rest and the file stays under the size limit.

use clauseo_protocol::{
    FailedStage, InstallWhen, ManagedBy, QueryResult, UpdateMode, UpdateState, UpdateView,
};

use super::*;

/// A time for the examples.
#[cfg(test)]
fn at() -> Timestamp {
    Timestamp::new(1_760_000_008_000).unwrap()
}

/// `HostEvent::UpdateChanged`'s example: a download under way, with both optional fields set.
#[cfg(test)]
pub(crate) fn update_view() -> UpdateView {
    UpdateView {
        current: "0.4.0".to_owned(),
        state: UpdateState::Downloading {
            version: "0.4.1".to_owned(),
            received: count(1_048_576),
            total: Some(count(52_428_800)),
        },
        mode: UpdateMode::Notify,
        checked_at: Some(at()),
        updated_from: Some("0.3.9".to_owned()),
    }
}

golden! { UpdateState::name_of {
    Idle => update_state_idle: UpdateState::Idle;
    Checking => update_state_checking: UpdateState::Checking;
    Downloading => update_state_downloading: UpdateState::Downloading {
        version: "0.4.1".to_owned(),
        received: count(0),
        total: None,
    };
    Ready => update_state_ready: UpdateState::Ready { version: "0.4.1".to_owned() };
    Scheduled => update_state_scheduled: UpdateState::Scheduled { version: "0.4.1".to_owned() };
    Installing => update_state_installing: UpdateState::Installing {
        version: "0.4.1".to_owned(),
    };
    Failed => update_state_failed: UpdateState::Failed {
        version: None,
        at: at(),
        stage: FailedStage::Check,
    };
    Managed => update_state_managed: UpdateState::Managed { by: ManagedBy::PerMachine };
} }

/// `GetUpdate`'s answer as `NoUpdater` gives it: managed by the package, no optional field.
#[test]
fn query_result_update() {
    let answer = QueryResult::Update(UpdateView {
        current: "0.4.0".to_owned(),
        state: UpdateState::Managed {
            by: ManagedBy::Package,
        },
        mode: UpdateMode::Auto,
        checked_at: None,
        updated_from: None,
    });
    snapshot!("query_result_update", answer);
    round_trips(&answer);
}

/// Each value of the updater's unit enums, in order.
#[test]
fn update_values() {
    let values = (
        FailedStage::ALL,
        ManagedBy::ALL,
        UpdateMode::ALL,
        InstallWhen::ALL,
    );
    snapshot!("update_values", values);
    round_trips(&values);
}
