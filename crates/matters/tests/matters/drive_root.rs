//! Linux: a mount point is a drive root, which cannot be a matter.

#![cfg(target_os = "linux")]

use std::path::Path;
use std::sync::atomic::AtomicBool;

use clauseo_matters::{Error, Matter, NewMatter, Refusal};

use crate::fixture::{Fixture, matter_id};

/// `/proc`, a mount point on every Linux system, is refused as a drive root before anything is
/// written.
#[test]
fn a_mount_point_is_refused_as_a_drive_root() {
    let fx = Fixture::new("drive-root-mount");
    let new = NewMatter {
        id: matter_id(1),
        folder: Path::new("/proc").to_owned(),
        ..fx.new_matter()
    };
    let cancel = AtomicBool::new(false);
    let err = Matter::import(fx.settings(), new, &[], &mut |_| {}, &cancel).unwrap_err();
    assert!(
        matches!(
            err,
            Error::FolderRefused {
                reason: Refusal::DriveRoot,
                ..
            }
        ),
        "{err:?}"
    );
    assert!(!fx.root().exists());
}
