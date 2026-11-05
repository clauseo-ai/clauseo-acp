//! Windows: the matters crate's Windows-only paths through its public interface, in real
//! folders. A drive root is refused; the probe finds a file with the offline attribute
//! cloud-only; a file or folder another program holds open without sharing fails a read
//! with an I/O error, never a finding that the file is missing or unchanged; and a restore's
//! preview answers with such a file locked, while the restore refuses with it. A folder whose
//! path is too deep for Windows to start programs in is refused as `TooDeep`.

#![cfg(windows)]
#![expect(
    clippy::disallowed_methods,
    reason = "the Windows tests make their files with std's writes and hold them open without \
              sharing; clippy.toml bans those calls elsewhere"
)]

/// The tests and their helpers, under `cfg(test)` so clippy treats the helpers as test code.
#[cfg(test)]
mod windows {
    use std::collections::BTreeSet;
    use std::fs::{File, OpenOptions};
    use std::io::{self, Write};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use clauseo_matters::{
        CheckpointOutcome, DEFAULT_MARKER, Error, Imported, Limits, Matter, MissingReason,
        NewMatter, OsProbe, PlaceholderProbe, Refusal, Restore, RestoreFiles, Retention, Settings,
        TurnMeta, WINDOWS_FOLDER_LIMIT, check_place,
    };
    use clauseo_protocol::{
        CheckpointId, ErrorCode, MatterId, MatterPath, SkippedFile, SkippedFileReason, Timestamp,
    };

    /// `FILE_ATTRIBUTE_OFFLINE`: the file's data is not on this computer.
    const FILE_ATTRIBUTE_OFFLINE: u32 = 0x0000_1000;
    /// `FILE_FLAG_BACKUP_SEMANTICS`, which `CreateFileW` needs to open a folder.
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    /// `ERROR_SHARING_VIOLATION`: another handle holds the file without sharing it.
    const ERROR_SHARING_VIOLATION: i32 = 32;

