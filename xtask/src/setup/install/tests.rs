//! Tests for `install.rs`: downloads checked against their pinned sha256 on a scratch bin, through
//! an injected runner, and the winget and no-release paths.

use super::*;
use crate::scratch::Scratch;
#[cfg(unix)]
use crate::setup::AssetSha256;
use crate::setup::BINARIES;
#[cfg(unix)]
use crate::setup::tests::fake_tool;
use crate::setup::tests::{
    ASSET_BYTES, binary, curl_output, exited, expected_asset, expected_curl, expected_url,
    expected_winget, linux,
};
use std::path::PathBuf;

/// The sha256 of `SWAPPED`, an asset that is not the pinned one.
const SWAPPED: &[u8] = b"a swapped asset";
const SWAPPED_SHA256: &str = "48e9f8d5aa337b952104b96fdaac9b42a1efb93b62d75e69967d48d9ea0a62de";

/// The files in `dir` with their bytes, sorted by name.
fn files_in(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let bytes = fs::read(entry.path()).unwrap_or_default();
            (name, bytes)
        })
        .collect();
    files.sort();
    files
}

/// Installs `b` on `machine` with a runner where curl writes `asset_bytes`, tar unpacks
/// `ASSET_BYTES` as the binary, and every other command ends as `other` says. Returns whether it
/// installed, what ran with its directory, and what was printed.
fn install_with(
    b: &Binary,
    machine: &Machine<'_>,
    asset_bytes: &[u8],
    other: &mut dyn FnMut(&Cmd) -> io::Result<Ended>,
) -> (bool, Vec<(String, PathBuf)>, String) {
    let mut ran = Vec::new();
    let mut runner = |cmd: &Cmd, at: &Path, _: Output| -> io::Result<Ended> {
        ran.push((cmd.to_string(), at.to_path_buf()));
        match cmd.program {
            Program::Named("curl") => Scratch::write(at.join(curl_output(cmd)), asset_bytes)?,
            Program::Named("tar") => {
                Scratch::write(at.join(cmd.args.last().unwrap()), ASSET_BYTES)?;
            }
            _ => return other(cmd),
        }
        exited(true, "", "")
    };
    let mut out = Printer::new(Vec::new(), false);
    let ok = install_binary(&mut out, b, machine, Path::new("."), &mut runner);
    (ok, ran, String::from_utf8(out.into_inner()).unwrap())
}

fn never(cmd: &Cmd) -> io::Result<Ended> {
    panic!("{cmd} was not expected to run")
}

#[test]
fn a_download_whose_sha256_differs_leaves_bin_untouched() {
    let dir = Scratch::new("setup-install-mismatch");
    let bin = dir.join("bin");
    Scratch::create_dir(&bin).unwrap();
    Scratch::write(bin.join("lefthook"), b"the working lefthook").unwrap();
    let before = files_in(&bin);
    let lefthook = binary("lefthook");
    let (ok, ran, printed) = install_with(lefthook, &linux(&bin, true), SWAPPED, &mut never);
    assert!(!ok);
    assert_eq!(ran.len(), 1, "only the download ran: {ran:?}");
    assert_eq!(files_in(&bin), before);
    let url = expected_url(lefthook, &expected_asset(lefthook, "linux", "x86_64"));
    let pin = lefthook
        .sha256
        .iter()
        .find(|p| (p.os, p.arch) == ("linux", "x86_64"))
        .unwrap()
        .sha256;
    assert_eq!(
        printed,
        format!(
            "installing lefthook {}: downloading {url}\n\
             {url} has sha256 {SWAPPED_SHA256}, but the pin is {pin}, so lefthook is not \
             installed\n    fix: run cargo xtask setup --binaries again. If the sha256 still \
             differs, the file at that URL is not the one the pin was taken from: do not install \
             it by hand\n",
            lefthook.version
        )
    );
}

/// The fix line of a failed curl download of `asset` from `url`.
fn curl_fix(asset: &str, url: &str) -> String {
    format!(
        "    fix: run `{}` by hand in an empty directory and check its output, then run cargo \
         xtask setup --binaries again; setup needs curl 7.71 or newer, for --retry-all-errors",
        expected_curl(asset, url)
    )
}

