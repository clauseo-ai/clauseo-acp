//! Tests for the `tauri-config` check: each rule on a failing and a passing config, and `run` on a
//! scratch tree laid out like the repo.

use super::*;
use crate::checks::tests::desktop_context;
use crate::scratch::Scratch;
use std::fs;

/// A config that passes every rule, shaped like the committed one.
const CONF: &str = r#"{
  "build": {
    "devUrl": "http://localhost:1420"
  },
  "app": {
    "withGlobalTauri": false,
    "windows": [],
    "security": {
      "freezePrototype": true,
      "capabilities": ["main"],
      "csp": "default-src 'self'; script-src 'self'",
      "devCsp": "default-src 'self'; style-src 'self' 'unsafe-inline'"
    }
  },
  "bundle": {
    "icon": []
  },
  "plugins": {
    "updater": {
      "pubkey": "",
      "endpoints": []
    }
  }
}
"#;

const BUILD: &str = "fn main() {\n    tauri_build::try_build(\n        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new()),\n    );\n}\n";

const PATH: &str = "crates/desktop/tauri.conf.json";

/// Line number of the first line of `text` that contains `needle`.
fn line_of(text: &str, needle: &str) -> u32 {
    let index = text
        .lines()
        .position(|line| line.contains(needle))
        .expect("the needle is in the fixture");
    checks::line_at(index)
}

/// The violations of `CONF` with `from` replaced by `to`.
fn with(from: &str, to: &str) -> (String, Vec<Violation>) {
    assert!(CONF.contains(from), "the fixture contains {from}");
    let text = CONF.replacen(from, to, 1);
    let found = conf_violations(PATH, Some(&text));
    (text, found)
}

#[test]
fn the_clean_config_passes() {
    assert_eq!(conf_violations(PATH, Some(CONF)), Vec::new());
}

#[test]
fn with_global_tauri_true_or_missing_fails() {
    let (text, found) = with("\"withGlobalTauri\": false", "\"withGlobalTauri\": true");
    assert_eq!(
        found,
        vec![violation(
            PATH,
            line_of(&text, "withGlobalTauri"),
            "app.withGlobalTauri must be false",
            GLOBAL_TAURI_FIX,
        )]
    );
    let (_, found) = with("\"withGlobalTauri\": false,", "");
    assert_eq!(
        found,
        vec![violation(
            PATH,
            1,
            "app.withGlobalTauri must be false",
            GLOBAL_TAURI_FIX
        )]
    );
}

#[test]
fn with_global_tauri_in_the_wrong_object_points_at_line_one() {
    let moved = CONF.replace("\"withGlobalTauri\": false,", "").replace(
        "\"devUrl\": \"http://localhost:1420\"",
        "\"devUrl\": \"http://localhost:1420\",\n    \"withGlobalTauri\": false",
    );
    assert_eq!(line_of(&moved, "withGlobalTauri"), 4);
    assert_eq!(
        conf_violations(PATH, Some(&moved)),
        vec![violation(
            PATH,
            1,
            "app.withGlobalTauri must be false",
            GLOBAL_TAURI_FIX
        )]
    );
}

#[test]
fn freeze_prototype_false_fails() {
    let (text, found) = with("\"freezePrototype\": true", "\"freezePrototype\": false");
    assert_eq!(
        found,
        vec![violation(
            PATH,
            line_of(&text, "freezePrototype"),
            "app.security.freezePrototype must be true",
            FREEZE_FIX,
        )]
    );
}

#[test]
fn an_empty_or_missing_csp_fails() {
    let (text, found) = with(
        "\"csp\": \"default-src 'self'; script-src 'self'\"",
        "\"csp\": \"\"",
    );
    assert_eq!(
        found,
        vec![violation(
            PATH,
            line_of(&text, "\"csp\""),
            "app.security.csp is missing or empty",
            CSP_MISSING_FIX,
        )]
    );
    let (_, found) = with("\"csp\": \"default-src 'self'; script-src 'self'\",", "");
    assert_eq!(found[0].message, "app.security.csp is missing or empty");
    assert_eq!(found[0].line, 1);
}

