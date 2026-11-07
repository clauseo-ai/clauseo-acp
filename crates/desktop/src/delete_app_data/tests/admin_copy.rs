//! What the sources must keep for the admin copy of S15's set-up program (`admin_copy.nsh`, which
//! `hooks.nsh` includes): it is written from the installer's own payload straight into
//! `$PROGRAMFILES64\Clauseo\Sandbox` and `--adopt` runs from there with PR 2's arguments. The
//! per-user installer's outer, unelevated instance passes the lawyer's SID and data folder to an
//! elevated instance of itself, started with a private switch and waited for; that instance runs
//! no section of the template's before the admin section, checks the record before writing
//! anything, and quits after it. The per-machine installer does it every time. The per-user ask
//! and the outcomes are in `admin_copy_ask.rs`. The Windows VM runs the real thing.

use super::sources::{hook_code, line_with, read};

/// `admin_copy.nsh`'s lines, comments and blank lines left out, trimmed.
pub(super) fn admin_code() -> Vec<String> {
    read("windows/admin_copy.nsh")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
        .map(str::to_owned)
        .collect()
}

/// The code lines of `!macro NAME`, up to its `!macroend`.
pub(super) fn macro_body<'a>(code: &'a [String], name: &str) -> &'a [String] {
    let open = format!("!macro {name}");
    let start = code
        .iter()
        .position(|line| *line == open || line.starts_with(&format!("{open} ")))
        .unwrap_or_else(|| panic!("no {open}"));
    let end = start + line_with(&code[start..], &["!macroend"]);
    &code[start..end]
}

/// The lines from the one equal to `first` to the next one equal to `last`, both included.
pub(super) fn block<'a>(body: &'a [String], first: &str, last: &str) -> &'a [String] {
    let start = body
        .iter()
        .position(|line| line == first)
        .unwrap_or_else(|| panic!("no {first:?} in {body:?}"));
    let end = start
        + body[start..]
            .iter()
            .position(|line| line == last)
            .unwrap_or_else(|| panic!("no {last:?} after {first:?}"));
    &body[start..=end]
}

/// Asserts `expected` stands in `body` from the line equal to its first.
pub(super) fn assert_run(body: &[String], expected: &[&str]) {
    let start = body
        .iter()
        .position(|line| line == expected[0])
        .unwrap_or_else(|| panic!("no {:?} in {body:?}", expected[0]));
    assert_eq!(body[start..start + expected.len()], *expected, "{body:?}");
}

pub(super) const ASK: &str = "CLAUSEO_ASK_FOR_THE_ADMIN_SECTION";

#[test]
fn hooks_nsh_includes_the_admin_copy_at_file_scope() {
    let code = hook_code();
    let include = line_with(&code, &["!include \"${__FILEDIR__}\\admin_copy.nsh\""]);
    let first_macro = line_with(&code, &["!macro "]);
    assert!(include < first_macro, "{code:?}");
}

