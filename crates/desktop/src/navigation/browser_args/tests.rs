use std::ffi::{OsStr, OsString};

use tauri::Url;

use super::{ENV_OVERRIDE, NO_DNS, WRY_DEFAULT_ARGS, browser_args, env_override_with_rule};

/// wry 0.57.0's default arguments, as `src/webview2/mod.rs` writes them when none are passed.
const WRY_DEFAULTS: &[&str] = &["msWebOOUI", "msPdfOOUI", "msSmartScreenProtection"];

#[test]
fn the_arguments_keep_every_wry_default_and_add_the_rule() {
    let args = browser_args(None);
    assert_eq!(
        args,
        r#"--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --host-resolver-rules="MAP * ~NOTFOUND""#
    );
    assert!(args.starts_with(WRY_DEFAULT_ARGS), "{args}");
    let features = WRY_DEFAULT_ARGS
        .strip_prefix("--disable-features=")
        .expect("one --disable-features switch");
    assert_eq!(features.split(',').collect::<Vec<_>>(), WRY_DEFAULTS);
    assert!(args.contains(NO_DNS), "{args}");
    assert!(!args.contains("EXCLUDE"), "{args}");
}

#[test]
fn the_rule_is_one_quoted_argument() {
    // WebView2 splits the string like a command line: the rule's value has spaces, so it must
    // sit inside one pair of quotes, and nothing else is quoted.
    let args = browser_args(None);
    assert_eq!(args.matches('"').count(), 2, "{args}");
    let switches: Vec<&str> = args.split(" --").collect();
    assert_eq!(switches.len(), 2, "{args}");
    assert_eq!(switches[1], r#"host-resolver-rules="MAP * ~NOTFOUND""#);
}

#[test]
fn a_development_build_excludes_only_the_dev_url_host() {
    let dev = Url::parse("http://localhost:1420").expect("devUrl");
    let args = browser_args(Some(&dev));
    assert_eq!(
        args,
        format!(r#"{WRY_DEFAULT_ARGS} --host-resolver-rules="MAP * ~NOTFOUND, EXCLUDE localhost""#)
    );
    assert_eq!(args.matches('"').count(), 2, "{args}");
}

fn read(path: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The builder keeps its arguments private, so this checks the wiring in the source: the
/// navigation guard applies the arguments on Windows, both windows the app builds
/// (`window_frame/tests.rs` holds that there are only these two) go through the guard before they
/// are built, and nothing else sets arguments.
#[test]
fn both_windows_take_the_arguments_on_windows() {
    let navigation = read("src/navigation.rs");
    let guard = navigation.find("pub(crate) fn guard<").expect("the guard");
    let call = "    #[cfg(windows)]\n    let builder = browser_args::apply(builder, app);\n";
    assert_eq!(
        navigation.matches(call).count(),
        1,
        "navigation.rs must call {call}"
    );
    let body_end = navigation[guard..].find("\n}\n").expect("the guard's end") + guard;
    let at = navigation.find(call).expect("the call");
    assert!(
        guard < at && at < body_end,
        "the guard itself applies the arguments"
    );
    for (path, call) in [
        (
            "src/lib.rs",
            "navigation::guard(csp::on_scripts(builder, app), app)",
        ),
        (
            "src/settings_window.rs",
            "crate::navigation::guard(builder, manager)",
        ),
    ] {
        let text = read(path);
        assert_eq!(text.matches(call).count(), 1, "{path} must call {call}");
        let at = text.find(call).expect("the call");
        let build = text.find("builder.build()").expect("the build");
        assert!(at < build, "{path}: the guard before the build");
    }
    let mut setters = Vec::new();
    for entry in walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")) {
        let text = std::fs::read_to_string(&entry).expect("source");
        if text.contains(concat!(".additional_", "browser_args(")) {
            setters.push(entry);
        }
    }
    assert_eq!(setters.len(), 1, "{setters:?}");
    assert!(
        setters[0].ends_with("navigation/browser_args.rs"),
        "{setters:?}"
    );
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).expect("src") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files
}

#[test]
fn an_unset_override_variable_stays_unset() {
    // The name WebView2 reads (its CreateCoreWebView2EnvironmentWithOptions documentation).
    assert_eq!(ENV_OVERRIDE, "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS");
    assert_eq!(env_override_with_rule(None), None);
}

#[test]
fn a_set_override_variable_keeps_its_flags_and_gains_the_rule() {
    assert_eq!(
        env_override_with_rule(Some(OsStr::new("--auto-open-devtools-for-tabs"))),
        Some(OsString::from(
            r#"--auto-open-devtools-for-tabs --host-resolver-rules="MAP * ~NOTFOUND""#
        ))
    );
    assert_eq!(
        env_override_with_rule(Some(OsStr::new(""))),
        Some(OsString::from(NO_DNS))
    );
}

#[test]
fn an_override_variable_with_its_own_rules_is_left_as_it_is() {
    for value in [
        NO_DNS,
        r#"--remote-debugging-port=0 --host-resolver-rules="MAP * 127.0.0.1""#,
    ] {
        assert_eq!(
            env_override_with_rule(Some(OsStr::new(value))),
            None,
            "{value}"
        );
    }
}
