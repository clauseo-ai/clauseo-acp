//! The one move across CPU types over the fake machine (`fake.rs`): x64 to a newer plain ARM64
//! build on a natively ARM64 machine is taken; each condition missing is exit 42, the old copy
//! kept and nothing renamed. The native machine comes from `Fake::native`, the copy on disk's
//! CPU type from the fake's admin file, the staged one from the staged bytes, and the running
//! copy's from `Trust::machine`, as `OWN_MACHINE` is tested.

use super::super::fake::*;
use super::super::test_signer::{TestKey, comment, image_for, image_with_load_config};
use super::super::*;

const X64: u16 = pe_version::MACHINE_X64;
const ARM64: u16 = pe_version::MACHINE_ARM64;
const X86: u16 = pe_version::MACHINE_X86;

/// The newer build for ARM64, at 1.4.0.
fn arm64() -> Vec<u8> {
    image_for(ARM64, [1, 4, 0, 0], b"the new setup exe, for arm64")
}

/// A hybrid image at 1.4.0: ARM64X when headed ARM64, ARM64EC when headed x64.
fn hybrid(machine: u16) -> Vec<u8> {
    image_with_load_config(machine, [1, 4, 0, 0], b"hybrid", 0x1_4000_2000)
}

/// An x64 copy recorded at 1.3.0 on a natively ARM64 machine, the user folder holding `new`
/// signed at its own `FILEVERSION`.
fn moving(new: &[u8], key: &TestKey) -> Fake {
    let mut fake = Fake::new(&old(), new, key, &good_for(new));
    fake.native = Ok(ARM64);
    fake
}

/// The run on `fake` from a copy built for `own` is exit 42 and changes nothing: the file at the
/// admin path and the record as they were, no rename, the staged copy discarded.
fn kept_out(fake: &mut Fake, key: &TestKey, own: u16, case: &str) {
    let (file, record) = (fake.admin_file.clone(), fake.record.clone());
    let exit = run_on(fake, &key.public(), own);
    assert_eq!(exit, RefreshExit::WrongMachine, "{case}: {:?}", fake.logs);
    assert_eq!(fake.admin_file, file, "{case}");
    assert_eq!(fake.record, record, "{case}");
    assert!(!fake.calls.contains(&"set_aside"), "{case}");
    assert_eq!(fake.staged, None, "{case}: the staged copy is discarded");
}

#[test]
fn an_x64_copy_on_a_native_arm64_machine_moves_to_a_newer_arm64_build() {
    let key = key();
    let mut fake = moving(&arm64(), &key);
    assert_eq!(run_on(&mut fake, &key.public(), X64), RefreshExit::Done);
    assert_eq!(fake.admin_file, Some(arm64()));
    assert_eq!(fake.set_aside, Some(old()));
    assert_eq!(fake.record.admin_copy_version.as_deref(), Some("1.4.0"));
    assert_eq!(fake.record.admin_copy_sha256, Some(sha_hex(&arm64())));
    for asked in ["admin_machine", "native"] {
        assert!(fake.calls.contains(&asked), "{asked}");
    }
    // A plain ARM64 build from MSVC carries a load config with no CHPE metadata: it moves too.
    let plain = image_with_load_config(ARM64, [1, 4, 0, 0], b"msvc", 0);
    let mut fake = moving(&plain, &key);
    assert_eq!(run_on(&mut fake, &key.public(), X64), RefreshExit::Done);
    assert_eq!(fake.admin_file, Some(plain));
    // So does a release over the recorded copy's own prerelease: newer by semver precedence.
    let mut fake = moving(&arm64(), &key);
    fake.record.admin_copy_version = Some("1.4.0-beta.1".to_owned());
    assert_eq!(run_on(&mut fake, &key.public(), X64), RefreshExit::Done);
}

