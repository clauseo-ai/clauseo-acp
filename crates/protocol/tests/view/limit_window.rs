//! A plan limit's window and its view row on the wire: `model` and `duration_mins` are left out
//! when none, and a window or row written before either field reads back with none.

use clauseo_protocol::{
    Finite, LimitWindow, LimitWindowView, MeterTone, WindowFreshness, WindowKind,
};

use super::fixtures::round_trips;

#[test]
fn a_limit_window_and_its_row_leave_out_model_and_duration_when_none() {
    let mut window = LimitWindow {
        id: "model_scoped:0".to_owned(),
        kind: WindowKind::Model,
        label: "Fable weekly limit".to_owned(),
        used_percent: Finite::new(12.0).unwrap(),
        resets_at: None,
        duration_mins: None,
        warning: false,
        model: None,
    };
    assert!(
        serde_json::to_value(&window)
            .unwrap()
            .get("model")
            .is_none()
    );
    window.model = Some("Fable".to_owned());
    assert_eq!(serde_json::to_value(&window).unwrap()["model"], "Fable");
    round_trips(&window);

    let row = LimitWindowView {
        id: "model_scoped:0".to_owned(),
        kind: WindowKind::Model,
        label: "Fable weekly limit".to_owned(),
        model: Some("Fable".to_owned()),
        duration_mins: Some(10_080),
        used_percent: Some(Finite::new(12.0).unwrap()),
        tone: MeterTone::Neutral,
        resets_at: None,
        freshness: WindowFreshness::Fresh,
    };
    round_trips(&row);
    let unknown = LimitWindowView {
        duration_mins: None,
        ..row.clone()
    };
    assert!(
        serde_json::to_value(&unknown)
            .unwrap()
            .get("duration_mins")
            .is_none()
    );
    // A window or row written before the field reads back with none.
    let mut old = serde_json::to_value(&window).unwrap();
    old.as_object_mut().unwrap().remove("model");
    assert_eq!(
        serde_json::from_value::<LimitWindow>(old).unwrap().model,
        None
    );
    let mut old = serde_json::to_value(&row).unwrap();
    old.as_object_mut().unwrap().remove("model");
    old.as_object_mut().unwrap().remove("duration_mins");
    let old = serde_json::from_value::<LimitWindowView>(old).unwrap();
    assert_eq!((old.model, old.duration_mins), (None, None));
}
