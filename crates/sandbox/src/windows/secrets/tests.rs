//! `secrets.bin`: the format, the store and the lookup on every OS, with a stand-in for DPAPI;
//! DPAPI itself and the owner's impersonation on Windows CI.

use super::*;
use crate::test_support::TempDir;

/// A pool SID numbered `rid`.
fn sid(rid: u32) -> Sid {
    Sid::parse(&format!("S-1-5-21-1000-2000-3000-{rid}")).unwrap()
}

/// `text` as a password.
fn password(text: &str) -> Zeroizing<String> {
    Zeroizing::new(text.to_owned())
}

/// The stand-in for `CryptProtectData`: a tag, then the bytes reversed. An empty password fails,
/// so the stand-in can fail as the real call can.
fn seal(plain: &[u8]) -> Result<Vec<u8>, SecretError> {
    if plain.is_empty() {
        return Err(SecretError::Dpapi {
            call: "CryptProtectData",
            code: 87,
        });
    }
    let mut blob = b"sealed:".to_vec();
    blob.extend(plain.iter().rev());
    Ok(blob)
}

/// The stand-in for `CryptUnprotectData`: undoes [`seal`]; anything else fails as DPAPI does.
fn unseal(blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, SecretError> {
    let rest = blob.strip_prefix(b"sealed:").ok_or(SecretError::Dpapi {
        call: "CryptUnprotectData",
        code: 13,
    })?;
    Ok(Zeroizing::new(rest.iter().rev().copied().collect()))
}

/// The password for `rid` stored under `temp`, through [`unseal`].
fn secret(temp: &TempDir, rid: u32) -> Result<String, SecretError> {
    secret_with(temp.path(), &sid(rid), unseal).map(|text| text.as_str().to_owned())
}

/// The file's bytes as they are on disk.
fn on_disk(temp: &TempDir) -> Vec<u8> {
    std::fs::read(path(temp.path())).unwrap()
}

#[test]
fn the_format_round_trips_and_names_each_user_once() {
    let mut file = SecretsFile::new();
    file.insert(sid(1001), vec![1, 2, 3]);
    file.insert(sid(1002), vec![4, 5]);
    file.insert(sid(1001), vec![9]);
    let bytes = file.to_bytes().unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("\"version\": 1"), "{text}");
    assert!(text.contains("\"blob_b64\": \"CQ==\""), "{text}");
    let back = SecretsFile::from_bytes(&bytes).unwrap();
    assert_eq!(back, file);
    assert_eq!(back.blob_for(&sid(1001)), Some([9].as_slice()));
    assert_eq!(
        back.sids().collect::<Vec<_>>(),
        vec![&sid(1001), &sid(1002)]
    );
}

#[test]
fn an_unknown_sid_has_no_blob() {
    let mut file = SecretsFile::new();
    file.insert(sid(1001), vec![1]);
    assert_eq!(file.blob_for(&sid(1002)), None);
    assert_eq!(SecretsFile::new().blob_for(&sid(1001)), None);
}

