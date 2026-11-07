use tauri::Url;

use super::{allowed, dev_url};

/// The app's origin on the platform under test (tauri `protocol/mod.rs`, `origin`, with
/// `use_https_scheme(true)`), and the other platforms' forms, which this build never serves from.
#[cfg(windows)]
const APP: &str = "https://tauri.localhost";
#[cfg(windows)]
const OTHER_FORMS: &[&str] = &["tauri://localhost/index.html", "http://tauri.localhost/"];
#[cfg(not(windows))]
const APP: &str = "tauri://localhost";
#[cfg(not(windows))]
const OTHER_FORMS: &[&str] = &[
    "https://tauri.localhost/index.html",
    "http://tauri.localhost/",
];

/// `devUrl` as `tauri.conf.json` sets it.
const DEV: &str = "http://localhost:1420";

fn url(text: &str) -> Url {
    Url::parse(text).unwrap_or_else(|error| panic!("{text}: {error}"))
}

fn check(text: &str, dev: Option<&str>) -> bool {
    let dev = dev.map(url);
    allowed(&url(text), dev.as_ref())
}

#[test]
fn the_app_origin_its_pages_and_empty_frames_are_allowed() {
    let cases = [
        APP.to_owned(),
        format!("{APP}/"),
        format!("{APP}/index.html"),
        format!("{APP}/index.html#matter-118"),
        format!("{APP}/settings.html"),
        format!("{APP}/settings.html#plans"),
        format!("{APP}/settings.html?section=plans#connectors"),
        format!("{APP}/assets/index-abc123.js"),
        // The app origin serves assets only, with no redirects: this is a 404, not a way out.
        format!("{APP}/redirect?to=https://evil.test"),
        "about:blank".to_owned(),
        "about:blank#x".to_owned(),
        "about:srcdoc".to_owned(),
    ];
    for case in &cases {
        assert!(check(case, None), "{case} should be allowed");
        assert!(
            check(case, Some(DEV)),
            "{case} should be allowed with a dev URL"
        );
    }
}

#[test]
fn the_dev_url_origin_is_allowed_when_given() {
    for case in [
        "http://localhost:1420",
        "http://localhost:1420/",
        "http://localhost:1420/settings.html#plans",
        "http://localhost:1420/src/main.tsx?t=1",
    ] {
        assert!(check(case, Some(DEV)), "{case} should be allowed in dev");
        assert!(
            !check(case, None),
            "{case} should be refused without a dev URL"
        );
    }
    for case in [
        "http://localhost:1421/",
        "https://localhost:1420/",
        "http://127.0.0.1:1420/",
        "http://localhost/",
        "http://localhost:1420@evil.test/",
        "http://user@localhost:1420/",
    ] {
        assert!(!check(case, Some(DEV)), "{case} should be refused in dev");
    }
}

#[test]
fn everything_else_is_refused() {
    let cases = [
        "https://evil.test/?x",
        "https://evil.test/?data=secret",
        "http://evil.test/",
        "http://localhost:9999",
        "file:///C:/x",
        "file:///etc/passwd",
        "data:text/html,<script>x</script>",
        "data:,x",
        "javascript:alert(1)",
        "javascript:location.href='https://evil.test'",
        "clauseo-file:matter-118/signature-page.png",
        "http://clauseo-file.localhost/x",
        "https://clauseo-file.localhost/x",
        "ipc://localhost/x",
        "https://ipc.localhost/x",
        "about:config",
        "about:blank.evil.test",
        "blob:https://evil.test/0b3a",
        "ws://evil.test/",
        "mailto:a@evil.test",
        // Lookalikes.
        "https://tauri.localhost.evil.test/",
        "https://tauri.localhost.evil.test/?x",
        "http://tauri.localhost@evil.test/",
        "https://tauri.localhost@evil.test/",
        "tauri://localhost@evil.test/",
        "tauri://localhost.evil.test/",
        "tauri://evil.test/",
        "https://evil.test/tauri.localhost",
        "https://evil.test/#https://tauri.localhost",
        "https://localhost/",
    ];
    for case in cases {
        assert!(!check(case, None), "{case} should be refused");
        assert!(
            !check(case, Some(DEV)),
            "{case} should be refused in dev too"
        );
    }
}

#[test]
fn the_app_origin_with_a_port_or_user_is_refused() {
    for case in [
        format!("{APP}:8080/index.html"),
        format!("{APP}:1/"),
        APP.replacen("://", "://user@", 1),
        APP.replacen("://", "://user:pass@", 1),
    ] {
        assert!(!check(&case, None), "{case} should be refused");
    }
}

#[test]
fn the_other_platforms_forms_are_refused() {
    for case in OTHER_FORMS {
        assert!(
            !check(case, None),
            "{case} should be refused on this platform"
        );
    }
}

#[test]
fn the_dev_url_passes_only_in_a_build_without_custom_protocol() {
    // A release, e2e or marks build comes from `tauri build`, which turns on `custom-protocol`:
    // there `tauri::is_dev()` is false and `dev_url` gives `None` whatever the config says.
    let dev = url(DEV);
    assert_eq!(dev_url(Some(&dev)), tauri::is_dev().then_some(&dev));
    assert_eq!(dev_url(None), None);
}

#[test]
fn both_windows_are_guarded() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for file in ["lib.rs", "settings_window.rs"] {
        let text = std::fs::read_to_string(dir.join(file)).unwrap();
        assert_eq!(
            text.matches("navigation::guard(").count(),
            1,
            "{file} builds a window without the navigation guard"
        );
    }
}