#[test]
fn a_failed_download_leaves_bin_untouched_and_names_curl_as_the_fix() {
    let dir = Scratch::new("setup-install-curl-fails");
    let bin = dir.join("bin");
    let gitleaks = binary("gitleaks");
    let asset = expected_asset(gitleaks, "linux", "x86_64");
    let url = expected_url(gitleaks, &asset);
    let fix = curl_fix(&asset, &url);
    let cases: [(&str, io::Result<Ended>); 4] = [
        (
            "failed: curl: (22) The requested URL returned error: 404",
            exited(
                false,
                "",
                "curl: (22) The requested URL returned error: 404\n\
                 curl: (22) The requested URL returned error: 404\n",
            ),
        ),
        (
            "failed: curl: (28) Operation timed out after 300002 milliseconds with 0 bytes \
             received",
            exited(
                false,
                "",
                "curl: (28) Operation timed out after 300001 milliseconds with 0 bytes received\n\
                 curl: (28) Operation timed out after 300002 milliseconds with 0 bytes received\n",
            ),
        ),
        (
            "did not finish within 1260 s, so it was stopped",
            Ok(Ended::TimedOut),
        ),
        (
            "failed: could not run curl: No such file or directory",
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "No such file or directory",
            )),
        ),
    ];
    for (said, ended) in cases {
        let mut ended = Some(ended);
        let mut runner = |_: &Cmd, _: &Path, _: Output| ended.take().unwrap();
        let mut out = Printer::new(Vec::new(), false);
        let ok = install_binary(&mut out, gitleaks, &linux(&bin, true), &dir, &mut runner);
        assert!(!ok);
        assert_eq!(files_in(&bin), [], "{said}");
        assert_eq!(
            String::from_utf8(out.into_inner()).unwrap(),
            format!(
                "installing gitleaks {}: downloading {url}\ndownloading {url} {said}\n{fix}\n",
                gitleaks.version
            )
        );
    }
}

/// What `install_binary` prints for gitleaks when its curl fails and writes `stderr`.
fn printed_for_failed_curl(case: &str, stderr: &str) -> String {
    let dir = Scratch::new(&format!("setup-install-curl-{case}"));
    let bin = dir.join("bin");
    let gitleaks = binary("gitleaks");
    let mut runner = |_: &Cmd, _: &Path, _: Output| exited(false, "", stderr);
    let mut out = Printer::new(Vec::new(), false);
    let ok = install_binary(&mut out, gitleaks, &linux(&bin, true), &dir, &mut runner);
    assert!(!ok);
    assert_eq!(files_in(&bin), []);
    String::from_utf8(out.into_inner()).unwrap()
}

#[test]
fn a_retried_curl_says_only_the_last_attempt_that_ended_the_run() {
    let gitleaks = binary("gitleaks");
    let asset = expected_asset(gitleaks, "linux", "x86_64");
    let url = expected_url(gitleaks, &asset);
    let last = "curl: (22) The requested URL returned error: 502";
    let stderr = format!(
        "curl: (56) Recv failure: Connection reset by peer\n\
         curl: (56) Recv failure: Connection reset by peer\n\
         some line that is not an attempt\n\
         {last}\n\
         a line after the last attempt\n"
    );
    assert_eq!(
        printed_for_failed_curl("retried", &stderr),
        format!(
            "installing gitleaks {}: downloading {url}\ndownloading {url} failed: {last}\n{}\n",
            gitleaks.version,
            curl_fix(&asset, &url)
        )
    );
}

#[test]
fn an_old_curl_that_rejects_retry_all_errors_says_both_lines() {
    let gitleaks = binary("gitleaks");
    let asset = expected_asset(gitleaks, "linux", "x86_64");
    let url = expected_url(gitleaks, &asset);
    let said = "curl: option --retry-all-errors: is unknown\n\
                curl: try 'curl --help' or 'curl --manual' for more information";
    assert_eq!(
        printed_for_failed_curl("old", &format!("{said}\n")),
        format!(
            "installing gitleaks {}: downloading {url}\ndownloading {url} failed: {said}\n{}\n",
            gitleaks.version,
            curl_fix(&asset, &url)
        )
    );
}

#[test]
fn a_failed_winget_names_it_as_the_fix() {
    let lefthook = binary("lefthook");
    let winget = expected_winget(lefthook);
    let machine = Machine {
        os: "windows",
        ..linux(Path::new("unused"), true)
    };
    let (ok, ran, printed) = install_with(lefthook, &machine, b"", &mut |_| exited(false, "", ""));
    assert!(!ok);
    assert_eq!(ran, [(winget.clone(), PathBuf::from("."))]);
    assert_eq!(
        printed,
        format!(
            "installing lefthook {}: {winget}\ninstalling lefthook failed\n    fix: run \
             `{winget}` by hand and check its output, then run cargo xtask setup again\n",
            lefthook.version
        )
    );
}

