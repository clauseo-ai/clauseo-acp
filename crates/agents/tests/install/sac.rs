//! Codex's bundled ripgrep under Smart App Control, end to end on any OS with a fake SAC state
//! (`AgentsConfig::sac_for_tests`) and a stand-in layout in `dep-a`: moved aside as the install
//! finishes, the moved install still `Ready` at the next start, and moved back at a start with
//! SAC off. The registry read itself is Windows-only and not run here.

use std::path::{Path, PathBuf};

use clauseo_agents::{InstallState, Installed, SacForTests, SacState};
use sha2::{Digest as _, Sha256};

use crate::support::{Events, FAKE_RG, Fixture, SAC_RG, agent_a, to_hex};

/// The stand-in layout's vendor folder below the install folder.
const VENDOR: &str = "node_modules/dep-a/vendor/x86_64-pc-windows-msvc";

/// `state`, with the stand-in layout pinned to `rg`'s bytes.
fn sac(state: SacState, rg: &[u8]) -> SacForTests {
    SacForTests {
        state,
        layouts: vec![(VENDOR.to_owned(), to_hex(&Sha256::digest(rg)))],
    }
}

/// Installs agent A and gives its `Installed`.
fn install_ready(fx: &Fixture) -> Installed {
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    agents.installed(&agent_a()).unwrap()
}

/// Agent A's state at a fresh start.
fn first_state(fx: &Fixture) -> (InstallState, Events) {
    let (_agents, events) = fx.start();
    let (a, _) = events.first_states();
    (a, events)
}

/// The vendor folder's `codex-path` and `codex-path.sac-off`, by whether each holds `rg.exe`.
fn dirs(folder: &Path) -> (bool, bool) {
    let vendor: PathBuf = VENDOR
        .split('/')
        .fold(folder.to_path_buf(), |p, s| p.join(s));
    (
        vendor.join("codex-path/rg.exe").is_file(),
        vendor.join("codex-path.sac-off/rg.exe").is_file(),
    )
}

#[test]
fn sac_on_moves_codex_path_aside_and_the_install_stays_ready() {
    assert!(SAC_RG.starts_with("vendor/x86_64-pc-windows-msvc/codex-path/"));
    let mut fx = Fixture::new("sac-on");
    fx.config.sac_for_tests = Some(sac(SacState::On, FAKE_RG));
    let installed = install_ready(&fx);
    assert_eq!(dirs(&installed.folder), (false, true));

    // The receipt still vouches for the folder: Ready at the next start, nothing moved back,
    // nothing fetched.
    let before = fx.registry.requests().len();
    let (a, _events) = first_state(&fx);
    assert!(matches!(a, InstallState::Ready { .. }), "{a:?}");
    assert_eq!(dirs(&installed.folder), (false, true));
    assert_eq!(fx.registry.requests().len(), before);

    // A start with SAC off moves it back, and the agent is Ready.
    fx.config.sac_for_tests = Some(sac(SacState::Off, FAKE_RG));
    let (a, _events) = first_state(&fx);
    assert!(matches!(a, InstallState::Ready { .. }), "{a:?}");
    assert_eq!(dirs(&installed.folder), (true, false));
    assert_eq!(fx.registry.requests().len(), before);
}

#[test]
fn another_rg_is_left_in_place_at_install() {
    let mut fx = Fixture::new("sac-mismatch");
    fx.config.sac_for_tests = Some(sac(SacState::On, b"the pinned ripgrep"));
    let installed = install_ready(&fx);
    assert_eq!(dirs(&installed.folder), (true, false));
}

#[test]
fn a_start_with_sac_on_moves_an_installed_codex_path_and_evaluation_moves_it_back() {
    let mut fx = Fixture::new("sac-start");
    let installed = install_ready(&fx);
    assert_eq!(dirs(&installed.folder), (true, false));
    fx.config.sac_for_tests = Some(sac(SacState::On, FAKE_RG));
    let (a, _events) = first_state(&fx);
    assert!(matches!(a, InstallState::Ready { .. }), "{a:?}");
    assert_eq!(dirs(&installed.folder), (false, true));

    // Evaluation blocks nothing: the next start moves ripgrep back, and the agent is Ready.
    fx.config.sac_for_tests = Some(sac(SacState::Evaluation, FAKE_RG));
    let (a, _events) = first_state(&fx);
    assert!(matches!(a, InstallState::Ready { .. }), "{a:?}");
    assert_eq!(dirs(&installed.folder), (true, false));
}

#[test]
fn sac_evaluation_keeps_codex_path_at_install() {
    let mut fx = Fixture::new("sac-eval");
    fx.config.sac_for_tests = Some(sac(SacState::Evaluation, FAKE_RG));
    let installed = install_ready(&fx);
    assert_eq!(dirs(&installed.folder), (true, false));
}
