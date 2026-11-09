//! The refresh's refusals over the fake machine (`fake.rs`): who may run it, the record, the
//! profile, the source files, the signature and the version; and its fixed values.

use std::path::{Path, PathBuf};

use super::fake::*;
use super::test_signer::{TestKey, comment, image, image_for};
use super::*;

#[test]
fn only_system_from_the_admin_copy_runs_it() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.system = false;
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::NotAllowed);
    assert_eq!(fake.calls, ["is_system"], "nothing read");
    for elsewhere in [
        r"C:\Users\Rohan\AppData\Local\Clauseo\clauseo-sandbox-setup.exe",
        r"C:\Program Files\Clauseo\Sandbox\staging\clauseo-sandbox-setup.exe.new",
        r"D:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe",
    ] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.exe = PathBuf::from(elsewhere);
        assert_eq!(run_fake(&mut fake, &key), RefreshExit::NotAllowed);
        assert!(!fake.calls.contains(&"record"), "{elsewhere}");
    }
    // The same path in another case or the verbatim form is the admin copy.
    for same in [ADMIN.to_uppercase(), format!(r"\\?\{ADMIN}")] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.exe = PathBuf::from(same);
        assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    }
}

#[test]
fn the_placeholder_key_refuses_before_reading_anything() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    assert_eq!(
        run_on(&mut fake, key::PLACEHOLDER_KEY, pe_version::MACHINE_X64),
        RefreshExit::PlaceholderKey
    );
    assert!(!fake.calls.contains(&"record") && !fake.calls.contains(&"read_source"));
    assert!(fake.logs.iter().all(|(error, _)| *error));
}

#[test]
fn a_missing_or_odd_install_folder_is_no_record() {
    let key = key();
    for dir in [
        None,
        Some(r"relative\Clauseo"),
        Some(r"C:\Users\Rohan\..\Admin"),
        Some(r"\\server\share\Clauseo"),
        Some(r"\\?\C:\Users\Rohan"),
        Some(r"C:\Users\Rohan\AppData:stream"),
    ] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.record.user_install_dir = dir.map(str::to_owned);
        assert_eq!(run_fake(&mut fake, &key), RefreshExit::NoRecord, "{dir:?}");
        assert!(!fake.calls.contains(&"read_source"), "{dir:?}");
    }
}

#[test]
fn a_refused_source_file_stages_nothing() {
    let key = key();
    for error in [
        SourceError::Missing,
        SourceError::ReparsePoint(0xA000_000C),
        SourceError::NotRegular,
        SourceError::Linked(2),
        SourceError::TooLarge(EXE_CAP),
    ] {
        for exe_side in [true, false] {
            let mut fake = Fake::new(&old(), &new(), &key, &good());
            if exe_side {
                fake.source_exe = Err(error.clone());
            } else {
                fake.source_sig = Err(error.clone());
            }
            assert_eq!(run_fake(&mut fake, &key), RefreshExit::SourceRefused);
            assert!(!fake.calls.contains(&"stage"), "{error:?}");
            assert_eq!(fake.admin_file, Some(old()));
            // The line names the file and why, never the install folder's path: the
            // Application log is readable by every local user.
            let (_, line) = fake.logs.last().unwrap();
            assert!(
                !line.contains(INSTALL) && line.contains(&error.to_string()),
                "{line}"
            );
        }
    }
}

/// A change to the fake machine before a run.
type SetUp<'a> = Box<dyn Fn(&mut Fake) + 'a>;

