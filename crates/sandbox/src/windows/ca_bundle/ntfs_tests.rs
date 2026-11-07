//! The CA bundle on NTFS: the export's enumeration over memory stores (the real `Root` and
//! `Disallowed` are never written: adding to the user's `Root` raises a prompt), the file's
//! descriptor and its rewrite rule, and, elevated, that a Low restricted token of a pool user
//! may read the file and not write it.
#![expect(unsafe_code, reason = "FFI to memory certificate stores in tests")]

use std::os::windows::fs::MetadataExt as _;
use std::path::Path;

use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Security::Cryptography::{
    CERT_CONTEXT, CERT_DISALLOWED_ENHKEY_USAGE_PROP_ID, CERT_DISALLOWED_FILETIME_PROP_ID,
    CERT_ENHKEY_USAGE_PROP_ID, CERT_OPEN_STORE_FLAGS, CERT_QUERY_ENCODING_TYPE,
    CERT_STORE_ADD_ALWAYS, CERT_STORE_PROV_MEMORY, CRYPT_INTEGER_BLOB,
    CertAddEncodedCertificateToStore, CertFreeCertificateContext, CertOpenStore,
    CertSetCertificateContextProperty, X509_ASN_ENCODING,
};

use super::store::{Store, export_from};
use super::tests::{
    CLIENT_ONLY, INTERMEDIATE, OTHER, ROOT, SERVER_AND_CODE, SERVER_ONLY, der_of, pem_of,
};
use super::{ensure_with, merge, path, write_merged};
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

/// A memory store holding [`ROOT`] with the property `id` set to `value`.
fn limited_root(id: u32, value: &[u8]) -> Store {
    let store = memory_store(&[]);
    let der = der_of(ROOT);
    let mut context: *mut CERT_CONTEXT = std::ptr::null_mut();
    // SAFETY: the store is open and `der` lives for the call; the context it gives back is
    // freed below.
    unsafe {
        CertAddEncodedCertificateToStore(
            Some(store.raw()),
            X509_ASN_ENCODING,
            &der,
            CERT_STORE_ADD_ALWAYS,
            Some(&raw mut context),
        )
    }
    .unwrap();
    let mut bytes = value.to_vec();
    let blob = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).unwrap(),
        pbData: bytes.as_mut_ptr(),
    };
    // SAFETY: the context is live; `blob` and the bytes it points at live for the call, which
    // copies them.
    unsafe { CertSetCertificateContextProperty(context, id, 0, Some((&raw const blob).cast())) }
        .unwrap();
    // SAFETY: the context came from the add above and is freed once; the store keeps the cert.
    assert!(unsafe { CertFreeCertificateContext(Some(context)) }.as_bool());
    store
}

#[test]
fn a_root_limited_by_its_store_properties_is_dropped_and_counted() {
    // A FILETIME, any one: a disallowed time set at all limits the root.
    let time = 0x01DB_0000_0000_0000_u64.to_le_bytes();
    for (what, id, value) in [
        (
            "every purpose off",
            CERT_ENHKEY_USAGE_PROP_ID,
            &[0x30, 0x00][..],
        ),
        ("client auth only", CERT_ENHKEY_USAGE_PROP_ID, CLIENT_ONLY),
        (
            "server auth disallowed",
            CERT_DISALLOWED_ENHKEY_USAGE_PROP_ID,
            SERVER_ONLY,
        ),
        (
            "a disallowed time",
            CERT_DISALLOWED_FILETIME_PROP_ID,
            &time[..],
        ),
    ] {
        let filtered = export_from(&[limited_root(id, value), memory_store(&[OTHER])], &[]);
        assert_eq!(filtered.dropped_limited, 1, "{what}");
        assert_eq!(filtered.pem, pem_of(OTHER), "{what}");
    }
    // Limited in one root store, plain in another: dropped from both.
    let both = [
        limited_root(CERT_ENHKEY_USAGE_PROP_ID, CLIENT_ONLY),
        memory_store(&[ROOT]),
    ];
    assert_eq!(export_from(&both, &[]).kept, 0);
    // Server auth among its usages, or another usage disallowed: kept.
    for (id, value) in [
        (CERT_ENHKEY_USAGE_PROP_ID, SERVER_AND_CODE),
        (CERT_DISALLOWED_ENHKEY_USAGE_PROP_ID, CLIENT_ONLY),
    ] {
        let filtered = export_from(&[limited_root(id, value)], &[]);
        assert_eq!((filtered.kept, filtered.dropped_limited), (1, 0));
        assert_eq!(filtered.pem, pem_of(ROOT));
    }
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
fn a_merged_bundle_is_written_once_beside_the_machine_bundle_with_its_descriptor() {
    let place = Place::new("ca-bundle-merged");
    let data = place.at("data");
    let desktop = current_user_sid().unwrap();
    let one = "S-1-5-21-1-2-3-1101";
    let merged = merge(pem_of(ROOT).as_bytes(), &[pem_of(OTHER).into_bytes()]);
    let file = write_merged(&data, &desktop, &[sid(one)], &merged.pem).unwrap();
    assert_eq!(file.parent().unwrap(), data.join("sandbox-ca"));
    assert_ne!(file, path(&data));
    assert_eq!(owner(&file).unwrap(), desktop);
    assert!(protected(&file));
    assert_eq!(entries(&file), expected_entries(&desktop, &[one]));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), merged.pem);
    let first = stamps(&file);
    let again = write_merged(&data, &desktop, &[sid(one)], &merged.pem).unwrap();
    assert_eq!(
        (again, stamps(&file)),
        (file, first),
        "the same bundle and pool: used as is"
    );
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
