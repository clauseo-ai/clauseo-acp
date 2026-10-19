//! Tests for `install.rs`: downloads checked against their pinned sha256 on a scratch bin, through
//! an injected runner, and the winget and no-release paths.

use super::*;
use crate::scratch::Scratch;
use crate::setup::BINARIES;
use crate::setup::tests::{
    ASSET_BYTES, binary, curl_output, exited, expected_asset, expected_url, expected_winget,
    fake_tool, linux,
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
    let mut runner = |cmd: &Cmd, at: &Path, _: Output, _: Duration| -> io::Result<Ended> {
        ran.push((cmd.to_string(), at.to_path_buf()));
        match cmd.program {
            Program::Named("curl") => fs::write(at.join(curl_output(cmd)), asset_bytes)?,
            Program::Named("tar") => fs::write(at.join(cmd.args.last().unwrap()), ASSET_BYTES)?,
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
    fs::create_dir(&bin).unwrap();
    fs::write(bin.join("lefthook"), b"the working lefthook").unwrap();
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

#[test]
fn leftovers_of_an_interrupted_run_are_removed_and_nothing_else_in_bin() {
    let dir = Scratch::new("setup-install-leftovers");
    let bin = dir.join("bin");
    let stale = bin.join(".xtask-setup-gitleaks-99999");
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("gitleaks_unverified.tar.gz"), b"half a download").unwrap();
    fs::write(bin.join(".xtask-setup-lefthook-1"), b"a stray file").unwrap();
    fs::write(bin.join("lefthook"), b"the working lefthook").unwrap();
    fs::write(bin.join(".xtask-other"), b"not ours").unwrap();
    // The download matches no pin, so the install itself adds nothing either.
    let (ok, _, _) = install_with(binary("lefthook"), &linux(&bin, true), SWAPPED, &mut never);
    assert!(!ok);
    assert_eq!(
        files_in(&bin),
        [
            (".xtask-other".to_owned(), b"not ours".to_vec()),
            ("lefthook".to_owned(), b"the working lefthook".to_vec()),
        ]
    );
}

#[test]
fn a_matching_download_replaces_the_binary_in_bin() {
    let dir = Scratch::new("setup-install-bare");
    let bin = dir.join("bin");
    fs::create_dir(&bin).unwrap();
    fs::write(bin.join("faketool"), b"an old faketool").unwrap();
    let tool = fake_tool(Packing::Bare);
    let (ok, ran, printed) = install_with(&tool, &linux(&bin, true), ASSET_BYTES, &mut never);
    assert!(ok, "{printed}");
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert_eq!(
        files_in(&bin),
        [("faketool".to_owned(), ASSET_BYTES.to_vec())]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(bin.join("faketool"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }
    assert!(printed.ends_with(&format!(
        "installed faketool {} to {}: its sha256 matches the pin\n",
        tool.version,
        bin.display()
    )));
}

#[test]
fn a_matching_tar_gz_is_unpacked_in_staging_then_moved_into_bin() {
    let dir = Scratch::new("setup-install-tar");
    let bin = dir.join("bin");
    let tool = fake_tool(Packing::TarGz);
    let (ok, ran, printed) = install_with(&tool, &linux(&bin, true), ASSET_BYTES, &mut never);
    assert!(ok, "{printed}");
    let asset = format!("faketool_{}_linux_x86_64", tool.version);
    let staging = bin.join(format!(".xtask-setup-faketool-{}", std::process::id()));
    let ran: Vec<String> = ran
        .into_iter()
        .map(|(cmd, at)| {
            assert_eq!(at, staging, "{cmd} runs in the staging dir");
            cmd
        })
        .collect();
    assert_eq!(ran[1], format!("tar --no-same-owner -xzf {asset} faketool"));
    assert_eq!(
        files_in(&bin),
        [("faketool".to_owned(), ASSET_BYTES.to_vec())]
    );
}

#[test]
fn a_failed_download_leaves_bin_untouched_and_names_curl_as_the_fix() {
    let dir = Scratch::new("setup-install-curl-fails");
    let bin = dir.join("bin");
    let gitleaks = binary("gitleaks");
    let asset = expected_asset(gitleaks, "linux", "x86_64");
    let url = expected_url(gitleaks, &asset);
    let fix = format!(
        "    fix: run `curl -sSfL --max-time 600 -o {asset} {url}` by hand in an empty directory and check its \
         output, then run cargo xtask setup --binaries again"
    );
    let cases: [(&str, io::Result<Ended>); 3] = [
        (
            "failed: curl: (22) The requested URL returned error: 404",
            exited(
                false,
                "",
                "curl: (22) The requested URL returned error: 404\n",
            ),
        ),
        (
            "did not finish within 600 s, so it was stopped",
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
        let mut runner = |_: &Cmd, _: &Path, _: Output, _: Duration| ended.take().unwrap();
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
    fs::write(&file, SWAPPED).unwrap();
    assert_eq!(sha256_of(&file).unwrap(), SWAPPED_SHA256);
}