    /// A test's own folder, empty when made, canonical, removed on drop.
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("clauseo-matters-win-{name}-{}", std::process::id()));
            if let Err(err) = std::fs::remove_dir_all(&path) {
                assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
            }
            std::fs::create_dir_all(&path).unwrap();
            Self(dunce::canonicalize(&path).unwrap())
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if let Err(err) = std::fs::remove_dir_all(&self.0) {
                tracing::warn!(path = %self.0.display(), error = %err, "test folder left behind");
            }
        }
    }

    /// `fs::write`, after making the parent folders.
    fn write(path: &Path, contents: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    /// Opens the file or folder at `path` with no sharing, as a program that locks it does: every
    /// other open that reads it fails with `ERROR_SHARING_VIOLATION` until the handle drops.
    fn hold(path: &Path) -> File {
        OpenOptions::new()
            .read(true)
            .share_mode(0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .unwrap()
    }

    fn cid(n: u64) -> CheckpointId {
        format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
    }

    fn matter_id() -> MatterId {
        "01910000-0000-7000-8000-000000000001".parse().unwrap()
    }

    fn at(n: u64) -> Timestamp {
        Timestamp::new(1_700_000_000_000 + i64::try_from(n).unwrap() * 1000).unwrap()
    }

    /// The checkpoint `n` of turn `n`.
    fn meta(n: u64) -> TurnMeta {
        TurnMeta {
            checkpoint: cid(n),
            thread: "01920000-0000-7000-8000-000000000001".parse().unwrap(),
            turn: format!("01930000-0000-7000-8000-{n:012x}").parse().unwrap(),
            at: at(n),
        }
    }

    fn path(text: &str) -> MatterPath {
        MatterPath::new(text).unwrap()
    }

    fn tools(paths: &[&str]) -> BTreeSet<MatterPath> {
        paths.iter().map(|text| path(text)).collect()
    }

    /// A matter folder and a matters root in one test folder, with the OS probe.
    struct Case {
        dir: TestDir,
        folder: PathBuf,
    }

    impl Case {
        fn new(name: &str) -> Self {
            let dir = TestDir::new(name);
            let folder = dir.path().join("matter");
            Self { dir, folder }
        }

        fn settings(&self) -> Settings {
            Settings {
                matters_root: self.dir.path().join("root"),
                refused_trees: Vec::new(),
                limits: Limits::DEFAULT,
                retention: Retention::KeepAll,
                probe: Arc::new(OsProbe),
            }
        }

        fn file(&self, rel: &str) -> PathBuf {
            self.folder.join(rel)
        }

        fn write(&self, rel: &str, contents: &str) {
            write(&self.file(rel), contents.as_bytes());
        }

        /// Imports the folder, root checkpoint 1.
        fn import(&self) -> Matter {
            let new = NewMatter {
                id: matter_id(),
                folder: self.folder.clone(),
                created_at: at(0),
                marker: DEFAULT_MARKER,
                checkpoint: cid(1),
            };
            let cancel = AtomicBool::new(false);
            match Matter::import(self.settings(), new, &[], &mut |_| {}, &cancel).unwrap() {
                Imported::Done { matter, .. } => *matter,
                Imported::Cancelled => panic!("cancelled"),
            }
        }

        fn open(&self, folder: &Path) -> Result<Matter, Error> {
            Matter::open(self.settings(), matter_id(), folder)
        }
    }

    /// The OS error of an `Error::Io` and what was being done, panicking on any other error.
    fn io_error(err: &Error) -> (&'static str, &io::Error) {
        assert_eq!(err.code(), ErrorCode::MatterCheckpointFailed, "{err:?}");
        match err {
            Error::Io { op, source, .. } => (op, source),
            other => panic!("not an Io error: {other:?}"),
        }
    }

    /// `act` on `matter` on its own thread, with the matter back and what `act` returned; fails
    /// the test when no answer comes within 60 s, so a call that hangs fails instead of hanging.
    fn within_deadline<T: Send + 'static>(
        mut matter: Matter,
        act: impl FnOnce(&mut Matter) -> T + Send + 'static,
    ) -> (Matter, T) {
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let out = act(&mut matter);
            let _gone = send.send((matter, out));
        });
        receive
            .recv_timeout(Duration::from_secs(60))
            .expect("no answer within 60 s")
    }

    /// The restore of the import, checkpoint 1, with outside-edits id `n` and restore id `n + 1`.
    fn restore_import(n: u64) -> Restore<'static> {
        Restore {
            from: cid(1),
            files: RestoreFiles::All,
            outside_edits: cid(n),
            checkpoint: cid(n + 1),
            at: at(n),
        }
    }

    /// The paths of the files a restore plan writes.
    fn written(plan: &clauseo_matters::RestorePlan) -> Vec<&str> {
        plan.writes.iter().map(|w| w.path().as_str()).collect()
    }

    /// The refusal in `err`, panicking on any other error.
    fn refusal(err: Error) -> Refusal {
        match err {
            Error::FolderRefused { reason, .. } => reason,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// A folder `units` UTF-16 units long, made under `base` in names of up to 40 units: the
    /// path as Windows counts it.
    fn folder_of_units(base: &Path, units: usize) -> PathBuf {
        let mut folder = base.to_owned();
        loop {
            let used = folder.as_os_str().encode_wide().count();
            let left = units - used - 1;
            if left <= 40 {
                let name = "f".repeat(left);
                folder.push(&name);
                std::fs::create_dir_all(&folder).unwrap();
                return folder;
            }
            folder.push("d".repeat(40));
            std::fs::create_dir_all(&folder).unwrap();
        }
    }

    /// A folder over the Windows limit is refused as too deep by the place check, and one at the
    /// limit or well under it is not. The folders are made under the temp dir; std makes the long
    /// paths itself.
    #[test]
    fn a_folder_too_deep_for_windows_is_refused_as_too_deep() {
        let dir = TestDir::new("too-deep");
        let deep = folder_of_units(dir.path(), WINDOWS_FOLDER_LIMIT + 1);
        assert_eq!(
            deep.as_os_str().encode_wide().count(),
            WINDOWS_FOLDER_LIMIT + 1
        );
        let err = check_place(&deep, &[]).unwrap_err();
        assert_eq!(refusal(err), Refusal::TooDeep);

        let at_limit = folder_of_units(dir.path(), WINDOWS_FOLDER_LIMIT);
        check_place(&at_limit, &[]).unwrap();
        let short = folder_of_units(dir.path(), 200);
        check_place(&short, &[]).unwrap();
    }

    /// The drive root, plain and in the verbatim form, is refused by the place check as a drive
    /// root; a folder on that drive is not.
    #[test]
    fn a_drive_root_is_refused_as_a_drive_root() {
        let dir = TestDir::new("drive-root");
        let drive = dir.path().ancestors().last().unwrap().to_owned();
        let verbatim = PathBuf::from(format!(r"\\?\{}", drive.display()));
        for root in [&drive, &verbatim] {
            let err = check_place(root, &[]).unwrap_err();
            assert_eq!(refusal(err), Refusal::DriveRoot, "{}", root.display());
        }
        check_place(dir.path(), &[]).unwrap();
    }

    /// A listed folder and its parents are refused, named by the listed entry; a folder beside it
    /// is not, and a folder that does not exist is missing.
    #[test]
    fn a_listed_folder_and_its_parents_are_refused() {
        let dir = TestDir::new("listed");
        let listed = dir.path().join("Home");
        let sibling = dir.path().join("Matter");
        std::fs::create_dir_all(&listed).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        for folder in [&listed, &dir.path().to_owned()] {
            let err = check_place(folder, std::slice::from_ref(&listed)).unwrap_err();
            assert_eq!(refusal(err), Refusal::Listed(listed.clone()));
        }
        check_place(&sibling, std::slice::from_ref(&listed)).unwrap();
        let gone = dir.path().join("gone");
        let err = check_place(&gone, &[]).unwrap_err();
        assert!(
            matches!(
                err,
                Error::FolderMissing {
                    reason: MissingReason::NoFolder,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    /// The OS probe finds a file with the offline attribute cloud-only, and a plain file not; a
    /// checkpoint with it skips the offline file as cloud-only and versions the plain one.
    #[test]
    fn the_os_probe_finds_an_offline_file_cloud_only() {
        let case = Case::new("probe-offline");
        case.write("plain.txt", "plain");
        let mut matter = case.import();

        let offline = case.file("cloud.docx");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .attributes(FILE_ATTRIBUTE_OFFLINE)
            .open(&offline)
            .unwrap();
        file.write_all(b"in the cloud").unwrap();
        drop(file);
        let metadata = offline.symlink_metadata().unwrap();
        assert_ne!(
            metadata.file_attributes() & FILE_ATTRIBUTE_OFFLINE,
            0,
            "the file has the offline attribute"
        );
        assert!(OsProbe.is_cloud_only(&offline, &metadata));
        let plain = case.file("plain.txt");
        assert!(!OsProbe.is_cloud_only(&plain, &plain.symlink_metadata().unwrap()));

        let checkpoint = matter.before_turn(&meta(2)).unwrap();
        assert_eq!(checkpoint.outcome, CheckpointOutcome::Reused(cid(1)));
        assert!(checkpoint.changes.is_empty(), "{:?}", checkpoint.changes);
        assert_eq!(
            checkpoint.skipped,
            [SkippedFile {
                path: path("cloud.docx"),
                reason: SkippedFileReason::CloudOnly,
            }]
        );
    }

    /// A matter folder Windows cannot look at (a `|` in its name) fails to open with an I/O error,
    /// not as a missing folder the lawyer must locate.
    #[test]
    fn open_fails_when_the_folder_name_cannot_be_looked_at() {
        let case = Case::new("open-bad-name");
        let err = case.open(&case.dir.path().join("bad|name")).unwrap_err();
        let (op, source) = io_error(&err);
        assert_eq!(op, "stat", "{err:?}");
        assert_ne!(source.kind(), io::ErrorKind::NotFound, "{err:?}");
    }

    /// A `matter.json` another program holds without sharing fails the open with an I/O error,
    /// not as a folder without a marker; once let go, the matter opens.
    #[test]
    fn open_fails_while_matter_json_is_held_without_sharing() {
        let case = Case::new("open-held-json");
        case.write("a.txt", "a");
        drop(case.import());

        let held = hold(&case.file(".clauseo/matter.json"));
        let err = case.open(&case.folder).unwrap_err();
        let (op, source) = io_error(&err);
        assert_eq!(op, "read", "{err:?}");
        assert_eq!(
            source.raw_os_error(),
            Some(ERROR_SHARING_VIOLATION),
            "{err:?}"
        );
        drop(held);
        case.open(&case.folder).unwrap();
    }

    /// `edited_since` fails with an I/O error while a target edited since is held without sharing:
    /// it never counts the file as unchanged. Once let go, the edit is found.
    #[test]
    fn edited_since_fails_while_a_target_is_held_without_sharing() {
        let case = Case::new("edited-held-file");
        case.write("a.txt", "one");
        let mut matter = case.import();
        case.write("a.txt", "two");
        matter.after_turn(&meta(3), &tools(&["a.txt"])).unwrap();

        // Edited since, so the stat cache cannot vouch for it and it must be read.
        case.write("a.txt", "three, by the lawyer");
        let held = hold(&case.file("a.txt"));
        let err = matter.edited_since(cid(1), cid(3)).unwrap_err();
        let (_, source) = io_error(&err);
        assert_eq!(
            source.raw_os_error(),
            Some(ERROR_SHARING_VIOLATION),
            "{err:?}"
        );
        drop(held);
        assert_eq!(
            matter.edited_since(cid(1), cid(3)).unwrap(),
            [path("a.txt")]
        );
    }

    /// `edited_since` fails with an I/O error while a target's folder is held without sharing, so
    /// it cannot be listed: it never counts the file as missing or unchanged. Once let go, the
    /// unchanged target is not edited.
    #[test]
    fn edited_since_fails_while_a_targets_folder_is_held_without_sharing() {
        let case = Case::new("edited-held-folder");
        case.write("keep.txt", "keep");
        let mut matter = case.import();
        case.write("sub/new.txt", "new");
        matter
            .after_turn(&meta(3), &tools(&["sub/new.txt"]))
            .unwrap();

        let held = hold(&case.file("sub"));
        let err = matter.edited_since(cid(1), cid(3)).unwrap_err();
        let (_, source) = io_error(&err);
        assert_eq!(
            source.raw_os_error(),
            Some(ERROR_SHARING_VIOLATION),
            "{err:?}"
        );
        drop(held);
        let edited = matter.edited_since(cid(1), cid(3)).unwrap();
        assert!(edited.is_empty(), "{edited:?}");
    }
    /// The stale owner-file rule on Windows (this binary never calls `test_support`'s pretend
    /// Word, so it reads the real process list; no Word runs on CI): an owner file whose document
    /// opens with no sharing is stale and shows nothing held; while the test holds the document
    /// with no sharing, as Word does, the same owner file is held.
    #[test]
    fn an_owner_file_is_stale_with_no_word_and_the_document_free_and_held_while_it_is_held() {
        let dir = TestDir::new("stale-owner");
        let root = dir.path();
        write(&root.join("Invoice.docx"), b"one");
        write(
            &root.join("~$nvoice.docx"),
            b"owner file left by a crashed Word",
        );
        assert_eq!(
            clauseo_matters::held_open(root, &path("Invoice.docx")),
            None,
            "stale: no Word runs and the document opens with no sharing"
        );
        let held = hold(&root.join("Invoice.docx"));
        assert_eq!(
            clauseo_matters::held_open(root, &path("Invoice.docx")),
            Some(clauseo_matters::HeldOpen::OwnerFile(
                "~$nvoice.docx".to_owned()
            )),
            "held: the no-share open fails"
        );
        drop(held);
    }

    /// Word holds a document the restore would write: open with no sharing, its owner file
    /// beside it, edited since the last checkpoint so the walk must read it (the Windows e2e's
    /// versions-locked.spec.ts). The preview answers within the deadline, naming the document
    /// locked and still written; it does not fail because the document cannot be read. The
    /// restore refuses with `matter.file_locked` and writes nothing. Once Word lets go, both go
    /// through.
    #[test]
    fn a_restore_preview_answers_with_a_file_held_without_sharing_locked() {
        let case = Case::new("preview-held");
        case.write("brief.txt", "Clause 4 draft\n");
        case.write("old.txt", "Old terms\n");
        let mut matter = case.import();
        case.write("brief.txt", "Clause 4 final\n");
        case.write("notes/a.txt", "Alpha\n");
        matter
            .after_turn(&meta(3), &tools(&["brief.txt", "notes/a.txt"]))
            .unwrap();
        let edited = "Clause 4 final, edited by the lawyer\n";
        case.write("brief.txt", edited);
        case.write("~$brief.txt", "owner file: open in Word");
        let held = hold(&case.file("brief.txt"));

        let (matter, preview) =
            within_deadline(matter, |m| m.preview_restore(cid(1), RestoreFiles::All));
        let plan = preview.expect("the preview answers while the file is held");
        assert_eq!(plan.locked, [path("brief.txt")], "{plan:?}");
        assert_eq!(written(&plan), ["brief.txt"], "{plan:?}");
        assert_eq!(plan.added_since, [path("notes/a.txt")], "{plan:?}");

        let (matter, restored) = within_deadline(matter, |m| m.restore(&restore_import(4)));
        let err = restored.expect_err("the restore went through a held file");
        assert_eq!(err.code(), ErrorCode::MatterFileLocked, "{err:?}");
        assert!(
            matches!(&err, Error::FileLocked { paths } if paths == &[path("brief.txt")]),
            "{err:?}"
        );
        drop(held);
        assert_eq!(
            std::fs::read_to_string(case.file("brief.txt")).unwrap(),
            edited
        );

        // Word closed the document; its owner file left behind is stale (no Word runs).
        let (mut matter, preview) =
            within_deadline(matter, |m| m.preview_restore(cid(1), RestoreFiles::All));
        let plan = preview.unwrap();
        assert!(plan.locked.is_empty(), "{plan:?}");
        assert_eq!(written(&plan), ["brief.txt"], "{plan:?}");
        matter.restore(&restore_import(4)).unwrap();
        assert_eq!(
            std::fs::read_to_string(case.file("brief.txt")).unwrap(),
            "Clause 4 draft\n"
        );
    }

    /// A file held without sharing that the version does not have, with no owner file beside it,
    /// is locked too: the restore would not write it, but its walk cannot read it, so the
    /// outside-edits checkpoint could not record it. A checkpoint still fails with the read's
    /// error, as before: only a restore and its preview report a held file.
    #[test]
    fn a_held_file_the_version_lacks_is_locked_and_a_checkpoint_still_fails() {
        let case = Case::new("preview-held-added");
        case.write("brief.txt", "Clause 4 draft\n");
        let matter = case.import();
        case.write("added.txt", "added since the import\n");
        let held = hold(&case.file("added.txt"));

        let (mut matter, preview) =
            within_deadline(matter, |m| m.preview_restore(cid(1), RestoreFiles::All));
        let plan = preview.expect("the preview answers while the file is held");
        assert_eq!(plan.locked, [path("added.txt")], "{plan:?}");
        assert!(plan.writes.is_empty(), "{plan:?}");

        let err = matter.restore(&restore_import(4)).unwrap_err();
        assert!(
            matches!(&err, Error::FileLocked { paths } if paths == &[path("added.txt")]),
            "{err:?}"
        );
        let err = matter.before_turn(&meta(6)).unwrap_err();
        let (_, source) = io_error(&err);
        assert_eq!(
            source.raw_os_error(),
            Some(ERROR_SHARING_VIOLATION),
            "{err:?}"
        );
        drop(held);
    }
}
