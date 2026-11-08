//! `open_settings`'s closed `section`: Connectors (R24), Protection (R17) and Remembered answers
//! (R18), each opening the page at its fragment; anything else refused as the arguments decode.

use serde_json::{Value, json};
use tauri::Manager as _;
use tauri::test::MockRuntime;

use super::{app, denied, invoke};
use crate::settings_window::{HashesSet, SETTINGS_WINDOW};
use crate::test_support::Scratch;

/// `open_settings` takes an optional, closed `section` (R24: the room's case-law hint and
/// banner open Connectors; R17's first run opens Protection): none, `"connectors"` and
/// `"protection"` and `"remembered_answers"` open the window; a value the build does not
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
        json!({"section": "RememberedAnswers"}),
        // the page's id is the fragment, never the wire's name
        json!({"section": "allowances"}),
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
    // R18: the matter menu's "Remembered answers…" opens the page's Remembered answers section.
    assert_eq!(
        invoke(
            &main,
            "open_settings",
            json!({"section": "remembered_answers"})
        ),
        Ok(Value::Null)
    );
    assert_eq!(hashes(&app), ["connectors", "protection", "allowances"]);
    assert_eq!(app.webview_windows().len(), 2);
}

/// R18: a new window built at Remembered answers opens at the page's section id.
#[test]
fn open_settings_builds_the_window_at_remembered_answers() {
    let scratch = Scratch::new("settings-remembered", &[]);
    let host = scratch.start(Vec::new());
    let (app, main) = app(host.bridge());
    app.manage(HashesSet::default());
    assert_eq!(
        invoke(
            &main,
            "open_settings",
            json!({"section": "remembered_answers"})
        ),
        Ok(Value::Null)
    );
    let url = app
        .get_webview_window(SETTINGS_WINDOW)
        .unwrap()
        .url()
        .unwrap();
    assert_eq!(url.fragment(), Some("allowances"), "{url}");
    assert_eq!(hashes(&app), Vec::<&str>::new());
}

/// The fragments `open_settings` set on the open settings window.
fn hashes(app: &tauri::App<MockRuntime>) -> Vec<&'static str> {
    app.state::<HashesSet>().0.lock().unwrap().clone()
}
