//! Tests of the staging sweep and the run lock through `run_with`.

use super::*;
use crate::scratch::Scratch;

/// Fills `bin` with what an interrupted setup leaves (a staging dir with half a download, a stray
/// staging file) and two entries that are not setup's.
fn plant_leftovers(bin: &Path) {
    let stale = bin.join(".xtask-setup-gitleaks-99999");
    Scratch::create_dir_all(&stale).unwrap();
    Scratch::write(stale.join("gitleaks_unverified.tar.gz"), b"half a download").unwrap();
    Scratch::write(bin.join(".xtask-setup-lefthook-1"), b"a stray file").unwrap();
    Scratch::write(bin.join("lefthook"), b"the working lefthook").unwrap();
    Scratch::write(bin.join(".xtask-other"), b"not ours").unwrap();
}

#[test]
fn a_binaries_run_removes_leftovers_of_an_interrupted_run_even_when_nothing_downloads() {
    let dir = Scratch::new("setup-run-leftovers-warm");
    let bin = dir.join("bin");
    plant_leftovers(&bin);
    let tool = fake_tool(Packing::Bare);
    let (ok, ran, printed) = run_fake(&linux(&bin, true), &tool, true, &[Some(tool.version)]);
    assert!(ok, "{printed}");
    assert!(!ran.iter().any(|cmd| cmd.starts_with("curl")), "{ran:?}");
    assert_eq!(
        names_in(&bin),
        [".xtask-other", ".xtask-setup.lock", "lefthook"]
    );
    assert_eq!(
        std::fs::read(bin.join("lefthook")).unwrap(),
        b"the working lefthook"
    );
}

#[test]
fn a_plain_setup_leaves_bin_alone() {
    let dir = Scratch::new("setup-run-leftovers-plain");
    let bin = dir.join("bin");
    plant_leftovers(&bin);
    let before = names_in(&bin);
    let tool = fake_tool(Packing::Bare);
    run_fake(&linux(&bin, true), &tool, false, &[Some(tool.version)]);
    assert_eq!(names_in(&bin), before);
}

/// A run lock that can be neither opened nor created: a symlink into a directory that is not there.
#[cfg(unix)]
#[test]
fn a_run_lock_that_cannot_be_taken_is_a_note_and_the_run_installs_without_the_sweep() {
    let dir = Scratch::new("setup-run-lock-error");
    let bin = dir.join("bin");
    plant_leftovers(&bin);
    Scratch::symlink(
        dir.join("no-such-dir").join("lock"),
        bin.join(".xtask-setup.lock"),
    )
    .unwrap();
    let tool = fake_tool(Packing::Bare);
    let (ok, _, printed) = run_fake(&linux(&bin, true), &tool, true, &[None, Some(tool.version)]);
    assert!(ok, "{printed}");
    let lock = bin.join(".xtask-setup.lock");
    // The error text is the OS's own, so only what this code adds around it is checked.
    assert!(printed.contains(&format!("could not lock {} (", lock.display())));
    assert!(printed.contains(&format!(
        "), so this run does not remove what an interrupted setup left in {}\n",
        bin.display()
    )));
    assert!(!printed.contains("fix:"), "{printed}");
    assert!(bin.join(".xtask-setup-gitleaks-99999").is_dir());
    assert_eq!(std::fs::read(bin.join("faketool")).unwrap(), ASSET_BYTES);
}
