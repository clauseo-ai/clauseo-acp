use std::path::Path;

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

#[cfg(unix)]
#[test]
fn a_changed_file_fails_the_contents_check_and_a_missing_manifest_the_receipt() {
    let data = TestDir::new("python-contents");
    let (path, entry, archive) = stage(&data, &fake_archive("echo 3.13.18"));
    let dir = runtime_dir(&data.0, &archive);
    install(&path, &entry, &archive, &dir, &launcher()).unwrap();
    write::make_writable(&dir).unwrap();
    let os_py = dir.join("lib/python3.13/os.py");
    write::set_mode(&os_py, 0o644).unwrap();
    write::write_atomic(&os_py, b"# changed\n").unwrap();
    assert!(python_runtime_valid(&dir, &archive, Os::Linux));
    assert!(!contents_match(&dir), "a changed file");
    write::remove_file(&dir.join(PYTHON_MANIFEST)).unwrap();
    assert!(
        !python_runtime_valid(&dir, &archive, Os::Linux),
        "no manifest"
    );
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
    remove_stale_runtimes(&data.0, Some("clauseo-python-3.13.18-c1-linux-x64")).unwrap();
    let mut names: Vec<String> = std::fs::read_dir(&runtime)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "clauseo-python-3.13.18-c1-linux-x64",
            "node-v24.22.0-linux-x64"
        ]
    );
    // With no Python for the platform, every Python folder goes.
    remove_stale_runtimes(&data.0, None).unwrap();
    let names: Vec<String> = std::fs::read_dir(&runtime)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["node-v24.22.0-linux-x64"]);
}

#[test]
fn no_python_for_the_platform_means_unavailable_and_no_command_folder() {
    let data = TestDir::new("python-none");
    let config = AgentsConfig::new(
        data.0.clone(),
        crate::platform::Platform::new(Os::Darwin, Arch::Arm64),
    );
    assert_eq!(resting_state(&config), PythonState::Unavailable);
    assert!(!needed(&config));
    assert_eq!(python_command_dir(&config), None);
}
