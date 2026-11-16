//! `open_settings`: the settings window at a known section, or where it was.

use serde_json::{Value, json};
use tauri::Manager as _;
use tauri::test::MockRuntime;

use super::{app, denied, invoke};
use crate::settings_window::{HashesSet, SETTINGS_WINDOW};
use crate::test_support::Scratch;

/// `open_settings` takes an optional, closed `section` (R24: the room's case-law hint and
/// banner open Connectors; R17's first run opens Protection): none, `"connectors"` and
/// `"protection"` open the window; a value the build does not
/// know, or one of another type, is refused as the arguments decode and opens nothing.
#[test]
fn open_settings_takes_a_known_section_or_none_and_refuses_others() {
    let scratch = Scratch::new("settings-section", &[]);
    let host = scratch.start(Vec::new());
    let (app, main) = app(host.bridge());
    for refused in [
        json!({"section": "billing"}),
        json!({"section": "Connectors"}),
        json!({"section": "Protection"}),
        json!({"section": 1}),
    ] {
        let answer = invoke(&main, "open_settings", refused.clone());
        assert!(answer.is_err() && !denied(&answer), "{refused}: {answer:?}");
        assert_eq!(app.webview_windows().len(), 1, "{refused} opened a window");
    }
    app.manage(HashesSet::default());
    let at = invoke(&main, "open_settings", json!({"section": "connectors"}));
    assert_eq!(at, Ok(Value::Null));
    assert_eq!(app.webview_windows().len(), 2);
    let settings = app.get_webview_window(SETTINGS_WINDOW).unwrap();
    let url = settings.url().unwrap();
    assert_eq!(url.fragment(), Some("connectors"), "{url}");
    assert!(url.path().ends_with("/settings.html"), "{url}");
    // Built at the fragment, so no hash set on it.
    assert_eq!(hashes(&app), Vec::<&str>::new());
    // Open already: brought forward at the section, still one settings window.
    assert_eq!(
        invoke(&main, "open_settings", json!({"section": "connectors"})),
        Ok(Value::Null)
    );
    assert_eq!(hashes(&app), ["connectors"]);
    assert_eq!(invoke(&main, "open_settings", json!({})), Ok(Value::Null));
    assert_eq!(
        invoke(&main, "open_settings", json!({"section": null})),
        Ok(Value::Null)
    );
    // No section: brought forward where it was, with no hash set.
    assert_eq!(hashes(&app), ["connectors"]);
    assert_eq!(app.webview_windows().len(), 2);
    // R17: UC's first-run button opens Protection.
    assert_eq!(
        invoke(&main, "open_settings", json!({"section": "protection"})),
        Ok(Value::Null)
    );
    assert_eq!(hashes(&app), ["connectors", "protection"]);
    assert_eq!(app.webview_windows().len(), 2);
}

/// The fragments `open_settings` set on the open settings window.
fn hashes(app: &tauri::App<MockRuntime>) -> Vec<&'static str> {
    app.state::<HashesSet>().0.lock().unwrap().clone()
}
