//! `read_version`: each refusal in its order, the deleted-file limit at both positions, renames,
//! paging, and that no call writes anything.

use serde_json::json;

use super::{Fixture, call, checkpoint, ok, path, read, refused, snapshot, v7};
use crate::versions::texts::Refusal;
use crate::versions::{DELETED_FILES_READABLE, MAX_TEXT_BYTES, PAGE_BYTES};

#[test]
fn a_version_of_another_matter_is_refused_as_a_made_up_one_is() {
    let mut fixture = Fixture::new("versions-read-other");
    fixture.put("brief.txt", b"ours");
    fixture.checkpoint(2);
    fixture.add_matter(v7(200), "other", 50);
    fixture.put_in("other", "brief.txt", b"theirs, private");
    let theirs = fixture.checkpoint_of(1, 51);
    let backend = fixture.backend();
    let other = read(&backend, theirs, "brief.txt");
    let made_up = read(&backend, checkpoint(999), "brief.txt");
    assert_eq!(refused(&other), Refusal::UnknownVersion.text());
    assert_eq!(other, made_up);
    let not_an_id = call(
        &backend,
        "read_version",
        json!({ "version": "HEAD~1", "path": "brief.txt" }),
    );
    assert_eq!(other, not_an_id);
    // The version check comes first: a bad path changes nothing about it.
    assert_eq!(read(&backend, theirs, "../other/brief.txt"), other);
}

#[test]
fn a_path_not_in_that_version_is_refused() {
    let mut fixture = Fixture::new("versions-read-absent");
    fixture.put("brief.txt", b"one");
    let v2 = fixture.checkpoint(2);
    fixture.put("later.txt", b"added after");
    fixture.checkpoint(3);
    let backend = fixture.backend();
    assert_eq!(
        refused(&read(&backend, v2, "later.txt")),
        Refusal::NotInVersion.text()
    );
    assert_eq!(
        refused(&read(&backend, v2, "Brief.txt")),
        Refusal::NotInVersion.text()
    );
}

#[test]
fn paths_into_the_history_store_or_out_of_the_folder_are_refused() {
    let mut fixture = Fixture::new("versions-read-store");
    fixture.put("brief.txt", b"one");
    let v2 = fixture.checkpoint(2);
    let backend = fixture.backend();
    let matter = fixture.session.matter;
    for asked in [
        format!("matters/{matter}/history.git/HEAD"),
        format!("../data/matters/{matter}/history.git/config"),
        "../../etc/hosts".to_owned(),
        "/etc/hosts".to_owned(),
        "C:/Windows/win.ini".to_owned(),
        ".clauseo/matter.json".to_owned(),
        String::new(),
    ] {
        assert_eq!(
            refused(&read(&backend, v2, &asked)),
            Refusal::NotInVersion.text(),
            "{asked}"
        );
    }
}

#[test]
fn old_agent_files_an_old_tree_holds_are_refused() {
    let mut fixture = Fixture::new("versions-read-dot");
    fixture.put("brief.txt", b"one");
    fixture.put(".claude/settings.json", b"{\"permissions\":{}}");
    fixture.put(".codex/config.toml", b"model = \"x\"");
    fixture.put(".agents/notes.md", b"agent notes");
    let old = [
        ".claude/settings.json",
        ".codex/config.toml",
        ".agents/notes.md",
    ];
    let v2 = fixture.checkpoint_with_old(2, &old);
    // The tree does hold them: `read_file`, which checks no exclusion, finds them.
    {
        let matters = fixture.source.matters.lock().unwrap();
        for file in old {
            let mut bytes = Vec::new();
            matters[0].read_file(v2, &path(file), &mut bytes).unwrap();
            assert!(!bytes.is_empty(), "{file}");
        }
    }
    let backend = fixture.backend_deleted(true);
    for file in old {
        assert_eq!(
            refused(&read(&backend, v2, file)),
            Refusal::NotInVersion.text(),
            "{file}"
        );
    }
    assert_eq!(ok(&read(&backend, v2, "brief.txt"))["text"], "one");
}

#[test]
fn pdfs_and_other_files_that_are_not_text_are_refused() {
    let mut fixture = Fixture::new("versions-read-pdf");
    fixture.put("scan.pdf", b"%PDF-1.4 looks like text");
    fixture.put("Scan2.PDF", b"%PDF-1.7");
    fixture.put("photo.png", b"\x89PNG\r\n\x1a\n\0\0\0");
    fixture.put("latin1.txt", b"r\xe9sum\xe9 in Latin-1");
    fixture.put("nul.txt", b"text\0more");
    let v2 = fixture.checkpoint(2);
    let backend = fixture.backend();
    for file in [
        "scan.pdf",
        "Scan2.PDF",
        "photo.png",
        "latin1.txt",
        "nul.txt",
    ] {
        assert_eq!(
            refused(&read(&backend, v2, file)),
            Refusal::Unsupported.text(),
            "{file}"
        );
    }
}

