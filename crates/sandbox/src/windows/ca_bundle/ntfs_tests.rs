//! The CA bundle on NTFS: the export's enumeration over memory stores (the real `Root` and
//! `Disallowed` are never written: adding to the user's `Root` raises a prompt), the file's
//! descriptor and its rewrite rule, and, elevated, that a Low restricted token of a pool user
//! may read the file and not write it.
#![expect(unsafe_code, reason = "FFI to memory certificate stores in tests")]

use std::os::windows::fs::MetadataExt as _;
use std::path::Path;

use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Security::Cryptography::{
    CERT_OPEN_STORE_FLAGS, CERT_QUERY_ENCODING_TYPE, CERT_STORE_ADD_ALWAYS, CERT_STORE_PROV_MEMORY,
    CertAddEncodedCertificateToStore, CertOpenStore, X509_ASN_ENCODING,
};

use super::store::{Store, export_from};
use super::tests::{INTERMEDIATE, OTHER, ROOT, der_of, pem_of};
use super::{ensure_with, path};
use crate::windows::private_dir::owner;
use crate::windows::record::current_user_sid;
use crate::windows::sids::Sid;
use crate::windows::test_acl::{Place, elevated_tests_enabled, entries, protected};
use crate::windows::test_pool::{Impersonating, TestUser, low_restricted_token, open};

/// `FILE_ALL_ACCESS`, as the SDDL's `FA` writes it.
const FA: u32 = 0x001F_01FF;
/// `FILE_GENERIC_READ`, as the SDDL's `FR` writes it.
const FR: u32 = 0x0012_0089;

/// A memory store holding the certificates `pems`.
fn memory_store(pems: &[&str]) -> Store {
    // SAFETY: the memory provider takes no parameter; the store is closed by `Store`'s drop.
    let raw = unsafe {
        CertOpenStore(
            CERT_STORE_PROV_MEMORY,
            CERT_QUERY_ENCODING_TYPE(0),
            None,
            CERT_OPEN_STORE_FLAGS(0),
            None,
        )
    }
    .unwrap();
    let store = Store::from_raw(raw);
    for pem in pems {
        let der = der_of(pem);
        // SAFETY: the store is open and `der` lives for the call; no context is asked back.
        unsafe {
            CertAddEncodedCertificateToStore(
                Some(store.raw()),
                X509_ASN_ENCODING,
                &der,
                CERT_STORE_ADD_ALWAYS,
                None,
            )
        }
        .unwrap();
    }
    store
}

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

#[test]
fn the_export_reads_every_store_and_filters() {
    let roots = [
        memory_store(&[ROOT, INTERMEDIATE]),
        memory_store(&[OTHER, ROOT]),
    ];
    let disallowed = [memory_store(&[]), memory_store(&[OTHER])];
    let filtered = export_from(&roots, &disallowed);
    assert_eq!(filtered.pem, pem_of(ROOT));
    assert_eq!(
        (
            filtered.kept,
            filtered.dropped_disallowed,
            filtered.dropped_not_self_issued
        ),
        (1, 1, 1)
    );
    assert_eq!(export_from(&[memory_store(&[])], &[]).kept, 0);
}

/// The file's creation and last write times: a new file put in place changes the first.
fn stamps(file: &Path) -> (u64, u64) {
    let meta = std::fs::symlink_metadata(file).unwrap();
    (meta.creation_time(), meta.last_write_time())
}

/// The entries `file_sddl` gives for `desktop` and `pool` (sorted), as `entries` reads them.
fn expected_entries(desktop: &Sid, pool: &[&str]) -> Vec<(u8, u8, u32, Sid)> {
    let mut out = vec![(0, 0, FA, desktop.clone()), (0, 0, FA, sid("S-1-5-18"))];
    out.extend(pool.iter().map(|reader| (0, 0, FR, sid(reader))));
    out
}

#[test]
fn the_bundle_is_written_with_its_descriptor_and_rewritten_only_on_change() {
    let place = Place::new("ca-bundle-write");
    let data = place.at("data");
    let desktop = current_user_sid().unwrap();
    let one = "S-1-5-21-1-2-3-1101";
    let two = "S-1-5-21-1-2-3-1102";
    let bundle = ensure_with(&data, &desktop, &[sid(one)]).unwrap();
    let file = path(&data);
    assert_eq!(bundle.path, file);
    assert_eq!(owner(&file).unwrap(), desktop);
    assert!(protected(&file));
    assert_eq!(entries(&file), expected_entries(&desktop, &[one]));
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        text.matches("-----BEGIN CERTIFICATE-----").count(),
        bundle.kept
    );
    let names: Vec<_> = std::fs::read_dir(data.join("sandbox-ca"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["roots.pem"], "no temp file left");

    let first = stamps(&file);
    assert_eq!(
        ensure_with(&data, &desktop, &[sid(one), sid(one)]).unwrap(),
        bundle
    );
    assert_eq!(stamps(&file), first, "the same pool: not rewritten");

    ensure_with(&data, &desktop, &[sid(two), sid(one)]).unwrap();
    assert_ne!(stamps(&file).0, first.0, "another pool: a new file");
    assert_eq!(entries(&file), expected_entries(&desktop, &[one, two]));

    crate::test_support::remove(&file);
    ensure_with(&data, &desktop, &[sid(two), sid(one)]).unwrap();
    assert!(file.is_file(), "a file gone is written again");
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_a_low_pool_token_reads_the_bundle_and_cannot_write_it() {
    if !elevated_tests_enabled() {
        return;
    }
    let place = Place::new("elevated-ca-bundle");
    let data = place.at("data");
    let user = TestUser::new();
    let desktop = current_user_sid().unwrap();
    let bundle = ensure_with(&data, &desktop, std::slice::from_ref(&user.sid)).unwrap();
    let token = low_restricted_token(&user);
    let (write, read) = {
        let _as_pool = Impersonating::start(&token);
        (
            open(&bundle.path, GENERIC_WRITE.0),
            open(&bundle.path, GENERIC_READ.0),
        )
    };
    assert_eq!(
        write,
        Err(5),
        "must refuse: the pool may not write the bundle"
    );
    assert_eq!(read, Ok(()), "a Low pool token can read the bundle");
}
