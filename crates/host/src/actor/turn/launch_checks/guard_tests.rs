//! The guard's folder at the launch (`folders.rs` `make_guard`): the record made for any agent but
//! Claude, and the no-copy list (its header alone) for every agent, in a folder of the session's
//! own, `pre_write_guard::record::folder`, with each file's identity kept, and removed with the run
//! folder; a Claude session makes the folder and the list alone; anything already at the session's
//! guard folder is left as it was: a symlink, a dangling one or a junction refuses the launch of
//! any agent, the run folder taken back; a folder holding a file of the record's name, or a file,
//! refuses any agent's but Claude's, which goes on without a list (the guard's live check still
//! applies). A link at the guard folders' parent, `<data>/browser/guard`, is removed, never
//! followed, and a real folder made in its place: the launch goes on, a Claude session's too.

use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, MatterId, ThreadId};

use super::{RunFolders, RunSweep, make_guard};
use crate::config::SessionKey;
use crate::pipeline::tests::support::id;
use crate::pre_write_guard::{no_copy, record};
use crate::write::{self, RecordId};

/// A fresh data folder under the temp folder for `test`, removed on drop.
struct Data(PathBuf);

impl Data {
    fn new(test: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("clauseo-guard-{test}-{}", std::process::id()));
        write::remove_dir_all(&root).unwrap();
        write::create_dir_all(&root).unwrap();
        Self(root)
    }

    /// A launch's folders, made.
    fn launch(&self) -> RunFolders {
        let key = SessionKey {
            matter: id::<MatterId>(7),
            thread: id::<ThreadId>(8),
            launch: 1,
        };
        let run = RunFolders::new(&self.0, key);
        run.make(&self.0, &RunSweep::default()).unwrap();
        run
    }
}

impl Drop for Data {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.0));
    }
}

/// The session's guard folder, where the host makes it.
fn guard_folder(data: &Path, run: &RunFolders) -> PathBuf {
    record::folder(data, &run.key.id())
}

#[test]
fn the_record_is_made_in_a_folder_of_the_sessions_own_and_removed_with_it() {
    let data = Data::new("made");
    let mut run = data.launch();
    make_guard(&mut run, &data.0, false).unwrap();
    let folder = guard_folder(&data.0, &run);
    let (file, id) = run.guard.clone().expect("a record");
    assert_eq!(file, record::file(&folder));
    assert_eq!(
        folder,
        data.0.join("browser").join("guard").join(run.key.id())
    );
    let opened = std::fs::File::open(&file).unwrap();
    assert_eq!(RecordId::of(&opened).unwrap(), id);
    assert_eq!(std::fs::read(&file).unwrap(), b"");
    assert_eq!(RecordId::parse(&id.hex()), Some(id));
    the_list_is_made(&folder, &run);
    run.remove();
    assert!(!folder.exists(), "the guard folder goes with the run");
    assert!(!run.root.exists());
}

