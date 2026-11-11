//! `--adopt` reads no CPU type, so the per-machine installer already moves an x64 copy to
//! ARM64 (0049, "The one move across CPU types").

use super::*;

#[test]
fn adopt_checks_no_cpu_type_so_the_installer_moves_an_x64_copy_to_arm64() {
    // `Adopter` reads no CPU type, neither the copy in place's nor the machine's: the installer
    // that ships the ARM64 build replaces an x64 install's copy on the version rule alone. This
    // pins it; the refresh's own move to ARM64 is `refresh/machine.rs`'s.
    for (target, install, placed) in [
        (user(), INSTALL, CopyOwner::User(lawyer())),
        (Target::Machine, MACHINE, CopyOwner::Machine),
    ] {
        let mut fake = Fake::new();
        fake.copy_owner = Some(placed);
        fake.version = Some("0.0.0".to_owned());
        assert_eq!(
            adopt(&mut fake, Path::new(install), &target),
            SetupExit::Done
        );
        let version = admin_copy::APP_VERSION;
        let recorded = fake
            .changes
            .iter()
            .any(|change| change.starts_with(&format!("record {HASH} {install} {version} ")));
        assert!(recorded, "{:?}", fake.changes);
    }
    // Nor does the machine side: neither file names a CPU-type read.
    for (file, text) in [
        ("adopt.rs", include_str!("../../adopt.rs")),
        ("windows_adopter.rs", include_str!("../windows_adopter.rs")),
    ] {
        for read in [
            "pe_version",
            "OWN_MACHINE",
            "native_machine",
            "IsWow64Process",
            "IMAGE_FILE_MACHINE",
        ] {
            assert!(!text.contains(read), "{file} reads {read}");
        }
    }
}
