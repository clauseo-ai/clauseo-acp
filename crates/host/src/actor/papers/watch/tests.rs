use std::path::PathBuf;

use notify::event::{AccessKind, AccessMode, CreateKind, EventKind, MetadataKind, ModifyKind};

use super::shows;

/// Whether an event of `kind` on `paths` counts for the folder at `/m`.
fn seen(kind: EventKind, paths: &[&str]) -> bool {
    let mut event = notify::Event::new(kind);
    for path in paths {
        event = event.add_path(PathBuf::from(path));
    }
    shows(&roots(), &Ok(event))
}

fn roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/m"), PathBuf::from("/private/m")]
}

const MADE: EventKind = EventKind::Create(CreateKind::File);

#[test]
fn a_paper_made_or_renamed_counts_under_either_spelling_of_the_folder() {
    assert!(seen(MADE, &["/m/brief.docx"]));
    assert!(seen(MADE, &["/private/m/pleadings/claim.pdf"]));
    let renamed = EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Both));
    assert!(seen(renamed, &["/m/~$a.docx", "/m/a.docx"]));
}

#[test]
fn the_folder_itself_and_a_lost_track_count() {
    let gone = EventKind::Remove(notify::event::RemoveKind::Folder);
    assert!(seen(gone, &["/m"]));
    assert!(seen(EventKind::Any, &[]));
    assert!(shows(&roots(), &Err(notify::Error::generic("overflow"))));
    // A path outside both spellings is not one the filter can judge: it counts.
    assert!(seen(MADE, &["/elsewhere/a.txt"]));
}

#[test]
fn files_the_listing_never_shows_and_reads_do_not_count() {
    for path in [
        "/m/~$brief.docx",
        "/m/a/draft.tmp",
        "/m/.clauseo/matter.json",
        "/m/.git/HEAD",
    ] {
        assert!(!seen(MADE, &[path]), "{path}");
    }
    let opened = EventKind::Access(AccessKind::Open(AccessMode::Read));
    assert!(!seen(opened, &["/m/brief.docx"]));
    let closed = EventKind::Access(AccessKind::Close(AccessMode::Write));
    assert!(!seen(closed, &["/m/brief.docx"]));
    let atime = EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime));
    assert!(!seen(atime, &["/m/brief.docx"]));
}
