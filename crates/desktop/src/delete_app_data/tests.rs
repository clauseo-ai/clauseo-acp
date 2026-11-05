//! `--delete-app-data` on scratch known folders: links under the data folders go as links and
//! their targets survive, refusals delete nothing, and the sandbox's folders are kept as the
//! contract says. On Windows the folder links are junctions (`make.rs`); elsewhere symbolic links
//! stand in, so the walk's logic runs on every CI OS.

mod make;
mod sources;
#[cfg(windows)]
mod windows;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_settings::{KnownFolder, PlatformDirs};

use super::walk::{self, check};
use super::{DELETED, FAILED, KEPT_SANDBOX, KEPT_SET_ASIDE, Mode, ops, run};
use crate::test_support::Scratch;

const DATA: &str = "ai.clauseo.desktop";

/// Known folders under a scratch folder: `Local` and `Roaming`, or none for a `None`.
struct Dirs {
    local: Option<PathBuf>,
    roaming: Option<PathBuf>,
}

impl PlatformDirs for Dirs {
    fn home(&self) -> Option<PathBuf> {
        None
    }

    fn known_folder(&self, folder: KnownFolder) -> Option<PathBuf> {
        match folder {
            KnownFolder::LocalAppData => self.local.clone(),
            KnownFolder::RoamingAppData => self.roaming.clone(),
            _ => None,
        }
    }
}

/// A scratch folder with `Local\ai.clauseo.desktop`, `Roaming\ai.clauseo.desktop` (holding a
/// file), and `outside\canary`, outside both, holding `secret.txt` and `deep\d.txt`.
struct World {
    scratch: Scratch,
}

impl World {
    fn new(test: &str) -> Self {
        let scratch = Scratch::folder(
            test,
            &[
                "Local/ai.clauseo.desktop",
                "Roaming/ai.clauseo.desktop",
                "outside/canary/deep",
            ],
        );
        scratch.file("Roaming/ai.clauseo.desktop/.window-state.json", b"{}");
        scratch.file("outside/canary/secret.txt", b"the lawyer's file");
        scratch.file("outside/canary/deep/d.txt", b"another");
        Self { scratch }
    }

    fn dirs(&self) -> Dirs {
        Dirs {
            local: Some(self.scratch.path().join("Local")),
            roaming: Some(self.scratch.path().join("Roaming")),
        }
    }

    /// `rel` under the scratch folder.
    fn at(&self, rel: &str) -> PathBuf {
        self.scratch.path().join(rel)
    }

    /// `rel` under the local data folder.
    fn data(&self, rel: &str) -> PathBuf {
        self.at(&format!("Local/{DATA}/{rel}"))
    }

    /// Makes the folder `rel` under the local data folder.
    fn data_dir(&self, rel: &str) -> PathBuf {
        self.scratch.dir(&format!("Local/{DATA}/{rel}"))
    }

    /// Writes the file `rel` under the local data folder (its folder must exist).
    fn data_file(&self, rel: &str) -> PathBuf {
        self.scratch.file(&format!("Local/{DATA}/{rel}"), b"data")
    }

    fn canary(&self) -> PathBuf {
        self.at("outside/canary")
    }

    /// Runs the deletion, with `--sandbox-removed` when `sandbox_removed`; answers its exit
    /// code and stderr.
    fn delete(&self, sandbox_removed: bool) -> (i32, String) {
        let mut stderr = Vec::new();
        let code = walk::delete(&self.dirs(), Mode { sandbox_removed }, &mut stderr);
        (code, String::from_utf8(stderr).unwrap())
    }

    /// The canary and both its files are there, unchanged.
    fn assert_canary_whole(&self) {
        let canary = self.canary();
        assert_eq!(
            std::fs::read(canary.join("secret.txt")).unwrap(),
            b"the lawyer's file"
        );
        assert_eq!(
            std::fs::read(canary.join("deep/d.txt")).unwrap(),
            b"another"
        );
    }