#[test]
fn a_platform_with_no_release_runs_nothing_and_fails_with_the_download_text() {
    for b in BINARIES {
        let machine = Machine {
            os: "freebsd",
            ..linux(Path::new("unused"), true)
        };
        let (ok, ran, printed) = install_with(b, &machine, b"", &mut never);
        assert!(!ok);
        assert_eq!(ran, []);
        assert_eq!(
            printed,
            format!(
                "{} has no release for freebsd-x86_64, so it is not installed\n    fix: download \
                 v{} for freebsd-x86_64 from https://github.com/{}/releases and put it on PATH\n",
                b.name, b.version, b.repo
            )
        );
    }
}

#[test]
fn without_a_cargo_bin_nothing_is_downloaded() {
    let machine = Machine {
        cargo_bin: None,
        ..linux(Path::new("unused"), true)
    };
    let (ok, ran, printed) = install_with(binary("gitleaks"), &machine, b"", &mut never);
    assert!(!ok);
    assert_eq!(ran, []);
    assert_eq!(
        printed,
        "gitleaks is not installed: neither CARGO_HOME nor the home directory is set, so there is \
         no cargo bin directory to put it in\n    fix: set CARGO_HOME to cargo's home directory, \
         then run cargo xtask setup --binaries again\n"
    );
}

#[test]
fn sha256_of_hashes_the_file_bytes() {
    let dir = Scratch::new("setup-install-sha");
    let file = dir.join("asset");
    Scratch::write(&file, SWAPPED).unwrap();
    assert_eq!(sha256_of(&file).unwrap(), SWAPPED_SHA256);
}

/// Makes `<member>.tar.gz` in `dir` with the system tar, from a `LICENSE` and a `gitleaks` that is
/// a file holding `ASSET_BYTES` ("file"), a symlink to `LICENSE` ("link") or a directory ("dir").
/// Returns its path.
#[cfg(unix)]
fn real_tar_gz(dir: &Path, member: &str) -> PathBuf {
    let src = dir.join(format!("{member}-src"));
    Scratch::create_dir_all(&src).unwrap();
    Scratch::write(src.join("LICENSE"), b"the license").unwrap();
    match member {
        "file" => Scratch::write(src.join("gitleaks"), ASSET_BYTES).unwrap(),
        "link" => Scratch::symlink("LICENSE", src.join("gitleaks")).unwrap(),
        _ => Scratch::create_dir(src.join("gitleaks")).unwrap(),
    }
    let name = member;
    let archive = dir.join(format!("{name}.tar.gz"));
    let archive_arg = archive.display().to_string();
    let tar = Cmd::new(
        Program::Named("tar"),
        ["-czf", archive_arg.as_str(), "gitleaks", "LICENSE"],
        crate::process::TEST_TIMEOUT,
    );
    let made = crate::process::run_quiet(&tar, &src).expect("tar runs");
    assert!(made.success, "{tar}: {}", made.stderr);
    archive
}

/// Installs a tool named gitleaks whose asset is `archive`, pinned to its real sha256, with the
/// download faked as a copy of `archive` and the extract run by the real `tar`.
#[cfg(unix)]
fn install_real_tar_gz(archive: &Path, bin: &Path) -> (bool, String) {
    let pin: &'static str = Box::leak(sha256_of(archive).unwrap().into_boxed_str());
    let pins: &'static [AssetSha256] = Box::leak(Box::new([AssetSha256 {
        os: "linux",
        arch: "x86_64",
        sha256: pin,
    }]));
    let tool = Binary {
        name: "gitleaks",
        sha256: pins,
        ..fake_tool(Packing::TarGz)
    };
    let mut runner = |cmd: &Cmd, at: &Path, output: Output| match cmd.program {
        Program::Named("curl") => {
            Scratch::copy(archive, at.join(curl_output(cmd)))?;
            exited(true, "", "")
        }
        Program::Named("tar") => crate::process::run(cmd, at, output),
        _ => panic!("{cmd} was not expected to run"),
    };
    let mut out = Printer::new(Vec::new(), false);
    let ok = install_binary(
        &mut out,
        &tool,
        &linux(bin, true),
        Path::new("."),
        &mut runner,
    );
    (ok, String::from_utf8(out.into_inner()).unwrap())
}

#[cfg(unix)]
#[test]
fn the_real_tar_extract_replaces_gitleaks_in_bin_from_a_release_like_archive() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("setup-install-real-tar");
    let archive = real_tar_gz(&dir, "file");
    let bin = dir.join("bin");
    Scratch::create_dir(&bin).unwrap();
    Scratch::write(bin.join("gitleaks"), b"an old gitleaks").unwrap();
    let (ok, printed) = install_real_tar_gz(&archive, &bin);
    assert!(ok, "{printed}");
    assert_eq!(
        files_in(&bin),
        [("gitleaks".to_owned(), ASSET_BYTES.to_vec())]
    );
    let mode = fs::metadata(bin.join("gitleaks"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);
}

