//! `CLAUSEO_PROXY_SECRET_FILE`: the proxy's password, read from its file once for the guard
//! alone and the file deleted; the variable is never in a child's map.

use std::path::{Path, PathBuf};

use super::super::{Os, WindowsDirs, build, chrome_env, proxy_secret};
use super::{abs, source_for};
use crate::browser_guard::proxy_auth::Password;
use crate::write;

/// The variable.
pub(super) const VAR: &str = "CLAUSEO_PROXY_SECRET_FILE";
/// The refusal of a file the one open could not open.
#[cfg(unix)]
const UNREADABLE: &str =
    "CLAUSEO_PROXY_SECRET_FILE names a file the guard cannot open to read and empty";
/// An obviously fake password: 64 of `0-9 a-f`.
pub(super) const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A folder of the test's own under the temp folder, removed when dropped.
pub(super) struct Scratch(pub(super) PathBuf);

impl Scratch {
    pub(super) fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "clauseo-mcp-proxy-secret-{test}-{}",
            std::process::id()
        ));
        drop(write::remove_dir_all(&path));
        write::create_dir(&path).unwrap();
        Self(path)
    }

    /// A file `name` in it holding `content`; its path.
    pub(super) fn file(&self, name: &str, content: &[u8]) -> String {
        let path = self.0.join(name);
        write::write(&path, content).unwrap();
        path.to_str().unwrap().to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.0));
    }
}

/// The error for the file at `file`, as text, with the proxy set.
pub(super) fn refusal(os: Os, file: &str) -> String {
    proxy_secret(&source_for(os, &[(VAR, file)], ""), os)
        .unwrap_err()
        .to_string()
}

/// The password is read with or without its one newline, under either family's rules, and its
/// file is gone once it is read.
#[test]
fn the_secret_is_read_from_its_file_and_the_file_deleted() {
    let scratch = Scratch::new("read");
    for os in [Os::Unix, Os::Windows] {
        for (name, content) in [
            ("plain", SECRET.to_owned()),
            ("newline", format!("{SECRET}\n")),
        ] {
            let file = scratch.file(name, content.as_bytes());
            let read = proxy_secret(&source_for(os, &[(VAR, file.as_str())], ""), os);
            assert_eq!(read, Ok(Password::parse(SECRET)), "{os:?} {name}");
            assert!(!Path::new(&file).exists(), "{os:?} {name}: not deleted");
        }
        // Neither the variable nor the proxy: no proxy at all.
        assert_eq!(
            proxy_secret(&source_for(os, &[], "AGENT_BROWSER_PROXY"), os),
            Ok(None),
            "{os:?}"
        );
    }
    // Windows names have no case.
    let file = scratch.file("lower", SECRET.as_bytes());
    let lower = source_for(
        Os::Windows,
        &[("clauseo_proxy_secret_file", file.as_str())],
        "",
    );
    assert_eq!(
        proxy_secret(&lower, Os::Windows),
        Ok(Password::parse(SECRET))
    );
    assert!(!Path::new(&file).exists());
}

/// A file that does not hold exactly 64 of `0-9 a-f` and at most one newline is refused, and
/// deleted all the same; the message names the variable and never the content.
#[test]
fn a_bad_file_is_refused_without_its_content_and_still_deleted() {
    let scratch = Scratch::new("bad");
    let short = &SECRET[1..];
    let long = format!("{SECRET}0");
    let upper = SECRET.to_ascii_uppercase();
    let spaced = format!(" {short}");
    let newlines = format!("{SECRET}\n\n");
    let crlf = format!("{SECRET}\r\n");
    let huge = SECRET.repeat(64);
    let bad: [&[u8]; 9] = [
        b"",
        short.as_bytes(),
        long.as_bytes(),
        upper.as_bytes(),
        spaced.as_bytes(),
        newlines.as_bytes(),
        crlf.as_bytes(),
        huge.as_bytes(),
        b"\xff\xfe not text",
    ];
    for (n, content) in bad.into_iter().enumerate() {
        let file = scratch.file(&format!("bad-{n}"), content);
        let error = refusal(Os::CURRENT, &file);
        assert!(
            error.starts_with(
                "CLAUSEO_PROXY_SECRET_FILE must name a file holding exactly 64 of 0-9 a-f"
            ),
            "{n}: {error}"
        );
        assert!(
            !error.contains(&SECRET[..16]) && !error.contains(&SECRET[1..17]),
            "{n}: {error}"
        );
        assert!(!Path::new(&file).exists(), "{n}: not deleted");
    }
}

/// A path that is not an existing local file is refused before anything reads it.
#[test]
fn a_missing_file_a_folder_or_a_relative_path_is_refused() {
    let scratch = Scratch::new("missing");
    let missing = scratch.0.join("none").to_str().unwrap().to_owned();
    let folder = scratch.0.to_str().unwrap().to_owned();
    for bad in [missing.as_str(), folder.as_str(), "proxy-secret", ""] {
        let error = refusal(Os::CURRENT, bad);
        assert!(
            error.starts_with("CLAUSEO_PROXY_SECRET_FILE must be the absolute path of an existing"),
            "{bad}: {error}"
        );
    }
    // Windows: a network path is never touched.
    let error = refusal(Os::Windows, r"\\host\share\proxy-secret");
    assert!(error.contains("not a network or device path"), "{error}");
    assert!(scratch.0.is_dir());
}

