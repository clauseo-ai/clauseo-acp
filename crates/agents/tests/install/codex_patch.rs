//! codex-acp's shell-network patch end to end, on a stand-in: `dep-a` carries codex-acp 2.2.2's
//! modes (`CODEX_MODES`), and `AgentsConfig::codex_patch_for_tests` points the patch at them with
//! their own hashes. The install patches them before the marker and is `Ready`; a file with
//! another hash stops the install, with no install folder left.

use clauseo_agents::{CodexPatchForTests, FailureKind, InstallState};
use sha2::{Digest as _, Sha256};

use crate::support::{CODEX_MODES, CODEX_MODES_JS, Fixture, agent_a, to_hex};

/// The stand-in below the install folder.
const FILE: &str = "node_modules/dep-a/dist/modes.js";
/// The stand-in's SHA-256 once patched (the unit tests' `MODES_PINS.patched`).
const PATCHED: &str = "081d767c5f2ad009f26ed777fa27190f260f2606d4498bd318862788e366e939";

fn sha(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

fn patch_for(pristine: String) -> CodexPatchForTests {
    CodexPatchForTests {
        agent: agent_a(),
        file: FILE.to_owned(),
        pristine,
        patched: PATCHED.to_owned(),
    }
}

#[test]
fn the_codex_patch_runs_before_the_marker_and_the_install_is_ready() {
    assert_eq!(FILE, format!("node_modules/dep-a/{CODEX_MODES}"));
    let mut fx = Fixture::new("codex-patch");
    fx.config.codex_patch_for_tests = Some(patch_for(sha(CODEX_MODES_JS)));
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    let folder = agents.installed(&agent_a()).unwrap().folder;
    let path = FILE.split('/').fold(folder, |p, s| p.join(s));
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(sha(&bytes), PATCHED);
    let text = String::from_utf8(bytes).unwrap();
    assert_eq!(
        text.matches("process.env.CLAUSEO_CODEX_NETWORK === \"1\"")
            .count(),
        2
    );

    // The receipt vouches for the patched tree: Ready again at the next start.
    drop(agents);
    let (_agents, events) = fx.start();
    let (state, _) = events.first_states();
    assert!(matches!(state, InstallState::Ready { .. }), "{state:?}");
}

#[test]
fn a_codex_file_with_another_hash_fails_the_install() {
    let mut fx = Fixture::new("codex-patch-other");
    fx.config.codex_patch_for_tests = Some(patch_for("0".repeat(64)));
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    let InstallState::Failed(failure) = last else {
        panic!("not failed: {last:?}");
    };
    // A deterministic failure (`TryError::stop`): `Failed`, which a retry cannot help.
    assert_eq!(failure.kind, FailureKind::Failed, "{}", failure.log);
    assert!(
        failure
            .log
            .lines()
            .iter()
            .any(|line| line.contains("not the 2.2.2 file Clauseo patches")),
        "{}",
        failure.log
    );
    assert!(agents.installed(&agent_a()).is_none());
    // Only the one try: a deterministic failure is not retried.
    assert_eq!(fx.requests_for("dep-a").len(), 1);
}
