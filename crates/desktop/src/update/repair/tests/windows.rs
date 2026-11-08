//! The attempt's handle and folder on Windows (run on the Windows VM or CI): no second writer,
//! delete or rename while the file is held; the folder's DACL protected and the user's and
//! SYSTEM's alone; and the sweep leaving a junction and a file in use.
#![expect(
    clippy::disallowed_methods,
    reason = "test set-up writes and removes files in its scratch folder; no write module here"
)]

use std::os::windows::fs::OpenOptionsExt as _;

use super::super::flow::Staged as _;
use super::super::sweep::{Swept, sweep};
use super::super::windows::{Stage, own_sid};
use crate::test_support::Scratch;

/// `FILE_SHARE_READ`.
const SHARE_READ: u32 = 1;

/// While the attempt holds its file, before and after the seal, no other handle may open it to
/// write, and it can't be deleted or renamed, nor its folder renamed; the check reads it back
/// through the held handle. Discarded, the folder is gone.
#[test]
fn no_second_writer_while_the_file_is_held() {
    let scratch = Scratch::new("repair-held", &[]);
    let updates = scratch.path().join("updates");
    let mut stage = Stage::create(&updates, "0.4.1", "Clauseo_0.4.1_x64-setup.exe").unwrap();
    stage.write(b"the installer's bytes").unwrap();
    let exe = stage.exe().to_path_buf();
    let folder = stage.folder().to_path_buf();
    let refused = |when: &str| {
        let writer = std::fs::OpenOptions::new().write(true).open(&exe);
        assert!(writer.is_err(), "{when}: a second writer opened the file");
        assert!(std::fs::remove_file(&exe).is_err(), "{when}: deleted");
        assert!(
            std::fs::rename(&exe, folder.join("other.exe")).is_err(),
            "{when}: renamed"
        );
        assert!(
            std::fs::rename(&folder, folder.with_file_name("moved")).is_err(),
            "{when}: the folder renamed"
        );
        let version = folder.parent().unwrap();
        assert!(
            std::fs::rename(version, version.with_file_name("0.4.9")).is_err(),
            "{when}: the version's folder renamed"
        );
    };
    refused("writing");
    stage.seal().unwrap();
    refused("sealed");
    // A reader that shares reads, as the loader does, may open it.
    let reader = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ)
        .open(&exe);
    assert!(reader.is_ok(), "{reader:?}");
    drop(reader);
    assert_eq!(stage.read_back().unwrap(), b"the installer's bytes");
    stage.discard();
    assert!(!folder.exists());
}

/// No other writer can open the file while it is written (the write handle shares reads only),
/// so only a reader can be there at the seal, and a reader does not stop it: the last handle
/// refuses writers, not readers.
#[test]
fn a_reader_does_not_stop_the_seal() {
    let scratch = Scratch::new("repair-seal-raced", &[]);
    let updates = scratch.path().join("updates");
    let mut stage = Stage::create(&updates, "0.4.1", "Clauseo_0.4.1_x64-setup.exe").unwrap();
    stage.write(b"bytes").unwrap();
    // Our write handle shares reads only, so another opener can only read, sharing writes.
    let other = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0b111)
        .open(stage.exe())
        .unwrap();
    stage.seal().unwrap();
    drop(other);
    stage.discard();
}

