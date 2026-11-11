use super::{
    DAY_GROUND, MIN_WINDOW_SIZE, NIGHT_GROUND, SETTINGS_SIZE, WEBVIEW2_BACKGROUND_ENV, WINDOW_SIZE,
    fit, fitted, ground, state_denylist, state_flags, webview2_background,
};
use std::path::Path;
use tauri_plugin_window_state::StateFlags;

/// `actual` and `expected` are equal to within a hundredth of a pixel.
fn close(actual: (f64, f64), expected: (f64, f64)) -> bool {
    (actual.0 - expected.0).abs() < 0.01 && (actual.1 - expected.1).abs() < 0.01
}

#[test]
fn no_monitor_opens_at_the_room_size() {
    assert_eq!(fitted(None), (1280.0, 800.0));
    assert_eq!(WINDOW_SIZE, (1280.0, 800.0));
}

#[test]
fn the_main_window_keeps_at_least_760_by_520() {
    assert_eq!(MIN_WINDOW_SIZE, (760.0, 520.0));
    // the size it opens at is above the least, so the minimum never shrinks a first window
    assert!(WINDOW_SIZE.0 >= MIN_WINDOW_SIZE.0 && WINDOW_SIZE.1 >= MIN_WINDOW_SIZE.1);
    // the main window's builder sets it (lib.rs, open_main_window)
    let lib = include_str!("../lib.rs");
    assert!(lib.contains("window_size::with_size(builder, (width, height))"));
    let own = include_str!("../window_size.rs");
    assert!(own.contains(".min_inner_size(MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1)"));
}

#[test]
fn a_large_work_area_opens_at_the_room_size() {
    assert_eq!(fitted(Some((1920.0, 1080.0))), (1280.0, 800.0));
}

#[test]
fn a_small_work_area_takes_ninety_percent_per_dimension() {
    assert!(close(fitted(Some((1366.0, 728.0))), (1229.4, 655.2)));
    assert!(close(fitted(Some((1024.0, 738.0))), (921.6, 664.2)));
}

#[test]
fn the_settings_window_fits_its_own_size_the_same_way() {
    assert_eq!(SETTINGS_SIZE, (960.0, 700.0));
    assert_eq!(fit(SETTINGS_SIZE, None), SETTINGS_SIZE);
    assert_eq!(fit(SETTINGS_SIZE, Some((1920.0, 1080.0))), SETTINGS_SIZE);
    assert!(close(
        fit(SETTINGS_SIZE, Some((1024.0, 738.0))),
        (921.6, 664.2)
    ));
}

/// `settings` is never restored (it opens at its size, centred); `main` only in the e2e build.
#[test]
fn the_state_denylist_holds_settings_always_and_main_in_the_e2e_build() {
    if cfg!(feature = "e2e") {
        assert_eq!(state_denylist(), &["main", "settings"]);
    } else {
        assert_eq!(state_denylist(), &["settings"]);
    }
}

/// The 6 hex digits after `/* #` on each line that declares `--ground:` with its hex comment in
/// `tokens-colour.css`, in file order: the day value, then the night one. The operating system's
/// day values at night carry no comment and are left out.
fn css_grounds() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/components/src/styles/tokens-colour.css");
    let css = std::fs::read_to_string(path).unwrap();
    css.lines()
        .filter(|line| line.trim_start().starts_with("--ground:") && line.contains("/* #"))
        .map(|line| line.split("/* #").nth(1).unwrap()[..6].to_owned())
        .collect()
}

/// `RRGGBB` as the three bytes.
fn rgb(hex: &str) -> (u8, u8, u8) {
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap();
    (byte(0), byte(2), byte(4))
}

#[test]
fn the_ground_colours_are_the_tokens_hex_comments() {
    let grounds = css_grounds();
    assert_eq!(
        grounds,
        ["E5E2DC", "1B1A18"],
        "--ground lines of tokens-colour.css: day, then night"
    );
    assert_eq!(rgb(&grounds[0]), DAY_GROUND);
    assert_eq!(rgb(&grounds[1]), NIGHT_GROUND);
    assert_eq!(ground(false), DAY_GROUND);
    assert_eq!(ground(true), NIGHT_GROUND);
}

/// The quit hides the windows before their state is saved (quit.rs), so the visibility is never
/// saved or restored: a relaunch never opens hidden. The decorations stay the frame's.
#[test]
fn the_window_state_leaves_out_the_visibility_and_the_decorations() {
    let flags = state_flags();
    assert!(!flags.contains(StateFlags::VISIBLE));
    assert!(!flags.contains(StateFlags::DECORATIONS));
    assert_eq!(
        flags.bits(),
        (StateFlags::all() - StateFlags::VISIBLE - StateFlags::DECORATIONS).bits()
    );
}

/// Window-state saves to Tauri's `app_config_dir`, which on Windows is the Roaming folder
/// (`%APPDATA%`). A firm can redirect that folder to a file share that is down (a laptop off
/// VPN). The plugin resolves it on the main thread before the first window and at both quit
/// saves, which held the launch for about 20 s and the quit for 20 to 40 s. So
/// `app.appDirectoriesOverride` moves the config folder to `$LOCALDATA`, which is not
/// redirected, and overrides that one folder only: `root` would move the `WebView2` profile,
/// and `data` or `localData` are not window-state's.
#[test]
fn window_state_is_in_the_local_data_folder_not_the_roaming_one() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let conf: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let identifier = conf["identifier"].as_str().unwrap();
    let over = conf["app"]["appDirectoriesOverride"]
        .as_object()
        .expect("app.appDirectoriesOverride is an object of directories");
    assert_eq!(
        over.get("config").and_then(serde_json::Value::as_str),
        Some(format!("$LOCALDATA/{identifier}").as_str())
    );
    let keys: Vec<&str> = over.keys().map(String::as_str).collect();
    assert_eq!(keys, ["config"], "only the config folder is overridden");
}

/// `WebView2` takes its first frame's colour from the variable as opaque ARGB hex: the ground of
/// the theme, the same `ground` the builder and the native mark take.
#[test]
fn webview2_s_first_background_is_the_ground_of_the_theme() {
    assert_eq!(WEBVIEW2_BACKGROUND_ENV, "WEBVIEW2_DEFAULT_BACKGROUND_COLOR");
    assert_eq!(webview2_background(false), "FFE5E2DC");
    assert_eq!(webview2_background(true), "FF1B1A18");
    for dark in [false, true] {
        let (r, g, b) = ground(dark);
        assert_eq!(
            webview2_background(dark),
            format!("FF{r:02X}{g:02X}{b:02X}")
        );
    }
}