    /// Both data folders are gone.
    fn assert_both_gone(&self) {
        for known in ["Local", "Roaming"] {
            let root = self.at(&format!("{known}/{DATA}"));
            assert!(
                std::fs::symlink_metadata(&root).is_err(),
                "{}",
                root.display()
            );
        }
    }
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

#[test]
fn only_nothing_or_sandbox_removed_is_accepted() {
    assert_eq!(
        Mode::parse(&[]),
        Some(Mode {
            sandbox_removed: false
        })
    );
    assert_eq!(
        Mode::parse(&args(&["--sandbox-removed"])),
        Some(Mode {
            sandbox_removed: true
        })
    );
    for refused in [
        &[r"C:\Users\a\AppData\Local\ai.clauseo.desktop"][..],
        &["/home/a/.local/share/ai.clauseo.desktop"][..],
        &["ai.clauseo.desktop"][..],
        &["--sandbox-removed", "--sandbox-removed"][..],
        &["--sandbox-removed", r"C:\elsewhere"][..],
        &[r"C:\elsewhere", "--sandbox-removed"][..],
        &["--sandbox-removed=1"][..],
        &["--Sandbox-Removed"][..],
        &[""][..],
    ] {
        assert_eq!(Mode::parse(&args(refused)), None, "{refused:?}");
    }
}

#[test]
fn extra_arguments_are_refused_before_any_folder_is_looked_at() {
    // `run` is the real entry point: with a valid argument list it would reach the real known
    // folders, so only refused lists are run here.
    for refused in [&[r"C:\elsewhere"][..], &["--sandbox-removed", "x"][..]] {
        let mut stderr = Vec::new();
        assert_eq!(run(&args(refused), &mut stderr), FAILED, "{refused:?}");
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "--delete-app-data takes no path; its only option is --sandbox-removed. Nothing \
             deleted.\n"
        );
    }
}

#[cfg(not(windows))]
#[test]
fn off_windows_it_says_it_is_for_the_windows_uninstaller_and_deletes_nothing() {
    let mut stderr = Vec::new();
    assert_eq!(run(&[], &mut stderr), FAILED);
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        "--delete-app-data is for the Windows uninstaller only. Nothing deleted.\n"
    );
}

#[test]
fn a_folder_link_in_the_data_folder_goes_as_a_link_and_its_target_survives() {
    let world = World::new("dad-link");
    world.data_dir("sandbox");
    world.data_file("store.sqlite");
    make::folder_link(&world.canary(), &world.data("sandbox/agent-tools"));
    // S15's removal ran, so `sandbox` goes too, its link as a link.
    let (code, stderr) = world.delete(true);
    assert_eq!((code, stderr.as_str()), (DELETED, ""));
    world.assert_both_gone();
    world.assert_canary_whole();
}

#[test]
fn a_folder_link_two_levels_down_goes_as_a_link_and_its_target_survives() {
    let world = World::new("dad-nested");
    world.data_dir("agents/claude/node_modules");
    world.data_file("agents/claude/package.json");
    make::folder_link(
        &world.canary(),
        &world.data("agents/claude/node_modules/linked"),
    );
    let (code, stderr) = world.delete(false);
    assert_eq!((code, stderr.as_str()), (DELETED, ""));
    world.assert_both_gone();
    world.assert_canary_whole();
}

#[test]
fn a_file_link_in_the_data_folder_goes_as_a_link_and_its_target_survives() {
    let world = World::new("dad-file-link");
    let link = world.data("secret-link.txt");
    if let Err(error) = make::symlink_file(&world.canary().join("secret.txt"), &link) {
        eprintln!("skipped: this machine may not make file symbolic links here ({error})");
        return;
    }
    let (code, stderr) = world.delete(false);
    assert_eq!((code, stderr.as_str()), (DELETED, ""));
    world.assert_both_gone();
    world.assert_canary_whole();
}

#[test]
fn a_data_folder_that_is_a_link_is_refused_and_nothing_is_deleted() {
    let world = World::new("dad-root-link");
    let local = world.at(&format!("Local/{DATA}"));
    make::remove_dir(&local);
    make::folder_link(&world.canary(), &local);
    let (code, stderr) = world.delete(false);
    assert_eq!(code, FAILED);
    assert!(stderr.contains("is a link"), "{stderr}");
    assert!(exists(&local), "the link itself is left alone");
    world.assert_canary_whole();
    // The other folder, checked first or not, is untouched too.
    assert!(exists(
        &world.at(&format!("Roaming/{DATA}/.window-state.json"))
    ));
}

