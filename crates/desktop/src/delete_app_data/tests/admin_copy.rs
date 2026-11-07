//! What the sources must keep for the admin copy of S15's set-up program (`admin_copy.nsh`, which
//! `hooks.nsh` includes): it is written from the installer's own payload straight into
//! `$PROGRAMFILES64\Clauseo\Sandbox` and `--adopt` runs from there with PR 2's arguments. The
//! per-user installer's outer, unelevated instance passes the lawyer's SID and data folder to an
//! elevated instance of itself, started with a private switch and waited for; that instance runs
//! no section of the template's before the admin section, checks the record before writing
//! anything, and quits after it. The per-machine installer does it every time. The per-user ask
//! and the outcomes are in `admin_copy_ask.rs`. The Windows VM runs the real thing.

use super::sources::{hook_code, line_with, read};

/// The code lines of `relative`, comments and blank lines left out, trimmed.
pub(super) fn code_of(relative: &str) -> Vec<String> {
    read(relative)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
        .map(str::to_owned)
        .collect()
}

/// The code lines of `admin_copy.nsh`, then `safe_temp.nsh` and `admin_elevated.nsh`, which it
/// includes.
pub(super) fn admin_code() -> Vec<String> {
    let mut code = code_of("windows/admin_copy.nsh");
    code.extend(code_of("windows/safe_temp.nsh"));
    code.extend(code_of("windows/admin_elevated.nsh"));
    code
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
    let copy = code_of("windows/admin_copy.nsh");
    let include = line_with(&copy, &["!include \"${__FILEDIR__}\\admin_elevated.nsh\""]);
    assert!(include < line_with(&copy, &["!macro "]), "{copy:?}");
}