#[test]
fn the_moved_copy_runs_on_as_arm64_and_heals_a_crash_after_the_renames() {
    let key = key();
    // The next run starts from the ARM64 copy: the same CPU type, so nothing is asked.
    let mut fake = moving(&arm64(), &key);
    assert_eq!(run_on(&mut fake, &key.public(), X64), RefreshExit::Done);
    fake.calls.clear();
    assert_eq!(run_on(&mut fake, &key.public(), ARM64), RefreshExit::Done);
    assert!(!fake.calls.contains(&"admin_machine"));
    assert!(!fake.calls.contains(&"native"));
    // A crash before the record's writes leaves the ARM64 copy under the old hash; the next run,
    // from that copy, writes its version and hash without another swap.
    let mut fake = moving(&arm64(), &key);
    fake.crash = Some("write_version");
    assert_eq!(
        run_on(&mut fake, &key.public(), X64),
        RefreshExit::RecordNotWritten
    );
    assert_eq!(fake.admin_file, Some(arm64()));
    assert!(fake.record_names_what_is_there(&old()));
    (fake.crash, fake.crashed) = (None, false);
    assert_eq!(run_on(&mut fake, &key.public(), ARM64), RefreshExit::Done);
    assert_eq!(fake.record.admin_copy_sha256, Some(sha_hex(&arm64())));
    assert_eq!(fake.record.admin_copy_version.as_deref(), Some("1.4.0"));
}

#[test]
fn the_reverse_arm64_to_x64_is_refused() {
    let key = key();
    let arm64_old = image_for(ARM64, [1, 3, 0, 0], b"the old setup exe, for arm64");
    let mut fake = Fake::new(&arm64_old, &new(), &key, &good());
    fake.native = Ok(ARM64);
    kept_out(&mut fake, &key, ARM64, "arm64 to x64 on arm64");
    assert!(
        !fake.calls.contains(&"native"),
        "refused on the CPU types alone"
    );
}

#[test]
fn on_a_machine_that_isnt_natively_arm64_the_move_is_refused() {
    let key = key();
    for (native, case) in [
        (Ok(X64), "x64 machine"),
        (Ok(X86), "x86 machine"),
        (Ok(pe_version::MACHINE_ARM64EC), "an ARM64EC native machine"),
        (Err("IsWow64Process2 failed"), "unreadable"),
    ] {
        let mut fake = moving(&arm64(), &key);
        fake.native = native;
        kept_out(&mut fake, &key, X64, case);
        assert!(fake.calls.contains(&"native"), "{case}");
    }
}

#[test]
fn the_move_needs_a_strictly_newer_version() {
    let key = key();
    for (recorded, case) in [
        ("1.4.0", "equal"),
        ("1.4.0.0", "equal, as FILEVERSION"),
        ("1.5.0", "older"),
        ("1.4.1-beta.1", "older than a prerelease"),
        ("not a version", "unreadable"),
    ] {
        let mut fake = moving(&arm64(), &key);
        fake.record.admin_copy_version = Some(recorded.to_owned());
        kept_out(&mut fake, &key, X64, case);
    }
    // The equal version stays a same-CPU repair: the admin path already holds the staged exe, so
    // its record is written.
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.record.admin_copy_version = Some("1.4.0".to_owned());
    fake.admin_file = Some(new());
    assert_eq!(run_on(&mut fake, &key.public(), X64), RefreshExit::Done);
    assert_eq!(fake.record.admin_copy_sha256, Some(sha_hex(&new())));
    assert!(!fake.calls.contains(&"native"));
}

#[test]
fn only_a_plain_arm64_build_moves() {
    let key = key();
    let staged = [
        (hybrid(ARM64), "ARM64X, headed ARM64"),
        (
            image_for(pe_version::MACHINE_ARM64X, [1, 4, 0, 0], b"x"),
            "ARM64X header",
        ),
        (
            image_for(pe_version::MACHINE_ARM64EC, [1, 4, 0, 0], b"ec"),
            "ARM64EC header",
        ),
        (image_for(X86, [1, 4, 0, 0], b"x86"), "x86"),
        (image_for(0x01C4, [1, 4, 0, 0], b"armnt"), "ARMNT"),
        (image_for(0x5064, [1, 4, 0, 0], b"riscv"), "unknown"),
    ];
    for (exe, case) in staged {
        let mut fake = moving(&exe, &key);
        kept_out(&mut fake, &key, X64, case);
        assert!(
            !fake.calls.contains(&"admin_machine"),
            "{case}: staged first"
        );
    }
    // An ARM64-headed image whose load config can't be read may be ARM64X: no plain ARM64.
    let mut stray = arm64();
    let directory = 0x58 + 112 + 80;
    stray[directory..directory + 4].copy_from_slice(&0x9000u32.to_le_bytes());
    let mut fake = moving(&stray, &key);
    kept_out(&mut fake, &key, X64, "unreadable load config");
}

