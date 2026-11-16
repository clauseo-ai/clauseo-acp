use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use clauseo_process::SpawnOptions;
use clauseo_process::{AsyncIoTimer, LaunchSpec, Launched, Launcher, PassthroughLauncher};
use futures::executor::block_on;

use super::*;
use crate::platform::Arch;
use crate::state::PythonBlocker;
use crate::testing::runtime_tar_gz;
use crate::write::TestDir;

/// A fake runtime archive whose `bin/python3` is a shell script running `script`.
fn fake_archive(script: &str) -> Vec<u8> {
    let program = format!("#!/bin/sh\n{script}\n");
    runtime_tar_gz(
        "python",
        &[
            ("bin/", 0o755, b""),
            ("bin/python3", 0o755, program.as_bytes()),
            ("lib/python3.13/os.py", 0o644, b"# os\n"),
            ("lib/python3.13/site-packages/lxml/__init__.py", 0o644, b""),
        ],
    )
    .unwrap()
}

/// The archive written to `<data>/agents/downloads/<name>` and its entry.
fn stage(data: &TestDir, bytes: &[u8]) -> (PathBuf, PythonEntry, PythonArchive) {
    use sha2::Digest as _;
    let name = "clauseo-python-3.13.18-c1-linux-x64.tar.gz";
    let sha = crate::integrity::to_hex(&sha2::Sha256::digest(bytes));
    let size = u64::try_from(bytes.len()).unwrap();
    let archive = PythonArchive::new(
        Os::Linux,
        Arch::X64,
        name,
        "https://python.test/a",
        size,
        size * 4,
        &sha,
    )
    .unwrap();
    let path = data.file(&format!("agents/downloads/{name}"), bytes);
    let entry = PythonEntry::new("3.13.18", vec![archive.clone()]);
    (path, entry, archive)
}

fn launcher() -> PassthroughLauncher {
    PassthroughLauncher::new(SpawnOptions::new())
}

fn install(
    path: &Path,
    entry: &PythonEntry,
    archive: &PythonArchive,
    dir: &Path,
    launcher: &dyn Launcher,
) -> Result<(), PythonError> {
    block_on(install_python_runtime(
        path,
        entry,
        archive,
        dir,
        Os::Linux,
        launcher,
        &AsyncIoTimer,
        &Cancel::new(),
    ))
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777
}

#[cfg(unix)]
#[test]
fn a_runtime_installs_read_only_with_its_manifest_and_receipt() {
    let data = TestDir::new("python-install");
    let (path, entry, archive) = stage(&data, &fake_archive("echo 3.13.18"));
    let dir = runtime_dir(&data.0, &archive);
    assert_eq!(
        dir,
        data.0
            .join("agents/runtime/clauseo-python-3.13.18-c1-linux-x64")
    );
    install(&path, &entry, &archive, &dir, &launcher()).unwrap();
    assert!(python_runtime_valid(&dir, &archive, Os::Linux));
    assert!(contents_match(&dir));
    let manifest = std::fs::read_to_string(dir.join(PYTHON_MANIFEST)).unwrap();
    let paths: Vec<&str> = manifest
        .lines()
        .map(|line| line.splitn(3, ' ').nth(2).unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "bin/python3",
            "lib/python3.13/os.py",
            "lib/python3.13/site-packages/lxml/__init__.py"
        ]
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join(PYTHON_RECEIPT)).unwrap()).unwrap();
    assert_eq!(receipt["schema"], 1);
    assert_eq!(receipt["archive"], archive.name());
    assert_eq!(receipt["sha256"], archive.sha256_hex());
    assert_eq!(receipt["manifest_sha256"], sha256_hex(manifest.as_bytes()));
    // Read-only: folders 0555, the program 0555, other files 0444.
    assert_eq!(mode(&dir), 0o555);
    assert_eq!(mode(&dir.join("bin")), 0o555);
    assert_eq!(mode(&dir.join("lib/python3.13/site-packages")), 0o555);
    assert_eq!(mode(&dir.join("bin/python3")), 0o555);
    assert_eq!(mode(&dir.join("lib/python3.13/os.py")), 0o444);
    let names: Vec<String> = std::fs::read_dir(dir.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [archive.stem()], "no temp folder is left");
    assert_eq!(runtime_command_dir(&dir, Os::Darwin), dir.join("bin"));
    assert_eq!(
        runtime_command_dir(&dir, Os::Win32),
        dir.join("clauseo-bin")
    );
    // The read-only tree goes with the one removal every clean-up uses.
    write::remove_dir_all(&dir).unwrap();
    assert!(!dir.exists());
}