#[test]
fn unsafe_eval_or_inline_in_the_csp_fails() {
    let (text, found) = with(
        "script-src 'self'\"",
        "script-src 'self' 'unsafe-eval' 'unsafe-inline'\"",
    );
    let line = line_of(&text, "\"csp\"");
    assert_eq!(
        found,
        vec![
            violation(
                PATH,
                line,
                "app.security.csp allows `'unsafe-eval'`",
                CSP_UNSAFE_FIX
            ),
            violation(
                PATH,
                line,
                "app.security.csp allows `'unsafe-inline'`",
                CSP_UNSAFE_FIX
            ),
        ]
    );
}

#[test]
fn a_csp_object_is_read_as_its_directives() {
    let (_, found) = with(
        "\"csp\": \"default-src 'self'; script-src 'self'\"",
        "\"csp\": { \"default-src\": \"'self'\", \"script-src\": [\"'self'\", \"'unsafe-eval'\"] }",
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].message, "app.security.csp allows `'unsafe-eval'`");
}

#[test]
fn unsafe_eval_in_the_dev_csp_fails_but_unsafe_inline_passes() {
    let (text, found) = with("'unsafe-inline'\"", "'unsafe-inline' 'unsafe-eval'\"");
    assert_eq!(
        found,
        vec![violation(
            PATH,
            line_of(&text, "devCsp"),
            "app.security.devCsp allows `'unsafe-eval'`",
            DEV_CSP_FIX,
        )]
    );
}

#[test]
fn a_config_window_or_a_missing_windows_array_fails() {
    let (text, found) = with("\"windows\": []", "\"windows\": [{ \"label\": \"main\" }]");
    assert_eq!(
        found,
        vec![violation(
            PATH,
            line_of(&text, "\"windows\""),
            "app.windows must be an empty array",
            WINDOWS_FIX,
        )]
    );
    let (_, found) = with("\"windows\": [],", "");
    assert_eq!(
        found,
        vec![violation(
            PATH,
            1,
            "app.windows must be an empty array",
            WINDOWS_FIX
        )]
    );
}

#[test]
fn an_enabled_asset_protocol_fails() {
    let (text, found) = with(
        "\"freezePrototype\": true,",
        "\"freezePrototype\": true,\n      \"assetProtocol\": { \"enable\": true },",
    );
    assert_eq!(
        found,
        vec![violation(
            PATH,
            line_of(&text, "assetProtocol"),
            "app.security.assetProtocol.enable is true",
            ASSET_FIX,
        )]
    );
    let (_, found) = with(
        "\"freezePrototype\": true,",
        "\"freezePrototype\": true,\n      \"assetProtocol\": { \"enable\": false },",
    );
    assert_eq!(found, Vec::new());
}

#[test]
fn a_dangerous_key_at_any_depth_fails() {
    let (text, found) = with(
        "\"freezePrototype\": true,",
        "\"freezePrototype\": true,\n      \"dangerousDisableAssetCspModification\": true,",
    );
    assert_eq!(
        found,
        vec![violation(
            PATH,
            line_of(&text, "dangerousDisableAssetCspModification"),
            "`app.security.dangerousDisableAssetCspModification` switches off a Tauri safeguard",
            DANGEROUS_FIX,
        )]
    );
    let (text, found) = with(
        "\"pubkey\": \"\",",
        "\"pubkey\": \"\",\n      \"dangerousInsecureTransportProtocol\": true,",
    );
    assert_eq!(
        found,
        vec![violation(
            PATH,
            line_of(&text, "dangerousInsecureTransportProtocol"),
            "`plugins.updater.dangerousInsecureTransportProtocol` switches off a Tauri safeguard",
            DANGEROUS_FIX,
        )]
    );
}

