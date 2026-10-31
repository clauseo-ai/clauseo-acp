//! Undo puts back only the bytes that were checkpointed. When the loose object behind a
//! checkpointed file has been replaced by a well-formed object of other bytes (a disk fault, a
//! sync tool, a bad restore of the history folder), undo either fails or restores the original
//! bytes; it never writes the other bytes into the matter folder and reports the file restored.
//!
//! The same holds for restore and `read_file`, and for a replaced tree object, which would
//! otherwise send them to another file's bytes. The first two tests come from
//! clauseo-acp-private#134 (found by the `matters_objects` fuzz target in the quality sweep).

use std::path::PathBuf;

use clauseo_matters::{CompareSide, Error, Matter, RestoreFiles, Undone};
use clauseo_protocol::ErrorCode;

use crate::fixture::{
    Fixture, assert_no_temp_files, cid, meta, path, restore_request, tools, undo,
};
use crate::support;

/// What the lawyer's file held at the checkpoint undo goes back to.
const BRIEF: &[u8] = b"Heads of terms: the Seller delivers the Goods on the Completion Date.\n";
/// What the turn changed it to.
const EDITED: &[u8] = b"Heads of terms, as the agent edited them.\n";
/// A second file the turn changed: `notes.txt` at checkpoint 1, then at checkpoint 3.
const NOTES: [&[u8]; 2] = [b"Call the buyer.\n", b"Called the buyer; no answer.\n"];
/// A file no turn touches.
const OTHER: &[u8] = b"Schedule 4: the Goods.\n";

/// Checkpoint 1 holds `brief.txt` as [`BRIEF`]; checkpoint 3, after a turn, as [`EDITED`].
fn stepped(name: &str) -> (Fixture, Matter) {
    let fx = Fixture::new(name);
    fx.write("brief.txt", BRIEF);
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("brief.txt", EDITED);
    matter.after_turn(&meta(3), &tools(&["brief.txt"])).unwrap();
    (fx, matter)
}

/// Like [`stepped`], with `notes.txt` (written after `brief.txt`) changed too, and `sub/brief.txt`
/// and `sub/other.txt` beside them, `sub/brief.txt` changed like `brief.txt`.
fn stepped_more(name: &str) -> (Fixture, Matter) {
    let fx = Fixture::new(name);
    fx.write("brief.txt", BRIEF);
    fx.write("notes.txt", NOTES[0]);
    fx.write("sub/brief.txt", BRIEF);
    fx.write("sub/other.txt", OTHER);
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("brief.txt", EDITED);
    fx.write("notes.txt", NOTES[1]);
    fx.write("sub/brief.txt", EDITED);
    let changed = ["brief.txt", "notes.txt", "sub/brief.txt"];
    matter.after_turn(&meta(3), &tools(&changed)).unwrap();
    (fx, matter)
}

/// The id of the loose object `kind` `bytes`.
fn object_id(kind: gix::objs::Kind, bytes: &[u8]) -> gix::ObjectId {
    gix::objs::compute_hash(gix::hash::Kind::Sha1, kind, bytes).unwrap()
}

/// The loose object file of `id` in the fixture's history.
fn id_path(fx: &Fixture, id: gix::ObjectId) -> PathBuf {
    let hex = id.to_hex().to_string();
    let (folder, file) = hex.split_at(2);
    fx.git_dir().join("objects").join(folder).join(file)
}

/// The loose object file of the blob `bytes` in the fixture's history.
fn object_path(fx: &Fixture, bytes: &[u8]) -> PathBuf {
    id_path(fx, object_id(gix::objs::Kind::Blob, bytes))
}

/// A well-formed loose object `kind` holding `bytes`, as git and gix write one.
fn loose_kind(fx: &Fixture, kind: gix::objs::Kind, bytes: &[u8]) -> Vec<u8> {
    use gix::objs::Write as _;
    let scratch = fx.dir.path().join("scratch-odb");
    support::create_dir_all(&scratch);
    let store = gix::odb::loose::Store::at(&scratch, gix::hash::Kind::Sha1);
    let id = store.write_buf(kind, bytes).unwrap();
    std::fs::read(store.object_path(&id)).unwrap()
}