/// The folder's DACL is protected (nothing inherited from `updates/`) and has exactly two
/// entries, full control inherited by files and folders (`OICI`): this process's user and
/// SYSTEM. The installer in it inherits exactly those two, and nothing else.
#[test]
fn the_folder_is_this_users_and_systems_alone() {
    let scratch = Scratch::new("repair-dacl", &[]);
    let updates = scratch.path().join("updates");
    let stage = Stage::create(&updates, "0.4.1", "Clauseo_0.4.1_x64-setup.exe").unwrap();
    let sid = own_sid().unwrap();
    let folder = dacl_text(stage.folder());
    assert!(folder.starts_with("D:P"), "{folder}");
    assert!(folder.contains(&format!("(A;OICI;FA;;;{sid})")), "{folder}");
    assert!(folder.contains("(A;OICI;FA;;;SY)"), "{folder}");
    assert_eq!(folder.matches("(A;").count(), 2, "{folder}");
    assert!(!folder.contains(";ID;"), "nothing inherited: {folder}");
    let file = dacl_text(stage.exe());
    assert!(file.contains(&format!("(A;ID;FA;;;{sid})")), "{file}");
    assert!(file.contains("(A;ID;FA;;;SY)"), "{file}");
    assert_eq!(file.matches("(A;").count(), 2, "{file}");
    stage.discard();
}

/// The core claim: an image written through the attempt's handle starts while the sealed,
/// read-only handle still holds it (a copy of `cmd.exe`, run with `/c exit 0`), and the file
/// still refuses a writer afterwards, until the handle closes.
#[test]
fn the_installer_starts_while_the_read_only_handle_holds_it() {
    let scratch = Scratch::new("repair-launch-held", &[]);
    let updates = scratch.path().join("updates");
    let system = std::env::var_os("SystemRoot").map_or_else(
        || std::path::PathBuf::from(r"C:\Windows"),
        std::path::PathBuf::from,
    );
    let image = std::fs::read(system.join("System32").join("cmd.exe")).unwrap();
    let mut stage = Stage::create(&updates, "0.4.1", "tiny.exe").unwrap();
    stage.write(&image).unwrap();
    stage.seal().unwrap();
    assert_eq!(stage.read_back().unwrap(), image);
    let started = stage.launch(&["/c", "exit", "0"]);
    assert!(started.is_ok(), "{started:?}");
    let writer = std::fs::OpenOptions::new().write(true).open(stage.exe());
    assert!(
        writer.is_err(),
        "a writer opened the held file after the launch"
    );
    stage.discard();
}

/// Sealed, the folder is locked down: nothing can be created in it, its owner can't change its
/// DACL back, the installer still starts through the held handle (the launch test above seals
/// too), and once the handle closes the start's sweep still deletes it.
#[test]
fn the_sealed_folder_is_read_and_run_only_and_the_sweep_still_removes_it() {
    let scratch = Scratch::new("repair-locked", &[]);
    let updates = scratch.path().join("updates");
    let mut stage = Stage::create(&updates, "0.4.1", "Clauseo_0.4.1_x64-setup.exe").unwrap();
    stage.write(b"the installer's bytes").unwrap();
    stage.seal().unwrap();
    let folder = stage.folder().to_path_buf();
    assert!(std::fs::write(folder.join("version.dll"), b"planted").is_err());
    assert!(std::fs::create_dir(folder.join("sub")).is_err());
    // Protected, four entries: the user's two, OWNER RIGHTS', SYSTEM's (the rights may print as
    // aliases, so only their holders are read).
    let sddl = dacl_text(&folder);
    let sid = own_sid().unwrap();
    assert!(sddl.starts_with("D:P"), "{sddl}");
    assert_eq!(sddl.matches("(A;").count(), 4, "{sddl}");
    assert_eq!(sddl.matches(&format!(";;;{sid})")).count(), 2, "{sddl}");
    assert!(sddl.contains(";;;OW)"), "{sddl}");
    assert!(sddl.contains("(A;OICI;FA;;;SY)"), "{sddl}");
    // As the owner, the DACL can't be set again: OWNER RIGHTS gives read and execute only.
    assert!(super::super::lockdown::lock_down(&folder).is_err());
    drop(stage);
    assert_eq!(sweep(&updates), [Swept::Removed]);
    assert!(!folder.exists());
}

