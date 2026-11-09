//! The data-folder fence in a Codex launch's `CODEX_CONFIG` (`agent::codex_fence`, decision record
//! 0030), checked from `codex.rs`'s launch test: denies scoped under the data folder, `run` whole
//! and `agent-state` entry by entry (the own folders reopen as `write` entries, checked in
//! `profile.rs`), the login home whole, never the session's state folder (its `CODEX_HOME`, R17
//! PR-C) but the login link and the config surfaces in it. Windows gets none.

use std::path::Path;

use clauseo_protocol::MatterId;
use serde_json::{Value, json};

use crate::support::Dir;

/// Checks the fence keys in `config` for a Codex session of `matter` whose run folder is `run`
/// and whose Codex home (its state folder) is `home`, and returns them as they are, for the
/// exact comparison.
pub(crate) fn checked_keys(
    config: &Value,
    dir: &Dir,
    matter: MatterId,
    run: &Path,
    home: &Path,
) -> [(&'static str, Value); 2] {
    let data = crate::profile_rig::path_text(dir.data());
    assert_eq!(config["default_permissions"], json!("clauseo"));
    let profile = &config["permissions"]["clauseo"];
    assert_eq!(profile["extends"], json!(":workspace"));
    let scoped = profile["filesystem"][data.as_str()]
        .as_object()
        .expect("denies scoped under the data folder");
    for subpath in [
        "logs",
        "search.sqlite",
        "state.sqlite",
        "skill-packs",
        "set-aside",
        "run",
        "agents/claude-acp",
        "agents/codex-acp/home",
    ] {
        assert_eq!(scoped.get(subpath), Some(&json!("deny")), "{subpath}");
    }
    let notes = crate::profile_rig::notes(dir, matter);
    for (subpath, mode) in scoped {
        if mode == &json!("write") || ["run", "notes", "agent-state"].contains(&subpath.as_str()) {
            continue;
        }
        assert_eq!(mode, &json!("deny"), "{subpath}");
        let denied = dir.data().join(subpath);
        if denied.starts_with(home) {
            // The login link, the transcripts and the config surfaces in the state folder.
            continue;
        }
        for own in [run, notes.as_path(), home] {
            assert!(!own.starts_with(&denied), "{subpath}");
        }
    }
    [
        ("default_permissions", config["default_permissions"].clone()),
        ("permissions", config["permissions"].clone()),
    ]
}

/// The fake `rg` of [`fake_install`] and its SHA-256 (`sha256sum`), the pin the guard checks.
#[cfg(target_os = "linux")]
const RG: (&[u8], [u8; 32]) = (
    b"#!/bin/sh\n",
    [
        0xa8, 0x07, 0x6d, 0x3d, 0x28, 0xd2, 0x1e, 0x02, 0x01, 0x2b, 0x20, 0xea, 0xf7, 0xdb, 0xf7,
        0x54, 0x09, 0xa6, 0x27, 0x71, 0x34, 0x43, 0x90, 0x25, 0xf2, 0x82, 0xe3, 0x68, 0xe3, 0x30,
        0x5a, 0xbf,
    ],
);

/// A fake Codex install under `install`, as `npm ci` of the app's pins lays it out, at the test
/// versions `9.0.0-test` (the adapter) and `1.2.3-test` (`@openai/codex`): the adapter's
/// package and script, `@openai/codex`, and this machine's platform package with its vendored
/// executable `rg`. Returns the adapter's script.
#[cfg(target_os = "linux")]
fn fake_install(install: &Path) -> std::path::PathBuf {
    let (npm, triple) = match std::env::consts::ARCH {
        "x86_64" => ("linux-x64", "x86_64-unknown-linux-musl"),
        "aarch64" => ("linux-arm64", "aarch64-unknown-linux-musl"),
        other => panic!("no Codex build for {other}"),
    };
    let modules = install.join("node_modules");
    let adapter = modules.join("@agentclientprotocol/codex-acp");
    let build = modules.join(format!("@openai/codex-{npm}"));
    for (folder, name, version) in [
        (
            &adapter,
            "@agentclientprotocol/codex-acp",
            "9.0.0-test".to_owned(),
        ),
        (
            &modules.join("@openai/codex"),
            "@openai/codex",
            "1.2.3-test".to_owned(),
        ),
        (&build, "@openai/codex", format!("1.2.3-test-{npm}")),
    ] {
        let manifest = json!({ "name": name, "version": version }).to_string();
        crate::write::write(&folder.join("package.json"), manifest.as_bytes());
    }
    crate::write::write(&adapter.join("dist/index.js"), b"");
    let rg = build.join("vendor").join(triple).join("codex-path/rg");
    crate::write::write(&rg, RG.0);
    crate::write::make_executable(&rg);
    adapter.join("dist/index.js")
}

/// Linux, R17 PR-D1's ripgrep guard (`agent::codex_fence::rg_guard`), turned on with a fake
/// install's pins (`HostConfig::with_codex_pins`; `Dir` turns it off): with no install at the
/// adapter's script, a Codex launch is refused with `agent.launch_failed` before anything starts,
/// whatever `rg` is on the `PATH`; with the fake pinned install it runs to `EndTurn`.
#[cfg(target_os = "linux")]
#[test]
fn a_codex_launch_without_its_pinned_ripgrep_is_refused() {
    use std::sync::Arc;

    use clauseo_host::CodexPins;
    use clauseo_protocol::{DomainEvent, ErrorCode, StopReason, StreamId};

    use crate::codex::{CODEX, Recording, codex_dir};
    use crate::support::matter_with_thread;

    for (test, installed) in [("codex_fence_no_rg", false), ("codex_fence_rg", true)] {
        let dir = codex_dir(test, "codex-handshake");
        let install = dir.work().join("codex-install");
        let script = if installed {
            fake_install(&install)
        } else {
            install.join("node_modules/@agentclientprotocol/codex-acp/dist/index.js")
        };
        let pins = CodexPins {
            codex_acp: "9.0.0-test",
            codex: "1.2.3-test",
            rg_sha256: RG.1,
            script,
        };
        let recording = Recording::new(&dir);
        let launches = Arc::clone(&recording.envs);
        let host = dir.start_launching(0, Arc::new(recording), |config| {
            config.with_codex_pins(Some(pins))
        });
        let (_, thread, _) = matter_with_thread(&dir, &host, "a", &[("brief.txt", "x\n")], CODEX);
        let mut events = host.events(StreamId::Thread(thread));
        let (turn, _) = host.prompt(thread, "Look at the matter.");
        let ended = events.until_turn_ended(turn).stop;
        if installed {
            assert_eq!(ended, StopReason::EndTurn);
            assert_eq!(launches.lock().unwrap().len(), 1);
        } else {
            assert_eq!(ended, StopReason::Failed);
            let code = events.events().find_map(|event| match event {
                DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.code),
                _ => None,
            });
            assert_eq!(code, Some(ErrorCode::AgentLaunchFailed));
            assert!(launches.lock().unwrap().is_empty(), "nothing launched");
        }
        host.shutdown();
    }
}
