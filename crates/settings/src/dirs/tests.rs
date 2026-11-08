use super::{LINUX_SYSTEM, MACOS_SYSTEM, Resolve, refused_folders, refused_for, refused_trees};
use crate::locations::{KnownFolder, Os};
use crate::testing::FakeDirs;
use std::path::{Path, PathBuf};

fn with_home() -> FakeDirs {
    FakeDirs {
        home: Some(PathBuf::from("/Users/ada")),
        ..FakeDirs::default()
    }
}

/// A resolver that takes every path as written.
const AS_WRITTEN: Resolve<'static> = &|path: &Path| Some(path.to_path_buf());

fn paths(list: &[&str]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

#[test]
fn refused_folders_include_home() {
    for os in [Os::Windows, Os::Mac, Os::Linux] {
        let refused = refused_for(os, &with_home(), AS_WRITTEN);
        assert_eq!(
            refused.first(),
            Some(&PathBuf::from("/Users/ada")),
            "{os:?}"
        );
    }
}

#[test]
fn refused_folders_include_home_dot_folders() {
    for os in [Os::Windows, Os::Mac, Os::Linux] {
        let refused = refused_for(os, &with_home(), AS_WRITTEN);
        let dot = paths(&[
            "/Users/ada/.config",
            "/Users/ada/.local",
            "/Users/ada/.cache",
            "/Users/ada/.ssh",
            "/Users/ada/.gnupg",
            "/Users/ada/.aws",
            "/Users/ada/.codex",
        ]);
        assert_eq!(refused.get(1..8), Some(dot.as_slice()), "{os:?}");
    }
}

/// R17 PR-E: the folder `CODEX_HOME` names is refused like `~/.codex`, on every OS.
#[test]
fn the_codex_home_is_refused_where_codex_home_moved_it() {
    for os in [Os::Windows, Os::Mac, Os::Linux] {
        let dirs = FakeDirs {
            codex_home: Some(PathBuf::from("/Volumes/Work/codex")),
            ..with_home()
        };
        let refused = refused_for(os, &dirs, AS_WRITTEN);
        assert_eq!(
            refused.get(8),
            Some(&PathBuf::from("/Volumes/Work/codex")),
            "{os:?}"
        );
        assert!(
            !refused_for(os, &with_home(), AS_WRITTEN)
                .contains(&PathBuf::from("/Volumes/Work/codex"))
        );
    }
}

/// R17 PR-E follow-up: a `CODEX_HOME` naming the home folder, a folder above it, or a root
/// counts as not set, so it can't make every matter refused; one inside home still counts.
#[test]
fn a_codex_home_over_the_home_folder_or_a_root_is_ignored() {
    let root = if cfg!(windows) { r"C:\" } else { "/" };
    for ignored in ["/Users/ada", "/Users/ada/", "/Users", root] {
        let dirs = FakeDirs {
            codex_home: Some(PathBuf::from(ignored)),
            ..with_home()
        };
        assert_eq!(
            refused_trees(&dirs, AS_WRITTEN),
            paths(&["/Users/ada/.codex"]),
            "{ignored}"
        );
        for os in [Os::Windows, Os::Mac, Os::Linux] {
            assert_eq!(
                refused_for(os, &dirs, AS_WRITTEN),
                refused_for(os, &with_home(), AS_WRITTEN),
                "{ignored}"
            );
        }
    }
    let inside = FakeDirs {
        codex_home: Some(PathBuf::from("/Users/ada/work/codex")),
        ..with_home()
    };
    assert_eq!(
        refused_trees(&inside, AS_WRITTEN),
        paths(&["/Users/ada/.codex", "/Users/ada/work/codex"])
    );
}

/// The guard compares what the resolver gives: a symlink to home, a `..` form of it, or a path
/// that doesn't resolve is ignored; another case of home is ignored only where folders ignore
/// case; one inside home, through a link or not, counts and is kept as written.
#[test]
fn a_codex_home_that_resolves_to_home_is_ignored() {
    let fake = |path: &Path| -> Option<PathBuf> {
        let real = match path.to_str()? {
            "/Users/link" | "/Users/ada/sub/.." => "/Users/ada",
            "/Users/link/.." | "/Users/ada/sub/../.." => "/Users",
            "/Users/ada/missing/.." => return None,
            "/Users/link/codex" => "/Users/ada/codex",
            other => other,
        };
        Some(PathBuf::from(real))
    };
    let with = |codex: &str| FakeDirs {
        codex_home: Some(PathBuf::from(codex)),
        ..with_home()
    };
    let home_only = paths(&["/Users/ada/.codex"]);
    for ignored in [
        "/Users/link",
        "/Users/link/..",
        "/Users/ada/sub/..",
        "/Users/ada/sub/../..",
        "/Users/ada/missing/..",
    ] {
        assert_eq!(refused_trees(&with(ignored), &fake), home_only, "{ignored}");
    }
    let upper = refused_trees(&with("/USERS/ADA"), &fake);
    if cfg!(any(windows, target_os = "macos")) {
        assert_eq!(upper, home_only);
    } else {
        assert_eq!(upper, paths(&["/Users/ada/.codex", "/USERS/ADA"]));
    }
    for kept in ["/Users/link/codex", "/Users/ada/work/codex"] {
        assert_eq!(
            refused_trees(&with(kept), &fake),
            paths(&["/Users/ada/.codex", kept])
        );
    }
}

/// R17 PR-E: `~/.codex` and the folder `CODEX_HOME` names are refused trees too: every folder
/// inside them is refused, not only the folder itself.
#[test]
fn the_codex_folders_are_refused_trees() {
    assert_eq!(
        refused_trees(&with_home(), AS_WRITTEN),
        paths(&["/Users/ada/.codex"])
    );
    let dirs = FakeDirs {
        codex_home: Some(PathBuf::from("/Volumes/Work/codex")),
        ..with_home()
    };
    assert_eq!(
        refused_trees(&dirs, AS_WRITTEN),
        paths(&["/Users/ada/.codex", "/Volumes/Work/codex"])
    );
    assert_eq!(
        refused_trees(&FakeDirs::default(), AS_WRITTEN),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn macos_refuses_library_and_the_system_folders() {
    let refused = refused_for(Os::Mac, &with_home(), AS_WRITTEN);
    let mut expected = vec![PathBuf::from("/Users/ada/Library")];
    expected.extend(paths(&MACOS_SYSTEM));
    assert_eq!(refused.get(8..), Some(expected.as_slice()));
    assert!(refused.contains(&PathBuf::from("/System")));
    assert!(refused.contains(&PathBuf::from("/Volumes")));
}

#[test]
fn linux_refuses_the_system_folders() {
    let refused = refused_for(Os::Linux, &with_home(), AS_WRITTEN);
    assert_eq!(refused.get(8..), Some(paths(&LINUX_SYSTEM).as_slice()));
    assert!(refused.contains(&PathBuf::from("/proc")));
}

#[test]
fn windows_refuses_its_known_folders_default_and_the_drive_root_folders() {
    let dirs = FakeDirs {
        home: Some(PathBuf::from("/C/Users/ada")),
        codex_home: None,
        known: vec![
            (
                KnownFolder::RoamingAppData,
                PathBuf::from("/C/Users/ada/AppData/Roaming"),
            ),
            (
                KnownFolder::LocalAppData,
                PathBuf::from("/C/Users/ada/AppData/Local"),
            ),
            (KnownFolder::Windows, PathBuf::from("/C/Windows")),
            (KnownFolder::ProgramFiles, PathBuf::from("/C/Program Files")),
            (
                KnownFolder::ProgramFilesX86,
                PathBuf::from("/C/Program Files (x86)"),
            ),
            (
                KnownFolder::ProgramFilesArm,
                PathBuf::from("/C/Program Files (Arm)"),
            ),
            (KnownFolder::ProgramData, PathBuf::from("/C/ProgramData")),
            (KnownFolder::Public, PathBuf::from("/C/Users/Public")),
        ],
    };
    let refused = refused_for(Os::Windows, &dirs, AS_WRITTEN);
    let expected = paths(&[
        "/C/Users/ada/AppData/Roaming",
        "/C/Users/ada/AppData/Local",
        "/C/Windows",
        "/C/Program Files",
        "/C/Program Files (x86)",
        "/C/Program Files (Arm)",
        "/C/ProgramData",
        "/C/Users/Public",
        "/C/Users/Default",
        "/$Recycle.Bin",
        "/System Volume Information",
    ]);
    assert_eq!(refused.get(8..), Some(expected.as_slice()));
}

#[test]
fn windows_leaves_out_the_folders_the_machine_lacks() {
    // No Arm folder, no Windows folder (so no drive root), no home (so no Default).
    let dirs = FakeDirs {
        known: vec![(KnownFolder::ProgramFiles, PathBuf::from("/C/Program Files"))],
        ..FakeDirs::default()
    };
    assert_eq!(
        refused_for(Os::Windows, &dirs, AS_WRITTEN),
        paths(&["/C/Program Files"])
    );
}

#[test]
fn without_a_home_only_the_system_folders_are_listed() {
    assert_eq!(
        refused_for(Os::Linux, &FakeDirs::default(), AS_WRITTEN),
        paths(&LINUX_SYSTEM)
    );
    assert_eq!(
        refused_for(Os::Mac, &FakeDirs::default(), AS_WRITTEN),
        paths(&MACOS_SYSTEM)
    );
}

#[test]
fn refused_folders_uses_this_os() {
    assert_eq!(
        refused_folders(&with_home(), AS_WRITTEN),
        refused_for(Os::CURRENT, &with_home(), AS_WRITTEN)
    );
}
