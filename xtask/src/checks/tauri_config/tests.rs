//! Unit tests for the `tauri-config` check: only what the planted cases cannot express (a key in
//! the wrong object, the CSP and `is_url` edge cases, a missing windows array, the platform config
//! names). Every other rule is a planted case in `tests/planted.rs`, built in `tests/repos.rs`.

pub(super) mod planted;
mod repos;

use super::*;

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
            Fix::GlobalTauri
        )]
    );
}

/// The `"csp": ...` entry of `CONF`.
fn csp_entry() -> String {
    format!("\"csp\": \"{PINNED_CSP}\"")
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
                Fix::CspPinned,
            )]
        );
    }
    let trimmed = PINNED_CSP.replace("; require-trusted-types-for 'script'", "");
    let (_, found) = with(&csp_entry(), &format!("\"csp\": \"{trimmed}\""));
    assert_eq!(
        found[0].message,
        "app.security.csp is not the pinned policy"
    );
}

#[test]
fn any_scheme_as_frontend_dist_fails_and_a_local_path_passes() {
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
                Fix::Frontend,
            )]
        );
    }
    for local in ["\"../../ui/app/dist\"", "\"dist\"", "\"./out/web\""] {
        let (_, found) = with(dist, &format!("\"frontendDist\": {local}"));
        assert_eq!(found, Vec::<Violation>::new(), "{local} is a local folder");
    }
}

#[test]
fn a_missing_windows_array_fails_at_line_one() {
    let (_, found) = with("\"windows\": [],", "");
    assert_eq!(
        found,
        vec![violation(
            PATH,
            1,
            "app.windows must be an empty array",
            Fix::Windows
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
