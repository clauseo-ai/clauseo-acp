//! The x64 per-machine installer on an ARM64 computer (`CLAUSEO_NATIVE_ARM64_NOTICE`, `hooks.nsh`):
//! one details line naming the arm64 per-machine installer as the native one, and nothing else. It
//! never blocks, prompts or sets an exit code, so a silent deployment (Intune, SCCM) still reads
//! as a success; the per-user installer says nothing, since the app's updater moves it.

use super::admin_copy::{block, macro_body};
use super::sources::{hook_code, line_with};

const NOTICE: &str = "CLAUSEO_NATIVE_ARM64_NOTICE";

#[test]
fn the_per_machine_installer_inserts_the_arm64_notice_and_the_per_user_one_does_not() {
    let code = hook_code();
    let post = macro_body(&code, "NSIS_HOOK_POSTINSTALL");
    let insert = format!("!insertmacro {NOTICE}");
    let machine = block(post, "!if \"${INSTALLMODE}\" == \"perMachine\"", "!else");
    let at = line_with(machine, &[&insert]);
    // Unconditional within the per-machine block, and before the admin copy, whose exit codes it
    // leaves alone.
    let opened = machine[..at]
        .iter()
        .filter(|line| line.starts_with("${If}") || line.starts_with("${IfNot}"))
        .count();
    let closed = machine[..at]
        .iter()
        .filter(|line| *line == "${EndIf}")
        .count();
    assert_eq!(opened, closed, "{machine:?}");
    assert!(at < line_with(machine, &["!insertmacro CLAUSEO_ADMIN_COPY_MACHINE"]));
    assert_eq!(
        post.iter().filter(|line| **line == insert).count(),
        1,
        "{post:?}"
    );
}

#[test]
fn the_arm64_notice_is_one_details_line_from_the_x64_installer_on_a_native_arm64_machine() {
    let code = hook_code();
    let body = macro_body(&code, NOTICE);
    // Compiled into the x64 installer only.
    let x64 = line_with(body, &["!if \"${ARCH}\" == \"x64\""]);
    // The native machine from IsWow64Process2, IMAGE_FILE_MACHINE_ARM64, and the call's success.
    let call = line_with(
        body,
        &["System::Call", "kernel32::IsWow64Process2(", ".R3", "i .R1"],
    );
    let success = line_with(body, &["${If} $R1 <> 0"]);
    let arm64 = line_with(body, &["${AndIf} $R3 = 0xAA64"]);
    let says: Vec<&String> = body
        .iter()
        .filter(|line| line.starts_with("DetailPrint"))
        .collect();
    assert_eq!(says.len(), 1, "{body:?}");
    let line = line_with(body, &["DetailPrint"]);
    assert!(
        says[0].contains("Clauseo_${VERSION}_arm64-machine-setup.exe"),
        "{}",
        says[0]
    );
    assert!(x64 < call && call < success && success < arm64 && arm64 < line);
    assert_eq!(body[line + 1], "${EndIf}");
    // No language string (English only, as the other plain details lines), and nothing that
    // blocks, prompts, ends or changes the exit code.
    assert!(!says[0].contains("$("), "{}", says[0]);
    for refused in [
        "SetErrorLevel",
        "MessageBox",
        "Abort",
        "Quit",
        "Exec",
        "SetErrors",
    ] {
        assert!(
            body.iter().all(|line| !line.contains(refused)),
            "{refused}: {body:?}"
        );
    }
    // The registers it uses come back as they were.
    let pushes: Vec<&str> = body
        .iter()
        .filter_map(|line| line.strip_prefix("Push "))
        .collect();
    let mut pops: Vec<&str> = body
        .iter()
        .filter_map(|line| line.strip_prefix("Pop "))
        .collect();
    pops.reverse();
    assert_eq!(pushes, ["$R1", "$R2", "$R3"]);
    assert_eq!(pushes, pops);
}