#[test]
fn a_bad_signature_keeps_the_old_copy() {
    let key = key();
    let other = TestKey::new(0x0BAD_0000_0000_0002);
    let cases: [(&str, SetUp<'_>); 5] = [
        (
            "wrong key",
            Box::new(|fake| fake.source_sig = Ok(other.sign(&new(), &good()))),
        ),
        (
            "garbage",
            Box::new(|fake| fake.source_sig = Ok(b"garbage".to_vec())),
        ),
        (
            "other file",
            Box::new(|fake| {
                fake.source_sig = Ok(key.sign(&new(), &comment("Clauseo-setup.exe", OWN)));
            }),
        ),
        (
            "legacy",
            Box::new(|fake| fake.source_sig = Ok(key.sign_legacy(&new(), &good()))),
        ),
        // The user swaps the exe after the read: what staging reads back is what is checked.
        (
            "swapped",
            Box::new(|fake| fake.staged_reads_back = Some(b"MZ evil".to_vec())),
        ),
    ];
    for (name, set_up) in cases {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        set_up(&mut fake);
        assert_eq!(
            run_fake(&mut fake, &key),
            RefreshExit::SignatureRefused,
            "{name}"
        );
        assert_eq!(fake.admin_file, Some(old()), "{name}");
        assert_eq!(
            fake.record.admin_copy_sha256,
            Some(sha_hex(&old())),
            "{name}"
        );
        assert_eq!(fake.staged, None, "{name}: the staged copy is discarded");
        assert!(!fake.calls.contains(&"set_aside"), "{name}");
    }
}

/// An x64 image whose resource directory is gone: a CPU type, no `FILEVERSION`.
fn unversioned_image() -> Vec<u8> {
    let mut bytes = image([1, 4, 0, 0], b"unversioned");
    // Data directory 2 (resources): its RVA, 112 + 16 bytes into the optional header at 0x58.
    bytes[0x58 + 128..0x58 + 132].copy_from_slice(&0u32.to_le_bytes());
    bytes
}

#[test]
fn an_older_or_unversioned_exe_is_refused_and_the_version_recorded_after_a_swap() {
    let key = key();
    // Signed, newer by hash, but older than the recorded version: kept.
    for (recorded, staged, exit) in [
        (
            "1.4.0.0",
            image([1, 3, 9, 0], b"older"),
            RefreshExit::Downgrade,
        ),
        (
            "1.4.0",
            image([1, 3, 0, 0], b"older"),
            RefreshExit::Downgrade,
        ),
        // Not a PE image at all: no CPU type to match.
        (
            OLD_VERSION,
            b"MZ no version resource".to_vec(),
            RefreshExit::WrongMachine,
        ),
        // A PE image with no VERSIONINFO.
        (OLD_VERSION, unversioned_image(), RefreshExit::Downgrade),
        (
            "not a version",
            image([9, 9, 9, 0], b"newer"),
            RefreshExit::Downgrade,
        ),
    ] {
        let mut fake = Fake::new(&old(), &staged, &key, &good_for(&staged));
        fake.record.admin_copy_version = Some(recorded.to_owned());
        assert_eq!(run_fake(&mut fake, &key), exit, "{recorded}");
        assert_eq!(fake.admin_file, Some(old()), "{recorded}");
        assert_eq!(
            fake.record.admin_copy_sha256,
            Some(sha_hex(&old())),
            "{recorded}"
        );
        assert!(!fake.calls.contains(&"set_aside"), "{recorded}");
    }
    // A newer version is taken; the signed version is recorded as signed, before the hash.
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert_eq!(fake.record.admin_copy_version.as_deref(), Some("1.4.0"));
    let writes: Vec<_> = fake
        .calls
        .iter()
        .filter(|c| c.starts_with("write_"))
        .collect();
    assert_eq!(writes, [&"write_version", &"write_sha256"]);
    // A record without AdminCopyVersion reads nothing.
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.record.admin_copy_version = None;
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::NoRecord);
    assert!(!fake.calls.contains(&"read_source"));
}

#[test]
fn the_same_version_with_other_bytes_is_refused() {
    let key = key();
    // The copy in place is 1.3.0 as recorded; a rebuilt 1.3.0 (another hash) is refused.
    let rebuilt = image([1, 3, 0, 0], b"rebuilt");
    let mut fake = Fake::new(&old(), &rebuilt, &key, &good_for(&rebuilt));
    fake.record.admin_copy_version = Some("1.3.0".to_owned());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Downgrade);
    assert_eq!(fake.admin_file, Some(old()));
    assert!(!fake.calls.contains(&"set_aside"));
    // The same bytes again: current, nothing written.
    let mut fake = Fake::new(&old(), &old(), &key, &good_for(&old()));
    fake.record.admin_copy_version = Some("1.3.0".to_owned());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert!(!fake.calls.iter().any(|call| call.starts_with("write_")));
}

