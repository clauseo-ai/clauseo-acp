//! Tests for the `tauri-config` check: each rule on a failing and a passing config, and `run` on a
//! scratch tree laid out like the repo.

use super::*;
use crate::checks::tests::desktop_context;
use crate::scratch::Scratch;
use std::fs;

/// A config that passes every rule, shaped like the committed one.
const CONF: &str = r#"{
  "build": {
    "devUrl": "http://localhost:1420",
    "frontendDist": "../../ui/app/dist"
  },
  "app": {
    "withGlobalTauri": false,
    "windows": [],
    "security": {
      "freezePrototype": true,
      "capabilities": ["main"],
      "csp": "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data: clauseo-file: http://clauseo-file.localhost https://clauseo-file.localhost; font-src 'self'; connect-src ipc: http://ipc.localhost https://ipc.localhost; worker-src 'self'; frame-src 'self'; child-src 'self'; object-src 'none'; media-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; require-trusted-types-for 'script'; trusted-types clauseo-sanitize"
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

/// The `"csp": ...` entry of `CONF`.
fn csp_entry() -> String {
    format!("\"csp\": \"{PINNED_CSP}\"")
}

#[test]
fn the_fixture_holds_the_pinned_csp() {
    assert!(CONF.contains(&csp_entry()));
}

#[test]
fn a_csp_other_than_the_pinned_string_fails_at_its_line() {
    for csp in [
        "default-src *; script-src * data:",
        "default-src 'self'; script-src 'self' https://cdn.example",
        "",
    ] {
        let (text, found) = with(&csp_entry(), &format!("\"csp\": \"{csp}\""));
        assert_eq!(
            found,
            vec![violation(
                PATH,
                line_of(&text, "\"csp\""),
                "app.security.csp is not the pinned policy",
                CSP_PINNED_FIX,
            )]
        );
    }
    let trimmed = PINNED_CSP.replace("; require-trusted-types-for 'script'", "");
    let (_, found) = with(&csp_entry(), &format!("\"csp\": \"{trimmed}\""));
    assert_eq!(
        found[0].message,
        "app.security.csp is not the pinned policy"
    );
    assert!(CSP_PINNED_FIX.contains("pinned in xtask/src/checks/tauri_config.rs"));
}

#[test]
fn a_csp_object_or_a_missing_csp_fails() {
    let (_, found) = with(
        &csp_entry(),
        "\"csp\": { \"default-src\": \"'self'\", \"script-src\": [\"'self'\"] }",
    );
    assert_eq!(
        found[0].message,
        "app.security.csp is an object, not the pinned policy string"
    );
    let (_, found) = with(&format!("[\"main\"],\n      {}", csp_entry()), "[\"main\"]");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].message, "app.security.csp is missing");
    assert_eq!(found[0].line, 1);
}

#[test]
fn a_dev_csp_is_not_checked() {
    let (_, found) = with(
        &csp_entry(),
        &format!(
            "{},\n      \"devCsp\": \"script-src 'unsafe-eval'\"",
            csp_entry()
        ),
    );
    assert_eq!(found, Vec::new());
}

