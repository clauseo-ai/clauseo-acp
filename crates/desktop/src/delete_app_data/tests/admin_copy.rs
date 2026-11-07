//! What the sources must keep for the admin copy of S15's set-up program (hooks.nsh, "The admin
//! copy"): it is written from the installer's own payload straight into
//! `$PROGRAMFILES64\Clauseo\Sandbox` and `--adopt` runs from there; the per-user installer does it
//! in an elevated instance of itself, started with a private switch and gated on it, never in
//! update, silent or passive mode; the per-machine installer does it every time; and every exit
//! code of `--adopt` has its outcome. The Windows VM runs the real thing.

use super::sources::{hook_code, line_with, read};

/// The code lines of `!macro NAME`, up to its `!macroend`.
fn macro_body<'a>(code: &'a [String], name: &str) -> &'a [String] {
    let open = format!("!macro {name}");
    let start = code
        .iter()
        .position(|line| *line == open || line.starts_with(&format!("{open} ")))
        .unwrap_or_else(|| panic!("no {open} in hooks.nsh"));
    let end = start + line_with(&code[start..], &["!macroend"]);
    &code[start..end]
}

/// The lines from the one equal to `first` to the next one equal to `last`, both included.
fn block<'a>(body: &'a [String], first: &str, last: &str) -> &'a [String] {
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

const ADMIN_COPY: &str = "CLAUSEO_ADMIN_COPY";
const ASK: &str = "CLAUSEO_ASK_FOR_THE_ADMIN_SECTION";

#[test]
fn the_admin_copy_is_written_from_the_payload_into_program_files_and_adopt_runs_from_there() {
    let code = hook_code();
    line_with(
        &code,
        &["!define CLAUSEO_ADMIN_DIR \"$PROGRAMFILES64\\Clauseo\\Sandbox\""],
    );
    line_with(&code, &["!define CLAUSEO_HOOKS_DIR \"${__FILEDIR__}\""]);
    let body = macro_body(&code, ADMIN_COPY);
    let expected = [
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
    ];
    let start = line_with(body, &["StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}"]);
    assert_eq!(body[start..start + expected.len()], expected, "{body:?}");
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
    let files = code.iter().filter(|line| line.starts_with("File "));
    assert_eq!(files.count(), 1, "{code:?}");
    let inserts = code
        .iter()
        .filter(|line| line.starts_with(&format!("!insertmacro {ADMIN_COPY} ")));
    assert_eq!(inserts.count(), 2, "{code:?}");
}

#[test]
fn the_elevated_instance_is_the_admin_section_only_when_the_switch_is_given_and_then_quits() {
    let code = hook_code();
    line_with(
        &code,
        &["!define CLAUSEO_ADMIN_SWITCH \"/CLAUSEO_ADMIN_SECTION\""],
    );
    // The template's own switches (.onInit, .onInstSuccess) and NSIS's: GetOptions must never
    // take the private one for one of them.
    for theirs in ["/P", "/NS", "/UPDATE", "/R", "/ARGS", "/S", "/D="] {
        assert!(!"/CLAUSEO_ADMIN_SECTION".starts_with(theirs), "{theirs}");
    }
    let body = macro_body(&code, "NSIS_HOOK_PREINSTALL");
    let expected = [
        "!if \"${INSTALLMODE}\" == \"currentUser\"",
        "Push $R0",
        "${GetOptions} $CMDLINE \"${CLAUSEO_ADMIN_SWITCH}\" $R0",
        "${IfNot} ${Errors}",
        "!insertmacro CLAUSEO_ADMIN_COPY '--user-install-dir \"$INSTDIR\" --data-dir \
         \"$LOCALAPPDATA\\${BUNDLEID}\"'",
        "${If} $R8 <> ${CLAUSEO_ADMIN_NOT_RUN}",
        "IntOp $R8 $R8 + ${CLAUSEO_ADMIN_EXIT_BASE}",
        "${EndIf}",
        "SetErrorLevel $R8",
        "Quit",
        "${EndIf}",
        "Pop $R0",
        "!endif",
    ];
    assert_eq!(body[1..], expected, "{body:?}");
}