/// An attempt is never made through a junction at `updates/<version>`.
#[test]
fn an_attempt_refuses_a_junction_in_the_updates_folder() {
    let scratch = Scratch::new("repair-create-junction", &[]);
    let updates = scratch.path().join("updates");
    let elsewhere = scratch.path().join("elsewhere");
    std::fs::create_dir_all(&updates).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    junction(&updates.join("0.4.1"), &elsewhere);
    assert!(Stage::create(&updates, "0.4.1", "Clauseo_0.4.1_x64-setup.exe").is_err());
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
}

/// `updates/` itself as a junction to a folder elsewhere is never walked.
#[test]
fn the_sweep_never_follows_a_junction_for_updates() {
    let scratch = Scratch::new("repair-sweep-junction-updates", &[]);
    let elsewhere = scratch.path().join("elsewhere");
    let attempt = elsewhere.join("0.4.1").join("repair-0001");
    std::fs::create_dir_all(&attempt).unwrap();
    std::fs::write(attempt.join("Clauseo_0.4.1_x64-setup.exe"), b"keep me").unwrap();
    let updates = scratch.path().join("updates");
    junction(&updates, &elsewhere);
    assert_eq!(sweep(&updates), []);
    assert!(attempt.join("Clauseo_0.4.1_x64-setup.exe").exists());
}

/// Makes `link` a directory junction to `target` (`mklink /J`, no admin rights needed).
fn junction(link: &std::path::Path, target: &std::path::Path) {
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .status()
        .unwrap();
    assert!(made.success());
}

/// A `repair-*` junction to a folder elsewhere is left alone: its target's files stay.
#[test]
fn the_sweep_leaves_a_junction() {
    let scratch = Scratch::new("repair-sweep-junction", &[]);
    let updates = scratch.path().join("updates");
    let target = scratch.path().join("lawyers-files");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("brief.docx"), b"keep me").unwrap();
    std::fs::create_dir_all(updates.join("0.4.1")).unwrap();
    let link = updates.join("0.4.1").join("repair-junction");
    junction(&link, &target);
    assert_eq!(sweep(&updates), [Swept::Link]);
    assert!(target.join("brief.docx").exists());
    assert!(link.exists());
}

/// A folder whose installer is open without delete sharing (as a running one is) stays for
/// the next start.
#[test]
fn the_sweep_leaves_a_folder_in_use() {
    let scratch = Scratch::new("repair-sweep-in-use", &[]);
    let updates = scratch.path().join("updates");
    let folder = updates.join("0.4.1").join("repair-0001");
    std::fs::create_dir_all(&folder).unwrap();
    let exe = folder.join("Clauseo_0.4.1_x64-setup.exe");
    std::fs::write(&exe, b"installer").unwrap();
    let open = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ)
        .open(&exe)
        .unwrap();
    assert_eq!(sweep(&updates), [Swept::InUse]);
    assert!(exe.exists());
    drop(open);
    assert_eq!(sweep(&updates), [Swept::Removed]);
}

/// `path`'s DACL in SDDL.
#[expect(unsafe_code, reason = "FFI to read a folder's security descriptor")]
fn dacl_text(path: &std::path::Path) -> String {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
        SDDL_REVISION_1, SE_FILE_OBJECT,
    };
    use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
    use windows::core::{HSTRING, PWSTR};
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: a live HSTRING and out-pointer; the descriptor is freed below.
    let read = unsafe {
        GetNamedSecurityInfoW(
            &HSTRING::from(path.as_os_str()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            &raw mut descriptor,
        )
    };
    assert!(read.is_ok(), "{read:?}");
    let mut text = PWSTR::null();
    // SAFETY: the descriptor is valid; `text` is freed below.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &raw mut text,
            None,
        )
    }
    .unwrap();
    // SAFETY: a NUL-terminated string from the conversion.
    let sddl = unsafe { text.to_string() }.unwrap();
    // SAFETY: both allocated with LocalAlloc, freed once.
    unsafe {
        LocalFree(Some(HLOCAL(text.0.cast())));
        LocalFree(Some(HLOCAL(descriptor.0)));
    }
    sddl
}