#[test]
fn the_check_refuses_anything_but_the_data_folder_directly_in_its_known_folder() {
    let world = World::new("dad-check");
    let known = world.at("Local");
    let root = known.join(DATA);
    assert_eq!(check(&known, &root), Ok(true));
    world.scratch.dir("Local/elsewhere");
    world.scratch.file("Local/a-file", b"");
    for (known, root) in [
        // The known folder itself, its parent, a sibling, a folder further down.
        (known.clone(), known.clone()),
        (known.clone(), world.scratch.path().to_owned()),
        (known.clone(), known.join("elsewhere")),
        (known.clone(), root.join("sandbox").join(DATA)),
        // A relative known folder.
        (PathBuf::from("Local"), PathBuf::from("Local").join(DATA)),
    ] {
        assert!(check(&known, &root).is_err(), "{}", root.display());
    }
    // A data folder that is a file.
    let file_root = world.at("Roaming").join(DATA);
    make::remove_file(&file_root.join(".window-state.json"));
    make::remove_dir(&file_root);
    world.scratch.file(&format!("Roaming/{DATA}"), b"");
    assert_eq!(
        check(&world.at("Roaming"), &file_root),
        Err("is not a folder".to_owned())
    );
}

#[test]
fn a_known_folder_windows_cannot_give_refuses_and_nothing_is_deleted() {
    let world = World::new("dad-no-known");
    world.data_file("store.sqlite");
    let mut stderr = Vec::new();
    let dirs = Dirs {
        roaming: None,
        ..world.dirs()
    };
    let code = walk::delete(
        &dirs,
        Mode {
            sandbox_removed: false,
        },
        &mut stderr,
    );
    assert_eq!(code, FAILED);
    assert!(exists(&world.data("store.sqlite")));
}

#[test]
fn folders_never_made_are_already_deleted() {
    let world = World::new("dad-absent");
    make::remove_dir(&world.at(&format!("Local/{DATA}")));
    assert_eq!(world.delete(false), (DELETED, String::new()));
    assert_eq!(world.delete(true), (DELETED, String::new()));
    world.assert_both_gone();
}

#[test]
fn without_sandbox_removed_the_sandbox_folder_stays_whole_and_the_rest_goes() {
    // S15's removal did not run or failed (or its tool is not there): its records stay.
    let world = World::new("dad-keep-sandbox");
    world.data_dir("sandbox/set-aside");
    world.data_file("sandbox/acl-ledger.json");
    world.data_file("store.sqlite");
    // A link inside the kept folder is not even looked at.
    make::folder_link(&world.canary(), &world.data("sandbox/agent-link"));
    let (code, stderr) = world.delete(false);
    assert_eq!((code, stderr.as_str()), (KEPT_SANDBOX, ""));
    assert!(exists(&world.data("sandbox/acl-ledger.json")));
    assert!(exists(&world.data("sandbox/set-aside")));
    assert!(exists(&world.data("sandbox/agent-link")));
    assert!(!exists(&world.data("store.sqlite")));
    assert!(!exists(&world.at(&format!("Roaming/{DATA}"))));
    world.assert_canary_whole();
}

#[test]
fn with_sandbox_removed_the_sandbox_folder_goes() {
    let world = World::new("dad-sandbox-removed");
    world.data_dir("sandbox/set-aside");
    world.data_file("sandbox/acl-ledger.json");
    world.data_file("store.sqlite");
    assert_eq!(world.delete(true), (DELETED, String::new()));
    world.assert_both_gone();
}

#[test]
fn the_kept_folders_are_found_whatever_their_case() {
    let world = World::new("dad-case");
    world.data_dir("Sandbox/Set-Aside/20261105T101500");
    world.data_file("Sandbox/Set-Aside/20261105T101500/brief.docx");
    world.data_file("Sandbox/acl-ledger.json");
    assert_eq!(world.delete(true), (KEPT_SET_ASIDE, String::new()));
    assert!(exists(
        &world.data("Sandbox/Set-Aside/20261105T101500/brief.docx")
    ));
    assert!(!exists(&world.data("Sandbox/acl-ledger.json")));
    world.data_file("Sandbox/acl-ledger.json");
    assert_eq!(world.delete(false), (KEPT_SANDBOX, String::new()));
    assert!(exists(&world.data("Sandbox/acl-ledger.json")));
}

#[test]
fn a_set_aside_holding_files_is_kept_and_everything_else_goes() {
    let world = World::new("dad-set-aside-full");
    world.data_dir("sandbox/set-aside/20261105T101500/.git");
    world.data_file("sandbox/set-aside/20261105T101500/.git/HEAD");
    world.data_file("sandbox/acl-ledger.json");
    world.data_file("store.sqlite");
    let (code, stderr) = world.delete(true);
    assert_eq!((code, stderr.as_str()), (KEPT_SET_ASIDE, ""));
    assert!(exists(
        &world.data("sandbox/set-aside/20261105T101500/.git/HEAD")
    ));
    assert!(!exists(&world.data("sandbox/acl-ledger.json")));
    assert!(!exists(&world.data("store.sqlite")));
    // Without --sandbox-removed the whole sandbox folder stays, the set-aside with it.
    world.data_file("sandbox/acl-ledger.json");
    assert_eq!(world.delete(false), (KEPT_SANDBOX, String::new()));
    assert!(exists(&world.data("sandbox/acl-ledger.json")));
}