#[test]
fn the_per_user_installer_asks_once_by_starting_itself_elevated_and_never_in_update_or_silent() {
    let code = hook_code();
    let post = macro_body(&code, "NSIS_HOOK_POSTINSTALL");
    let tail: Vec<&str> = post[post.len() - 3..].iter().map(String::as_str).collect();
    assert_eq!(
        tail,
        ["!else", &format!("!insertmacro {ASK}"), "!endif"],
        "{post:?}"
    );
    let body = macro_body(&code, ASK);
    // Update first, doing nothing; then /S and /P, a details line and, for /S, the exit code.
    let expected = [
        "${If} $UpdateMode = 1",
        "${ElseIf} ${Silent}",
        "${OrIf} $PassiveMode = 1",
        "DetailPrint \"$(adminCopySkippedSilent)\"",
        "${If} ${Silent}",
        "SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_SKIPPED}",
        "${EndIf}",
        "${Else}",
    ];
    let start = line_with(body, &["${If} $UpdateMode = 1"]);
    assert_eq!(body[start..start + expected.len()], expected, "{body:?}");
    let interactive = start + expected.len();
    // The one elevation: this installer, "runas", the switch, /UPDATE so its WebView2 section
    // installs nothing, /S, and /D= last (NSIS reads the rest of the line as the folder).
    let calls: Vec<usize> = (0..body.len())
        .filter(|&i| body[i].contains("ShellExecuteExW"))
        .collect();
    assert_eq!(calls.len(), 1, "{body:?}");
    let call = calls[0];
    assert!(interactive < call, "{body:?}");
    let info = &body[call - 1];
    for part in [
        "w \"runas\"",
        "w \"$EXEPATH\"",
        "w \"/S /UPDATE ${CLAUSEO_ADMIN_SWITCH} /D=$INSTDIR\"",
        "w \"$SYSDIR\"",
        "i 0x800440",
    ] {
        assert!(info.contains(part), "{part}: {info}");
    }
    assert!(
        body[..interactive]
            .iter()
            .all(|line| !line.contains("MessageBox")),
        "{body:?}"
    );
}

#[test]
fn the_installers_exit_codes_are_their_own() {
    let code = hook_code();
    // Every installer exit code the hooks define, by its !define line.
    let codes: Vec<(String, u32)> = code
        .iter()
        .filter_map(|line| line.strip_prefix("!define CLAUSEO_EXIT_"))
        .map(|rest| {
            let (name, value) = rest.split_once(' ').unwrap();
            (name.to_owned(), value.parse().unwrap())
        })
        .collect();
    let admin: Vec<&(String, u32)> = codes
        .iter()
        .filter(|(name, _)| name.starts_with("ADMIN_COPY_"))
        .collect();
    assert_eq!(
        admin,
        [
            &("ADMIN_COPY_SKIPPED".to_owned(), 22),
            &("ADMIN_COPY_FAILED".to_owned(), 23)
        ]
    );
    // Each installer code once (#638's 20 and 21 join these when both land).
    let mut values: Vec<u32> = codes.iter().map(|(_, value)| *value).collect();
    values.sort_unstable();
    values.dedup();
    assert_eq!(values.len(), codes.len(), "{codes:?}");
    // Clear of NSIS's 0 to 2, of #638's 20 and 21 (packaging-setup-exe-missing), of the codes
    // Intune and SCCM read as success or retry, and of the elevated instance's 99 and 100 + code.
    let taken = [0, 1, 2, 20, 21, 99, 1223, 1618, 1641, 1707, 3010];
    for (name, value) in admin {
        assert!(!taken.contains(value), "{name}");
        assert!(!(100..200).contains(value), "{name}");
    }
}