/// A well-formed loose blob object holding `bytes`, as git and gix write one.
fn loose_object(fx: &Fixture, bytes: &[u8]) -> Vec<u8> {
    loose_kind(fx, gix::objs::Kind::Blob, bytes)
}

/// Replaces the object file `object` with `replacement`.
fn replace_file(object: &PathBuf, replacement: Vec<u8>) {
    assert!(object.is_file(), "no loose object at {}", object.display());
    // Loose objects are read-only; replace the file.
    support::remove_file(object);
    support::write(object, replacement);
}

/// Replaces the object of `brief.txt`'s checkpointed bytes with a well-formed object of `wrong`.
fn replace_object(fx: &Fixture, wrong: &[u8]) {
    replace_file(&object_path(fx, BRIEF), loose_object(fx, wrong));
}

/// The data of a tree holding the blobs `files` (name, bytes; in git's order).
fn tree_data(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut data = Vec::new();
    for (name, bytes) in files {
        data.extend_from_slice(b"100644 ");
        data.extend_from_slice(name.as_bytes());
        data.push(0);
        data.extend_from_slice(object_id(gix::objs::Kind::Blob, bytes).as_bytes());
    }
    data
}

/// Replaces checkpoint 1's tree of `sub` (`brief.txt` [`BRIEF`], `other.txt` [`OTHER`]) with a
/// well-formed tree whose `brief.txt` is [`OTHER`]'s blob, an object the history holds.
fn replace_sub_tree(fx: &Fixture) {
    let then = tree_data(&[("brief.txt", BRIEF), ("other.txt", OTHER)]);
    let wrong = tree_data(&[("brief.txt", OTHER), ("other.txt", OTHER)]);
    let object = id_path(fx, object_id(gix::objs::Kind::Tree, &then));
    replace_file(&object, loose_kind(fx, gix::objs::Kind::Tree, &wrong));
}

/// Undo either fails, leaving the edited bytes, or restores exactly the checkpointed bytes.
fn assert_never_wrong_bytes(fx: &Fixture, undone: Result<Undone, Error>) {
    let now = fx.read("brief.txt");
    match undone {
        Ok(undone) => {
            if undone.restored.contains(&path("brief.txt")) {
                assert!(
                    now == BRIEF,
                    "undo reported brief.txt restored, but it holds {} bytes {:?}, not the {} \
                     bytes checkpointed",
                    now.len(),
                    String::from_utf8_lossy(&now),
                    BRIEF.len()
                );
            }
        }
        Err(_) => assert_eq!(now, EDITED, "a failed undo changed brief.txt"),
    }
}

/// `err` is a history error that names the object `id` and, when given, the path `at`.
fn assert_names(err: &Error, id: gix::ObjectId, at: Option<&str>) {
    assert!(matches!(err, Error::History { .. }), "{err:?}");
    assert_eq!(err.code(), ErrorCode::MatterCheckpointFailed);
    let text = format!("{err}");
    let hex = id.to_hex().to_string();
    assert!(
        text.contains(&format!("{hex} does not match its content")),
        "{text}"
    );
    if let Some(at) = at {
        assert!(text.contains(at), "{text}");
    }
}

/// The folder holds checkpoint 3's bytes and no temp file.
fn assert_folder_untouched(fx: &Fixture) {
    assert_eq!(fx.read("brief.txt"), EDITED);
    assert_eq!(fx.read("notes.txt"), NOTES[1]);
    assert_eq!(fx.read("sub/brief.txt"), EDITED);
    assert_eq!(fx.read("sub/other.txt"), OTHER);
    assert_no_temp_files(&fx.folder);
}