/// An installed runtime in `data`, made writable so a test can change it.
#[cfg(unix)]
fn installed(data: &TestDir) -> (PathBuf, PythonEntry, PythonArchive, PathBuf) {
    let (path, entry, archive) = stage(data, &fake_archive("echo 3.13.18"));
    let dir = runtime_dir(&data.0, &archive);
    install(&path, &entry, &archive, &dir, &launcher()).unwrap();
    assert!(python_runtime_valid(&dir, &archive, Os::Linux));
    write::make_writable(&dir).unwrap();
    (path, entry, archive, dir)
}

/// The limit of the whole-check, which compares sizes only: a file changed to other bytes of
/// the same size stays valid. Only the repair check, which hashes every byte, finds it.
#[cfg(unix)]
#[test]
fn a_same_size_change_fails_only_the_contents_check_and_a_missing_manifest_the_receipt() {
    let data = TestDir::new("python-contents");
    let (_, _, archive, dir) = installed(&data);
    let os_py = dir.join("lib/python3.13/os.py");
    write::set_mode(&os_py, 0o644).unwrap();
    write::write_atomic(&os_py, b"# OS\n").unwrap();
    whole::forget(&dir);
    assert!(python_runtime_valid(&dir, &archive, Os::Linux));
    assert!(!contents_match(&dir), "a changed file");
    write::remove_file(&dir.join(PYTHON_MANIFEST)).unwrap();
    assert!(
        !python_runtime_valid(&dir, &archive, Os::Linux),
        "no manifest"
    );
}

/// A file gone is noticed at the next start (the check's result is kept for the rest of this
/// one): the runtime is then not valid.
#[cfg(unix)]
#[test]
fn a_runtime_missing_one_file_is_not_valid_from_the_next_start() {
    let data = TestDir::new("python-file-gone");
    let (_, _, archive, dir) = installed(&data);
    write::remove_file(&dir.join("lib/python3.13/os.py")).unwrap();
    assert!(
        python_runtime_valid(&dir, &archive, Os::Linux),
        "kept this start"
    );
    whole::forget(&dir);
    assert!(!python_runtime_valid(&dir, &archive, Os::Linux));
}

#[cfg(unix)]
#[test]
fn a_truncated_file_is_not_valid() {
    let data = TestDir::new("python-truncated");
    let (_, _, archive, dir) = installed(&data);
    let os_py = dir.join("lib/python3.13/os.py");
    write::set_mode(&os_py, 0o644).unwrap();
    write::write_atomic(&os_py, b"# o").unwrap();
    whole::forget(&dir);
    assert!(!python_runtime_valid(&dir, &archive, Os::Linux));
    assert!(
        !whole::sizes_match(&dir, "x 0 bin\n"),
        "a folder is no file"
    );
}

#[cfg(unix)]
#[test]
fn a_removed_runtime_folder_is_not_valid_and_installs_again() {
    let data = TestDir::new("python-folder-gone");
    let (path, entry, archive, dir) = installed(&data);
    write::remove_dir_all(&dir).unwrap();
    assert!(!python_runtime_valid(&dir, &archive, Os::Linux));
    install(&path, &entry, &archive, &dir, &launcher()).unwrap();
    assert!(python_runtime_valid(&dir, &archive, Os::Linux));
    assert!(contents_match(&dir));
}