#[test]
fn every_exit_code_of_adopt_has_its_outcome() {
    let code = hook_code();
    // SetupExit's codes (S15 PR 2), the elevated instance's offset and its "not run".
    let adopt = [
        ("DONE", 0),
        ("USAGE", 1),
        ("OTHER_ACCOUNT", 2),
        ("FAILED", 3),
        ("LOCK_HELD", 5),
        ("DATA_DIR", 6),
        ("NOT_ELEVATED", 7),
        ("NOT_ADMIN_COPY", 8),
    ];
    for (name, value) in adopt {
        line_with(&code, &[&format!("!define CLAUSEO_ADOPT_{name} {value}")]);
    }
    line_with(&code, &["!define CLAUSEO_ADMIN_EXIT_BASE 100"]);
    line_with(&code, &["!define CLAUSEO_ADMIN_NOT_RUN 99"]);
    let body = macro_body(&code, ASK);
    // The code is read only after an ended wait, seeded "not run"; past 5 minutes, outcome 3.
    let wait = line_with(body, &["WaitForSingleObject(p R7, i 300000) i .R9"]);
    assert_eq!(
        body[wait + 1..wait + 3],
        ["${If} $R9 = 258", "StrCpy $R6 3"]
    );
    line_with(
        body,
        &["GetExitCodeProcess(p R7, *i ${CLAUSEO_ADMIN_NOT_RUN} R8)"],
    );
    // --adopt's own code only from 100 + code; anything else (99, NSIS's 1 or 2) is -1.
    let unwrap = [
        "StrCpy $R9 -1",
        "${If} $R8 >= ${CLAUSEO_ADMIN_EXIT_BASE}",
        "${AndIf} $R8 < 200",
        "IntOp $R9 $R8 - ${CLAUSEO_ADMIN_EXIT_BASE}",
        "${EndIf}",
    ];
    let start = line_with(body, &["StrCpy $R9 -1"]);
    assert_eq!(body[start..start + unwrap.len()], unwrap, "{body:?}");
    // Each code named in one ${Case} line, with its outcome on the next: 0 set up, 2 another
    // account, the rest not set up; and anything else not set up.
    let select = block(body, "${Select} $R9", "${EndSelect}");
    for (name, _) in adopt {
        let define = format!("${{CLAUSEO_ADOPT_{name}}}");
        let cases: Vec<usize> = (0..select.len())
            .filter(|&i| select[i].starts_with("${Case") && select[i].contains(&define))
            .collect();
        assert_eq!(cases.len(), 1, "{name}: {select:?}");
        let outcome = match name {
            "DONE" => "StrCpy $R6 0",
            "OTHER_ACCOUNT" => "StrCpy $R6 2",
            _ => "StrCpy $R6 1",
        };
        assert_eq!(select[cases[0] + 1], outcome, "{name}: {select:?}");
    }
    let other = line_with(select, &["${CaseElse}"]);
    assert_eq!(select[other + 1], "StrCpy $R6 1", "{select:?}");
    // The outcomes: a box and a details line for 1 and 2, a details line only for 3.
    let shown = [
        "${If} $R6 = 1",
        "DetailPrint \"$(adminCopyNotSetUp)\"",
        "MessageBox MB_OK|MB_ICONEXCLAMATION \"$(adminCopyNotSetUp)\"",
        "${ElseIf} $R6 = 2",
        "DetailPrint \"$(adminCopyOtherAccount)\"",
        "MessageBox MB_OK|MB_ICONEXCLAMATION \"$(adminCopyOtherAccount)\"",
        "${ElseIf} $R6 = 3",
        "DetailPrint \"$(adminCopyStillRunning)\"",
        "${EndIf}",
    ];
    let start = line_with(body, &["${If} $R6 = 1"]);
    assert_eq!(body[start..start + shown.len()], shown, "{body:?}");
    // A declined prompt or a failed start never reads a code: $R6 stays 1 from before the call.
    let seeded = line_with(body, &["StrCpy $R6 1"]);
    assert!(
        seeded < line_with(body, &["ShellExecuteExW(p R5)"]),
        "{body:?}"
    );
}

#[test]
fn the_per_machine_installer_writes_the_admin_copy_and_adopts_it_every_time() {
    let code = hook_code();
    let post = macro_body(&code, "NSIS_HOOK_POSTINSTALL");
    let machine = block(post, "!if \"${INSTALLMODE}\" == \"perMachine\"", "!else");
    let insert = line_with(
        machine,
        &["!insertmacro CLAUSEO_ADMIN_COPY '--per-machine --user-install-dir \"$INSTDIR\"'"],
    );
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
    let expected = [
        "SetOutPath \"$INSTDIR\"",
        "${If} $R8 <> ${CLAUSEO_ADOPT_DONE}",
        "DetailPrint \"$(adminCopyNotSetUp)\"",
        "${If} ${Silent}",
        "SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_FAILED}",
        "${Else}",
        "MessageBox MB_OK|MB_ICONEXCLAMATION \"$(adminCopyNotSetUp)\"",
        "${EndIf}",
        "${EndIf}",
    ];
    assert_eq!(
        machine[insert + 1..insert + 1 + expected.len()],
        expected,
        "{machine:?}"
    );
}

#[test]
fn the_words_about_the_admin_copy_are_the_drafts() {
    let english = read("windows/nsis/English.nsh");
    for (key, words) in [
        (
            "adminCopyNotSetUp",
            "Clauseo is installed, but its strongest protection isn't set up. Clauseo works \
             without it. To set it up, run this installer again and allow it to make changes \
             when Windows asks.",
        ),
        (
            "adminCopyOtherAccount",
            "Clauseo is installed, but its strongest protection isn't set up, because another \
             Windows account on this computer already has it. Clauseo works without it.",
        ),
        (
            "adminCopySkippedSilent",
            "Clauseo's strongest protection isn't set up, because this install is silent. To set \
             it up, run this installer again without /S.",
        ),
        (
            "adminCopyStillRunning",
            "Clauseo is still setting up its strongest protection. It finishes on its own.",
        ),
    ] {
        let line = format!("LangString {key} ${{LANG_ENGLISH}} \"{words}\"");
        assert!(english.lines().any(|l| l == line), "{line}");
    }
}
