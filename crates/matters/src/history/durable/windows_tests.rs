//! Windows: a torn object that Windows will not let the store remove, and a timeline folder
//! that cannot be made. Windows gives `NotFound`, not `NotADirectory`, for a path through a
//! file, so the Unix tests of these errors (`history/tests/io_errors.rs`) do not run here.

use std::io;

use gix::objs::Kind;

use super::tests::{blob_id, make_folder, objects, write_file};
use super::{NewObjects, move_ref, put};
use crate::Error;
use crate::history::{Oid, TIMELINE_REF};
use crate::test_dir::TestDir;
use crate::write;

/// A torn object that Windows will not remove (read-only, so `DeleteFileW` is refused) is an
/// error naming the remove. Windows skips gix's rename onto an existing file, so writing over it
/// without the remove would leave the torn file in place under a write that seemed to succeed.
#[test]
fn a_torn_object_that_cannot_be_removed_is_an_error() {
    let dir = TestDir::new("durable-win-torn-read-only");
    let objects = objects(&dir);
    let data = b"the signed lease\n";
    let id = blob_id(data);
    make_folder(&objects, &id);
    let object = objects.deflated().object_path(&id);
    write_file(&object, b"not zlib at all");
    let writable = std::fs::metadata(&object).unwrap().permissions();
    let mut read_only = writable.clone();
    read_only.set_readonly(true);
    write::set_permissions(&object, read_only).unwrap();

    let mut new_objects = NewObjects::default();
    let stored = put(objects.deflated(), &mut new_objects, Kind::Blob, data, id);
    write::set_permissions(&object, writable).unwrap();

    assert!(
        matches!(
            &stored,
            Err(Error::Io { op: "remove", path, source })
                if *path == object && source.kind() == io::ErrorKind::PermissionDenied
        ),
        "{stored:?}"
    );
    assert_eq!(std::fs::read(&object).unwrap(), b"not zlib at all");
}

/// A timeline folder that cannot be made for a reason other than its existence (`refs` is a
/// file) is an error naming that folder, not a folder taken to exist and written into.
#[test]
fn a_timeline_folder_that_cannot_be_made_is_an_error_naming_it() {
    let dir = TestDir::new("durable-win-ref-folder");
    let git_dir = dir.path().join("history.git");
    write::create_dir(&git_dir).unwrap();
    write_file(&git_dir.join("refs"), b"not a folder");

    let moved = move_ref(&git_dir, Oid([7; 20]), None);
    let ref_path = git_dir.join(TIMELINE_REF);
    let folder = ref_path.parent().unwrap();
    assert!(
        matches!(&moved, Err(Error::Io { op: "create", path, .. }) if path == folder),
        "{moved:?}"
    );
}