/// An empty, well-formed object in place of the checkpointed one.
#[test]
fn undo_never_restores_an_empty_object_in_place_of_the_checkpointed_bytes() {
    let (fx, mut matter) = stepped("undo-corrupt-empty");
    replace_object(&fx, b"");
    let undone = undo(&mut matter, (1, 3), &["brief.txt"], (4, 5));
    assert_never_wrong_bytes(&fx, undone);
}

/// A well-formed object of the same size but other bytes in place of the checkpointed one.
#[test]
fn undo_never_restores_other_bytes_of_the_same_size() {
    let (fx, mut matter) = stepped("undo-corrupt-same-size");
    let mut wrong = BRIEF.to_vec();
    wrong.reverse();
    replace_object(&fx, &wrong);
    let undone = undo(&mut matter, (1, 3), &["brief.txt"], (4, 5));
    assert_never_wrong_bytes(&fx, undone);
}

/// Undo of three files, the middle one's object replaced: nothing changes, the error names the
/// object and a path, and no temp file is left.
#[test]
fn undo_with_one_corrupt_object_changes_no_file() {
    let (fx, mut matter) = stepped_more("undo-corrupt-many");
    let mut wrong = NOTES[0].to_vec();
    wrong.reverse();
    replace_file(&object_path(&fx, NOTES[0]), loose_object(&fx, &wrong));
    let changed = ["brief.txt", "notes.txt", "sub/brief.txt"];
    let err = undo(&mut matter, (1, 3), &changed, (4, 5)).unwrap_err();
    assert_names(
        &err,
        object_id(gix::objs::Kind::Blob, NOTES[0]),
        Some("notes.txt"),
    );
    assert_folder_untouched(&fx);
}

/// Restore of the whole matter with one object replaced: nothing changes, no temp file is left.
#[test]
fn restore_with_one_corrupt_object_changes_no_file() {
    let (fx, mut matter) = stepped_more("restore-corrupt");
    let mut wrong = NOTES[0].to_vec();
    wrong.reverse();
    replace_file(&object_path(&fx, NOTES[0]), loose_object(&fx, &wrong));
    let err = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap_err();
    assert_names(
        &err,
        object_id(gix::objs::Kind::Blob, NOTES[0]),
        Some("notes.txt"),
    );
    assert_folder_untouched(&fx);
}

/// `read_file` of a file whose object was replaced fails, naming the object and the path.
#[test]
fn read_file_of_a_corrupt_object_fails() {
    let (fx, matter) = stepped("read-corrupt");
    replace_object(&fx, b"");
    let mut out = Vec::new();
    let err = matter
        .read_file(cid(1), &path("brief.txt"), &mut out)
        .unwrap_err();
    assert_names(
        &err,
        object_id(gix::objs::Kind::Blob, BRIEF),
        Some("brief.txt"),
    );
    // Checkpoint 3's object is whole and still reads.
    let mut out = Vec::new();
    matter
        .read_file(cid(3), &path("brief.txt"), &mut out)
        .unwrap();
    assert_eq!(out, EDITED);
}

/// A replaced tree would send restore, `read_file` and compare to another file's blob: each
/// fails instead, and restore changes no file.
#[test]
fn a_corrupt_tree_is_never_followed() {
    let (fx, mut matter) = stepped_more("tree-corrupt");
    replace_sub_tree(&fx);
    let tree = object_id(
        gix::objs::Kind::Tree,
        &tree_data(&[("brief.txt", BRIEF), ("other.txt", OTHER)]),
    );

    let mut out = Vec::new();
    let err = matter
        .read_file(cid(1), &path("sub/brief.txt"), &mut out)
        .unwrap_err();
    assert_names(&err, tree, Some("sub"));
    assert!(out.is_empty(), "{out:?}");

    let err = matter
        .compare(
            CompareSide::Checkpoint(cid(1)),
            CompareSide::Checkpoint(cid(3)),
        )
        .unwrap_err();
    // The diff reads the subtree without knowing its folder: the error names the object.
    assert_names(&err, tree, None);

    let err = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap_err();
    assert_names(&err, tree, Some("sub"));
    assert_folder_untouched(&fx);
}
