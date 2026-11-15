//! The copy where no move can be made: another drive, a file system without hard links, a
//! drive that cannot keep the mark (the fallback folder).

use std::cell::Cell;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{NONE, names, scratch};
use crate::browser_guard::downloads::saved::{FALLBACK_MARKER, fallback_note, note};
use crate::browser_guard::downloads::{How, Saved, Sweeper, deliver};
use crate::write;

/// A move that always crosses devices, as `linkat` and `SetFileInformationByHandle` fail when
/// the downloads folder is on another drive than staging.
fn crosses(_: &write::Folder, _: &Path, _: &std::ffi::OsStr) -> io::Result<()> {
    Err(io::ErrorKind::CrossesDevices.into())
}

/// A mark that never holds, as on exFAT or a share without streams.
fn unmarkable(_: &File, _: &File) -> io::Result<()> {
    Err(io::Error::other("this drive keeps no mark"))
}

/// A delivery that always copies.
fn copying() -> How {
    How {
        mover: crosses,
        ..How::default()
    }
}

/// A sweeper with no wait, recording what it tells the agent.
fn sweeper(staging: &Path, downloads: &Path, how: How) -> (Sweeper, Arc<Saved>) {
    let saved = Arc::new(Saved::default());
    let mut sweeper = Sweeper::new(staging.to_owned(), downloads.to_owned(), 0);
    sweeper.saved = Some(Arc::clone(&saved));
    sweeper.how = how;
    (sweeper, saved)
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// A download whose move crosses devices is copied: it lands under its name with its content,
/// the staging file goes, no temporary file stays, and the agent gets the same line as for a
/// move.
#[test]
fn a_move_across_devices_is_copied_and_told_as_saved() {
    let (dir, staging, downloads) = scratch("copy-crosses");
    write::write(&staging.join("report.pdf"), "the download").unwrap();
    let (mut sweeper, saved) = sweeper(&staging, &downloads, copying());
    sweeper.sweep();
    assert_eq!(names(&downloads), ["report.pdf"]);
    assert_eq!(read(downloads.join("report.pdf")), "the download");
    assert_eq!(names(&staging), NONE);
    assert_eq!(saved.take(), [note("report.pdf")]);
    write::remove_dir_all(&dir).unwrap();
}

/// A real second file system: `/dev/shm` (tmpfs) when it is not the temporary folder's. The
/// sweep's own move fails with `EXDEV` and the copy delivers the file.
#[cfg(target_os = "linux")]
#[test]
fn a_downloads_folder_on_another_file_system_gets_the_file() {
    use std::os::unix::fs::MetadataExt;
    let shm = Path::new("/dev/shm");
    let (dir, staging, _) = scratch("copy-tmpfs");
    let device = |path: &Path| std::fs::metadata(path).map(|meta| meta.dev()).ok();
    if device(shm).is_none() || device(shm) == device(&dir) {
        write::remove_dir_all(&dir).unwrap();
        return;
    }
    let other = shm.join(format!("clauseo-mcp-copy-{}", std::process::id()));
    if other.exists() {
        write::remove_dir_all(&other).unwrap();
    }
    let downloads = other.join("downloads");
    write::create_dir_all(&downloads).unwrap();
    write::write(&staging.join("brief.docx"), "brief").unwrap();
    let error = write::Folder::open(&downloads)
        .unwrap()
        .move_new(&staging.join("brief.docx"), "x".as_ref())
        .unwrap_err();
    assert!(write::needs_copy(&error), "{error}");
    let (mut sweeper, saved) = sweeper(&staging, &downloads, How::default());
    sweeper.sweep();
    assert_eq!(read(downloads.join("brief.docx")), "brief");
    assert_eq!(names(&downloads), ["brief.docx"]);
    assert_eq!(names(&staging), NONE);
    assert_eq!(saved.take(), [note("brief.docx")]);
    write::remove_dir_all(&other).unwrap();
    write::remove_dir_all(&dir).unwrap();
}

thread_local! {
    /// What the checking mark saw: the copy's size when it was marked.
    static MARKED_AT: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Records the copy's size when it is marked, and that the source is the download.
fn checking(source: &File, copy: &File) -> io::Result<()> {
    assert!(source.metadata()?.len() > 0);
    MARKED_AT.set(Some(copy.metadata()?.len()));
    Ok(())
}

/// The copy is marked while it is still empty: before a byte of the download is in it.
#[test]
fn the_copy_is_marked_before_any_content() {
    let (dir, staging, downloads) = scratch("copy-mark-first");
    write::write(&staging.join("a.pdf"), "content").unwrap();
    let how = How {
        mover: crosses,
        mark: checking,
    };
    let delivered = deliver(&staging.join("a.pdf"), &downloads, "a.pdf", how, |_| {}).unwrap();
    assert_eq!(MARKED_AT.get(), Some(0));
    assert_eq!(read(delivered.path), "content");
    write::remove_dir_all(&dir).unwrap();
}

/// A name taken by the time of the copy's rename is never replaced: the copy takes the next free
/// name, and so on.
#[test]
fn a_copy_never_replaces_a_file_that_appears_at_its_name() {
    let (dir, staging, downloads) = scratch("copy-clash");
    write::write(&staging.join("report.pdf"), "download").unwrap();
    let mut raced = 0;
    let delivered = deliver(
        &staging.join("report.pdf"),
        &downloads,
        "report.pdf",
        copying(),
        |target: &Path| {
            // The first look is the move's; then the copy's renames.
            if (1..3).contains(&raced) {
                write::write(target, "the agent's own").unwrap();
            }
            raced += 1;
        },
    )
    .unwrap();
    assert_eq!(delivered.path, downloads.join("report (3).pdf"));
    assert_eq!(read(downloads.join("report.pdf")), "the agent's own");
    assert_eq!(read(downloads.join("report (2).pdf")), "the agent's own");
    assert_eq!(read(downloads.join("report (3).pdf")), "download");
    assert_eq!(names(&staging), NONE);
    write::remove_dir_all(&dir).unwrap();
}

/// A link at the download's name is neither replaced nor followed: the copy takes the next name
/// and the link's target is untouched.
#[cfg(unix)]
#[test]
fn a_link_at_the_name_is_not_followed_by_the_copy() {
    let (dir, staging, downloads) = scratch("copy-link-name");
    let outside = dir.join("outside.txt");
    write::write(&outside, "outside").unwrap();
    write::symlink(&outside, &downloads.join("report.pdf")).unwrap();
    write::write(&staging.join("report.pdf"), "download").unwrap();
    let delivered = deliver(
        &staging.join("report.pdf"),
        &downloads,
        "report.pdf",
        copying(),
        |_| {},
    )
    .unwrap();
    assert_eq!(delivered.path, downloads.join("report (2).pdf"));
    assert_eq!(read(outside), "outside");
    write::remove_dir_all(&dir).unwrap();
}

/// A downloads folder that is a link gets no copy: the delivery is refused before anything is
/// written, and the file stays in staging.
#[cfg(unix)]
#[test]
fn a_downloads_folder_that_is_a_link_gets_no_copy() {
    let (dir, staging, downloads) = scratch("copy-link-folder");
    let elsewhere = dir.join("elsewhere");
    write::create_dir_all(&elsewhere).unwrap();
    write::remove_dir_all(&downloads).unwrap();
    write::symlink(&elsewhere, &downloads).unwrap();
    write::write(&staging.join("a.pdf"), "download").unwrap();
    let failed = deliver(
        &staging.join("a.pdf"),
        &downloads,
        "a.pdf",
        copying(),
        |_| {},
    );
    assert!(matches!(
        failed,
        Err(crate::browser_guard::downloads::deliver::Failed::Retry(_))
    ));
    assert_eq!(names(&elsewhere), NONE);
    assert_eq!(names(&staging), ["a.pdf"]);
    write::remove_dir_all(&dir).unwrap();
}

/// A file system without hard links (FAT, exFAT: `linkat` fails with `EPERM`) on the same drive
/// gets the copy, where it used to fail silently at every sweep.
#[cfg(unix)]
#[test]
fn a_file_system_without_hard_links_gets_the_copy() {
    fn no_links(_: &write::Folder, _: &Path, _: &std::ffi::OsStr) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(nix::libc::EPERM))
    }
    let (dir, staging, downloads) = scratch("copy-no-links");
    write::write(&staging.join("a.pdf"), "download").unwrap();
    let how = How {
        mover: no_links,
        ..How::default()
    };
    let (mut sweeper, saved) = sweeper(&staging, &downloads, how);
    sweeper.sweep();
    assert_eq!(read(downloads.join("a.pdf")), "download");
    assert_eq!(saved.take(), [note("a.pdf")]);
    write::remove_dir_all(&dir).unwrap();
}