/// A file read at version 2 and deleted at version 3.
fn deleted_fixture(name: &str) -> (Fixture, clauseo_protocol::CheckpointId) {
    let mut fixture = Fixture::new(name);
    fixture.put("gone.txt", b"what the lawyer deleted");
    fixture.put("kept.txt", b"kept");
    let v2 = fixture.checkpoint(2);
    fixture.remove("gone.txt");
    fixture.checkpoint(3);
    (fixture, v2)
}

#[test]
fn a_deleted_file_is_refused_while_deleted_files_are_not_readable() {
    let (fixture, v2) = deleted_fixture("versions-read-deleted-off");
    let backend = fixture.backend_deleted(false);
    assert_eq!(
        refused(&read(&backend, v2, "gone.txt")),
        Refusal::Deleted.text()
    );
    assert_eq!(ok(&read(&backend, v2, "kept.txt"))["text"], "kept");
    // Deleted from the folder now, before any checkpoint records it: refused too.
    fixture.remove("kept.txt");
    assert_eq!(
        refused(&read(&backend, v2, "kept.txt")),
        Refusal::Deleted.text()
    );
}

#[test]
fn a_deleted_file_is_read_while_deleted_files_are_readable() {
    let (fixture, v2) = deleted_fixture("versions-read-deleted-on");
    let backend = fixture.backend_deleted(true);
    assert_eq!(
        ok(&read(&backend, v2, "gone.txt"))["text"],
        "what the lawyer deleted"
    );
}

#[test]
fn the_constant_keeps_deleted_files_unreadable() {
    const { assert!(!DELETED_FILES_READABLE) };
    let (fixture, v2) = deleted_fixture("versions-read-deleted-const");
    assert_eq!(
        refused(&read(&fixture.backend(), v2, "gone.txt")),
        Refusal::Deleted.text()
    );
}

#[cfg(unix)]
#[test]
fn a_file_put_back_as_a_link_is_not_in_the_matter() {
    let (fixture, v2) = deleted_fixture("versions-read-deleted-link");
    let outside = fixture.dir.path().join("outside.txt");
    crate::write::write_file(&outside, b"elsewhere").unwrap();
    crate::write::symlink(&outside, &fixture.folder().join("gone.txt")).unwrap();
    assert_eq!(
        refused(&read(&fixture.backend(), v2, "gone.txt")),
        Refusal::Deleted.text()
    );
}

#[test]
fn a_file_put_back_under_another_case_is_not_in_the_matter() {
    let (fixture, v2) = deleted_fixture("versions-read-deleted-case");
    fixture.put("GONE.txt", b"another file");
    assert_eq!(
        refused(&read(&fixture.backend(), v2, "gone.txt")),
        Refusal::Deleted.text()
    );
}

#[test]
fn a_rename_the_history_records_is_followed() {
    let mut fixture = Fixture::new("versions-read-renamed");
    fixture.put("draft.txt", b"the first draft");
    let v2 = fixture.checkpoint(2);
    fixture.put("draft.txt", b"the second draft, longer");
    fixture.checkpoint(3);
    fixture.rename("draft.txt", "sub/final.txt");
    fixture.checkpoint(4);
    let backend = fixture.backend();
    // Version 3's file went to `sub/final.txt` unchanged: a recorded rename.
    let v3 = checkpoint(3);
    assert_eq!(
        ok(&read(&backend, v3, "draft.txt"))["text"],
        "the second draft, longer"
    );
    // Version 2's file is the same file, edited at 3 and then renamed: it reads too.
    assert_eq!(
        ok(&read(&backend, v2, "draft.txt"))["text"],
        "the first draft"
    );
    // Once the renamed file is gone too, the old name is refused.
    fixture.remove("sub/final.txt");
    assert_eq!(
        refused(&read(&backend, v3, "draft.txt")),
        Refusal::Deleted.text()
    );
}

#[test]
fn a_rename_with_an_edit_is_refused() {
    let mut fixture = Fixture::new("versions-read-renamed-edited");
    fixture.put("draft.txt", b"the first draft");
    let v2 = fixture.checkpoint(2);
    fixture.rename("draft.txt", "final.txt");
    fixture.put("final.txt", b"the first draft, signed");
    fixture.checkpoint(3);
    let backend = fixture.backend();
    assert_eq!(
        refused(&read(&backend, v2, "draft.txt")),
        Refusal::Deleted.text()
    );
}

#[test]
fn an_old_version_of_a_file_still_here_is_read_with_text_since_removed() {
    let mut fixture = Fixture::new("versions-read-removed-text");
    fixture.put("contract.txt", b"Clause 1. Clause 2: the secret fee.");
    let v2 = fixture.checkpoint(2);
    fixture.put("contract.txt", b"Clause 1.");
    fixture.checkpoint(3);
    let backend = fixture.backend();
    let page = read(&backend, v2, "contract.txt");
    let page = ok(&page);
    assert_eq!(page["text"], "Clause 1. Clause 2: the secret fee.");
    assert_eq!(page["format"], "text");
    assert_eq!(page["version"], v2.to_string());
    assert_eq!(page["path"], "contract.txt");
    assert_eq!(page["offset"], 0);
    assert_eq!(page["total_bytes"], 35);
    assert!(page.get("next_offset").is_none(), "{page}");
}

