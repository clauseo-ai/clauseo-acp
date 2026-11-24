//! codex-acp's shell-network patch end to end, on a stand-in: `dep-a` carries codex-acp 2.10.0's
//! modes (`CODEX_MODES`), and `AgentsConfig::codex_patch_for_tests` points the patch at them.
//! The install patches them before the marker and is `Ready`; a file without the modes stops
//! the install, with no install folder left.

use crate::support::{CODEX_MODES, CODEX_MODES_JS, Fixture, agent_a};
use clauseo_agents::{CodexPatchForTests, FailureKind, InstallState};

/// The stand-in below the install folder.
const FILE: &str = "node_modules/dep-a/dist/modes.js";
/// A file of `dep-a` without the modes.
const OTHER: &str = "node_modules/dep-a/index.js";
/// The patched line.
const PATCHED_LINE: &str = "networkAccess: process.env.CLAUSEO_CODEX_NETWORK === \"1\",";

fn patch_for(file: &str) -> CodexPatchForTests {
    CodexPatchForTests {
        agent: agent_a(),
        file: file.to_owned(),
    }
}

#[test]
fn the_codex_patch_runs_before_the_marker_and_the_install_is_ready() {
    assert_eq!(FILE, format!("node_modules/dep-a/{CODEX_MODES}"));
    let mut fx = Fixture::new("codex-patch");
    fx.config.codex_patch_for_tests = Some(patch_for(FILE));
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    let folder = agents.installed(&agent_a()).unwrap().folder;
    let path = FILE.split('/').fold(folder, |p, s| p.join(s));
    let text = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(text.matches(PATCHED_LINE).count(), 2);
    assert_eq!(
        text.replace(PATCHED_LINE, "networkAccess: false,")
            .as_bytes(),
        CODEX_MODES_JS
    );

    // The receipt vouches for the patched tree: Ready again at the next start.
    drop(agents);
    let (_agents, events) = fx.start();
    let (state, _) = events.first_states();
    assert!(matches!(state, InstallState::Ready { .. }), "{state:?}");
}

#[test]
fn a_codex_file_without_the_modes_fails_the_install() {
    let mut fx = Fixture::new("codex-patch-other");
    fx.config.codex_patch_for_tests = Some(patch_for(OTHER));
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
            .any(|line| line.contains("has no single workspace-write mode")),
        "{}",
        failure.log
    );
    assert!(agents.installed(&agent_a()).is_none());
    // Only the one try: a deterministic failure is not retried.
    assert_eq!(fx.requests_for("dep-a").len(), 1);
}