#[test]
fn a_url_or_a_file_list_as_frontend_dist_fails_at_its_line() {
    let dist = "\"frontendDist\": \"../../ui/app/dist\"";
    for value in [
        "\"https://evil.example\"",
        "\"http://localhost:1420\"",
        "\"tauri://localhost\"",
        "\"HTTPS:evil.example\"",
        "\"file:/x\"",
        "\"ftp:host\"",
        "\"data:text/html,x\"",
        "\"C:\\\\dist\"",
    ] {
        let (text, found) = with(dist, &format!("\"frontendDist\": {value}"));
        assert_eq!(
            found,
            vec![violation(
                PATH,
                line_of(&text, "frontendDist"),
                "build.frontendDist is a URL",
                FRONTEND_FIX,
            )]
        );
    }
    for local in ["\"../../ui/app/dist\"", "\"dist\"", "\"./out/web\""] {
        let (_, found) = with(dist, &format!("\"frontendDist\": {local}"));
        assert_eq!(found, Vec::<Violation>::new(), "{local} is a local folder");
    }
    let (_, found) = with(dist, "\"frontendDist\": [\"index.html\"]");
    assert_eq!(
        found[0].message,
        "build.frontendDist is a list of files, not the dist folder"
    );
    assert_eq!(
        FRONTEND_FIX,
        "frontendDist must be the local dist folder; a URL makes the release app load a remote site"
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
fn updater_endpoints_without_a_pubkey_fail() {
    let (text, found) = with(
        "\"endpoints\": []",
        "\"endpoints\": [\"https://releases.example/latest.json\"]",
    );
    assert_eq!(
        found,
        vec![violation(
            PATH,
            line_of(&text, "\"endpoints\""),
            "plugins.updater has endpoints but no pubkey",
            PUBKEY_FIX,
        )]
    );
    let keyed = text.replace(
        "\"pubkey\": \"\"",
        "\"pubkey\": \"dW50cnVzdGVkIGNvbW1lbnQ=\"",
    );
    assert_eq!(conf_violations(PATH, Some(&keyed)), Vec::new());
}

#[test]
fn v1_compatible_updater_artifacts_fail() {
    let (text, found) = with(
        "\"icon\": []",
        "\"icon\": [],\n    \"createUpdaterArtifacts\": \"v1Compatible\"",
    );
    assert_eq!(
        found,
        vec![violation(
            PATH,
            line_of(&text, "createUpdaterArtifacts"),
            "bundle.createUpdaterArtifacts is \"v1Compatible\"",
            V1_FIX,
        )]
    );
    let (_, found) = with(
        "\"icon\": []",
        "\"icon\": [],\n    \"createUpdaterArtifacts\": true",
    );
    assert_eq!(found, Vec::new());
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
fn a_commented_out_manifest_does_not_count() {
    let build = "crates/desktop/build.rs";
    let commented = "// app_manifest( removed, AppManifest::new() too\nfn main() {\n    tauri_build::build();\n}\n";
    assert_eq!(build_rs_violations(build, Some(commented)).len(), 1);
    let half = "fn main() {\n    let a = Attributes::new().app_manifest(manifest()); // AppManifest::new()\n}\n";
    assert_eq!(build_rs_violations(build, Some(half)).len(), 1);
}

#[test]
fn a_manifest_with_commands_passes() {
    let build = "crates/desktop/build.rs";
    let with_commands = BUILD.replace(
        "AppManifest::new()",
        "AppManifest::new().commands(&[\"open_matter\"])",
    );
    assert_eq!(build_rs_violations(build, Some(&with_commands)), Vec::new());
}

#[test]
fn capabilities_path_pattern_fails_at_its_line() {
    let build = "crates/desktop/build.rs";
    let text = BUILD.replace(
        "tauri_build::AppManifest::new())",
        "tauri_build::AppManifest::new())\n            .capabilities_path_pattern(\"elsewhere/*\")",
    );
    assert_eq!(
        build_rs_violations(build, Some(&text)),
        vec![violation(
            build,
            line_of(&text, "capabilities_path_pattern("),
            "build.rs calls `capabilities_path_pattern(`",
            PATH_PATTERN_FIX,
        )]
    );
}

#[test]
fn the_check_is_registered_after_plugins() {
    let names: Vec<&str> = crate::checks::ALL.iter().map(|check| check.name).collect();
    assert_eq!(
        names,
        [
            "budgets",
            "capabilities",
            "plugins",
            "tauri-config",
            "buildtime-edits",
            "file-size"
        ]
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
    assert_eq!(run(&ctx), Inspection::new(2, Vec::new()));
    let planted = CONF.replace("\"withGlobalTauri\": false", "\"withGlobalTauri\": true");
    fs::write(dir.join(conf), &planted).expect("write tauri.conf.json");
    assert_eq!(
        run(&ctx).violations,
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
        Inspection::new(
            3,
            vec![violation(
                platform,
                1,
                "platform config `crates/desktop/tauri.macos.conf.json` overrides tauri.conf.json",
                PLATFORM_FIX,
            )]
        )
    );
}

#[test]
fn run_fails_once_when_the_desktop_crate_is_not_a_member() {
    let dir = Scratch::new("tauri-config-no-member");
    let ctx = Context {
        root: dir.to_path_buf(),
        members: Vec::new(),
    };
    let found = run(&ctx).violations;
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].path.as_str(), found[0].line), ("Cargo.toml", 1));
}
