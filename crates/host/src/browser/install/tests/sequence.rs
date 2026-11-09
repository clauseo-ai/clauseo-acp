//! The local headers walked in order (`archive/sequence.rs`), with crafted zips: a local entry
//! the central directory does not list, and a directory entry naming a local header hidden in
//! another entry's data, are both refused; a plain zip passes.

use clauseo_protocol::BrowserInstallFailure;

use super::support::{file, zip};
use crate::browser::install::{Failure, archive};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// The parts of a zip from [`zip`] (no comment): its local entries, its central directory.
fn parts(bytes: &[u8]) -> (&[u8], &[u8]) {
    let eocd = bytes.len() - 22;
    let size = u32::from_le_bytes(bytes[eocd + 12..eocd + 16].try_into().unwrap());
    let offset = u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap());
    let (offset, size) = (offset as usize, size as usize);
    (&bytes[..offset], &bytes[offset..offset + size])
}

/// Local entries, then a central directory of `entries` entries, then the end record.
fn assemble(locals: &[u8], central: &[u8], entries: u16) -> Vec<u8> {
    let mut out = locals.to_vec();
    out.extend_from_slice(central);
    out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
    out.extend_from_slice(&entries.to_le_bytes());
    out.extend_from_slice(&entries.to_le_bytes());
    out.extend_from_slice(&u32::try_from(central.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&u32::try_from(locals.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}

/// `archive::check_zip` on `bytes`.
fn check(bytes: &[u8]) -> Result<(), Failure> {
    let dir = TempDir::new("cft-sequence");
    let path = dir.path().join("t.zip");
    write::write_file(&path, bytes).unwrap();
    archive::check_zip(&path, true)
}

#[test]
fn a_plain_zip_walks_to_its_directory() {
    check(&zip(&[
        file("a", b"one"),
        file("b/c", b"two"),
        file("d", b""),
    ]))
    .unwrap();
    check(&zip(&[])).unwrap();
}

#[test]
fn a_local_entry_the_directory_does_not_list_is_refused() {
    // Both local entries, only the first one's directory entry.
    let both = zip(&[file("a", b"one"), file("evil", b"two")]);
    let one = zip(&[file("a", b"one")]);
    let (locals, _) = parts(&both);
    let (_, central) = parts(&one);
    let failure = check(&assemble(locals, central, 1)).unwrap_err();
    assert_eq!(failure.code, BrowserInstallFailure::Unpack);
    assert!(
        failure.message.contains("does not list"),
        "{}",
        failure.message
    );
}

#[test]
fn a_directory_entry_naming_a_header_hidden_in_another_entry_is_refused() {
    // `a`'s data is a whole local entry for `b`; a second directory entry points into it.
    let inner = zip(&[file("b", b"evil")]);
    let (inner_local, inner_central) = parts(&inner);
    let outer = zip(&[file("a", inner_local)]);
    let (locals, outer_central) = parts(&outer);
    let mut hidden = inner_central.to_vec();
    let at = u32::try_from(30 + "a".len()).unwrap();
    hidden[42..46].copy_from_slice(&at.to_le_bytes());
    let mut central = outer_central.to_vec();
    central.extend_from_slice(&hidden);
    let failure = check(&assemble(locals, &central, 2)).unwrap_err();
    assert_eq!(failure.code, BrowserInstallFailure::Unpack);
    assert!(
        failure.message.contains("in sequence"),
        "{}",
        failure.message
    );
}