#[cfg(unix)]
#[test]
fn the_real_tar_extract_refuses_a_gitleaks_member_that_is_a_symlink_or_a_directory() {
    for member in ["link", "dir"] {
        let dir = Scratch::new(&format!("setup-install-real-tar-{member}"));
        let archive = real_tar_gz(&dir, member);
        let bin = dir.join("bin");
        Scratch::create_dir(&bin).unwrap();
        Scratch::write(bin.join("gitleaks"), b"the working gitleaks").unwrap();
        let before = files_in(&bin);
        let (ok, printed) = install_real_tar_gz(&archive, &bin);
        assert!(!ok, "{member}");
        assert_eq!(files_in(&bin), before, "{member}");
        refused(&printed);
    }
}

/// Asserts that `printed` ends with the failure for an asset with no regular file named gitleaks.
#[cfg(unix)]
fn refused(printed: &str) {
    let asset = "faketool_fake-version_linux_x86_64";
    let url =
        format!("https://github.com/example/faketool/releases/download/vfake-version/{asset}");
    assert!(
        printed.ends_with(&format!(
            "{asset} holds no regular file named gitleaks, so gitleaks is not installed\n    fix: \
             the release at {url} holds no regular file named gitleaks: check the asset layout \
             before bumping the pin\n"
        )),
        "{printed}"
    );
}

#[test]
fn the_download_commands_and_curl_stopping_itself_before_its_limit_does() {
    let (asset, url) = ("a.tar.gz", "https://example.com/a.tar.gz");
    assert_eq!(curl_cmd(asset, url).to_string(), expected_curl(asset, url));
    assert_eq!(
        tar_cmd(asset, "gitleaks").to_string(),
        "tar --no-same-owner -xzf a.tar.gz gitleaks"
    );
    // The last retry starts within 900 s and runs at most 300 s.
    assert!(DOWNLOAD_TIMEOUT > Duration::from_mins(15 + 5));
    assert_eq!(DOWNLOAD_TIMEOUT, Duration::from_mins(21));
}

#[cfg(unix)]
#[test]
fn a_run_lock_the_user_cannot_write_is_still_taken_without_a_note() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("setup-install-read-only-lock");
    let bin = dir.join("bin");
    Scratch::create_dir(&bin).unwrap();
    let lock_path = bin.join(".xtask-setup.lock");
    Scratch::write(&lock_path, b"left by a sudo run").unwrap();
    Scratch::set_permissions(&lock_path, fs::Permissions::from_mode(0o444)).unwrap();
    assert!(
        Scratch::open_for_writing(&lock_path).is_err(),
        "the lock file is not writable"
    );
    let mut out = Printer::new(Vec::new(), false);
    let (held, ok) = lock_bin_and_sweep(&mut out, &linux(&bin, true));
    assert!(ok && held.is_some());
    let other = fs::File::open(&lock_path).unwrap();
    assert!(matches!(
        other.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    assert_eq!(String::from_utf8(out.into_inner()).unwrap(), "");
    assert_eq!(
        fs::read(&lock_path).unwrap(),
        b"left by a sudo run",
        "the lock file is not rewritten"
    );
}

#[test]
fn a_second_binaries_run_waits_for_the_run_lock_then_sweeps_every_staging_entry() {
    let dir = Scratch::new("setup-install-run-lock");
    let bin = dir.join("bin");
    let (held, ok) = lock_bin_and_sweep(&mut Printer::new(Vec::new(), false), &linux(&bin, true));
    assert!(ok && held.is_some());
    // The first run's staging dir, in use while it holds the lock.
    Scratch::create_dir(bin.join(".xtask-setup-gitleaks-1")).unwrap();
    let (done, second) = std::sync::mpsc::channel();
    let second_bin = bin.clone();
    std::thread::spawn(move || {
        let mut out = Printer::new(Vec::new(), false);
        let (lock, ok) = lock_bin_and_sweep(&mut out, &linux(&second_bin, true));
        assert!(ok && lock.is_some());
        done.send(String::from_utf8(out.into_inner()).unwrap())
    });
    let early = second.recv_timeout(Duration::from_millis(300));
    assert!(
        early.is_err(),
        "the second run waits while the lock is held"
    );
    drop(held);
    assert_eq!(
        second.recv().unwrap(),
        format!(
            "another cargo xtask setup --binaries is using {}: waiting for it to finish\n",
            bin.display()
        )
    );
    let names: Vec<String> = files_in(&bin).into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, [".xtask-setup.lock"]);
}