#[test]
fn a_prerelease_sorts_below_its_release() {
    let key = key();
    let beta = image([1, 4, 0, 0], b"beta");
    let release = image([1, 4, 0, 0], b"release");
    let signed = |version: &str| comment(signature::SIGNED_FILE, version);
    // The beta of the recorded release: older, kept out.
    let mut fake = Fake::new(&old(), &beta, &key, &signed("1.4.0-beta.1"));
    fake.record.admin_copy_version = Some("1.4.0".to_owned());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Downgrade);
    assert_eq!(fake.admin_file, Some(old()));
    // The release over its beta: taken, and recorded as the release.
    let mut fake = Fake::new(&beta, &release, &key, &signed("1.4.0"));
    fake.record.admin_copy_version = Some("1.4.0-beta.1".to_owned());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert_eq!(fake.admin_file, Some(release));
    assert_eq!(fake.record.admin_copy_version.as_deref(), Some("1.4.0"));
    // A later beta over an earlier one: taken, recorded as signed.
    let beta2 = image([1, 4, 0, 0], b"beta 2");
    let mut fake = Fake::new(&beta, &beta2, &key, &signed("1.4.0-beta.2"));
    fake.record.admin_copy_version = Some("1.4.0-beta.1".to_owned());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert_eq!(
        fake.record.admin_copy_version.as_deref(),
        Some("1.4.0-beta.2")
    );
}

#[test]
fn an_exe_for_another_cpu_never_replaces_this_copy() {
    let key = key();
    // A validly signed arm64 exe, newer, on an x64 copy on an x64 machine: kept out (the one
    // move to ARM64, on an ARM64 machine, is `machine/tests.rs`'s).
    let arm64 = image_for(pe_version::MACHINE_ARM64, [1, 4, 0, 0], b"arm64");
    let mut fake = Fake::new(&old(), &arm64, &key, &good_for(&arm64));
    let exit = run_on(&mut fake, &key.public(), pe_version::MACHINE_X64);
    assert_eq!(exit, RefreshExit::WrongMachine);
    assert_eq!(fake.admin_file, Some(old()));
    assert!(!fake.calls.contains(&"set_aside"));
    assert_eq!(fake.staged, None, "the staged copy is discarded");
    // And an x64 exe on an arm64 copy.
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    let exit = run_on(&mut fake, &key.public(), pe_version::MACHINE_ARM64);
    assert_eq!(exit, RefreshExit::WrongMachine);
    // The matching CPU: taken.
    let mut fake = Fake::new(&old(), &arm64, &key, &good_for(&arm64));
    let exit = run_on(&mut fake, &key.public(), pe_version::MACHINE_ARM64);
    assert_eq!(exit, RefreshExit::Done);
}

#[test]
fn the_signed_version_must_be_the_staged_exes_file_version() {
    let key = key();
    // The comment says 0.1.0 (X.Y.Z, as release.yml signs); the PE says otherwise.
    for (staged, exit) in [
        (image([0, 1, 0, 0], b"matching"), RefreshExit::Done),
        (
            image([0, 1, 0, 1], b"fourth part"),
            RefreshExit::SignatureRefused,
        ),
        (image([0, 1, 1, 0], b"newer"), RefreshExit::SignatureRefused),
    ] {
        let mut fake = Fake::new(
            &old(),
            &staged,
            &key,
            &comment(signature::SIGNED_FILE, "0.1.0"),
        );
        fake.record.admin_copy_version = Some("0.0.9".to_owned());
        assert_eq!(
            run_fake(&mut fake, &key),
            exit,
            "{:?}",
            pe_version::file_version(&staged)
        );
        if exit != RefreshExit::Done {
            assert_eq!(fake.admin_file, Some(old()));
            assert!(!fake.calls.contains(&"set_aside"));
        }
    }
    // No version in the comment: refused as a signature.
    let unversioned = format!("timestamp:1\tfile:{}", signature::SIGNED_FILE);
    let mut fake = Fake::new(&old(), &new(), &key, &unversioned);
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::SignatureRefused);
    assert_eq!(fake.admin_file, Some(old()));
}

#[test]
fn an_install_folder_outside_the_owners_profile_reads_nothing() {
    let key = key();
    for (dir, profile) in [
        (r"C:\Users\Other\AppData\Local\Clauseo", Some(PROFILE)),
        (r"C:\Windows\System32", Some(PROFILE)),
        (r"D:\Users\Rohan\AppData\Local\Clauseo", Some(PROFILE)),
        (r"C:\Users\RohanX\AppData", Some(PROFILE)),
        (PROFILE, Some(PROFILE)),
        (INSTALL, None),
    ] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.record.user_install_dir = Some(dir.to_owned());
        fake.profile = profile.map(PathBuf::from);
        assert_eq!(
            run_fake(&mut fake, &key),
            RefreshExit::OutsideProfile,
            "{dir}"
        );
        assert!(!fake.calls.contains(&"read_source"), "{dir}");
        // The Application log is readable by every local user: no path, no SID.
        let (_, line) = fake.logs.last().unwrap();
        assert!(!line.contains(dir) && !line.contains(OWNER), "{line}");
    }
    for owner in [None, Some("not a sid")] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.record.copy_owner = owner.map(str::to_owned);
        assert_eq!(
            run_fake(&mut fake, &key),
            RefreshExit::NoRecord,
            "{owner:?}"
        );
    }
    // Inside, in another case or with a trailing separator: read.
    assert!(inside_profile(
        Path::new(&INSTALL.to_uppercase()),
        Path::new(PROFILE)
    ));
    assert!(inside_profile(
        Path::new(INSTALL),
        Path::new(r"c:\users\rohan\")
    ));
    assert!(!inside_profile(
        Path::new(INSTALL),
        Path::new(r"\\server\Users\Rohan")
    ));
}