#[test]
fn an_archive_that_does_not_match_is_deleted_and_nothing_unpacked() {
    let data = TestDir::new("python-mismatch");
    let bytes = fake_archive("echo 3.13.18");
    let (path, entry, archive) = stage(&data, &bytes);
    // Same size, other bytes.
    let mut other = bytes;
    if let Some(last) = other.last_mut() {
        *last ^= 0xff;
    }
    write::write_atomic(&path, &other).unwrap();
    let dir = runtime_dir(&data.0, &archive);
    let err = install(&path, &entry, &archive, &dir, &launcher()).unwrap_err();
    assert!(err.to_string().contains("pinned checksum"), "{err}");
    assert!(!path.exists(), "the archive is deleted");
    assert!(!dir.exists());
    assert!(
        !dir.with_file_name(format!(".tmp-{}", archive.stem()))
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn a_runtime_whose_check_exits_non_zero_is_removed_and_its_archive_kept() {
    let data = TestDir::new("python-check-fails");
    let (path, entry, archive) = stage(&data, &fake_archive("exit 3"));
    let dir = runtime_dir(&data.0, &archive);
    let err = install(&path, &entry, &archive, &dir, &launcher()).unwrap_err();
    let PythonError::Run(RunFailure::Failed(detail)) = err else {
        panic!("{err:?}");
    };
    assert!(detail.contains("code 3"), "{detail}");
    assert!(!dir.exists(), "the unpacked tree is removed");
    assert!(path.exists(), "the archive stays for the next try");
}

/// A launcher the OS refuses with `error`.
struct Refusing(fn() -> io::Error);

impl Launcher for Refusing {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        Err(clauseo_process::Error::Spawn {
            program: spec.program().to_owned(),
            source: (self.0)(),
        })
    }
}

#[test]
fn a_refused_start_is_blocked_and_another_launch_error_is_failed() {
    let data = TestDir::new("python-refused");
    let (path, entry, archive) = stage(&data, &fake_archive("echo 3.13.18"));
    let dir = runtime_dir(&data.0, &archive);
    let blocked: [(fn() -> io::Error, PythonBlocker); 5] = [
        (
            || io::Error::from(io::ErrorKind::PermissionDenied),
            PythonBlocker::Unknown,
        ),
        (
            || io::Error::from_raw_os_error(1260),
            PythonBlocker::AppLocker,
        ),
        (
            || io::Error::from_raw_os_error(4551),
            PythonBlocker::AppControl,
        ),
        (
            || io::Error::from_raw_os_error(0xC0E9_0002_u32.cast_signed()),
            PythonBlocker::AppControl,
        ),
        (
            || io::Error::from_raw_os_error(0xC000_0428_u32.cast_signed()),
            PythonBlocker::Unknown,
        ),
    ];
    for (error, want) in blocked {
        let err = install(&path, &entry, &archive, &dir, &Refusing(error)).unwrap_err();
        let PythonError::Run(RunFailure::Blocked(_, by)) = err else {
            panic!("{err:?}");
        };
        assert_eq!(by, want, "{:?}", error());
        assert!(!dir.exists());
    }
    let missing = || io::Error::from(io::ErrorKind::NotFound);
    let err = install(&path, &entry, &archive, &dir, &Refusing(missing)).unwrap_err();
    assert!(
        matches!(err, PythonError::Run(RunFailure::Failed(_))),
        "{err:?}"
    );
}

#[test]
fn a_process_refused_at_load_names_its_blocker_from_its_exit_status() {
    use super::run::exit_result;
    let refused = |status: u32| match exit_result(0, Some(status.cast_signed())) {
        Err(RunFailure::Blocked(_, by)) => by,
        other => panic!("{other:?}"),
    };
    assert_eq!(refused(0xC0E9_0002), PythonBlocker::AppControl);
    assert_eq!(refused(0xC000_0428), PythonBlocker::Unknown);
    assert_eq!(exit_result(12, Some(0)), Ok(()));
    for (seen, code) in [(0, Some(0)), (12, Some(3)), (12, None)] {
        assert!(
            matches!(exit_result(seen, code), Err(RunFailure::Failed(_))),
            "{seen} {code:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn stale_python_folders_go_read_only_or_not_and_node_and_the_current_one_stay() {
    let data = TestDir::new("python-stale");
    let runtime = data.0.join("agents/runtime");
    let (path, entry, archive) = stage(&data, &fake_archive("echo 3.13.18"));
    // An old build's runtime, installed and so read-only.
    let old = runtime.join("clauseo-python-3.13.17-c4-linux-x64");
    install(&path, &entry, &archive, &old, &launcher()).unwrap();
    assert_eq!(mode(&old), 0o555);
    data.file(
        "agents/runtime/.tmp-clauseo-python-3.13.18-c1-linux-x64/bin/x",
        b"",
    );
    write::seal_below(&runtime.join(".tmp-clauseo-python-3.13.18-c1-linux-x64")).unwrap();
    data.file("agents/runtime/node-v24.22.0-linux-x64/bin/node", b"");
    data.file(
        "agents/runtime/clauseo-python-3.13.18-c1-linux-x64/keep",
        b"",
    );
    // `old`'s receipt names the current archive, but the current folder is there: it goes.
    remove_stale_runtimes(&data.0, Some(&archive)).unwrap();
    assert_eq!(
        names(&runtime),
        [
            "clauseo-python-3.13.18-c1-linux-x64",
            "node-v24.22.0-linux-x64"
        ]
    );
    assert!(runtime.join(archive.stem()).join("keep").is_file());
    // With no Python for the platform, every Python folder goes.
    remove_stale_runtimes(&data.0, None).unwrap();
    assert_eq!(names(&runtime), ["node-v24.22.0-linux-x64"]);
}

/// The names in `dir`, sorted.
#[cfg(unix)]
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A copy of the current runtime renamed away, with the current folder gone, is moved back
/// (and then judged by the whole-check); a renamed copy of an old pin goes.
#[cfg(unix)]
#[test]
fn a_renamed_copy_of_the_current_runtime_moves_back_and_an_old_pin_s_goes() {
    let data = TestDir::new("python-move-back");
    let runtime = data.0.join("agents/runtime");
    let (path, entry, archive) = stage(&data, &fake_archive("echo 3.13.18"));
    let copy = runtime.join("clauseo-python-3.13.18-c1-linux-x64 (2)");
    install(&path, &entry, &archive, &copy, &launcher()).unwrap();
    let old = data.file(
        "agents/runtime/clauseo-python-3.13.17-c4-linux-x64-moved/clauseo-python.json",
        br#"{"schema":1,"version":"3.13.17","archive":"clauseo-python-3.13.17-c4-linux-x64.tar.gz",
"sha256":"","manifest_sha256":""}"#,
    );
    assert!(read_receipt(old.parent().unwrap()).is_some());
    remove_stale_runtimes(&data.0, Some(&archive)).unwrap();
    assert_eq!(names(&runtime), [archive.stem()]);
    let dir = runtime_dir(&data.0, &archive);
    assert!(python_runtime_valid(&dir, &archive, Os::Linux));
    assert!(contents_match(&dir));

    // Moved back with a file gone: not valid, so Python shows as missing and installs again.
    let copy_gone = runtime.join("clauseo-python-3.13.18-c1-linux-x64-old");
    write::rename(&dir, &copy_gone).unwrap();
    write::make_writable(&copy_gone).unwrap();
    write::remove_file(&copy_gone.join("lib/python3.13/os.py")).unwrap();
    remove_stale_runtimes(&data.0, Some(&archive)).unwrap();
    assert_eq!(names(&runtime), [archive.stem()]);
    assert!(!python_runtime_valid(&dir, &archive, Os::Linux));
}

#[test]
fn no_python_for_the_platform_means_unavailable_and_no_command_folder() {
    let data = TestDir::new("python-none");
    // Linux has no Python archive (macOS and Windows do, since 3.13.18-c4 is pinned).
    let config = AgentsConfig::new(
        data.0.clone(),
        crate::platform::Platform::new(Os::Linux, Arch::X64),
    );
    assert_eq!(resting_state(&config), PythonState::Unavailable);
    assert!(!needed(&config));
    assert_eq!(python_command_dir(&config), None);
}

#[cfg(unix)]
#[test]
fn the_session_path_dirs_come_only_from_a_runtime_that_passes_its_receipt_check() {
    let data = TestDir::new("python-path-dirs");
    let (path, entry, archive) = stage(&data, &fake_archive("echo 3.13.18"));
    let mut config = AgentsConfig::new(
        data.0.clone(),
        crate::platform::Platform::new(Os::Linux, Arch::X64),
    );
    config.snapshot = config.snapshot.clone().with_python(entry.clone());
    assert_eq!(
        python_path_dirs(&config),
        Vec::<PathBuf>::new(),
        "none installed"
    );
    let dir = runtime_dir(&data.0, &archive);
    install(&path, &entry, &archive, &dir, &launcher()).unwrap();
    assert_eq!(python_path_dirs(&config), [dir.join("bin")]);
    // No manifest: the receipt check fails, and so nothing goes on the PATH.
    write::make_writable(&dir).unwrap();
    write::remove_file(&dir.join(PYTHON_MANIFEST)).unwrap();
    assert_eq!(python_path_dirs(&config), Vec::<PathBuf>::new(), "invalid");
}

#[test]
fn the_session_path_dirs_per_os_are_those_that_exist() {
    let data = TestDir::new("python-path-layout");
    let dir = data.0.join("rt");
    // Nothing there yet: no folder on either OS.
    assert_eq!(runtime_path_dirs(&dir, Os::Darwin), Vec::<PathBuf>::new());
    assert_eq!(runtime_path_dirs(&dir, Os::Win32), Vec::<PathBuf>::new());
    data.file("rt/bin/python3", b"");
    data.file("rt/clauseo-bin/pandoc.exe", b"");
    data.file("rt/python.exe", b"");
    assert_eq!(runtime_path_dirs(&dir, Os::Darwin), [dir.join("bin")]);
    assert_eq!(runtime_path_dirs(&dir, Os::Linux), [dir.join("bin")]);
    // Windows: pandoc's folder, then the interpreter's; no `Scripts`, so none.
    assert_eq!(
        runtime_path_dirs(&dir, Os::Win32),
        [dir.join("clauseo-bin"), dir.clone()]
    );
    data.file("rt/Scripts/pip.exe", b"");
    assert_eq!(
        runtime_path_dirs(&dir, Os::Win32),
        [dir.join("clauseo-bin"), dir.clone(), dir.join("Scripts")]
    );
    // A file where a folder should be is no folder.
    data.file("rt-file/bin", b"");
    assert_eq!(
        runtime_path_dirs(&data.0.join("rt-file"), Os::Darwin),
        Vec::<PathBuf>::new()
    );
}

/// Desktop's setup thread keeps a config in a `Send + Sync` lookup for [`python_path_dirs`].
#[test]
fn a_config_is_send_and_sync_for_the_path_lookup() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<AgentsConfig>();
}

/// What a test's subscriber wrote.
#[derive(Clone, Default)]
struct Written(Arc<Mutex<Vec<u8>>>);

impl io::Write for Written {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Whether [`open_checked`] accepts the archive at `path`, and the lines it logged on the
/// lifecycle target.
fn checked_logs(path: &Path, archive: &PythonArchive) -> (bool, Vec<String>) {
    let written = Written::default();
    let writer = written.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let ok = tracing::subscriber::with_default(subscriber, || open_checked(path, archive).is_ok());
    let bytes = written
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let text = String::from_utf8(bytes).unwrap();
    let lines = text
        .lines()
        .filter(|line| line.contains(LIFECYCLE))
        .map(str::to_owned)
        .collect();
    (ok, lines)
}

/// The archive check logs its verdict at INFO for a match and WARN for a mismatch: the name, the
/// first 12 hex digits of the pinned SHA-256, never the whole digest nor the file's digest.
#[test]
fn the_archive_check_logs_its_verdict_with_twelve_digits_only() {
    use sha2::Digest as _;
    let data = TestDir::new("python-verdict");
    let bytes = fake_archive("echo 3.13.18");
    let (path, _entry, archive) = stage(&data, &bytes);
    let digest = archive.sha256_hex();
    let short = digest.get(..12).unwrap_or_default();

    let (ok, lines) = checked_logs(&path, &archive);
    assert!(ok);
    assert_eq!(lines.len(), 1, "{lines:#?}");
    let line = &lines[0];
    assert!(line.contains("INFO"), "{line}");
    for field in [
        format!("archive=\"{}\"", archive.name()),
        format!("expected=\"{short}\""),
        "verdict=\"match\"".to_owned(),
    ] {
        assert!(line.contains(&field), "{field} in {line}");
    }
    assert!(!line.contains(&digest), "{line}");

    // The same size with other bytes: refused, deleted, and the line at WARN.
    let mut other = bytes;
    if let Some(last) = other.last_mut() {
        *last ^= 0xff;
    }
    let actual = crate::integrity::to_hex(&sha2::Sha256::digest(&other));
    write::write_atomic(&path, &other).unwrap();
    let (ok, lines) = checked_logs(&path, &archive);
    assert!(!ok);
    assert!(!path.exists(), "the archive is deleted");
    assert_eq!(lines.len(), 1, "{lines:#?}");
    let line = &lines[0];
    assert!(line.contains("WARN"), "{line}");
    for field in [
        format!("archive=\"{}\"", archive.name()),
        format!("expected=\"{short}\""),
        "verdict=\"mismatch\"".to_owned(),
    ] {
        assert!(line.contains(&field), "{field} in {line}");
    }
    assert!(!line.contains(&digest) && !line.contains(&actual), "{line}");
}

/// The tessdata folder is there only for a runtime that passes its receipt check and has the
/// folder: `bin/tessdata` on macOS and Linux, `clauseo-bin/tessdata` on Windows.
#[cfg(unix)]
#[test]
fn the_tessdata_folder_is_given_only_for_a_valid_runtime_that_has_it() {
    let data = TestDir::new("python-tessdata");
    let (path, entry, archive) = stage(&data, &fake_archive("echo 3.13.18"));
    let mut config = AgentsConfig::new(
        data.0.clone(),
        crate::platform::Platform::new(Os::Linux, Arch::X64),
    );
    config.snapshot = config.snapshot.clone().with_python(entry.clone());
    assert_eq!(python_tessdata_dir(&config), None, "none installed");
    let dir = runtime_dir(&data.0, &archive);
    install(&path, &entry, &archive, &dir, &launcher()).unwrap();
    assert_eq!(python_tessdata_dir(&config), None, "installed, no folder");
    write::make_writable(&dir).unwrap();
    write::create_dir_all(&dir.join("bin/tessdata")).unwrap();
    assert_eq!(python_tessdata_dir(&config), Some(dir.join("bin/tessdata")));
    write::make_writable(&dir).unwrap();
    write::remove_file(&dir.join(PYTHON_MANIFEST)).unwrap();
    assert_eq!(python_tessdata_dir(&config), None, "invalid receipt");
}

/// The tessdata folder is beside tesseract: `bin/tessdata` on macOS, `clauseo-bin\tessdata` on
/// Windows, and the same folder the PATH entry's `bin` or `clauseo-bin` is.
#[test]
fn the_tessdata_folder_sits_beside_tesseract_on_each_os() {
    let dir = PathBuf::from("/data/agents/runtime/clauseo-python-3.13.18-c4-macos-arm64");
    assert_eq!(
        runtime_tessdata_dir(&dir, Os::Darwin),
        dir.join("bin").join("tessdata")
    );
    assert_eq!(
        runtime_tessdata_dir(&dir, Os::Darwin),
        runtime_command_dir(&dir, Os::Darwin).join("tessdata")
    );
    assert_eq!(
        runtime_tessdata_dir(&dir, Os::Win32),
        dir.join("clauseo-bin").join("tessdata")
    );
    assert_eq!(
        runtime_tessdata_dir(&dir, Os::Win32),
        runtime_command_dir(&dir, Os::Win32).join("tessdata")
    );
}