/// The no-copy list in the guard folder `folder`: its header alone, 0600 on Unix, with the
/// identity kept in `run`.
fn the_list_is_made(folder: &Path, run: &RunFolders) {
    let list = run.no_copy.clone().expect("a list");
    assert_eq!(list.file, folder.join(no_copy::FILE_NAME));
    assert_eq!(
        std::fs::read_to_string(&list.file).unwrap(),
        no_copy::HEADER
    );
    let opened = std::fs::File::open(&list.file).unwrap();
    assert_eq!(RecordId::of(&opened).unwrap(), list.id);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&list.file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

/// Claude's hook passes no record: a Claude launch makes the guard folder with the no-copy list
/// alone, removed with the run folder.
#[test]
fn a_claude_session_makes_the_guard_folder_with_the_list_alone() {
    let data = Data::new("claude");
    let mut run = data.launch();
    make_guard(&mut run, &data.0, true).unwrap();
    assert_eq!(run.guard, None);
    let folder = guard_folder(&data.0, &run);
    assert!(!record::file(&folder).exists());
    the_list_is_made(&folder, &run);
    run.remove();
    assert!(!folder.exists(), "the guard folder goes with the run");
}

/// Refuses the launch for what is at the session's guard folder: `agent.launch_failed`, no
/// record and no list, the run folder taken back; for a Claude session too.
fn refused(data: &Data, run: &RunFolders) {
    refused_for(data, run, &[false, true]);
}

/// [`refused`] for the agents `claudes` says (`true`: Claude).
fn refused_for(data: &Data, run: &RunFolders, claudes: &[bool]) {
    for &claude in claudes {
        let mut run = run.clone();
        // Not `run.make` again: a new sweep would set the planted folder aside.
        write::create_dir_all(&run.root).unwrap();
        let error = make_guard(&mut run, &data.0, claude).unwrap_err();
        assert_eq!(error.code(), ErrorCode::AgentLaunchFailed, "{error}");
        assert!(error.to_string().contains("guard"), "{error}");
        assert_eq!((run.guard, run.no_copy), (None, None));
        assert!(!run.root.exists(), "the run folder is taken back");
    }
}

/// A Claude launch whose guard folder cannot be made for an I/O reason (no link at its place)
/// goes on without a list, nothing made or removed there, and the run folder kept: Claude's hook
/// needs no record, and the guard's live check still applies.
fn claude_launches_without_a_list(data: &Data, run: &RunFolders) {
    let mut run = run.clone();
    write::create_dir_all(&run.root).unwrap();
    make_guard(&mut run, &data.0, true).unwrap();
    assert_eq!((run.guard, run.no_copy), (None, None));
    assert!(run.root.exists(), "the run folder is kept");
}

/// A folder already at the session's guard folder, holding a file of the record's name: the
/// create-new call refuses it, so the host's `O_EXCL` never meets the file, which keeps its text;
/// and `O_EXCL` alone refuses that file too. Codex's launch is refused; Claude's goes on without a
/// list (a stale folder or file is an I/O reason, not a link).
#[test]
fn a_folder_or_a_file_at_the_guard_folders_place_refuses_the_launch() {
    let data = Data::new("folder");
    let run = data.launch();
    let folder = guard_folder(&data.0, &run);
    write::create_dir_all(&folder).unwrap();
    let file = record::file(&folder);
    write::write_file(&file, b"planted\n").unwrap();
    refused_for(&data, &run, &[false]);
    claude_launches_without_a_list(&data, &run);
    assert_eq!(std::fs::read(&file).unwrap(), b"planted\n");
    let error = write::create_record(&file).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&file).unwrap(), b"planted\n");

    let data = Data::new("file");
    let run = data.launch();
    let folder = guard_folder(&data.0, &run);
    write::create_dir_all(folder.parent().unwrap()).unwrap();
    write::write_file(&folder, b"a file\n").unwrap();
    refused_for(&data, &run, &[false]);
    claude_launches_without_a_list(&data, &run);
    assert_eq!(std::fs::read(&folder).unwrap(), b"a file\n");
}

/// A symlink at the session's guard folder, to a folder outside or to nothing, refuses the
/// launch: nothing is made through it.
#[cfg(unix)]
#[test]
fn a_symlink_at_the_guard_folders_place_refuses_the_launch() {
    let data = Data::new("symlink");
    let run = data.launch();
    let folder = guard_folder(&data.0, &run);
    write::create_dir_all(folder.parent().unwrap()).unwrap();
    let outside = data.0.join("outside");
    write::create_dir_all(&outside).unwrap();
    write::symlink(&outside, &folder).unwrap();
    refused(&data, &run);
    assert!(folder.is_symlink(), "left as it was");
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);

    let data = Data::new("dangling");
    let run = data.launch();
    let folder = guard_folder(&data.0, &run);
    write::create_dir_all(folder.parent().unwrap()).unwrap();
    let nowhere = data.0.join("nowhere");
    write::symlink(&nowhere, &folder).unwrap();
    refused(&data, &run);
    assert!(!nowhere.exists(), "nothing made through the dangling link");
}

