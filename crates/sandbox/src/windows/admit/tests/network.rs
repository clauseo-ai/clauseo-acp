//! The gate classifies the data folder and the registered matters' folders before it touches
//! them: one on a network or unknown drive is refused (the data folder) or left out (a registered
//! matter), with no file call on it.

use super::*;
use crate::links::network::{Drive, Mounts, read_if_link};
use crate::windows::acl_plan::Sid;
use crate::windows::admit::neighbours::Net;
use crate::windows::fence::{MatterFolders, RegisteredMatter};
use crate::windows::test_world::OTHER_POOL_SIDS;

/// Whether `path` is one of this test's network or unknown-drive paths.
fn off_disk(path: &Path) -> bool {
    let text = path.to_string_lossy();
    ["\\\\", "Z:", "V:"]
        .iter()
        .any(|start| text.starts_with(start))
}

/// A mount table with `Z:` a network drive and `V:` an unknown device, and a link read that
/// fails the test on any of those paths and reads the file system elsewhere.
fn net() -> Net {
    Net {
        mounts: Mounts::new(Vec::new(), b"Z").with_drive(b'V', Drive::Unknown),
        read: Box::new(|path: &Path| {
            assert!(
                !off_disk(path),
                "a network path was looked at: {}",
                path.display()
            );
            read_if_link(path)
        }),
    }
}

/// A folder check that fails the test on a network path and answers `Ok` elsewhere.
fn never_off(path: &Path) -> FolderSupport {
    assert!(
        !off_disk(path),
        "a network path was checked: {}",
        path.display()
    );
    FolderSupport::Ok
}

fn checks() -> Checks<
    impl FnMut(&Path) -> io::Result<FolderSupport>,
    impl FnMut(&Path) -> io::Result<FolderSupport>,
> {
    Checks {
        entries: |path: &Path| Ok(never_off(path)),
        label: |path: &Path| Ok(never_off(path)),
        net: net(),
    }
}

/// A data folder on a server path, a network drive or an unknown device refuses, as one that
/// doesn't resolve does, before anything opens it.
#[test]
fn a_data_folder_off_the_local_disks_refuses_untouched() {
    let w = World::new("admit-net-data");
    let policy = w.builder().build().unwrap();
    for data in [r"\\host\share\data", r"Z:\data", r"V:\data"] {
        let got = gate(
            &policy,
            Path::new(data),
            &Neighbours::default(),
            checks(),
            &FakeFolders::default(),
        );
        assert!(
            matches!(
                got,
                Err(Error::FolderUnsupported {
                    support: FolderRefusal::OverlapsDataFolder,
                    ..
                })
            ),
            "{data}: {got:?}"
        );
    }
}

/// A registered matter on a share, a network drive or an unknown device is left out of the
/// fences untouched, and keeps its spelling; the launch goes on.
#[test]
fn a_registered_matter_off_the_local_disks_is_left_out_untouched() {
    let w = World::new("admit-net-matter");
    let policy = w.builder().build().unwrap();
    for root in [r"\\host\share\m2", r"Z:\m2", r"V:\m2"] {
        let given = Neighbours {
            registered: vec![RegisteredMatter {
                id: matter_two(),
                folders: MatterFolders {
                    root: PathBuf::from(root),
                    state: PathBuf::from(root).join("state"),
                    notes: None,
                },
                pool_user: Sid::parse(OTHER_POOL_SIDS[0]).unwrap(),
                fenced: false,
            }],
            reach: Vec::new(),
        };
        let admitted = gate(
            &policy,
            &w.at("data"),
            &given,
            checks(),
            &FakeFolders::default(),
        )
        .unwrap();
        assert!(admitted.fences().is_empty(), "{root}");
        let kept = &admitted.neighbours().registered[0].folders.root;
        assert_eq!(kept, Path::new(root));
    }
}