#[test]
fn the_admin_copy_is_written_from_the_payload_into_program_files_and_adopt_runs_from_there() {
    let code = admin_code();
    line_with(
        &code,
        &["!define CLAUSEO_ADMIN_DIR \"$PROGRAMFILES64\\Clauseo\\Sandbox\""],
    );
    line_with(&code, &["!define CLAUSEO_HOOKS_DIR \"${__FILEDIR__}\""]);
    let body = macro_body(&code, "CLAUSEO_ADMIN_COPY");
    assert_run(
        body,
        &[
            "StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}",
            "ClearErrors",
            "SetOutPath \"${CLAUSEO_ADMIN_DIR}\"",
            "${IfNot} ${Errors}",
            "SetOverwrite try",
            "File \"/oname=clauseo-sandbox-setup.exe\" \"${CLAUSEO_HOOKS_DIR}\\..\\binaries\\\
             clauseo-sandbox-setup-${CLAUSEO_SETUP_TRIPLE}.exe\"",
            "SetOverwrite lastused",
            "${IfNot} ${Errors}",
            "ExecWait '\"${CLAUSEO_ADMIN_DIR}\\clauseo-sandbox-setup.exe\" --adopt ${ARGS}' $R8",
            "${If} ${Errors}",
            "StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}",
        ],
    );
    // The build sidecars.mjs writes for each Windows target (binaries/<name>-<triple>.exe).
    for (arch, triple) in [
        ("x64", "x86_64-pc-windows-msvc"),
        ("arm64", "aarch64-pc-windows-msvc"),
    ] {
        let test = line_with(body, &[&format!("\"${{ARCH}}\" == \"{arch}\"")]);
        assert_eq!(
            body[test + 1],
            format!("!define CLAUSEO_SETUP_TRIPLE \"{triple}\""),
            "{body:?}"
        );
    }
    // Nothing a lawyer's process can write is copied or run: no $INSTDIR, $TEMP or plug-in
    // folder, no copy, rename or shell in between.
    for refused in [
        "$INSTDIR",
        "$TEMP",
        "$PLUGINSDIR",
        "CopyFiles",
        "Rename",
        "ExecShell",
        "cmd",
    ] {
        assert!(
            body.iter().all(|line| !line.contains(refused)),
            "{refused}: {body:?}"
        );
    }
    // The payload's only other copy is the template's own, beside the app: the hooks hold one
    // File, this one, inserted once by each installer.
    let all: Vec<String> = hook_code().into_iter().chain(code.clone()).collect();
    let files = all.iter().filter(|line| line.starts_with("File "));
    assert_eq!(files.count(), 1, "{all:?}");
    let inserts = all
        .iter()
        .filter(|line| line.starts_with("!insertmacro CLAUSEO_ADMIN_COPY '"));
    assert_eq!(inserts.count(), 2, "{all:?}");
}

#[test]
fn the_switch_counts_only_as_a_whole_argument() {
    let code = admin_code();
    line_with(
        &code,
        &["!define CLAUSEO_ADMIN_SWITCH \"/CLAUSEO_ADMIN_SECTION\""],
    );
    // The template's own switches (.onInit, .onInstSuccess) and NSIS's: GetOptions must never
    // take a private one for one of them.
    for private in [
        "/CLAUSEO_ADMIN_SECTION",
        "/CLAUSEO_OWNER_SID=",
        "/CLAUSEO_DATA_DIR=",
    ] {
        for theirs in ["/P", "/NS", "/UPDATE", "/R", "/ARGS", "/S", "/D="] {
            assert!(!private.starts_with(theirs), "{private} {theirs}");
        }
    }
    let body = macro_body(&code, "CLAUSEO_ADMIN_SWITCH_GIVEN");
    let expected = [
        "StrCpy $R0 0",
        "${GetParameters} $R1",
        "ClearErrors",
        "${GetOptions} $R1 \"${CLAUSEO_ADMIN_SWITCH}\" $R1",
        "${IfNot} ${Errors}",
        "${AndIf} $R1 == \"\"",
        "StrCpy $R0 1",
        "${EndIf}",
    ];
    assert_eq!(body[1..], expected, "{body:?}");
}

#[test]
fn with_the_switch_the_first_section_unselects_every_section_but_install() {
    let code = admin_code();
    let hooks = hook_code();
    let sections: Vec<&String> = hooks
        .iter()
        .chain(&code)
        .filter(|line| line.starts_with("Section "))
        .collect();
    // One section, hidden ("-": always run, never listed).
    assert_eq!(sections, ["Section \"-CLAUSEO_ADMIN_SECTION_ONLY\""]);
    let body = block(&code, sections[0], "SectionEnd");
    let expected = [
        "!insertmacro CLAUSEO_ADMIN_SWITCH_GIVEN",
        "${If} $R0 = 1",
        "StrCpy $R2 0",
        "${Do}",
        "ClearErrors",
        "SectionGetText $R2 $R1",
        "${If} ${Errors}",
        "${ExitDo}",
        "${EndIf}",
        "${If} $R1 != \"Install\"",
        "SectionSetFlags $R2 0",
        "${EndIf}",
        "IntOp $R2 $R2 + 1",
        "${Loop}",
        "${EndIf}",
    ];
    assert_run(body, &expected);
}

