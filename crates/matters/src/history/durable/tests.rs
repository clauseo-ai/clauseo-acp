//! The record of new `objects/xx` folders that decides whether a publish syncs `objects`, for
//! an object written whole and for a streamed one; on every OS. The Windows-only cases are in
//! `windows_tests.rs`.

use std::io::Write as _;
use std::path::Path;

use gix::objs::Kind;

use super::{NewObjects, put};
use crate::history::blob::{Blobs, Target};
use crate::history::objects::Objects;
use crate::no_download::NoDownload;
use crate::test_dir::TestDir;
use crate::{OsProbe, PlaceholderProbe, write};

/// A guard for a test's reads, with the OS probe.
pub(super) fn guard() -> NoDownload<'static> {
    const PROBE: &dyn PlaceholderProbe = &OsProbe;
    NoDownload::begin(PROBE)
}

/// An empty loose object store at `<dir>/objects`.
pub(super) fn objects(dir: &TestDir) -> Objects {
    let path = dir.path().join("objects");
    write::create_dir_all(&path).unwrap();
    Objects::at(&path)
}

pub(super) fn write_file(path: &Path, bytes: &[u8]) {
    write::create_new(path).unwrap().write_all(bytes).unwrap();
}

/// The id git gives a blob of `data`.
pub(super) fn blob_id(data: &[u8]) -> gix::ObjectId {
    gix::objs::compute_hash(gix::hash::Kind::Sha1, Kind::Blob, data).unwrap()
}

/// Makes the `objects/xx` folder that object `id`'s file goes in.
pub(super) fn make_folder(objects: &Objects, id: &gix::ObjectId) {
    let path = objects.deflated().object_path(id);
    write::create_dir(path.parent().unwrap()).unwrap();
}

/// An object written whole into a new `objects/xx` folder is recorded with the folder new, so
/// the publish syncs `objects`; one written into a folder that exists is not.
#[test]
fn an_object_put_in_a_new_folder_records_the_folder_as_new() {
    for (case, folder_exists) in [("new", false), ("existing", true)] {
        let dir = TestDir::new(&format!("durable-put-{case}"));
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
        let dir = TestDir::new(&format!("durable-stream-{case}"));
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