#[test]
fn a_newer_version_is_refused_and_a_bad_file_is_malformed() {
    let newer = br#"{ "version": 2, "users": [], "keys": {} }"#;
    assert_eq!(
        SecretsFile::from_bytes(newer),
        Err(SecretError::Newer { found: 2 })
    );
    let user = format!(r#"{{ "sid": "{}", "blob_b64": "AQ==" }}"#, sid(1001));
    for bad in [
        r#"{ "version": 0, "users": [] }"#.to_owned(),
        r#"{ "version": 1, "users": [], "more": 1 }"#.to_owned(),
        r#"{ "version": 1 }"#.to_owned(),
        format!(r#"{{ "version": 1, "users": [{user}, {user}] }}"#),
        format!(
            r#"{{ "version": 1, "users": [{{ "sid": "{}", "blob_b64": "" }}] }}"#,
            sid(1)
        ),
        r#"{ "version": 1, "users": [{ "sid": "nobody", "blob_b64": "AQ==" }] }"#.to_owned(),
        "not json".to_owned(),
    ] {
        assert!(
            matches!(
                SecretsFile::from_bytes(bad.as_bytes()),
                Err(SecretError::Malformed(_))
            ),
            "{bad}"
        );
    }
}

#[test]
fn no_error_or_debug_text_holds_a_blob() {
    // A blob where the version belongs, and one that is not base64: neither is echoed.
    let blob = "QmxvYkJ5dGVzVGhhdE11c3ROb3RMZWFr";
    let misplaced = format!(r#"{{ "version": "{blob}", "users": [] }}"#);
    let error = SecretsFile::from_bytes(misplaced.as_bytes()).unwrap_err();
    assert!(!error.to_string().contains(blob), "{error}");
    let not_base64 = format!(
        r#"{{ "version": 1, "users": [{{ "sid": "{}", "blob_b64": "{blob}!" }}] }}"#,
        sid(1001)
    );
    let error = SecretsFile::from_bytes(not_base64.as_bytes()).unwrap_err();
    assert!(!error.to_string().contains(blob), "{error}");
    let mut file = SecretsFile::new();
    file.insert(sid(1001), b"BlobBytesThatMustNotLeak".to_vec());
    let debug = format!("{file:?}");
    assert!(!debug.contains("BlobBytes"), "{debug}");
    assert!(!debug.contains(blob), "{debug}");
    assert!(debug.contains("24 bytes"), "{debug}");
}

#[test]
fn stored_passwords_come_back_and_an_extend_keeps_the_old_blobs() {
    let temp = TempDir::new("secrets-store");
    let pool = [sid(1001), sid(1002)];
    let set = [
        (sid(1001), password("first-one")),
        (sid(1002), password("second")),
    ];
    store(temp.path(), &set, &pool, seal).unwrap();
    assert_eq!(secret(&temp, 1001).unwrap(), "first-one");
    assert_eq!(secret(&temp, 1002).unwrap(), "second");
    let before = read(temp.path()).unwrap();
    // An extend: only the new user's password; the others keep their blobs as they are.
    let grown = [sid(1001), sid(1002), sid(1003)];
    store(temp.path(), &[(sid(1003), password("third"))], &grown, seal).unwrap();
    let after = read(temp.path()).unwrap();
    for kept in &pool {
        assert_eq!(after.blob_for(kept), before.blob_for(kept));
    }
    assert_eq!(secret(&temp, 1003).unwrap(), "third");
    assert_eq!(after.sids().count(), 3);
}

#[test]
fn a_kept_sid_with_no_blob_is_no_secret_and_never_another_users() {
    let temp = TempDir::new("secrets-kept-sid");
    // No file yet: every SID is no secret.
    assert_eq!(
        secret(&temp, 1001),
        Err(SecretError::NoSecret { sid: sid(1001) })
    );
    // The record keeps 1002 (account gone, a ledger entry names it); set-up writes live users.
    store(
        temp.path(),
        &[(sid(1001), password("live"))],
        &[sid(1001)],
        seal,
    )
    .unwrap();
    let error = secret(&temp, 1002).unwrap_err();
    assert_eq!(error, SecretError::NoSecret { sid: sid(1002) });
    assert!(error.to_string().contains(sid(1002).as_str()), "{error}");
    assert_eq!(secret(&temp, 1001).unwrap(), "live");
    // Writing a pool that names it, with no password for it, fails and keeps the old file.
    let old = on_disk(&temp);
    let pool = [sid(1001), sid(1002)];
    let refused = store(temp.path(), &[(sid(1001), password("new"))], &pool, seal);
    assert_eq!(refused, Err(SecretError::NoSecret { sid: sid(1002) }));
    assert_eq!(on_disk(&temp), old);
}

#[test]
fn a_password_outside_the_pool_is_refused() {
    let temp = TempDir::new("secrets-not-in-pool");
    let refused = store(
        temp.path(),
        &[(sid(1009), password("x"))],
        &[sid(1001)],
        seal,
    );
    assert_eq!(refused, Err(SecretError::NotInPool { sid: sid(1009) }));
    assert!(!path(temp.path()).exists());
}

#[test]
fn a_failed_write_leaves_the_old_file_byte_for_byte() {
    let temp = TempDir::new("secrets-failed-write");
    let pool = [sid(1001), sid(1002)];
    let set = [(sid(1001), password("one")), (sid(1002), password("two"))];
    store(temp.path(), &set, &pool, seal).unwrap();
    let old = on_disk(&temp);
    let reset = [(sid(1001), password("uno")), (sid(1002), password("dos"))];
    // The write stops halfway, as a full disk would stop it.
    let half = |file: &mut File, bytes: &[u8]| {
        file.write_all(&bytes[..bytes.len() / 2])?;
        Err(io::Error::other("the disk is full"))
    };
    let failed = store_with(temp.path(), &reset, &pool, seal, half).unwrap_err();
    assert!(failed.to_string().contains("the disk is full"), "{failed}");
    assert_eq!(on_disk(&temp), old);
    // No new file is left beside it.
    let names: Vec<_> = std::fs::read_dir(path(temp.path()).parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from(SECRETS_FILE)]);
    // A failed protect writes nothing either.
    let refuse = |_: &[u8]| {
        Err(SecretError::Dpapi {
            call: "CryptProtectData",
            code: 5,
        })
    };
    assert!(store(temp.path(), &reset, &pool, refuse).is_err());
    assert_eq!(on_disk(&temp), old);
    assert_eq!(secret(&temp, 1001).unwrap(), "one");
}

#[test]
fn a_reset_of_every_user_replaces_an_unreadable_file_and_a_keep_does_not() {
    let temp = TempDir::new("secrets-unreadable");
    temp.file("sandbox/secrets.bin", "not json");
    let pool = [sid(1001), sid(1002)];
    let keep = store(temp.path(), &[(sid(1001), password("a"))], &pool, seal);
    assert!(matches!(keep, Err(SecretError::Malformed(_))), "{keep:?}");
    assert_eq!(on_disk(&temp), b"not json");
    let all = [(sid(1001), password("a")), (sid(1002), password("b"))];
    store(temp.path(), &all, &pool, seal).unwrap();
    assert_eq!(secret(&temp, 1002).unwrap(), "b");
}

#[test]
fn a_password_that_is_not_text_is_refused() {
    let temp = TempDir::new("secrets-not-text");
    store(
        temp.path(),
        &[(sid(1001), password("x"))],
        &[sid(1001)],
        seal,
    )
    .unwrap();
    let not_text = |_: &[u8]| Ok(Zeroizing::new(vec![0xff, 0xfe]));
    let error = secret_with(temp.path(), &sid(1001), not_text).unwrap_err();
    assert!(matches!(error, SecretError::Malformed(_)), "{error:?}");
}

#[cfg(unix)]
#[test]
fn a_linked_or_hard_linked_file_is_refused_and_left_alone() {
    let temp = TempDir::new("secrets-links");
    let elsewhere = temp.file("elsewhere.json", "{}");
    temp.dir("sandbox");
    let link = temp.symlink("sandbox/secrets.bin", &elsewhere);
    let pool = [sid(1001)];
    let set = [(sid(1001), password("x"))];
    assert!(matches!(read(temp.path()), Err(SecretError::Io(_))));
    assert!(matches!(
        store(temp.path(), &set, &pool, seal),
        Err(SecretError::Io(_))
    ));
    assert_eq!(std::fs::read(&elsewhere).unwrap(), b"{}");
    crate::test_support::remove(&link);
    store(temp.path(), &set, &pool, seal).unwrap();
    let second = temp.hard_link("second.bin", &path(temp.path()));
    let old = on_disk(&temp);
    assert!(matches!(read(temp.path()), Err(SecretError::Io(_))));
    assert!(matches!(
        store(temp.path(), &set, &pool, seal),
        Err(SecretError::Io(_))
    ));
    assert_eq!(std::fs::read(&second).unwrap(), old);
}

#[test]
fn a_secret_error_becomes_a_failed_setup_error() {
    let error = crate::SetupError::from(SecretError::NoSecret { sid: sid(1001) });
    assert!(matches!(error, crate::SetupError::Failed(why) if why.contains("no password")));
}

/// Windows CI: real DPAPI as the runner's user, and the owner's impersonation.
#[cfg(windows)]
mod windows {
    use super::*;
    use crate::windows::setup::owner::{Owner, elevated_tests_on};

    #[test]
    fn dpapi_round_trips_as_this_user_and_other_entropy_fails() {
        let blob = dpapi::protect(b"a pool password").unwrap();
        assert!(
            !blob.windows(4).any(|w| w == b"pool"),
            "the blob is not the plaintext"
        );
        assert_eq!(
            dpapi::unprotect(&blob).unwrap().as_slice(),
            b"a pool password"
        );
        let other = dpapi::protect_with(b"a pool password", b"other entropy").unwrap();
        let error = dpapi::unprotect(&other).unwrap_err();
        assert!(matches!(
            error,
            SecretError::Dpapi {
                call: "CryptUnprotectData",
                ..
            }
        ));
        assert!(dpapi::unprotect(b"not a blob").is_err());
    }

    #[test]
    fn stored_with_dpapi_the_password_comes_back_through_secret_for() {
        let temp = TempDir::new("secrets-dpapi");
        let set = [(sid(1001), password("dpapi-password"))];
        store(temp.path(), &set, &[sid(1001)], dpapi::protect).unwrap();
        assert_eq!(
            secret_for(temp.path(), &sid(1001)).unwrap().as_str(),
            "dpapi-password"
        );
        assert_eq!(
            secret_for(temp.path(), &sid(1002)),
            Err(SecretError::NoSecret { sid: sid(1002) })
        );
    }

    #[test]
    fn protected_for_the_owner_the_password_comes_back() {
        let temp = TempDir::new("secrets-owner");
        let owner = Owner::of_this_process().unwrap();
        let set = [(sid(1001), password("owners-password"))];
        protect_for_owner(&owner, temp.path(), &set, &[sid(1001)]).unwrap();
        assert_eq!(
            secret_for(temp.path(), &sid(1001)).unwrap().as_str(),
            "owners-password"
        );
    }

    #[test]
    #[ignore = "elevated: CI's elevated step only (CLAUSEO_ELEVATED_TESTS=1)"]
    fn elevated_secrets_round_trip_through_a_duplicated_token_of_this_process() {
        if !elevated_tests_on() {
            eprintln!("skipped: needs an elevated token and CLAUSEO_ELEVATED_TESTS=1");
            return;
        }
        // The folder's name holds this process's ID, so a leftover from a killed earlier try
        // has another name; TempDir clears one with this name.
        let temp = TempDir::new("elevated-secrets");
        let owner = Owner::of_this_process().unwrap();
        let pool = [sid(1001), sid(1002)];
        let set = [(sid(1001), password("one")), (sid(1002), password("two"))];
        protect_for_owner(&owner, temp.path(), &set, &pool).unwrap();
        let grown = [sid(1001), sid(1002), sid(1003)];
        protect_for_owner(
            &owner,
            temp.path(),
            &[(sid(1003), password("three"))],
            &grown,
        )
        .unwrap();
        for (rid, expected) in [(1001, "one"), (1002, "two"), (1003, "three")] {
            assert_eq!(
                secret_for(temp.path(), &sid(rid)).unwrap().as_str(),
                expected
            );
        }
        let owner_of_file = crate::windows::private_dir::owner(&path(temp.path())).unwrap();
        assert!(
            owner_of_file == *owner.sid() || owner_of_file.as_str() == "S-1-5-32-544",
            "{owner_of_file}"
        );
    }
}