/// A file the guard may not read is refused as unreadable (unless the tests run as root, who
/// reads it anyway).
#[cfg(unix)]
#[test]
fn an_unreadable_file_is_refused() {
    let scratch = Scratch::new("unreadable");
    let file = scratch.file("locked", SECRET.as_bytes());
    write::set_mode(Path::new(&file), 0).unwrap();
    if std::fs::File::open(&file).is_ok() {
        return;
    }
    let error = refusal(Os::Unix, &file);
    assert_eq!(error, UNREADABLE);
}

/// The file without the proxy is refused, and left alone: the guard deletes nothing it has not
/// checked.
#[test]
fn a_secret_file_without_the_proxy_is_refused() {
    let scratch = Scratch::new("alone");
    let file = scratch.file("alone", SECRET.as_bytes());
    let alone = source_for(Os::Unix, &[(VAR, file.as_str())], "AGENT_BROWSER_PROXY");
    let error = proxy_secret(&alone, Os::Unix).unwrap_err().to_string();
    assert_eq!(
        error,
        "CLAUSEO_PROXY_SECRET_FILE is set without AGENT_BROWSER_PROXY, whose password it is"
    );
    assert!(Path::new(&file).is_file());
}

/// The proxy without the file is refused: every page would get the proxy's 407.
#[test]
fn the_proxy_without_its_secret_file_is_refused() {
    for os in [Os::Unix, Os::Windows] {
        let error = proxy_secret(&source_for(os, &[], ""), os)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "AGENT_BROWSER_PROXY is set without CLAUSEO_PROXY_SECRET_FILE, the file of its \
             password; the host sets both",
            "{os:?}"
        );
    }
}

/// A file whose folder the guard may not change cannot be deleted: it is emptied instead, and
/// the password is still read (unless the tests run as root, who deletes it anyway).
#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_deleted_is_emptied() {
    let scratch = Scratch::new("empty");
    let folder = scratch.0.join("locked");
    write::create_dir(&folder).unwrap();
    let file = folder.join("proxy-secret");
    write::write(&file, format!("{SECRET}\n")).unwrap();
    write::set_mode(&folder, 0o500).unwrap();
    let read = proxy_secret(
        &source_for(Os::Unix, &[(VAR, file.to_str().unwrap())], ""),
        Os::Unix,
    );
    let left = std::fs::metadata(&file).map(|metadata| metadata.len());
    write::set_mode(&folder, 0o700).unwrap();
    assert_eq!(read, Ok(Password::parse(SECRET)));
    if let Ok(len) = left {
        assert_eq!(len, 0, "the undeletable file was not emptied");
    }
}

/// A file the guard may read but not write is refused before anything reads it, and left as it
/// was: the one open asks for both (unless the tests run as root, who may do both).
#[cfg(unix)]
#[test]
fn a_file_the_guard_may_not_write_is_refused_and_left() {
    let scratch = Scratch::new("stuck");
    let folder = scratch.0.join("locked");
    write::create_dir(&folder).unwrap();
    let file = folder.join("proxy-secret");
    write::write(&file, SECRET).unwrap();
    write::set_mode(&file, 0o400).unwrap();
    let root = nix::unistd::geteuid().is_root();
    let read = proxy_secret(
        &source_for(Os::Unix, &[(VAR, file.to_str().unwrap())], ""),
        Os::Unix,
    );
    let left = std::fs::read(&file);
    if root {
        return;
    }
    assert_eq!(read.unwrap_err().to_string(), UNREADABLE);
    assert_eq!(
        left.unwrap(),
        SECRET.as_bytes(),
        "the file is left as it was"
    );
}

/// Neither agent-browser's map nor Chrome's holds the variable or the file's path, under any
/// name, and every proxy variable is the bare URL.
#[test]
fn the_secret_file_reaches_no_child() {
    for os in [Os::Unix, Os::Windows] {
        let cwd = abs("cwd");
        let file = abs("run-proxy-secret");
        let env = build(
            &source_for(os, &[(VAR, file.as_str())], ""),
            os,
            Path::new(&cwd),
            &WindowsDirs::default(),
        )
        .unwrap();
        for map in [&env, &chrome_env(&env)] {
            for (name, value) in map {
                assert!(
                    !name.to_ascii_uppercase().contains("SECRET"),
                    "{os:?}: {name}"
                );
                assert!(!name.contains("PROXY_USERNAME") && !name.contains("PROXY_PASSWORD"));
                let value = value.to_string_lossy();
                assert!(!value.contains("run-proxy-secret"), "{os:?}: {name}");
                if name.to_ascii_uppercase().ends_with("_PROXY") {
                    assert_eq!(value, "http://127.0.0.1:8080", "{os:?}: {name}");
                }
            }
        }
    }
}

#[test]
fn the_password_never_prints() {
    let password = Password::parse(SECRET).unwrap();
    assert_eq!(format!("{password:?}"), "Password(<hidden>)");
    let scratch = Scratch::new("print");
    let file = scratch.file("short", b"deadbeef");
    let error =
        proxy_secret(&source_for(Os::Unix, &[(VAR, file.as_str())], ""), Os::Unix).unwrap_err();
    assert!(!format!("{error:?}").contains("deadbeef"), "{error:?}");
    assert!(!error.to_string().contains("deadbeef"), "{error}");
}
