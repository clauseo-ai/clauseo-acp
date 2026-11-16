//! The own matter under a linked parent (`~/Clients` a link to `/Volumes/Data/Clients`), another
//! matter beside it: no deny covers the own matter by either spelling, and the other matter is
//! denied. The same under macOS's data volume, a firmlink no walk sees (modelled: a link the walk
//! is told is a folder); and names that differ only by their Unicode form are two folders where
//! the file system says so.

use std::io;
use std::path::{Path, PathBuf};

use super::{LaunchDeny, MatterFence, Other, claude_deny_rules, denied, id, layout, matters};
use crate::network_paths::NetEnv;
use crate::network_paths::resolve::{Disk, Entry, Fs, RealFs};
use crate::write;

/// The own matter `Clients/Own`, as the launch passes it (as stored, through the link, and
/// where it leads), and another matter `Clients/Other` registered through the link or where it
/// leads: the parent, the own matter and its file are in no rule in either spelling; the other
/// matter is denied `Read` and `Edit` both ways.
#[test]
fn a_linked_parent_of_the_own_matter_is_in_no_edit_rule() {
    let dir = layout("matters-linked-parent");
    let root = dir.path();
    let volume = root.join("Volumes/Data/Clients");
    for file in ["Own/a.txt", "Other/b.txt"] {
        write::create_dir_all(volume.join(file).parent().unwrap()).unwrap();
        write::write_file(&volume.join(file), b"x").unwrap();
    }
    let home = root.join("Clients");
    write::symlink(&volume, &home).unwrap();
    let real = dunce::canonicalize(&volume).unwrap();
    let (stored, led) = (home.join("Own"), real.join("Own"));
    for registered in [home.join("Other"), real.join("Other")] {
        let fence = matters(
            &[(registered.clone(), false)],
            &[stored.as_path(), led.as_path()],
            &[],
        );
        let deny = LaunchDeny {
            matters: fence,
            ..LaunchDeny::default()
        };
        let rules = claude_deny_rules(&deny, false);
        let ways = [home.clone(), volume.clone(), real.clone()];
        for parent in &ways {
            for path in [parent.clone(), parent.join("Own"), parent.join("Own/a.txt")] {
                for tool in ["Read", "Edit"] {
                    let refused = denied(&rules, tool, &path);
                    assert!(!refused, "{registered:?}: {tool} {path:?}: {rules:#?}");
                }
            }
            let other: PathBuf = parent.join("Other/b.txt");
            let canonical = dunce::canonicalize(&other).unwrap();
            for tool in ["Read", "Edit"] {
                let refused = denied(&rules, tool, &other) || denied(&rules, tool, &canonical);
                assert!(refused, "{registered:?}: {tool} {other:?}: {rules:#?}");
            }
        }
    }
}

/// The real disk, but the link `firm` reads as a folder, as a firmlink does to the walk.
struct Firmlink<'a> {
    firm: &'a Path,
}

impl Fs for Firmlink<'_> {
    fn entry(&self, path: &Path) -> io::Result<Entry> {
        if path == self.firm {
            return Ok(Entry::Dir);
        }
        RealFs.entry(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        RealFs.read_link(path)
    }

    fn identity(&self, path: &Path) -> io::Result<(u64, u128)> {
        RealFs.identity(path)
    }
}

/// The own matter `Users/a/Clients/Own` on the modelled data volume (`Users` is
/// `System/Volumes/Data/Users`), given in one spelling, another matter registered at the parent
/// in the other: the own matter is kept by its twin, so no rule covers it in either spelling; the
/// other matter is denied by the spelling it has (the twin is in no rule).
#[test]
fn a_firmlinked_own_matter_is_in_no_rule_and_the_other_is_denied() {
    let dir = layout("matters-firmlink");
    let root = dunce::canonicalize(dir.path()).unwrap();
    let data = root.join("System/Volumes/Data");
    for file in ["Users/a/Clients/Own/a.txt", "Users/a/Clients/Other/b.txt"] {
        write::create_dir_all(data.join(file).parent().unwrap()).unwrap();
        write::write_file(&data.join(file), b"x").unwrap();
    }
    let firm = root.join("Users");
    write::symlink(&data.join("Users"), &firm).unwrap();
    let (short, long) = (firm.join("a/Clients"), data.join("Users/a/Clients"));
    let net = NetEnv::current();
    let fs = Firmlink { firm: &firm };
    let disk = Disk {
        fs: &fs,
        real: &|_: &Path| None,
        net: &net,
    };
    for (own, parent) in [(&short, &long), (&long, &short)] {
        let other = Other::new(id(1), "M1".into(), parent.clone(), false);
        let own_folder = own.join("Own");
        let fence = MatterFence::asked(vec![other]).resolve_on(
            &[own_folder.as_path()],
            &[],
            &disk,
            Some((&root, &data)),
        );
        assert_eq!(fence.others[0].paths, [parent.join("Other")], "{own:?}");
        let rules = claude_deny_rules(
            &LaunchDeny {
                matters: fence,
                ..LaunchDeny::default()
            },
            false,
        );
        for base in [&short, &long] {
            for path in [base.join("Own"), base.join("Own/a.txt")] {
                for tool in ["Read", "Edit"] {
                    assert!(
                        !denied(&rules, tool, &path),
                        "{own:?}: {tool} {path:?}: {rules:#?}"
                    );
                }
            }
        }
        for tool in ["Read", "Edit"] {
            let path = parent.join("Other/b.txt");
            assert!(
                denied(&rules, tool, &path),
                "{own:?}: {tool} {path:?}: {rules:#?}"
            );
        }
    }
}

/// Beside the own matter `Zoë` (NFC), a folder `Zoë` (NFD) is another folder on ext4 and
/// NTFS (APFS takes them for one, so not there): a parent registered as a matter fences it, and
/// leaves the own matter out. The keep side folds as the file system does, never NFD.
#[cfg(not(target_os = "macos"))]
#[test]
fn a_folder_named_in_another_unicode_form_is_fenced_beside_the_own_matter() {
    let dir = layout("matters-unicode-forms");
    let root = dunce::canonicalize(dir.path()).unwrap().join("Clients");
    let (own, other) = (root.join("Zo\u{eb}"), root.join("Zoe\u{308}"));
    for file in [own.join("a.txt"), other.join("b.txt")] {
        write::create_dir_all(file.parent().unwrap()).unwrap();
        write::write_file(&file, b"x").unwrap();
    }
    let fence = matters(&[(root.clone(), false)], &[own.as_path()], &[]);
    let rules = claude_deny_rules(
        &LaunchDeny {
            matters: fence,
            ..LaunchDeny::default()
        },
        false,
    );
    for tool in ["Read", "Edit"] {
        assert!(
            denied(&rules, tool, &other.join("b.txt")),
            "{tool}: {rules:#?}"
        );
        assert!(
            !denied(&rules, tool, &own.join("a.txt")),
            "{tool}: {rules:#?}"
        );
    }
}
