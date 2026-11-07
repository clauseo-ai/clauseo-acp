//! The refresh's refusals over the fake machine (`fake.rs`): who may run it, the record, the
//! profile, the source files, the signature and the version; and its fixed values.

use std::path::{Path, PathBuf};

use super::fake::*;
use super::test_signer::{TestKey, comment, image};
use super::*;

#[test]
fn only_system_from_the_admin_copy_runs_it() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.system = false;
    assert_eq!(refresh(&mut fake, &key.public()), RefreshExit::NotAllowed);
    assert_eq!(fake.calls, ["is_system"], "nothing read");
    for elsewhere in [
        r"C:\Users\Rohan\AppData\Local\Clauseo\clauseo-sandbox-setup.exe",
        r"C:\Program Files\Clauseo\Sandbox\staging\clauseo-sandbox-setup.exe.new",
        r"D:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe",
    ] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.exe = PathBuf::from(elsewhere);
        assert_eq!(refresh(&mut fake, &key.public()), RefreshExit::NotAllowed);
        assert!(!fake.calls.contains(&"record"), "{elsewhere}");
    }
    // The same path in another case or the verbatim form is the admin copy.
    for same in [ADMIN.to_uppercase(), format!(r"\\?\{ADMIN}")] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.exe = PathBuf::from(same);
        assert_eq!(refresh(&mut fake, &key.public()), RefreshExit::Done);
    }
}

#[test]
fn the_placeholder_key_refuses_before_reading_anything() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    assert_eq!(
        refresh(&mut fake, key::PLACEHOLDER_KEY),
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
        assert_eq!(
            refresh(&mut fake, &key.public()),
            RefreshExit::NoRecord,
            "{dir:?}"
        );
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
            assert_eq!(
                refresh(&mut fake, &key.public()),
                RefreshExit::SourceRefused
            );
            assert!(!fake.calls.contains(&"stage"), "{error:?}");
            assert_eq!(fake.admin_file, Some(old()));
            // The line names the file, the install folder and why; no other path.
            let (_, line) = fake.logs.last().unwrap();
            assert!(
                line.contains(INSTALL) && line.contains(&error.to_string()),
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
            refresh(&mut fake, &key.public()),
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

#[test]
fn an_older_or_unversioned_exe_is_refused_and_the_version_recorded_after_a_swap() {
    let key = key();
    // Signed, newer by hash, but older than the recorded version: kept.
    for (recorded, staged, exit) in [
        (
            "1.4.0.0",
            image([1, 3, 9, 9], b"older"),
            RefreshExit::Downgrade,
        ),
        (
            "1.4.0",
            image([1, 3, 0, 0], b"older"),
            RefreshExit::Downgrade,
        ),
        (
            OLD_VERSION,
            b"MZ no version resource".to_vec(),
            RefreshExit::Downgrade,
        ),
        (
            "not a version",
            image([9, 9, 9, 9], b"newer"),
            RefreshExit::Downgrade,
        ),
    ] {
        let mut fake = Fake::new(&old(), &staged, &key, &good());
        fake.record.admin_copy_version = Some(recorded.to_owned());
        assert_eq!(refresh(&mut fake, &key.public()), exit, "{recorded}");
        assert_eq!(fake.admin_file, Some(old()), "{recorded}");
        assert_eq!(
            fake.record.admin_copy_sha256,
            Some(sha_hex(&old())),
            "{recorded}"
        );
        assert!(!fake.calls.contains(&"set_aside"), "{recorded}");
    }
    // The same version rebuilt (another hash) is taken, and the version is written first.
    let same = image([1, 3, 0, 0], b"rebuilt");
    let mut fake = Fake::new(&old(), &same, &key, &good());
    assert_eq!(refresh(&mut fake, &key.public()), RefreshExit::Done);
    assert_eq!(fake.admin_file, Some(same));
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    assert_eq!(refresh(&mut fake, &key.public()), RefreshExit::Done);
    assert_eq!(fake.record.admin_copy_version.as_deref(), Some("1.4.0.0"));
    let writes: Vec<_> = fake
        .calls
        .iter()
        .filter(|c| c.starts_with("write_"))
        .collect();
    assert_eq!(writes, [&"write_version", &"write_sha256"]);
    // A record without AdminCopyVersion reads nothing.
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.record.admin_copy_version = None;
    assert_eq!(refresh(&mut fake, &key.public()), RefreshExit::NoRecord);
    assert!(!fake.calls.contains(&"read_source"));
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
            refresh(&mut fake, &key.public()),
            RefreshExit::OutsideProfile,
            "{dir}"
        );
        assert!(!fake.calls.contains(&"read_source"), "{dir}");
    }
    for owner in [None, Some("not a sid")] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.record.owner_sid = owner.map(str::to_owned);
        assert_eq!(
            refresh(&mut fake, &key.public()),
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
    ];
    let codes: Vec<u8> = all.iter().map(|exit| exit.as_code()).collect();
    assert_eq!(codes, [0, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41]);
    let setup_max = crate::windows::setup::SetupExit::ALL
        .iter()
        .map(|exit| exit.as_code())
        .max()
        .unwrap();
    assert!(setup_max < 32);
}

#[test]
fn the_descriptors_say_what_the_module_says() {
    let owner = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    assert_eq!(
        task_sddl(&owner),
        "D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;S-1-5-21-1-2-3-1001)"
    );
    // Read back with the flags SetSecurityInfo adds, the staging DACL still matches; any other
    // entry, owner or a lost protection doesn't.
    assert!(sddl_matches("O:SYD:PAI(A;OICI;FA;;;SY)", STAGING_SDDL));
    assert!(sddl_matches("O:SYD:P(A;OICI;FA;;;SY)", STAGING_SDDL));
    for wrong in [
        "O:SYD:AI(A;OICI;FA;;;SY)",
        "O:BAD:PAI(A;OICI;FA;;;SY)",
        "O:SYD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BU)",
        "O:SYD:PAI(A;OICI;FA;;;BA)",
        "O:SY",
    ] {
        assert!(!sddl_matches(wrong, STAGING_SDDL), "{wrong}");
    }
    assert_eq!(REFRESH_ARG, "--refresh");
    assert_eq!(EXECUTION_LIMIT, "PT5M");
}
