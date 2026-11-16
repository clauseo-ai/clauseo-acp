//! A launch readies the conversation's work folder and the matter's notes, which outlive it
//! (`folders::clear_work`, `folders::ready_own`): what an agent put in the work folder's place is
//! removed, never followed, and the folder made anew; a re-used folder is set to 0700; and a
//! `.claude` in either is removed, through no link. The agent's own files stay.

use super::{RunSweep, folders, names, scratch};
use crate::write;

/// A file an agent put in its work folder's place: the next launch removes it and makes the
/// folder, where it would otherwise fail at every launch of the conversation.
#[test]
fn a_file_in_the_work_folders_place_is_removed_and_the_folder_made_anew() {
    let data = scratch("work-file");
    let first = folders(&data, 1);
    first.make(&data, &RunSweep::default()).unwrap();
    first.remove();
    write::remove_dir_all(&first.work).unwrap();
    write::write_file(&first.work, b"not a folder").unwrap();
    let second = folders(&data, 2);
    assert_eq!(second.work, first.work);
    second.make(&data, &RunSweep::default()).unwrap();
    assert!(write::is_real_dir(&second.work));
    assert!(names(&second.work).is_empty(), "{:?}", names(&second.work));
    #[cfg(unix)]
    assert_eq!(mode(&second.work), 0o700);
    write::remove_dir_all(&data).unwrap();
}

/// A link an agent put in its work folder's place: the next launch removes the link itself,
/// never what it points to, and makes the folder.
#[cfg(unix)]
#[test]
fn a_link_in_the_work_folders_place_is_removed_and_the_folder_made_anew() {
    let data = scratch("work-link");
    let outside = scratch("work-link-outside");
    write::write_file(&outside.join("keep.md"), b"keep").unwrap();
    let first = folders(&data, 1);
    first.make(&data, &RunSweep::default()).unwrap();
    first.remove();
    write::remove_dir_all(&first.work).unwrap();
    write::symlink(&outside, &first.work).unwrap();
    let second = folders(&data, 2);
    second.make(&data, &RunSweep::default()).unwrap();
    assert!(write::is_real_dir(&second.work), "a folder, not the link");
    assert!(names(&second.work).is_empty(), "{:?}", names(&second.work));
    assert_eq!(mode(&second.work), 0o700);
    assert_eq!(
        names(&outside),
        ["keep.md"],
        "the link's target is left as it was"
    );
    for folder in [&data, &outside] {
        write::remove_dir_all(folder).unwrap();
    }
}

/// A work folder an earlier launch made, its mode opened by the agent, is set back to 0700 at
/// the next launch, the agent's draft kept.
#[cfg(unix)]
#[test]
fn a_reused_work_folder_is_set_back_to_private() {
    let data = scratch("work-mode");
    let first = folders(&data, 1);
    first.make(&data, &RunSweep::default()).unwrap();
    write::write_file(&first.work.join("draft.md"), b"draft").unwrap();
    first.remove();
    write::set_mode(&first.work, 0o755).unwrap();
    let second = folders(&data, 2);
    second.make(&data, &RunSweep::default()).unwrap();
    assert_eq!(mode(&second.work), 0o700);
    assert_eq!(names(&second.work), ["draft.md"]);
    write::remove_dir_all(&data).unwrap();
}

/// A `.claude` in the work folder or the matter's notes is gone at the next launch: a folder
/// with all it holds, a link itself, never its target. The agent's other files stay.
#[test]
fn a_claude_folder_in_the_work_and_notes_folders_is_removed_at_launch() {
    let data = scratch("work-claude");
    let outside = scratch("work-claude-outside");
    write::write_file(&outside.join("settings.json"), b"{}").unwrap();
    let first = folders(&data, 1);
    first.make(&data, &RunSweep::default()).unwrap();
    first.remove();
    let settings = first.work.join(".claude").join("settings.json");
    write::create_dir_all(settings.parent().unwrap()).unwrap();
    write::write_file(&settings, b"{}").unwrap();
    write::write_file(&first.work.join("draft.md"), b"draft").unwrap();
    write::write_file(&first.notes.join("notes.md"), b"notes").unwrap();
    #[cfg(unix)]
    write::symlink(&outside, &first.notes.join(".claude")).unwrap();
    #[cfg(not(unix))]
    write::write_file(&first.notes.join(".claude"), b"a file").unwrap();
    let second = folders(&data, 2);
    second.make(&data, &RunSweep::default()).unwrap();
    assert_eq!(names(&second.work), ["draft.md"]);
    assert_eq!(names(&second.notes), ["notes.md"]);
    assert_eq!(
        names(&outside),
        ["settings.json"],
        "the link's target is left"
    );
    for folder in [&data, &outside] {
        write::remove_dir_all(folder).unwrap();
    }
}

/// `path`'s permission bits.
#[cfg(unix)]
fn mode(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777
}