#[test]
fn the_elevated_instance_runs_adopt_with_the_outer_instances_owner_and_then_quits() {
    let code = admin_code();
    let body = macro_body(&code, "NSIS_HOOK_PREINSTALL");
    let expected = [
        "!if \"${INSTALLMODE}\" == \"currentUser\"",
        "Push $R0",
        "Push $R1",
        "Push $R2",
        "Push $R3",
        "!insertmacro CLAUSEO_ADMIN_SWITCH_GIVEN",
        "${If} $R0 = 1",
        "${GetParameters} $R1",
        "${GetOptions} $R1 \"${CLAUSEO_OWNER_SID_OPTION}\" $R2",
        "${GetOptions} $R1 \"${CLAUSEO_DATA_DIR_OPTION}\" $R3",
        "StrCpy $R0 1",
        "!insertmacro CLAUSEO_CHECK_SID $R2",
        "!insertmacro CLAUSEO_CHECK_PATH $R3",
        "!insertmacro CLAUSEO_CHECK_PATH $INSTDIR",
        "StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}",
        "${If} $R0 = 1",
        "!insertmacro CLAUSEO_RECORD_CHECK $R2",
        "${If} $R1 = 1",
        "IntOp $R8 ${CLAUSEO_ADMIN_EXIT_BASE} + ${CLAUSEO_ADOPT_OTHER_ACCOUNT}",
        "${ElseIf} $R1 = 2",
        "StrCpy $R8 ${CLAUSEO_ADMIN_NEWER_KEPT}",
        "${Else}",
        "!insertmacro CLAUSEO_ADMIN_COPY '--owner-sid $R2 --user-install-dir \"$INSTDIR\" \
         --data-dir \"$R3\"'",
        "${If} $R8 <> ${CLAUSEO_ADMIN_NOT_RUN}",
        "IntOp $R8 $R8 + ${CLAUSEO_ADMIN_EXIT_BASE}",
        "${EndIf}",
        "${EndIf}",
        "${EndIf}",
        "SetErrorLevel $R8",
        "Quit",
        "${EndIf}",
        "Pop $R3",
        "Pop $R2",
        "Pop $R1",
        "Pop $R0",
        "!endif",
    ];
    assert_eq!(body[1..], expected, "{body:?}");
    // Nothing of the elevated account's own: no $LOCALAPPDATA, profile or token read there.
    for refused in [
        "$LOCALAPPDATA",
        "$PROFILE",
        "CLAUSEO_OWN_SID",
        "OpenProcessToken",
    ] {
        assert!(body.iter().all(|line| !line.contains(refused)), "{refused}");
    }
    // The checks keep the command line whole: a SID of digits and dashes, paths with no quote
    // and no trailing backslash.
    let sid = macro_body(&code, "CLAUSEO_CHECK_SID");
    line_with(sid, &["${IfNot} $R5 S== \"S-1-\""]);
    line_with(sid, &["${StrLoc} $R7 \"0123456789-\" $R5 \">\""]);
    let path = macro_body(&code, "CLAUSEO_CHECK_PATH");
    line_with(path, &["${OrIf} $R5 == \"\\\""]);
    line_with(path, &["${StrLoc} $R5 \"${VALUE}\" \"$\\\"\" \">\""]);
}