#[test]
fn an_empty_set_aside_goes_with_the_rest() {
    let world = World::new("dad-set-aside-empty");
    world.data_dir("sandbox/set-aside");
    world.data_file("sandbox/acl-ledger.json");
    assert_eq!(world.delete(true), (DELETED, String::new()));
    world.assert_both_gone();
}

#[test]
fn a_set_aside_reached_through_a_linked_sandbox_is_not_kept() {
    // `sandbox` itself a link: it goes as a link, and nothing behind it is kept or touched.
    let world = World::new("dad-sandbox-link");
    world.scratch.dir("outside/canary/set-aside");
    world
        .scratch
        .file("outside/canary/set-aside/kept.txt", b"x");
    make::folder_link(&world.canary(), &world.data("sandbox"));
    assert_eq!(world.delete(true), (DELETED, String::new()));
    world.assert_both_gone();
    world.assert_canary_whole();
    assert!(exists(&world.at("outside/canary/set-aside/kept.txt")));
}

#[cfg(not(windows))]
#[test]
fn what_cannot_be_deleted_is_reported_and_the_rest_still_goes() {
    let world = World::new("dad-partial");
    let locked = world.data_dir("agents/claude/.claude");
    world.data_file("agents/claude/.claude/credentials-copy");
    world.data_file("store.sqlite");
    world.data_dir("logs");
    world.data_file("logs/host.log");
    make::set_mode(&locked, 0o555);
    if make::can_write_in(&locked) {
        make::set_mode(&locked, 0o755);
        eprintln!("skipped: running as root, a mode cannot refuse the deletion");
        return;
    }
    let (code, stderr) = world.delete(false);
    make::set_mode(&locked, 0o755);
    assert_eq!(code, FAILED);
    // `agents` went to std's `remove_dir_all`, which stopped there: reported, not walked.
    assert!(stderr.contains("agents: could not delete it"), "{stderr}");
    assert!(exists(
        &world.data("agents/claude/.claude/credentials-copy")
    ));
    assert!(!exists(&world.data("store.sqlite")));
    assert!(!exists(&world.data("logs")));
    assert!(!exists(&world.at(&format!("Roaming/{DATA}"))));
}

#[cfg(not(windows))]
#[test]
fn a_kept_folder_is_the_answer_even_when_something_else_failed() {
    // The uninstaller then names the kept folder, never the whole one with its records.
    let world = World::new("dad-partial-kept");
    let locked = world.data_dir("agents/claude/.claude");
    world.data_file("agents/claude/.claude/credentials-copy");
    world.data_dir("sandbox");
    world.data_file("sandbox/acl-ledger.json");
    make::set_mode(&locked, 0o555);
    if make::can_write_in(&locked) {
        make::set_mode(&locked, 0o755);
        eprintln!("skipped: running as root, a mode cannot refuse the deletion");
        return;
    }
    let (code, stderr) = world.delete(false);
    make::set_mode(&locked, 0o755);
    assert_eq!(code, KEPT_SANDBOX);
    assert!(stderr.contains("could not delete it"), "{stderr}");
    assert!(exists(&world.data("sandbox/acl-ledger.json")));
}

#[test]
fn a_folder_is_pinned_only_when_it_is_a_real_folder() {
    let world = World::new("dad-pin");
    let real = world.data_dir("real");
    let link = world.data("link");
    make::folder_link(&world.canary(), &link);
    let file = world.data_file("file");
    assert!(ops::pin_folder(&real).is_ok());
    let refused = ops::pin_folder(&link).err().unwrap();
    assert!(refused.to_string().contains("reparse point"), "{refused}");
    assert!(ops::pin_folder(&file).is_err());
}

#[test]
fn the_folder_names_are_the_sandboxs() {
    assert_eq!(walk::SANDBOX_DIR, "sandbox");
    // TODO(#465): compare with clauseo_sandbox::windows::SET_ASIDE_DIR once it is exported.
    assert_eq!(walk::SET_ASIDE_DIR, "set-aside");
}
