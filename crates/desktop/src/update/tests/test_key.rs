//! The throwaway test key kept out of every build but `e2e`, by the text of the crate's files
//! and by the bytes of this test binary ("release builds reject the test key", R23 PR 5).

use std::path::Path;

use super::{crate_dir, test_key_file};

/// Standard base64 (`tauri signer` writes its keys so), decoded. The test keeps no copy of the
/// key's text or id, so none is compiled into this test binary.
fn base64(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let (mut bits, mut held, mut out) = (0u32, 0u32, Vec::new());
    for byte in text
        .bytes()
        .filter(|byte| *byte != b'=' && !byte.is_ascii_whitespace())
    {
        let value = ALPHABET.iter().position(|letter| *letter == byte).unwrap();
        bits = (bits << 6) | u32::try_from(value).unwrap();
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push(u8::try_from(bits >> held).unwrap());
            bits &= (1 << held) - 1;
        }
    }
    out
}

/// The test key in each form it could be wired in by: the file's base64 text, the minisign key
/// line inside it (`RW...`) and the key id of its comment, all read from `updater-test.pub`.
fn test_key_forms() -> Vec<String> {
    let file = test_key_file();
    let decoded = String::from_utf8(base64(&file)).unwrap();
    let mut lines = decoded.lines();
    let comment = lines.next().unwrap();
    let id = comment.rsplit(' ').next().unwrap().to_owned();
    let key = lines.next().unwrap().trim().to_owned();
    assert_eq!(id.len(), 16, "not a minisign key id: {comment}");
    assert!(key.starts_with("RW"), "not a minisign key line");
    vec![file, key, id]
}

/// (b) of "release builds reject the test key", by text: no file under `crates/desktop` (its
/// sources, configs and release overlays; tests aside) holds the test key in any form, or names
/// its file (`updater-test`, however the path is pieced together), except the file itself and
/// `src/update/choice.rs`, the `e2e`-gated module that loads it, where the one `include_str!` of
/// it sits directly under `#[cfg(feature = "e2e")]`.
#[test]
fn only_the_key_s_file_and_the_e2e_module_name_the_test_key() {
    let mut needles = test_key_forms();
    needles.push("updater-test".to_owned());
    let allowed = [
        Path::new("release/updater-test.pub"),
        Path::new("src/update/choice.rs"),
    ];
    let root = crate_dir();
    let (mut inspected, mut naming) = (0, Vec::new());
    let mut folders = vec![root.clone()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(&root).unwrap().to_path_buf();
            let name = relative.to_string_lossy().replace('\\', "/");
            // Tests are never in a release build; `target` and `node_modules` are build output.
            if name.split('/').any(|part| {
                part == "tests" || part == "tests.rs" || part == "target" || part == "node_modules"
            }) {
                continue;
            }
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            inspected += 1;
            let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
            if needles.iter().any(|needle| text.contains(needle.as_str()))
                && !allowed.contains(&relative.as_path())
            {
                naming.push(name);
            }
        }
    }
    assert!(inspected > 100, "only {inspected} files inspected");
    assert_eq!(naming, Vec::<String>::new(), "files naming the test key");
    let choice = std::fs::read_to_string(root.join("src/update/choice.rs")).unwrap();
    let lines: Vec<&str> = choice.lines().collect();
    let includes: Vec<usize> = (0..lines.len())
        .filter(|at| lines[*at].contains("updater-test") && !lines[*at].trim().starts_with("//"))
        .collect();
    assert_eq!(includes.len(), 1, "choice.rs names the key's file once");
    let at = includes[0];
    assert!(lines[at].contains("include_str!"), "{}", lines[at]);
    assert_eq!(lines[at - 1].trim(), r#"#[cfg(feature = "e2e")]"#);
}

/// (b) by the bytes built: this test binary, without `e2e`, holds the test key in no form, so
/// no `include_bytes!`, `concat!` path or pasted key put it in the crate.
#[cfg(not(feature = "e2e"))]
#[test]
fn a_build_without_e2e_holds_no_byte_of_the_test_key() {
    let binary = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    for form in test_key_forms() {
        assert_eq!(
            memchr::memmem::find(&binary, form.as_bytes()),
            None,
            "the test binary holds the test key"
        );
    }
}

/// The scan above finds the key where it is: the `e2e` build holds it.
#[cfg(feature = "e2e")]
#[test]
fn the_e2e_build_holds_the_test_key() {
    let binary = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    let file = test_key_forms().remove(0);
    assert!(memchr::memmem::find(&binary, file.as_bytes()).is_some());
}