#[test]
fn the_admin_copy_is_written_from_the_payload_into_program_files_and_adopt_runs_from_there() {
    let code = admin_code();
    line_with(
        &code,
        &["!define CLAUSEO_ADMIN_DIR \"$PROGRAMFILES64\\Clauseo\\Sandbox\""],
    );
    line_with(&code, &["!define CLAUSEO_HOOKS_DIR \"${__FILEDIR__}\""]);
    let body = macro_body(&code, "CLAUSEO_ADMIN_EXTRACT");
    assert_run(
        body,
        &[
            "ClearErrors",
            "SetOutPath \"${CLAUSEO_ADMIN_DIR}\"",
            "${IfNot} ${Errors}",
            "SetOverwrite try",
            "File \"/oname=clauseo-sandbox-setup.exe\" \"${CLAUSEO_HOOKS_DIR}\\..\\binaries\\\
             clauseo-sandbox-setup-${CLAUSEO_SETUP_TRIPLE}.exe\"",
            "SetOverwrite lastused",
            "${EndIf}",
        ],
    );
    // Then --adopt from there: the elevated branch's with ExecWait (plain NSIS), the per-machine
    // installer's started directly and waited for up to 10 minutes.
    let copy = macro_body(&code, "CLAUSEO_ADMIN_COPY");
    assert_run(
        copy,
        &[
            "StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}",
            "!insertmacro CLAUSEO_ADMIN_EXTRACT",
            "${IfNot} ${Errors}",
            "ExecWait '\"${CLAUSEO_ADMIN_DIR}\\clauseo-sandbox-setup.exe\" --adopt ${ARGS}' $R8",
            "${If} ${Errors}",
            "StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}",
        ],
    );
    let bounded = macro_body(&code, "CLAUSEO_ADMIN_COPY_BOUNDED");
    assert_run(
        bounded,
        &[
            "StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}",
            "!insertmacro CLAUSEO_ADMIN_EXTRACT",
            "${IfNot} ${Errors}",
            "StrCpy $R9 '\"${CLAUSEO_ADMIN_DIR}\\clauseo-sandbox-setup.exe\" --adopt ${ARGS}'",
        ],
    );
    line_with(bounded, &["kernel32::CreateProcessW(p 0, w R9,"]);
    line_with(
        bounded,
        &["WaitForSingleObject(p r1, i ${CLAUSEO_ADMIN_WAIT_MS})"],
    );
    assert_run(
        bounded,
        &["${If} $2 = 258", "StrCpy $R8 ${CLAUSEO_ADMIN_TIMED_OUT}"],
    );
    line_with(&code, &["!define CLAUSEO_ADMIN_WAIT_MS 600000"]);
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
        for lines in [body, copy, bounded] {
            assert!(
                lines.iter().all(|line| !line.contains(refused)),
                "{refused}: {lines:?}"
            );
        }
    }
    // The payload's only other copy is the template's own, beside the app: the hooks hold one
    // File, this one, inserted once by each installer.
    let all: Vec<String> = hook_code().into_iter().chain(code.clone()).collect();
    let files = all.iter().filter(|line| line.starts_with("File "));
    assert_eq!(files.count(), 1, "{all:?}");
    for name in ["CLAUSEO_ADMIN_COPY", "CLAUSEO_ADMIN_COPY_BOUNDED"] {
        let prefix = format!("!insertmacro {name} '");
        let inserts = all.iter().filter(|line| line.starts_with(&prefix));
        assert_eq!(inserts.count(), 1, "{name}: {all:?}");
    }
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
    // Clauseo running first (nothing written, exit 24), then the record: another user's copy
    // stays (failed), a newer copy stays (a line).
    let expected = [
        "Push $0",
        "Push $1",
        "Push $2",
        "Push $3",
        "Push $4",
        "Push $R1",
        "FindWindow $R5 \"${CLAUSEO_MAIN_WINDOW_CLASS}\"",
        "${If} $R5 <> 0",
        "DetailPrint \"$(adminCopyAppRunning)\"",
        "SetErrorLevel ${CLAUSEO_EXIT_APP_RUNNING}",
        "${IfNot} ${Silent}",
        "${AndIf} $PassiveMode <> 1",
        "${AndIf} $UpdateMode <> 1",
        "MessageBox MB_OK|MB_ICONEXCLAMATION \"$(adminCopyAppRunning)\"",
        "${EndIf}",
        "${Else}",
        "!insertmacro CLAUSEO_RECORD_CHECK \"\"",
        "${If} $R1 = 1",
        "StrCpy $R8 ${CLAUSEO_ADOPT_OTHER_ACCOUNT}",
        "!insertmacro CLAUSEO_ADMIN_COPY_MACHINE_SAYS",
        "${ElseIf} $R1 = 2",
        "DetailPrint \"$(adminCopyNewerKept)\"",
        "${Else}",
        "DetailPrint \"$(adminCopyWorking)\"",
        "!insertmacro CLAUSEO_ADMIN_COPY_BOUNDED '--per-machine --user-install-dir \"$INSTDIR\"'",
        "SetOutPath \"$INSTDIR\"",
        "!insertmacro CLAUSEO_ADMIN_COPY_MACHINE_SAYS",
        "${EndIf}",
        "${EndIf}",
        "Pop $R1",
        "Pop $4",
        "Pop $3",
        "Pop $2",
        "Pop $1",
        "Pop $0",
    ];
    assert_eq!(body[1..], expected, "{body:?}");
    // A box only interactively: never under /S (23, also past the wait), /P or /UPDATE.
    let says = macro_body(&admin, "CLAUSEO_ADMIN_COPY_MACHINE_SAYS");
    let expected = [
        "${If} $R8 = ${CLAUSEO_ADOPT_DONE}",
        "DetailPrint \"$(adminCopyDone)\"",
        "${ElseIf} $R8 = ${CLAUSEO_ADOPT_DOWNGRADE}",
        "DetailPrint \"$(adminCopyNewerKept)\"",
        "${ElseIf} $R8 = ${CLAUSEO_ADMIN_TIMED_OUT}",
        "DetailPrint \"$(adminCopyStillRunning)\"",
        "StrCpy $ClauseoAdminTimedOut 1",
        "${If} ${Silent}",
        "SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_FAILED}",
        "${EndIf}",
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
    // A newer x.y.z, part by part as numbers, or a release over this installer's pre-release of
    // the same x.y.z.
    let newer = macro_body(&code, "CLAUSEO_NEWER_THAN_THIS");
    let expected = [
        "StrCpy $2 $R5",
        "StrCpy $3 \"${VERSION}\"",
        "StrCpy $4 0",
        "!insertmacro CLAUSEO_COMPARE_PART",
        "!insertmacro CLAUSEO_COMPARE_PART",
        "!insertmacro CLAUSEO_COMPARE_PART",
        "${If} $4 = 0",
        "StrCpy $0 $2 1",
        "StrCpy $1 $3 1",
        "${If} $1 == \"-\"",
        "${AndIf} $0 != \"-\"",
        "StrCpy $4 1",
        "${EndIf}",
        "${EndIf}",
        "${If} $4 = 1",
        "StrCpy $R1 2",
        "${EndIf}",
    ];
    assert_eq!(newer[1..], expected, "{newer:?}");
    let part = macro_body(&code, "CLAUSEO_COMPARE_PART");
    assert_run(
        part,
        &[
            "${If} $R7 > $R8",
            "StrCpy $4 1",
            "${ElseIf} $R7 < $R8",
            "StrCpy $4 -1",
        ],
    );
    // No plug-in on the elevated instance's own path: each call loads a DLL from its plug-in
    // folder, under %TEMP%.
    for name in [
        "CLAUSEO_RECORD_CHECK",
        "CLAUSEO_NEWER_THAN_THIS",
        "CLAUSEO_COMPARE_PART",
        "CLAUSEO_TAKE_NUMBER",
        "CLAUSEO_ADMIN_EXTRACT",
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
    for name in ["CLAUSEO_ELEVATED_BRANCH", "CLAUSEO_ADMIN_COPY_MACHINE"] {
        let body = macro_body(&code, name);
        let check = line_with(body, &["!insertmacro CLAUSEO_RECORD_CHECK"]);
        let copy = line_with(
            body,
            &["!insertmacro CLAUSEO_ADMIN_COPY", "--user-install-dir"],
        );
        assert!(check < copy, "{name}: {body:?}");
    }
}

/// The codes in the first table after `heading` in firm-it.md.
fn firm_it_codes(firm: &str, heading: &str) -> Vec<u8> {
    let (_, rest) = firm
        .split_once(heading)
        .unwrap_or_else(|| panic!("no {heading:?}"));
    rest.lines()
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        .filter_map(|line| line.split('|').nth(1)?.trim().parse().ok())
        .collect()
}

#[test]
fn the_installers_exit_codes_are_the_ones_firm_it_lists_by_mode() {
    let code = admin_code();
    // The values themselves: admin_copy_ask.rs's the_installers_exit_codes_are_their_own.
    let firm = read("../../docs/firm-it.md");
    assert_eq!(
        firm_it_codes(&firm, "What a silent (`/S`) install can return:"),
        [22, 23, 24, 25]
    );
    assert_eq!(
        firm_it_codes(&firm, "What an interactive install can also return:"),
        [24]
    );
    // Per user, 24 only past the /S and /P skip, which sets 22; 23 under /S also when the wait
    // ran out.
    let ask = macro_body(&code, ASK);
    let silent = line_with(ask, &["${ElseIf} ${Silent}"]);
    let interactive = silent + line_with(&ask[silent..], &["${Else}"]);
    assert!(interactive < line_with(ask, &["SetErrorLevel ${CLAUSEO_EXIT_APP_RUNNING}"]));
    // Per machine, 24 in every mode: set before the box's mode test, not inside it.
    let machine = macro_body(&code, "CLAUSEO_ADMIN_COPY_MACHINE");
    assert_run(
        machine,
        &[
            "${If} $R5 <> 0",
            "DetailPrint \"$(adminCopyAppRunning)\"",
            "SetErrorLevel ${CLAUSEO_EXIT_APP_RUNNING}",
            "${IfNot} ${Silent}",
        ],
    );
    // 25 in every mode, the box skipped under /S by /SD.
    let temp = macro_body(&code, "CLAUSEO_SAFE_PLUGINSDIR");
    assert_run(
        temp,
        &[
            "!if \"${INSTALLMODE}\" == \"perMachine\"",
            "MessageBox MB_OK|MB_ICONSTOP \"$(installTempFailed)\" /SD IDOK",
            "SetErrorLevel ${CLAUSEO_EXIT_TEMP_FAILED}",
        ],
    );
    let says = macro_body(&code, "CLAUSEO_ADMIN_COPY_MACHINE_SAYS");
    assert_run(
        says,
        &[
            "${ElseIf} $R8 = ${CLAUSEO_ADMIN_TIMED_OUT}",
            "DetailPrint \"$(adminCopyStillRunning)\"",
            "StrCpy $ClauseoAdminTimedOut 1",
            "${If} ${Silent}",
            "SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_FAILED}",
        ],
    );
}