#[test]
fn the_per_machine_installer_writes_the_admin_copy_and_adopts_it_every_time() {
    let code = hook_code();
    let post = macro_body(&code, "NSIS_HOOK_POSTINSTALL");
    let machine = block(post, "!if \"${INSTALLMODE}\" == \"perMachine\"", "!else");
    let insert = line_with(machine, &["!insertmacro CLAUSEO_ADMIN_COPY_MACHINE"]);
    // Unconditional: every ${If} before it is closed again, so no mode (update, silent) skips it.
    let opened = machine[..insert]
        .iter()
        .filter(|line| line.starts_with("${If}") || line.starts_with("${IfNot}"))
        .count();
    let closed = machine[..insert]
        .iter()
        .filter(|line| *line == "${EndIf}")
        .count();
    assert_eq!(opened, closed, "{machine:?}");
    let admin = admin_code();
    let body = macro_body(&admin, "CLAUSEO_ADMIN_COPY_MACHINE");
    // The record first: another user's copy stays (failed), a newer copy stays (a line).
    let expected = [
        "!insertmacro CLAUSEO_RECORD_CHECK \"\"",
        "${If} $R1 = 1",
        "StrCpy $R8 ${CLAUSEO_ADOPT_OTHER_ACCOUNT}",
        "!insertmacro CLAUSEO_ADMIN_COPY_MACHINE_SAYS",
        "${ElseIf} $R1 = 2",
        "DetailPrint \"$(adminCopyNewerKept)\"",
        "${Else}",
        "DetailPrint \"$(adminCopyWorking)\"",
        "!insertmacro CLAUSEO_ADMIN_COPY '--per-machine --user-install-dir \"$INSTDIR\"'",
        "SetOutPath \"$INSTDIR\"",
        "!insertmacro CLAUSEO_ADMIN_COPY_MACHINE_SAYS",
        "${EndIf}",
    ];
    assert_eq!(body[1..], expected, "{body:?}");
    // A box only interactively: never under /S (23), /P or /UPDATE.
    let says = macro_body(&admin, "CLAUSEO_ADMIN_COPY_MACHINE_SAYS");
    let expected = [
        "${If} $R8 = ${CLAUSEO_ADOPT_DONE}",
        "DetailPrint \"$(adminCopyDone)\"",
        "${ElseIf} $R8 = ${CLAUSEO_ADOPT_DOWNGRADE}",
        "DetailPrint \"$(adminCopyNewerKept)\"",
        "${Else}",
        "DetailPrint \"$(adminCopyMachineFailed)\"",
        "${If} ${Silent}",
        "SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_FAILED}",
        "${ElseIf} $PassiveMode <> 1",
        "${AndIf} $UpdateMode <> 1",
        "MessageBox MB_OK|MB_ICONEXCLAMATION \"$(adminCopyMachineFailed)\"",
        "${EndIf}",
        "${EndIf}",
    ];
    assert_eq!(says[1..], expected, "{says:?}");
}