thread_local! {
    /// How many moves [`crosses_once`] has seen.
    static MOVES: Cell<u32> = const { Cell::new(0) };
}

/// The first move crosses devices (to the matter's drive); later ones (to the fallback folder,
/// on the data drive) are real moves.
fn crosses_once(folder: &write::Folder, from: &Path, name: &std::ffi::OsStr) -> io::Result<()> {
    MOVES.set(MOVES.get() + 1);
    if MOVES.get() == 1 {
        return crosses(folder, from, name);
    }
    folder.move_new(from, name)
}

/// A downloads folder whose drive cannot keep the mark gets nothing, not even a temporary file:
/// the download goes, marked, to the fallback folder (made then), and the agent is told its path
/// in the one line the host reads.
#[test]
fn a_drive_without_marks_sends_the_file_to_the_fallback_folder() {
    let (dir, staging, downloads) = scratch("copy-fallback");
    let fallback = dir.join("agent-state-downloads");
    write::write(&staging.join("CLAUDE.md"), "download").unwrap();
    let how = How {
        mover: crosses_once,
        mark: unmarkable,
    };
    let (mut sweeper, saved) = sweeper(&staging, &downloads, how);
    sweeper.fallback = Some(fallback.clone());
    sweeper.sweep();
    assert_eq!(names(&downloads), NONE);
    assert_eq!(names(&staging), NONE);
    let landed = fallback.join("CLAUDE (download).md");
    assert_eq!(read(landed.clone()), "download");
    let told = saved.take();
    assert_eq!(told, [fallback_note("CLAUDE (download).md", &landed)]);
    let path = told[0].rsplit_once(FALLBACK_MARKER).unwrap().1;
    assert_eq!(Path::new(path), landed);
    assert!(told[0].starts_with("Saved CLAUDE (download).md outside the matter"));
    write::remove_dir_all(&dir).unwrap();
}