#[test]
fn a_missing_or_invalid_tauri_conf_fails_once_at_line_one() {
    assert_eq!(
        conf_violations(PATH, None),
        vec![violation(
            PATH,
            1,
            format!("`{PATH}` is missing"),
            CONF_MISSING_FIX
        )]
    );
    let invalid = conf_violations(PATH, Some("{ \"app\": "));
    assert_eq!(invalid.len(), 1);
    assert_eq!(invalid[0].line, 1);
    assert!(
        invalid[0]
            .message
            .starts_with("`crates/desktop/tauri.conf.json` is not valid JSON")
    );
}

#[test]
fn build_rs_without_app_manifest_fails_at_line_one() {
    let build = "crates/desktop/build.rs";
    assert_eq!(
        build_rs_violations(build, Some("fn main() {}\n")),
        vec![violation(
            build,
            1,
            "build.rs declares no AppManifest",
            APP_MANIFEST_FIX
        )]
    );
    assert_eq!(build_rs_violations(build, None).len(), 1);
    assert_eq!(build_rs_violations(build, Some(BUILD)), Vec::new());
}

#[test]
fn the_check_is_registered_after_plugins() {
    let names: Vec<&str> = crate::checks::ALL.iter().map(|check| check.name).collect();
    assert_eq!(
        names,
        ["budgets", "capabilities", "plugins", "tauri-config"]
    );
}

#[test]
fn run_passes_a_clean_tree_and_fails_a_planted_setting_with_its_path() {
    let dir = Scratch::new("tauri-config-run");
    let ctx = desktop_context(&dir);
    let conf = "crates/desktop/tauri.conf.json";
    fs::create_dir_all(dir.join("crates/desktop")).expect("create the desktop dir");
    fs::write(dir.join("crates/desktop/build.rs"), BUILD).expect("write build.rs");
    fs::write(dir.join(conf), CONF).expect("write tauri.conf.json");
    assert_eq!(run(&ctx), Vec::new());
    let planted = CONF.replace("\"withGlobalTauri\": false", "\"withGlobalTauri\": true");
    fs::write(dir.join(conf), &planted).expect("write tauri.conf.json");
    assert_eq!(
        run(&ctx),
        vec![violation(
            conf,
            line_of(&planted, "withGlobalTauri"),
            "app.withGlobalTauri must be false",
            GLOBAL_TAURI_FIX,
        )]
    );
}

#[test]
fn every_platform_config_name_is_covered() {
    let names = platform_configs();
    assert_eq!(names.len(), 15);
    for name in [
        "tauri.macos.conf.json",
        "tauri.windows.conf.json5",
        "Tauri.linux.toml",
        "tauri.ios.conf.json",
        "Tauri.android.toml",
    ] {
        assert!(names.contains(&name.to_owned()), "{name}");
    }
}

#[test]
fn run_fails_on_a_platform_config() {
    let dir = Scratch::new("tauri-config-platform");
    fs::create_dir_all(dir.join("crates/desktop")).expect("create the desktop dir");
    fs::write(dir.join("crates/desktop/build.rs"), BUILD).expect("write build.rs");
    fs::write(dir.join("crates/desktop/tauri.conf.json"), CONF).expect("write tauri.conf.json");
    let platform = "crates/desktop/tauri.macos.conf.json";
    fs::write(dir.join(platform), "{\"app\":{\"withGlobalTauri\":true}}")
        .expect("write the platform config");
    assert_eq!(
        run(&desktop_context(&dir)),
        vec![violation(
            platform,
            1,
            "platform config `crates/desktop/tauri.macos.conf.json` overrides tauri.conf.json",
            PLATFORM_FIX,
        )]
    );
}

#[test]
fn run_fails_once_when_the_desktop_crate_is_not_a_member() {
    let dir = Scratch::new("tauri-config-no-member");
    let ctx = Context {
        root: dir.to_path_buf(),
        members: Vec::new(),
    };
    let found = run(&ctx);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].path.as_str(), found[0].line), ("Cargo.toml", 1));
}