#[test]
fn the_profile_check_keys_on_the_admin_copy_owner_not_the_pool_owner() {
    let key = key();
    // A user's copy: its profile, as before. `OwnerSid` plays no part: the in-app `--remove`
    // deletes it and keeps the copy and the task, and the copy still refreshes.
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert!(!fake.calls.contains(&"admin_only_dir"));
    // A per-machine copy: the install folder must be administrators-only, no profile read.
    for (admin_only, exit) in [
        (true, RefreshExit::Done),
        (false, RefreshExit::OutsideProfile),
    ] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.record.copy_owner = Some("machine".to_owned());
        fake.profile = None;
        fake.admin_only = admin_only;
        assert_eq!(run_fake(&mut fake, &key), exit, "{admin_only}");
        assert!(fake.calls.contains(&"admin_only_dir"));
        assert_eq!(fake.calls.contains(&"read_source"), admin_only);
    }
    // No `AdminCopyOwner`, or `Machine` in another case: no record, nothing read.
    for owner in [None, Some("Machine")] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.record.copy_owner = owner.map(str::to_owned);
        assert_eq!(
            run_fake(&mut fake, &key),
            RefreshExit::NoRecord,
            "{owner:?}"
        );
        assert!(!fake.calls.contains(&"read_source"), "{owner:?}");
    }
}

#[test]
fn exit_codes_are_distinct_and_clear_of_the_set_up_codes() {
    let all = [
        RefreshExit::Done,
        RefreshExit::NotAllowed,
        RefreshExit::NoRecord,
        RefreshExit::SourceRefused,
        RefreshExit::SignatureRefused,
        RefreshExit::PlaceholderKey,
        RefreshExit::StagingFailed,
        RefreshExit::ReplaceFailed,
        RefreshExit::RecordNotWritten,
        RefreshExit::Downgrade,
        RefreshExit::OutsideProfile,
        RefreshExit::WrongMachine,
        RefreshExit::FolderBusy,
        RefreshExit::LockHeld,
    ];
    let codes: Vec<u8> = all.iter().map(|exit| exit.as_code()).collect();
    assert_eq!(
        codes,
        [0, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44]
    );
    let setup: Vec<u8> = crate::windows::setup::SetupExit::ALL
        .iter()
        .map(|exit| exit.as_code())
        .collect();
    // PR 2's set-up codes, as the installer's hooks read them, and the refresh's from 32: the
    // one exe exits with either, and only 0 (done) means the same in both.
    assert_eq!(setup, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
    for code in &codes[1..] {
        assert!(!setup.contains(code), "{code} is a set-up code too");
    }
    assert!(setup.iter().all(|code| *code < 32));
}

#[test]
fn the_descriptors_say_what_the_module_says() {
    let owner = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    assert_eq!(
        task_sddl(&owner),
        "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;S-1-5-21-1-2-3-1001)"
    );
    // Read back with the flags SetSecurityInfo adds, the staging DACL still matches; any other
    // entry, owner or a lost protection doesn't.
    assert_eq!(STAGING_SDDL, "O:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)");
    assert!(sddl_matches(
        "O:SYD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)",
        STAGING_SDDL
    ));
    assert!(sddl_matches(STAGING_SDDL, STAGING_SDDL));
    for wrong in [
        "O:SYD:AI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)",
        "O:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)",
        "O:SYD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;BU)",
        // The old SYSTEM-only DACL: a folder an older refresh made is set again.
        "O:SYD:PAI(A;OICI;FA;;;SY)",
        "O:SYD:PAI(A;OICI;FA;;;BA)",
        "O:SY",
    ] {
        assert!(!sddl_matches(wrong, STAGING_SDDL), "{wrong}");
    }
    assert_eq!(REFRESH_ARG, "--refresh");
    assert_eq!(EXECUTION_LIMIT, "PT5M");
}