#[test]
fn the_copy_on_disk_must_be_plain_x64_and_so_must_the_running_one() {
    let key = key();
    let on_disk = [
        (Some(image_for(ARM64, [1, 3, 0, 0], b"a")), "arm64 on disk"),
        (Some(hybrid(X64)), "ARM64EC on disk"),
        (Some(image_for(X86, [1, 3, 0, 0], b"x86")), "x86 on disk"),
        (Some(b"MZ not a PE".to_vec()), "not a PE image"),
        (None, "no file"),
    ];
    for (file, case) in on_disk {
        let mut fake = moving(&arm64(), &key);
        fake.admin_file = file;
        kept_out(&mut fake, &key, X64, case);
        assert!(!fake.calls.contains(&"native"), "{case}");
    }
    let mut fake = moving(&arm64(), &key);
    fake.fail = Some("admin_machine");
    kept_out(&mut fake, &key, X64, "unreadable copy");
    // A running copy built for anything but x64 never moves, whatever the disk says.
    for own in [X86, pe_version::MACHINE_ARM64EC] {
        let mut fake = moving(&arm64(), &key);
        kept_out(&mut fake, &key, own, "not an x64 copy");
        assert!(!fake.calls.contains(&"admin_machine"));
    }
}

/// A change to the moving fake, the exit it gives, and its name.
type Case = (fn(&mut Fake), RefreshExit, &'static str);

#[test]
fn every_other_check_still_holds_for_the_move() {
    let key = key();
    let exe = arm64();
    let signed = |file: &str, version: &str| key.sign(&exe, &comment(file, version));
    let cases: [Case; 6] = [
        (
            |fake| fake.source_sig = Ok(TestKey::new(7).sign(&arm64(), &good())),
            RefreshExit::SignatureRefused,
            "another key",
        ),
        (
            |fake| fake.fail = Some("lock"),
            RefreshExit::LockHeld,
            "lock held",
        ),
        (
            |fake| fake.fail = Some("stage"),
            RefreshExit::StagingFailed,
            "staging",
        ),
        (
            |fake| fake.source_exe = Err(SourceError::Linked(2)),
            RefreshExit::SourceRefused,
            "linked source",
        ),
        (
            |fake| fake.staged_reads_back = Some(new()),
            RefreshExit::SignatureRefused,
            "staged bytes differ",
        ),
        (
            |fake| fake.record.admin_copy_sha256 = Some(sha_hex(&arm64())),
            RefreshExit::Done,
            "already recorded",
        ),
    ];
    for (change, exit, case) in cases {
        let mut fake = moving(&exe, &key);
        change(&mut fake);
        assert_eq!(run_on(&mut fake, &key.public(), X64), exit, "{case}");
        assert_eq!(fake.admin_file, Some(old()), "{case}: nothing moved");
    }
    // The signature's `file:` and `version:` bind as before.
    for (sig, case) in [
        (signed("clauseo-setup.exe", "1.4.0"), "another file"),
        (
            signed(signature::SIGNED_FILE, "1.5.0"),
            "not its FILEVERSION",
        ),
    ] {
        let mut fake = moving(&exe, &key);
        fake.source_sig = Ok(sig);
        let exit = run_on(&mut fake, &key.public(), X64);
        assert_eq!(exit, RefreshExit::SignatureRefused, "{case}");
        assert_eq!(fake.admin_file, Some(old()), "{case}");
    }
    // The placeholder key refuses before anything is read; WSL's deny phases still run, first
    // and last (ADR 0049 "WSL").
    let mut fake = moving(&exe, &key);
    let exit = run_on(&mut fake, key::PLACEHOLDER_KEY, X64);
    assert_eq!(exit, RefreshExit::PlaceholderKey);
    let wsl = ["lock_waiting", "reapply_wsl", "unlock"];
    assert_eq!(fake.calls, [&["is_system"][..], &wsl, &wsl].concat());
}

#[test]
fn a_same_cpu_run_never_asks_for_the_cpu_types() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.native = Err("never asked");
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert!(!fake.calls.contains(&"admin_machine"));
    assert!(!fake.calls.contains(&"native"));
}