/// Windows: a junction (which any user may make) at the session's guard folder refuses the
/// launch, and nothing is made in its target.
#[cfg(windows)]
#[test]
fn a_junction_at_the_guard_folders_place_refuses_the_launch() {
    let data = Data::new("junction");
    let run = data.launch();
    let folder = guard_folder(&data.0, &run);
    write::create_dir_all(folder.parent().unwrap()).unwrap();
    let outside = data.0.join("outside");
    write::create_dir_all(&outside).unwrap();
    let target = dunce::canonicalize(&outside).unwrap();
    clauseo_sandbox::windows::test_junction::junction(&folder, &target).unwrap();
    refused(&data, &run);
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
}

/// A symlink at `<data>/browser/guard` to a scratch folder: the link is removed, never followed,
/// a real folder is made in its place, and the launch goes on with its record and list there,
/// a Claude session's with its list alone. The scratch folder keeps what it held, and nothing is
/// made in it.
#[cfg(unix)]
#[test]
fn a_symlink_at_the_guard_folders_parent_is_replaced_and_the_launch_goes_on() {
    for claude in [false, true] {
        symlinked_parent(claude);
    }
}

#[cfg(unix)]
fn symlinked_parent(claude: bool) {
    let data = Data::new(&format!("parent-symlink-{claude}"));
    let mut run = data.launch();
    let parent = data.0.join("browser").join("guard");
    write::create_dir_all(parent.parent().unwrap()).unwrap();
    let scratch = data.0.join("scratch");
    write::create_dir_all(&scratch).unwrap();
    write::write_file(&scratch.join("keep.txt"), b"keep\n").unwrap();
    write::symlink(&scratch, &parent).unwrap();
    make_guard(&mut run, &data.0, claude).unwrap();
    assert!(
        write::is_real_dir(&parent),
        "a real folder in the link's place"
    );
    let folder = guard_folder(&data.0, &run);
    assert!(folder.starts_with(&parent));
    the_list_is_made(&folder, &run);
    assert_eq!(run.guard.is_some(), !claude);
    if let Some((file, _)) = &run.guard {
        assert_eq!(*file, record::file(&folder));
    }
    assert_eq!(names(&scratch), ["keep.txt"]);
    assert_eq!(std::fs::read(scratch.join("keep.txt")).unwrap(), b"keep\n");
}

/// Windows: a dangling junction at `<data>/browser/guard` (its target never made) is removed,
/// a real folder made in its place, and the launch goes on, a Claude session's too; nothing is
/// made at the target.
#[cfg(windows)]
#[test]
fn a_dangling_junction_at_the_guard_folders_parent_is_replaced_and_the_launch_goes_on() {
    for claude in [false, true] {
        junction_parent(claude);
    }
}

#[cfg(windows)]
fn junction_parent(claude: bool) {
    let data = Data::new(&format!("parent-junction-{claude}"));
    let mut run = data.launch();
    let parent = data.0.join("browser").join("guard");
    write::create_dir_all(parent.parent().unwrap()).unwrap();
    let nowhere = data.0.join("nowhere");
    clauseo_sandbox::windows::test_junction::junction(&parent, &nowhere).unwrap();
    assert!(!write::is_real_dir(&parent), "a junction counts as a link");
    make_guard(&mut run, &data.0, claude).unwrap();
    assert!(
        write::is_real_dir(&parent),
        "a real folder in the junction's place"
    );
    let folder = guard_folder(&data.0, &run);
    assert!(folder.starts_with(&parent));
    the_list_is_made(&folder, &run);
    assert_eq!(run.guard.is_some(), !claude);
    assert!(!nowhere.exists(), "nothing made at the junction's target");
}

/// The names in `folder`, sorted.
#[cfg(unix)]
fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}