/// Without a fallback folder, a download the matter's drive cannot mark stays in staging, is
/// told once, and is never tried again.
#[test]
fn without_a_fallback_folder_an_unmarkable_download_is_held_and_told_once() {
    let (dir, staging, downloads) = scratch("copy-held");
    write::write(&staging.join("a.pdf"), "download").unwrap();
    let how = How {
        mover: crosses,
        mark: unmarkable,
    };
    let (mut sweeper, saved) = sweeper(&staging, &downloads, how);
    sweeper.sweep();
    sweeper.sweep();
    assert_eq!(names(&downloads), NONE);
    assert_eq!(names(&staging), ["a.pdf"]);
    let told = saved.take();
    assert_eq!(told.len(), 1);
    assert!(
        told[0].contains("a.pdf, but it is not in the matter"),
        "{told:?}"
    );
    write::remove_dir_all(&dir).unwrap();
}

/// Where the fallback folder's drive cannot keep the mark either, the file is held and told
/// once: never delivered unmarked.
#[test]
fn a_fallback_folder_without_marks_holds_the_file() {
    let (dir, staging, downloads) = scratch("copy-fallback-unmarkable");
    let fallback = dir.join("fallback");
    write::write(&staging.join("a.pdf"), "download").unwrap();
    let how = How {
        mover: crosses,
        mark: unmarkable,
    };
    let (mut sweeper, saved) = sweeper(&staging, &downloads, how);
    sweeper.fallback = Some(fallback.clone());
    sweeper.sweep();
    sweeper.sweep();
    assert_eq!(names(&downloads), NONE);
    assert_eq!(names(&fallback), NONE);
    assert_eq!(names(&staging), ["a.pdf"]);
    let told = saved.take();
    assert_eq!(told.len(), 1);
    assert!(told[0].contains("neither the matter's drive"), "{told:?}");
    write::remove_dir_all(&dir).unwrap();
}
