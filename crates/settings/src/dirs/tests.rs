use super::{LINUX_SYSTEM, MACOS_SYSTEM, refused_folders, refused_for};
use crate::locations::{KnownFolder, Os};
use crate::testing::FakeDirs;
use std::path::PathBuf;

fn with_home() -> FakeDirs {
    FakeDirs {
        home: Some(PathBuf::from("/Users/ada")),
        ..FakeDirs::default()
    }
}

fn paths(list: &[&str]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

#[test]
fn refused_folders_include_home() {
    for os in [Os::Windows, Os::Mac, Os::Linux] {
        let refused = refused_for(os, &with_home());
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
        let refused = refused_for(os, &with_home());
        let dot = paths(&[
            "/Users/ada/.config",
            "/Users/ada/.local",
            "/Users/ada/.cache",
            "/Users/ada/.ssh",
            "/Users/ada/.gnupg",
            "/Users/ada/.aws",
        ]);
        assert_eq!(refused.get(1..7), Some(dot.as_slice()), "{os:?}");
    }
}

#[test]
fn macos_refuses_library_and_the_system_folders() {
    let refused = refused_for(Os::Mac, &with_home());
    let mut expected = vec![PathBuf::from("/Users/ada/Library")];
    expected.extend(paths(&MACOS_SYSTEM));
    assert_eq!(refused.get(7..), Some(expected.as_slice()));
    assert!(refused.contains(&PathBuf::from("/System")));
    assert!(refused.contains(&PathBuf::from("/Volumes")));
}

#[test]
fn linux_refuses_the_system_folders() {
    let refused = refused_for(Os::Linux, &with_home());
    assert_eq!(refused.get(7..), Some(paths(&LINUX_SYSTEM).as_slice()));
    assert!(refused.contains(&PathBuf::from("/proc")));
}

#[test]
fn windows_refuses_its_known_folders_default_and_the_drive_root_folders() {
    let dirs = FakeDirs {
        home: Some(PathBuf::from("/C/Users/ada")),
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
    let refused = refused_for(Os::Windows, &dirs);
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
    assert_eq!(refused.get(7..), Some(expected.as_slice()));
}

#[test]
fn windows_leaves_out_the_folders_the_machine_lacks() {
    // No Arm folder, no Windows folder (so no drive root), no home (so no Default).
    let dirs = FakeDirs {
        known: vec![(KnownFolder::ProgramFiles, PathBuf::from("/C/Program Files"))],
        ..FakeDirs::default()
    };
    assert_eq!(
        refused_for(Os::Windows, &dirs),
        paths(&["/C/Program Files"])
    );
}

#[test]
fn without_a_home_only_the_system_folders_are_listed() {
    assert_eq!(
        refused_for(Os::Linux, &FakeDirs::default()),
        paths(&LINUX_SYSTEM)
    );
    assert_eq!(
        refused_for(Os::Mac, &FakeDirs::default()),
        paths(&MACOS_SYSTEM)
    );
}

#[test]
fn refused_folders_uses_this_os() {
    assert_eq!(
        refused_folders(&with_home()),
        refused_for(Os::CURRENT, &with_home())
    );
}