#[test]
fn the_record_is_checked_before_anything_is_written_into_the_admin_folder() {
    let code = admin_code();
    // PR 2's names (record_values.rs): who placed the copy, the set-up's owner, the copy's
    // version, in HKLM\SOFTWARE\Clauseo\Sandbox.
    for define in [
        "!define CLAUSEO_RECORD_KEY \"SOFTWARE\\Clauseo\\Sandbox\"",
        "!define CLAUSEO_RECORD_COPY_OWNER \"AdminCopyOwner\"",
        "!define CLAUSEO_RECORD_MACHINE \"machine\"",
        "!define CLAUSEO_RECORD_OWNER_SID \"OwnerSid\"",
        "!define CLAUSEO_RECORD_VERSION \"AdminCopyVersion\"",
    ] {
        line_with(&code, &[define]);
    }
    let body = macro_body(&code, "CLAUSEO_RECORD_CHECK");
    let expected = [
        "StrCpy $R1 0",
        "ReadRegStr $R5 HKLM \"${CLAUSEO_RECORD_KEY}\" \"${CLAUSEO_RECORD_COPY_OWNER}\"",
        "${If} $R5 != \"\"",
        "${If} \"${OWNER}\" == \"\"",
        "${If} $R5 S!= \"${CLAUSEO_RECORD_MACHINE}\"",
        "StrCpy $R1 1",
        "${EndIf}",
        "${ElseIf} $R5 S!= \"${OWNER}\"",
        "StrCpy $R1 1",
        "${EndIf}",
        "${EndIf}",
        "${If} \"${OWNER}\" != \"\"",
        "ReadRegStr $R5 HKLM \"${CLAUSEO_RECORD_KEY}\" \"${CLAUSEO_RECORD_OWNER_SID}\"",
        "${If} $R5 != \"\"",
        "${AndIf} $R5 S!= \"${OWNER}\"",
        "StrCpy $R1 1",
        "${EndIf}",
        "${EndIf}",
        "${If} $R1 = 0",
        "ReadRegStr $R5 HKLM \"${CLAUSEO_RECORD_KEY}\" \"${CLAUSEO_RECORD_VERSION}\"",
        "${If} $R5 != \"\"",
        "!insertmacro CLAUSEO_NEWER_THAN_THIS",
        "${EndIf}",
        "${EndIf}",
    ];
    assert_eq!(body[1..], expected, "{body:?}");
    // A newer x.y.z, or a release over this installer's pre-release of the same x.y.z.
    let newer = macro_body(&code, "CLAUSEO_NEWER_THAN_THIS");
    assert_run(
        newer,
        &[
            "${VersionCompare} $R5 $R9 $R6",
            "${If} $R6 = 1",
            "StrCpy $R1 2",
            "${ElseIf} $R6 = 0",
            "${AndIf} $R7 = 1",
            "StrCpy $R1 2",
            "${EndIf}",
        ],
    );
    // No plug-in on the elevated instance's own path: each call loads a DLL from its plug-in
    // folder, under %TEMP%.
    for name in [
        "CLAUSEO_ADMIN_SWITCH_GIVEN",
        "NSIS_HOOK_PREINSTALL",
        "CLAUSEO_RECORD_CHECK",
        "CLAUSEO_NEWER_THAN_THIS",
        "CLAUSEO_CHECK_SID",
        "CLAUSEO_CHECK_PATH",
        "CLAUSEO_ADMIN_COPY",
    ] {
        let body = macro_body(&code, name);
        assert!(
            body.iter().all(|line| !line.contains("::")),
            "{name}: {body:?}"
        );
    }
    // Each use checks the record before its CLAUSEO_ADMIN_COPY: the elevated instance's and the
    // per-machine installer's.
    for name in ["NSIS_HOOK_PREINSTALL", "CLAUSEO_ADMIN_COPY_MACHINE"] {
        let body = macro_body(&code, name);
        let check = line_with(body, &["!insertmacro CLAUSEO_RECORD_CHECK"]);
        let copy = line_with(body, &["!insertmacro CLAUSEO_ADMIN_COPY '"]);
        assert!(check < copy, "{name}: {body:?}");
    }
}

#[test]
fn the_section_order_the_hidden_section_relies_on_is_tauri_cli_2_12_2s() {
    // installer.nsi in tauri-cli 2.12.2 includes the hooks at line 36, before Section WebView2
    // (553) and Section Install (645), so the hidden section is the script's first. That was
    // checked by rendering that template; a Tauri upgrade must check it again and move this pin.
    let ui = read("../../ui/package.json");
    assert!(
        ui.contains("\"@tauri-apps/cli\": \"2.12.2\","),
        "the Tauri CLI moved: check the template's section order"
    );
    line_with(
        &hook_code(),
        &["!include \"${__FILEDIR__}\\admin_copy.nsh\""],
    );
    assert!(read("windows/hooks.nsh").contains("(tauri-cli 2.12.2,"));
    // The section sits at file scope, outside every macro.
    let code = admin_code();
    let section = line_with(&code, &["Section \"-CLAUSEO_ADMIN_SECTION_ONLY\""]);
    let opened = code[..section]
        .iter()
        .filter(|line| line.starts_with("!macro "))
        .count();
    let closed = code[..section]
        .iter()
        .filter(|line| *line == "!macroend")
        .count();
    assert_eq!(opened, closed, "{code:?}");
}
