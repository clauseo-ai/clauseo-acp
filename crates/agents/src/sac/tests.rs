//! The rules on a fake layout, on any OS: the pin, each SAC state, and running twice.

use super::*;
use crate::write::{self, TestDir};

/// The fake ripgrep's bytes.
const RG_BYTES: &[u8] = b"not really ripgrep\n";
/// A layout's vendor folder below the fake install folder.
const VENDOR: &str = "node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc";

/// The SHA-256 of `bytes` in hex.
fn sha(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

/// An install folder with Codex's layout, `rg.exe` in `codex-path`.
fn install() -> (TestDir, PathBuf) {
    let dir = TestDir::new("sac");
    dir.file(&format!("{VENDOR}/{PATH_DIR}/{RG}"), RG_BYTES);
    dir.file("clauseo-receipt.json", b"{}");
    let vendor = rel(&dir.0, VENDOR);
    (dir, vendor)
}

/// Applies `state` with `pin` to the folder's one layout.
fn run(vendor: &Path, pin: &str, state: SacState) -> Vec<Outcome> {
    apply(&[(vendor.to_path_buf(), pin)], || state)
}

/// Whether `codex-path` and `codex-path.sac-off` are there, each holding `rg.exe`.
fn dirs(vendor: &Path) -> (bool, bool) {
    (
        vendor.join(PATH_DIR).join(RG).is_file(),
        vendor.join(MOVED_DIR).join(RG).is_file(),
    )
}

#[test]
fn the_registry_values_map_to_states() {
    assert_eq!(SacState::from_value(Some(0)), SacState::Off);
    assert_eq!(SacState::from_value(Some(1)), SacState::On);
    assert_eq!(SacState::from_value(Some(2)), SacState::Evaluation);
    assert_eq!(SacState::from_value(Some(3)), SacState::Unknown(Some(3)));
    assert_eq!(SacState::from_value(None), SacState::Unknown(None));
}

#[test]
fn the_pins_are_codex_0_162_1s_ripgrep() {
    // The x86_64 pin is the official ripgrep 15.2.0 msvc rg.exe (sac-ripgrep-plan.md).
    assert_eq!(
        LAYOUTS[0],
        (
            VENDOR,
            "14231169855ec5205cf5a1b6f1db358ff4aed4247c86b69ce8aae647c77f6680"
        )
    );
    for (dir, pin) in LAYOUTS {
        assert!(dir.ends_with("-pc-windows-msvc"), "{dir}");
        assert_eq!(pin.len(), 64, "{pin}");
        assert!(
            pin.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
            "{pin}"
        );
    }
}

#[test]
fn sac_on_with_the_pinned_rg_moves_codex_path_aside() {
    let (_dir, vendor) = install();
    assert_eq!(run(&vendor, &sha(RG_BYTES), SacState::On), [Outcome::Moved]);
    assert_eq!(dirs(&vendor), (false, true));
}

#[test]
fn sac_evaluation_keeps_codex_path_and_moves_a_moved_one_back() {
    let (_dir, vendor) = install();
    let pin = sha(RG_BYTES);
    assert_eq!(run(&vendor, &pin, SacState::Evaluation), []);
    assert_eq!(dirs(&vendor), (true, false));
    assert_eq!(run(&vendor, &pin, SacState::On), [Outcome::Moved]);
    assert_eq!(
        run(&vendor, &pin, SacState::Evaluation),
        [Outcome::Restored]
    );
    assert_eq!(dirs(&vendor), (true, false));
}

#[test]
fn another_rg_is_left_in_place_with_a_warning() {
    let (_dir, vendor) = install();
    let outcomes = run(&vendor, &sha(b"the pinned ripgrep"), SacState::On);
    assert_eq!(outcomes, [Outcome::Mismatch(sha(RG_BYTES))]);
    assert_eq!(outcomes[0].level(), tracing::Level::WARN);
    assert_eq!(dirs(&vendor), (true, false));
}

#[test]
fn a_missing_rg_is_a_mismatch_and_nothing_moves() {
    let (_dir, vendor) = install();
    write::remove_file(&vendor.join(PATH_DIR).join(RG)).unwrap();
    let outcomes = run(&vendor, &sha(RG_BYTES), SacState::On);
    assert!(
        matches!(outcomes[..], [Outcome::Mismatch(_)]),
        "{outcomes:?}"
    );
    assert!(vendor.join(PATH_DIR).is_dir());
    assert!(!vendor.join(MOVED_DIR).exists());
}

#[test]
fn sac_off_keeps_codex_path() {
    let (_dir, vendor) = install();
    assert_eq!(run(&vendor, &sha(RG_BYTES), SacState::Off), []);
    assert_eq!(dirs(&vendor), (true, false));
}

#[test]
fn sac_off_moves_a_moved_codex_path_back() {
    let (_dir, vendor) = install();
    run(&vendor, &sha(RG_BYTES), SacState::On);
    let outcomes = run(&vendor, &sha(RG_BYTES), SacState::Off);
    assert_eq!(outcomes, [Outcome::Restored]);
    assert_eq!(outcomes[0].level(), tracing::Level::INFO);
    assert_eq!(dirs(&vendor), (true, false));
}

#[test]
fn an_unknown_state_leaves_either_folder_as_it_is() {
    for value in [Some(7), None] {
        let (_dir, vendor) = install();
        let pin = sha(RG_BYTES);
        let outcomes = run(&vendor, &pin, SacState::Unknown(value));
        assert_eq!(outcomes, [Outcome::Unknown(value)]);
        assert_eq!(outcomes[0].level(), tracing::Level::INFO);
        assert_eq!(dirs(&vendor), (true, false));
        run(&vendor, &pin, SacState::On);
        run(&vendor, &pin, SacState::Unknown(value));
        assert_eq!(dirs(&vendor), (false, true));
    }
}

#[test]
fn running_twice_changes_nothing_more() {
    let (_dir, vendor) = install();
    let pin = sha(RG_BYTES);
    assert_eq!(run(&vendor, &pin, SacState::On), [Outcome::Moved]);
    assert_eq!(run(&vendor, &pin, SacState::On), []);
    assert_eq!(dirs(&vendor), (false, true));
    assert_eq!(run(&vendor, &pin, SacState::Off), [Outcome::Restored]);
    assert_eq!(run(&vendor, &pin, SacState::Off), []);
    assert_eq!(dirs(&vendor), (true, false));
}

#[test]
fn a_leftover_moved_folder_is_replaced_by_the_move() {
    let (dir, vendor) = install();
    dir.file(&format!("{VENDOR}/{MOVED_DIR}/stale.txt"), b"stale");
    assert_eq!(run(&vendor, &sha(RG_BYTES), SacState::On), [Outcome::Moved]);
    assert_eq!(dirs(&vendor), (false, true));
    assert!(!vendor.join(MOVED_DIR).join("stale.txt").exists());
}

#[test]
fn a_folder_with_no_codex_reads_no_state() {
    let dir = TestDir::new("sac-none");
    dir.file("node_modules/adapter/index.js", b"");
    let vendor = rel(&dir.0, VENDOR);
    let outcomes = apply(&[(vendor, "00")], || panic!("SAC's state was read"));
    assert_eq!(outcomes, []);
}
