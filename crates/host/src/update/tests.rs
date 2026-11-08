//! The boundary's values: `NoUpdater`, the marker's JSON, the progress sink and the opaque
//! handle.

use futures::executor::block_on;

use super::*;

#[test]
fn update_no_updater_is_a_package_install_that_offers_nothing() {
    assert_eq!(NoUpdater.install_kind(), InstallKind::Package);
    assert_eq!(block_on(NoUpdater.check()), Ok(None));
}

#[test]
fn update_marker_json_is_from_to_and_installing() {
    let marker = UpdateMarker {
        from: "0.4.0".to_owned(),
        to: "0.4.1".to_owned(),
        state: MarkerState::Installing,
    };
    let json = r#"{"from":"0.4.0","to":"0.4.1","state":"installing"}"#;
    assert_eq!(serde_json::to_string(&marker).unwrap(), json);
    assert_eq!(serde_json::from_str::<UpdateMarker>(json).unwrap(), marker);
    let other = r#"{"from":"0.4.0","to":"0.4.1","state":"done"}"#;
    assert!(serde_json::from_str::<UpdateMarker>(other).is_err());
}

#[test]
fn update_progress_sink_drops_values_when_full_and_never_blocks() {
    let (mut sink, mut rx) = ProgressSink::channel(1);
    for received in 1..=10 {
        sink.post(received, Some(10));
    }
    let mut got = Vec::new();
    while let Ok(p) = rx.try_recv() {
        got.push(p.received);
    }
    // The channel's one slot plus the sender's own.
    assert_eq!(got, [1, 2]);
    drop(rx);
    sink.post(11, None);
}

#[test]
fn update_opaque_hands_back_its_value_and_compares_by_identity() {
    let a = Opaque::new(String::from("update"));
    assert_eq!(
        a.downcast_ref::<String>().map(String::as_str),
        Some("update")
    );
    assert_eq!(a.downcast_ref::<u32>(), None);
    assert_eq!(a.clone(), a);
    assert_ne!(Opaque::new(String::from("update")), a);
    assert_eq!(format!("{a:?}"), "Opaque(..)");
}
