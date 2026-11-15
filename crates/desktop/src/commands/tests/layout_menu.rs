//! `set_layout_menu`: granted to `main` alone, by its own permission; two booleans, always ok.

use serde_json::{Value, json};
use tauri::{Manager as _, WebviewWindowBuilder};

use super::{COMMANDS, SETTINGS_COMMANDS, app, denied, granted, invoke, invoke_from};
use crate::settings_window::SETTINGS_WINDOW;
use crate::test_support::Scratch;

/// `capabilities/main.json` grants `allow-set-layout-menu` and `settings.json` does not; the
/// settings window, a second window and a page that is not the app's are refused by Tauri's
/// access check, and `main` gets ok for any pair of flags (the mock app has no menu bar).
#[test]
fn set_layout_menu_is_granted_to_main_only() {
    let name = "set_layout_menu";
    assert!(COMMANDS.contains(&name));
    assert!(!SETTINGS_COMMANDS.contains(&name));
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let permission = "allow-set-layout-menu";
    assert!(granted(dir, "main.json", "main").contains(permission));
    let settings_grants = granted(dir, "settings.json", SETTINGS_WINDOW);
    assert!(!settings_grants.contains(permission), "{settings_grants:?}");

    let scratch = Scratch::new("layout-menu-granted", &[]);
    let host = scratch.start(Vec::new());
    let (app, main) = app(host.bridge());
    for (sidebar, papers) in [(true, true), (false, true), (true, false), (false, false)] {
        let body = json!({ "sidebar": sidebar, "papers": papers });
        assert_eq!(invoke(&main, name, body), Ok(Value::Null));
    }
    let flags = json!({ "sidebar": false, "papers": false });
    assert_eq!(invoke(&main, "open_settings", json!({})), Ok(Value::Null));
    let settings = app.get_webview_window(SETTINGS_WINDOW).unwrap();
    let other = WebviewWindowBuilder::new(&app, "other", tauri::WebviewUrl::default())
        .build()
        .unwrap();
    for answer in [
        invoke(&settings, name, flags.clone()),
        invoke(&other, name, flags.clone()),
        invoke_from(&main, "https://example.com", name, flags),
    ] {
        assert!(denied(&answer), "{answer:?}");
    }
}
