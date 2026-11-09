//! The ripgrep guard's pins and inputs (`rg_guard.rs`, Linux): the pinned versions equal the app's
//! lock, the pinned hashes the published hex; the inputs come from the settings' script with no
//! `PATH`, so an `rg` on a `PATH` never helps; a test host's seam gives a fake install's pins or
//! turns the guard off.

use std::path::PathBuf;

use super::super::{PINNED_CODEX, PINNED_CODEX_ACP, PINNED_RG, Pins};
use super::{MISSING, RG, World, pins, refused};
use crate::agent::codex_fence::rg_guard::{CodexPins, Inputs, Seam, runs};
use crate::write;

/// The app's pins file's lock (`crates/limits/tests/live`), whose `npm ci` is what the app
/// installs.
const LOCK: &str = include_str!("../../../../limits/tests/live/package-lock.json");

/// A pins bump fails here, not by refusing every Codex launch: the adapter's, `@openai/codex`'s
/// and both Linux builds' versions in the lock are the pinned ones.
#[test]
fn the_pins_match_the_apps_lock() {
    let lock: serde_json::Value = serde_json::from_str(LOCK).unwrap();
    let version = |package: &str| {
        lock["packages"][format!("node_modules/{package}")]["version"]
            .as_str()
            .unwrap_or_else(|| panic!("{package} in the lock"))
            .to_owned()
    };
    assert_eq!(version("@agentclientprotocol/codex-acp"), PINNED_CODEX_ACP);
    assert_eq!(version("@openai/codex"), PINNED_CODEX);
    for npm in ["linux-x64", "linux-arm64"] {
        assert_eq!(
            version(&format!("@openai/codex-{npm}")),
            format!("{PINNED_CODEX}-{npm}")
        );
    }
}

/// The pinned bytes are the published hashes, and this machine's pins carry its triple's.
#[test]
fn the_pinned_hashes_are_the_published_hex() {
    let hex = |bytes: &[u8; 32]| {
        use std::fmt::Write as _;
        bytes.iter().fold(String::new(), |mut text, b| {
            write!(text, "{b:02x}").unwrap();
            text
        })
    };
    assert_eq!(
        PINNED_RG.map(|(triple, bytes)| (triple, hex(&bytes))),
        [
            (
                "x86_64-unknown-linux-musl",
                "e62198eb19b136b88c330af83647b5a962cb99b6b1f066758568f12de1974849".to_owned()
            ),
            (
                "aarch64-unknown-linux-musl",
                "e36d0eb52e70696bdf1781392722e05a21bb91d3b7b762ef5ec20e5df2ec687b".to_owned()
            ),
        ]
    );
    let pinned = Pins::pinned().expect("x86_64 or aarch64");
    let triple = format!("{}-unknown-linux-musl", std::env::consts::ARCH);
    let (_, bytes) = PINNED_RG.iter().find(|(t, _)| *t == triple).unwrap();
    assert_eq!(
        pinned,
        Pins {
            codex_acp: PINNED_CODEX_ACP,
            codex: PINNED_CODEX,
            rg_sha256: *bytes,
        }
    );
}

/// A Codex launch's settings running `script`, with `path` as their `PATH`.
fn settings(script: &std::path::Path, path: &std::path::Path) -> clauseo_settings::AgentLaunch {
    serde_json::from_value(serde_json::json!({
        "command": "/usr/bin/node",
        "args": [script, "--flag"],
        "env": { "PATH": path },
    }))
    .unwrap()
}

/// The inputs are the settings' script and the pinned values, whatever `PATH` the launch has: an
/// executable `rg` on it (the settings', and this machine's own) never stands in for the bundled
/// one, so an install with no platform package is refused.
#[test]
fn a_path_rg_never_helps() {
    let w = World::bare("rg-guard-path", "install");
    let bin = w.at("bin");
    write::create_dir_all(&bin).unwrap();
    write::write_file(&bin.join("rg"), RG).unwrap();
    write::set_mode(&bin.join("rg"), 0o755).unwrap();
    let wiring = crate::config::LaunchWiring {
        path_prepend: Some(bin.clone()),
        inherited_env: Some(vec![("PATH".into(), bin.clone().into_os_string())]),
        ..crate::config::LaunchWiring::default()
    };
    assert!(runs(&wiring));
    let inputs = Inputs::of(&settings(&w.script(), &bin), &wiring);
    assert_eq!(inputs.script, Some(w.script()));
    assert_eq!(inputs.pins, Pins::pinned());
    // The fake adapter is not at the pinned version: refused before any `rg` is looked for.
    refused(w.check_inputs(&inputs), "Reinstall Codex");
    // With the fake install's pins, its missing platform package refuses, a `PATH` `rg` holding
    // the pinned bytes or not.
    let inputs = Inputs {
        pins: Some(pins()),
        ..inputs
    };
    refused(w.check_inputs(&inputs), MISSING);
}

/// `HostConfig::with_codex_pins`'s seam: `None` turns the guard off; `Some(pins)` gives
/// the fake install's pins and script in place of the settings'.
#[test]
fn a_test_hosts_seam_gives_pins_or_turns_the_guard_off() {
    let launch = settings(
        &PathBuf::from("/opt/a/index.js"),
        &PathBuf::from("/usr/bin"),
    );
    let off = crate::config::LaunchWiring {
        codex_pins: Seam::Off,
        ..crate::config::LaunchWiring::default()
    };
    assert!(!runs(&off));
    let given = CodexPins {
        codex_acp: pins().codex_acp,
        codex: pins().codex,
        rg_sha256: pins().rg_sha256,
        script: PathBuf::from("/opt/fake/dist/index.js"),
    };
    let on = crate::config::LaunchWiring {
        codex_pins: Seam::Given(given),
        ..crate::config::LaunchWiring::default()
    };
    assert!(runs(&on));
    let inputs = Inputs::of(&launch, &on);
    assert_eq!(
        inputs.script,
        Some(PathBuf::from("/opt/fake/dist/index.js"))
    );
    assert_eq!(inputs.pins, Some(pins()));
}
