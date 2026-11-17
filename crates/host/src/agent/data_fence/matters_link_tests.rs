//! The own matter under a linked parent (`~/Clients` a link to `/Volumes/Data/Clients`), another
//! matter beside it: no deny covers the own matter by either spelling, and the other matter is
//! denied. The same under macOS's data volume, a firmlink no walk sees (modelled: a link the walk
//! is told is a folder).

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
        // Exactly the spelling it has, and where the model's link leads when that differs
        // (`forms::canonical`; a real firmlink it doesn't follow); never the data-volume twin
        // (`forms::twin`, for comparisons only). Registered on the data volume, the other matter
        // is in the rules by that spelling alone, not by its `Users` twin.
        let registered = parent.join("Other");
        let mut expected = vec![registered.clone()];
        let led = dunce::canonicalize(&registered).unwrap();
        if led != registered {
            expected.push(led);
        }
        let paths = &fence.others[0].paths;
        assert_eq!(paths, &expected, "{own:?}");
        if parent == &long {
            assert_eq!(paths, std::slice::from_ref(&registered), "{own:?}: no twin");
        }
        let rules = claude_deny_rules(
            &LaunchDeny {
                matters: fence,
                ..LaunchDeny::default()
            },
            false,
        );
        if parent == &long {
            let twin = short.join("Other");
            let twin = twin.to_string_lossy();
            assert!(
                rules.iter().all(|rule| !rule.contains(twin.as_ref())),
                "{own:?}: the twin {twin} in a rule: {rules:#?}"
            );
        }
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
