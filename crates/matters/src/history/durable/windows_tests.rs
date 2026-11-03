//! Windows: the record of new `objects/xx` folders that decides whether a publish syncs
//! `objects`, a torn object that Windows will not let the store remove, and a timeline folder
//! that cannot be made. Windows gives `NotFound`, not `NotADirectory`, for a path through a
//! file, so the Unix tests of these errors (`history/tests/io_errors.rs`) do not run here.

use std::io::{self, Write as _};
use std::path::Path;

use gix::objs::Kind;

use super::{NewObjects, move_ref, put};
use crate::Error;
use crate::history::blob::{Blobs, Target};
use crate::history::objects::Objects;
use crate::history::{Oid, TIMELINE_REF};
use crate::no_download::NoDownload;
use crate::test_dir::TestDir;
use crate::{OsProbe, PlaceholderProbe, write};

/// A guard for a test's reads, with the OS probe.
fn guard() -> NoDownload<'static> {
    const PROBE: &dyn PlaceholderProbe = &OsProbe;
    NoDownload::begin(PROBE)
}

/// An empty loose object store at `<dir>/objects`.
fn objects(dir: &TestDir) -> Objects {
    let path = dir.path().join("objects");
    write::create_dir_all(&path).unwrap();
    Objects::at(&path)
}

fn write_file(path: &Path, bytes: &[u8]) {
    write::create_new(path).unwrap().write_all(bytes).unwrap();
}

/// The id git gives a blob of `data`.
fn blob_id(data: &[u8]) -> gix::ObjectId {
    gix::objs::compute_hash(gix::hash::Kind::Sha1, Kind::Blob, data).unwrap()
}

/// Makes the `objects/xx` folder that object `id`'s file goes in.
fn make_folder(objects: &Objects, id: &gix::ObjectId) {
    let path = objects.deflated().object_path(id);
    write::create_dir(path.parent().unwrap()).unwrap();
}

/// An object written whole into a new `objects/xx` folder is recorded with the folder new, so
/// the publish syncs `objects`; one written into a folder that exists is not.
#[test]
fn an_object_put_in_a_new_folder_records_the_folder_as_new() {
    for (case, folder_exists) in [("new", false), ("existing", true)] {
        let dir = TestDir::new(&format!("durable-win-put-{case}"));
        let objects = objects(&dir);
        let data = b"the signed lease\n";
        let id = blob_id(data);
        if folder_exists {
            make_folder(&objects, &id);
        }
        let mut new_objects = NewObjects::default();

        put(objects.deflated(), &mut new_objects, Kind::Blob, data, id).unwrap();

        assert_eq!(new_objects.made_folder, !folder_exists, "{case}");
        assert_eq!(
            new_objects.files(),
            [objects.deflated().object_path(&id)],
            "{case}"
        );
    }
}

/// The same for a streamed file: its object in a new folder is recorded with the folder new.
#[test]
fn a_streamed_object_in_a_new_folder_records_the_folder_as_new() {
    for (case, folder_exists) in [("new", false), ("existing", true)] {
        let dir = TestDir::new(&format!("durable-win-stream-{case}"));
        let objects = objects(&dir);
        let content: Vec<u8> = (0..5000_u32).map(|i| (i % 251) as u8).collect();
        let id = blob_id(&content);
        if folder_exists {
            make_folder(&objects, &id);
        }
        let path = dir.path().join("big.bin");
        write_file(&path, &content);
        let mut blobs = Blobs::new();
        blobs.set_stream_threshold(1024);
        let mut new_objects = NewObjects::default();
        let target = Target {
            stored: objects.stored(),
            deflated: objects.deflated(),
            new_objects: &mut new_objects,
        };

        blobs
            .read(&path, 5000, u64::MAX, Some(target), &guard())
            .unwrap();

        assert_eq!(new_objects.made_folder, !folder_exists, "{case}");
        assert_eq!(
            new_objects.files(),
            [objects.deflated().object_path(&id)],
            "{case}"
        );
    }
}

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
