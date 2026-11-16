//! `open_privacy_settings`: granted to `main` alone, by its own permission, with no arguments.

use serde_json::{Value, json};
use tauri::{Manager as _, WebviewWindowBuilder};

use super::{COMMANDS, SETTINGS_COMMANDS, app, denied, granted, invoke, invoke_from};
use crate::settings_window::SETTINGS_WINDOW;
use crate::test_support::Scratch;

/// `capabilities/main.json` grants `allow-open-privacy-settings` and `settings.json` does not;
/// the settings window, a second window and a page that is not the app's are refused by
/// Tauri's access check. `main` is not called with it here: on macOS it would open System
/// Settings (`open/tests/privacy.rs` checks what it opens); elsewhere it is `command.invalid`.
#[test]
fn open_privacy_settings_is_granted_to_main_only() {
    let name = "open_privacy_settings";
    assert!(COMMANDS.contains(&name));
    assert!(!SETTINGS_COMMANDS.contains(&name));
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let permission = "allow-open-privacy-settings";
    assert!(granted(dir, "main.json", "main").contains(permission));
    let settings_grants = granted(dir, "settings.json", SETTINGS_WINDOW);
    assert!(!settings_grants.contains(permission), "{settings_grants:?}");

    let scratch = Scratch::new("privacy-granted", &[]);
    let host = scratch.start(Vec::new());
    let (app, main) = app(host.bridge());
    assert_eq!(invoke(&main, "open_settings", json!({})), Ok(Value::Null));
    let settings = app.get_webview_window(SETTINGS_WINDOW).unwrap();
    let other = WebviewWindowBuilder::new(&app, "other", tauri::WebviewUrl::default())
        .build()
        .unwrap();
    for answer in [
        invoke(&settings, name, json!({})),
        invoke(&other, name, json!({})),
        invoke_from(&main, "https://example.com", name, json!({})),
    ] {
        assert!(denied(&answer), "{answer:?}");
    }
    #[cfg(not(target_os = "macos"))]
    {
        let answer = invoke(&main, name, json!({})).unwrap_err();
        assert_eq!(answer["code"], "command.invalid", "{answer}");
    }
}