#[test]
fn a_long_text_file_pages_on_character_boundaries() {
    let mut fixture = Fixture::new("versions-read-pages");
    // Two-byte characters, so 64 KiB falls inside one when the page starts at 1.
    let text: String = "§".repeat(PAGE_BYTES) + "end";
    fixture.put("long.txt", text.as_bytes());
    let v2 = fixture.checkpoint(2);
    let backend = fixture.backend();
    let mut offset = 0_u64;
    let mut joined = String::new();
    loop {
        let page = call(
            &backend,
            "read_version",
            json!({ "version": v2.to_string(), "path": "long.txt", "offset": offset }),
        );
        let page = ok(&page).clone();
        assert_eq!(page["total_bytes"], text.len());
        let piece = page["text"].as_str().unwrap();
        assert!(piece.len() <= PAGE_BYTES);
        joined.push_str(piece);
        match page["next_offset"].as_u64() {
            Some(next) => offset = next,
            None => break,
        }
    }
    assert_eq!(joined, text);
    // An offset inside a character starts at the next one.
    let inside = call(
        &backend,
        "read_version",
        json!({ "version": v2.to_string(), "path": "long.txt", "offset": 1 }),
    );
    let inside = ok(&inside);
    assert_eq!(inside["offset"], 2);
    assert_eq!(inside["text"].as_str().unwrap().len(), PAGE_BYTES);
    let end = text.len();
    let at_end = call(
        &backend,
        "read_version",
        json!({ "version": v2.to_string(), "path": "long.txt", "offset": end }),
    );
    assert_eq!(ok(&at_end)["text"], "");
    let past = call(
        &backend,
        "read_version",
        json!({ "version": v2.to_string(), "path": "long.txt", "offset": end + 1 }),
    );
    assert_eq!(refused(&past), Refusal::PastEnd.text());
    for bad in [json!(-1), json!("0"), json!(1.5)] {
        let refusal = call(
            &backend,
            "read_version",
            json!({ "version": v2.to_string(), "path": "long.txt", "offset": bad }),
        );
        assert_eq!(refused(&refusal), Refusal::BadOffset.text());
    }
}

#[test]
fn missing_or_mistyped_arguments_are_refused() {
    let mut fixture = Fixture::new("versions-read-args");
    fixture.put("brief.txt", b"one");
    let v2 = fixture.checkpoint(2);
    let backend = fixture.backend();
    let no_version = call(&backend, "read_version", json!({ "path": "brief.txt" }));
    assert_eq!(refused(&no_version), Refusal::BadVersion.text());
    let no_path = call(
        &backend,
        "read_version",
        json!({ "version": v2.to_string(), "path": 3 }),
    );
    assert_eq!(refused(&no_path), Refusal::BadPath.text());
}

#[test]
fn neither_tool_writes_in_the_matter_or_its_history() {
    let mut fixture = Fixture::new("versions-read-writes");
    fixture.put("brief.txt", b"one");
    fixture.put("gone.txt", b"gone");
    let v2 = fixture.checkpoint(2);
    fixture.put("brief.txt", b"one, two");
    fixture.remove("gone.txt");
    fixture.checkpoint(3);
    let folder = fixture.folder();
    let history = fixture.data().join("matters");
    let before = (snapshot(&folder), snapshot(&history));
    let backend = fixture.backend();
    ok(&call(&backend, "list_versions", json!({ "limit": 50 })));
    ok(&read(&backend, v2, "brief.txt"));
    refused(&read(&backend, v2, "gone.txt"));
    refused(&read(&backend, v2, "nothing.txt"));
    refused(&read(&backend, checkpoint(999), "brief.txt"));
    assert_eq!((snapshot(&folder), snapshot(&history)), before);
}

#[test]
fn a_text_file_over_the_cap_is_refused_by_its_size_and_one_at_it_reads() {
    let mut fixture = Fixture::new("versions-read-large");
    let at_cap = usize::try_from(MAX_TEXT_BYTES).unwrap();
    fixture.put("at-cap.txt", "a".repeat(at_cap).as_bytes());
    fixture.put("over-cap.log", "b".repeat(at_cap + 1).as_bytes());
    let v2 = fixture.checkpoint(2);
    let backend = fixture.backend();
    assert_eq!(
        refused(&read(&backend, v2, "over-cap.log")),
        Refusal::TooLarge.text()
    );
    let page = read(&backend, v2, "at-cap.txt");
    assert_eq!(ok(&page)["total_bytes"], MAX_TEXT_BYTES);
}
