//! The settings canary's read-back (`canary.rs`): exactly the nonce passes, anything else does
//! not; its nonces; and the pin that new, load and resume are every session setup the host can
//! send, all from `session.rs`, so none skips the canary (no `session/fork`).

use std::path::{Path, PathBuf};

use super::{Expect, Found, nonce, read};
use crate::pipeline::tests::support::TempDir;
use crate::write;

fn expect(file: PathBuf, nonce: &str) -> Expect {
    Expect {
        file,
        nonce: nonce.to_owned(),
    }
}

#[test]
fn exactly_the_nonce_passes_and_anything_else_does_not() {
    let dir = TempDir::new("canary-read");
    let file = dir.path().join("session-canary.abc");
    assert_eq!(read(&expect(file.clone(), "abc")), Found::Missing);
    write::write_file(&file, b"abc").unwrap();
    assert_eq!(read(&expect(file.clone(), "abc")), Found::Written);
    assert_eq!(read(&expect(file.clone(), "abz")), Found::Other);
    assert_eq!(read(&expect(file.clone(), "ab")), Found::Other);
    for other in [&b"abc\n"[..], &b""[..], &b"xabc"[..], &[b'a'; 300][..]] {
        write::write_file(&file, other).unwrap();
        assert_eq!(read(&expect(file.clone(), "abc")), Found::Other);
    }
}

#[cfg(unix)]
#[test]
fn a_link_at_the_nonce_file_is_not_followed() {
    let dir = TempDir::new("canary-link");
    let target = dir.path().join("target");
    write::write_file(&target, b"abc").unwrap();
    let file = dir.path().join("session-canary.abc");
    write::symlink(&target, &file).unwrap();
    assert_eq!(read(&expect(file, "abc")), Found::Unread);
}

#[test]
fn each_nonce_is_32_lowercase_hex_digits_and_new() {
    let (one, two) = (nonce().unwrap(), nonce().unwrap());
    for nonce in [&one, &two] {
        assert_eq!(nonce.len(), 32);
        assert!(
            nonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }
    assert_ne!(one, two);
}

/// The workspace's `crates` folder.
fn crates() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// `clauseo-acp` can send exactly three session setup calls (its `Command`s that end in
/// `Session`): new, load and resume, and nothing in it names a fork. A `ForkSession` (or any
/// other setup call) added there fails this test: it must arm and check the canary first.
#[test]
fn the_only_session_setup_calls_are_new_load_and_resume() {
    let source = std::fs::read_to_string(crates().join("acp/src/client.rs")).unwrap();
    let start = source.find("pub(crate) enum Command {").unwrap();
    let end = start + source.get(start..).unwrap().find("\n}\n").unwrap();
    let setups: Vec<&str> = source
        .get(start..end)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let name = line.strip_prefix("    ")?;
            let name = name.split([' ', '(', ',']).next()?;
            (name.ends_with("Session") && name.starts_with(|c: char| c.is_ascii_uppercase()))
                .then_some(name)
        })
        .collect();
    assert_eq!(setups, ["NewSession", "LoadSession", "ResumeSession"]);
    for file in ["acp/src/client.rs", "acp/src/driver.rs"] {
        let text = std::fs::read_to_string(crates().join(file)).unwrap();
        assert!(!text.to_lowercase().contains("fork"), "{file} names a fork");
    }
}

/// Every `.rs` file under `folder`.
fn rust_files(folder: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// The host sends each setup call from `actor/turn/session.rs` alone, where each carries the
/// canary, and from nowhere else.
#[test]
fn every_setup_call_goes_from_session_rs() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let session = src.join("actor").join("turn").join("session.rs");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    // Built here, so this file holds none of the calls it counts.
    let calls = ["new", "load", "resume"].map(|kind| format!(".{kind}_session("));
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for call in &calls {
            let count = text.matches(call.as_str()).count();
            if file == session {
                assert_eq!(count, 1, "{call} in session.rs");
            } else {
                assert_eq!(count, 0, "{call} in {}", file.display());
            }
        }
    }
}
