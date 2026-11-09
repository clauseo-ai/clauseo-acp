use super::{Held, apfs_key, as_listed_by, stored_spelling};
use crate::write;

/// A held file found by a path the agent spelled in another case or Unicode form is shown as the
/// matter lists it, on APFS's rules, run on every system: the exact path first, the folded one
/// next, the agent's spelling when none is listed; with no folding (ext4) nothing is mapped.
#[test]
fn a_held_file_found_in_another_spelling_is_shown_as_listed() {
    let path = |text: &str| {
        clauseo_protocol::MatterPath::from_relative(std::path::Path::new(text)).unwrap()
    };
    let held = [
        Held::new(path("Drafts/Contract.docx")),
        Held::new(path("P\u{e9}tition.docx")),
        Held::new(path("CONTRACT.DOCX")),
    ];
    for (typed, shown) in [
        ("drafts/CONTRACT.docx", "Drafts/Contract.docx"),
        ("Pe\u{301}tition.docx", "P\u{e9}tition.docx"),
        ("PE\u{301}TITION.DOCX", "P\u{e9}tition.docx"),
        ("CONTRACT.DOCX", "CONTRACT.DOCX"),
        ("Free.docx", "Free.docx"),
    ] {
        assert_eq!(
            as_listed_by(path(typed), &held, apfs_key),
            path(shown),
            "{typed}"
        );
    }
    assert_eq!(
        as_listed_by(path("Pe\u{301}tition.docx"), &held, str::to_owned),
        path("Pe\u{301}tition.docx")
    );
}

/// A held file's stored spelling is read from the folders on its way, on APFS's rules, run on
/// every system: each name as listed, the exact one first; `None` for a name no folder lists.
#[test]
fn a_held_file_found_by_a_tool_is_shown_as_its_folders_list_it() {
    let path = |text: &str| {
        clauseo_protocol::MatterPath::from_relative(std::path::Path::new(text)).unwrap()
    };
    let root = std::env::temp_dir().join(format!("clauseo-host-stored-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root.join("Drafts")).unwrap();
    for name in ["Drafts/Contract.docx", "P\u{e9}tition.docx", "Free.docx"] {
        write::write_file(&root.join(name), b"x").unwrap();
    }
    for (typed, shown) in [
        ("drafts/CONTRACT.docx", Some("Drafts/Contract.docx")),
        ("Pe\u{301}tition.docx", Some("P\u{e9}tition.docx")),
        ("FREE.DOCX", Some("Free.docx")),
        ("Free.docx", Some("Free.docx")),
        ("Missing.docx", None),
    ] {
        assert_eq!(
            stored_spelling(&root, &path(typed), apfs_key),
            shown.map(path),
            "{typed}"
        );
    }
    assert_eq!(
        stored_spelling(&root, &path("FREE.DOCX"), str::to_owned),
        None
    );
    drop(write::remove_dir_all(&root));
}
